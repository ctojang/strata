"""Focused tests for the interactive Bash installer helpers."""

import fcntl
import hashlib
import http.server
import io
import os
import pathlib
import pty
import select
import shutil
import signal
import struct
import subprocess
import sys
import tarfile
import tempfile
import termios
import threading
import time
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALLER = ROOT / "install.sh"
BASH = shutil.which("bash")


def bash(script: str, *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    test_env = os.environ.copy()
    test_env["STRATA_INSTALLER_TESTING"] = "1"
    if env:
        test_env.update(env)
    with tempfile.TemporaryDirectory() as directory:
        installer = pathlib.Path(directory) / "install.sh"
        installer.write_text(
            INSTALLER.read_text().replace(
                "/usr/share/omarchy/version", f"{directory}/system-version"
            )
        )
        return subprocess.run(
            [BASH, "-c", f'source "$2"; {script}', "bash", str(INSTALLER), str(installer)],
            check=False,
            capture_output=True,
            text=True,
            env=test_env,
        )


class InstallerTerminal:
    def __init__(self, installer: pathlib.Path, environment: dict[str, str], stdout: pathlib.Path | None = None, *, piped: bool = False):
        self.pid, self.master = pty.fork()
        if self.pid == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 88, 0, 0))
            if stdout is not None:
                with stdout.open("w") as stream:
                    os.dup2(stream.fileno(), 1)
            arguments = [BASH, str(installer)]
            if piped:
                arguments = [BASH, "-o", "pipefail", "-c", 'cat "$1" | bash', "bash", str(installer)]
            os.execve(BASH, arguments, environment)
        self.output = ""
        self.returncode = None

    def read(self, timeout: float = 0.1) -> None:
        if select.select([self.master], [], [], timeout)[0]:
            try:
                chunk = os.read(self.master, 65536)
            except OSError:
                chunk = b""
            self.output += chunk.decode("utf-8", errors="replace")
        if self.returncode is None:
            waited, status = os.waitpid(self.pid, os.WNOHANG)
            if waited:
                self.returncode = os.waitstatus_to_exitcode(status)

    def expect(self, text: str, timeout: float = 10) -> None:
        deadline = time.monotonic() + timeout
        while text not in self.output and time.monotonic() < deadline:
            self.read()
            if self.returncode is not None:
                break
        if text not in self.output:
            raise AssertionError(f"Installer did not show {text!r}:\n{self.output}")

    def send(self, keys: bytes) -> None:
        os.write(self.master, keys)

    def finish(self, timeout: float = 15) -> int:
        deadline = time.monotonic() + timeout
        while self.returncode is None and time.monotonic() < deadline:
            self.read()
        if self.returncode is None:
            raise AssertionError(f"Installer did not exit:\n{self.output}")
        self.read(0)
        return self.returncode

    def close(self) -> None:
        if self.returncode is None:
            os.killpg(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
        os.close(self.master)


class InstallerFixture:
    def __init__(
        self, *, omarchy: bool = False, verify_status: int = 0,
        release_files: dict[str, str | None] | None = None, github_cli: str = "authenticated",
    ):
        self.directory = tempfile.TemporaryDirectory(prefix="strata-installer-")
        self.root = pathlib.Path(self.directory.name)
        self.home = self.root / "home"
        self.commands = self.root / "commands"
        self.calls = self.root / "calls"
        self.terminals = []
        self.home.mkdir()
        self.commands.mkdir()
        (self.root / "tmp").mkdir()
        (self.root / "runtime").mkdir(mode=0o700)
        for name in (
            "bash", "uname", "getconf", "awk", "sort", "head", "install", "sed", "grep",
            "dirname", "mkdir", "cp", "rm", "tar", "gzip", "sha256sum", "mktemp", "sleep", "cat",
            "tee", "stty", "date", "tail",
        ):
            (self.commands / name).symlink_to(shutil.which(name))
        distro = self.root / "os-release"
        distro.write_text('ID=manjaro\nID_LIKE=arch\nPRETTY_NAME="Manjaro Linux"\n', encoding="utf-8")
        self.installer = self.root / "install.sh"
        self.installer.write_text(
            INSTALLER.read_text(encoding="utf-8")
            .replace("/etc/os-release", str(distro))
            .replace("/usr/share/omarchy/version", str(self.root / "system-version")),
            encoding="utf-8",
        )
        self.environment = os.environ.copy()
        for name in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "DBUS_STARTER_ADDRESS", "STRATA_INSTALLER_TESTING"):
            self.environment.pop(name, None)
        self.environment.update({
            "HOME": str(self.home), "PATH": f"{self.commands}:{self.home / '.local/bin'}",
            "XDG_DATA_HOME": str(self.home / "data"), "XDG_CONFIG_HOME": str(self.home / "config"),
            "XDG_STATE_HOME": str(self.home / "state"), "XDG_RUNTIME_DIR": str(self.root / "runtime"),
            "TMPDIR": str(self.root / "tmp"), "CALLS": str(self.calls),
            "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "LANGUAGE": "", "TERM": "xterm-256color",
            "HYPRLAND_INSTANCE_SIGNATURE": "", "VERIFY_RESULT": str(verify_status),
        })
        self.environment.pop("NO_COLOR", None)
        target = subprocess.check_output(["uname", "-m"], text=True).strip()
        target = "aarch64" if target in ("aarch64", "arm64") else "x86_64"
        self.archive_name = f"strata-1.0.0-{target}-unknown-linux-gnu.tar.gz"
        archive = self.root / self.archive_name
        with tarfile.open(archive, "w:gz") as bundle:
            files = {
                "strata": '#!/bin/bash\nprintf "%s %s\\n" "$0" "$*" >> "$CALLS"\n',
                "io.github.lgse.Strata.svg": "<svg/>\n",
            }
            files["io.github.lgse.Strata.desktop"] = (ROOT / "data/io.github.lgse.Strata.desktop").read_text(encoding="utf-8")
            files.update(release_files or {})
            for name, content in files.items():
                if content is None:
                    continue
                member = tarfile.TarInfo(f"{self.archive_name.removesuffix('.tar.gz')}/{name}")
                encoded = content.encode()
                member.size = len(encoded)
                member.mode = 0o755 if name == "strata" else 0o644
                bundle.addfile(member, io.BytesIO(encoded))
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_suffix(".gz.sha256").write_text(f"{digest}  {self.archive_name}\n", encoding="utf-8")
        self.environment["FIXTURE_ARCHIVE"] = str(archive)
        self.command("curl", '''output=""
while (($#)); do
  case $1 in
    --output|-o) output=$2; shift 2 ;;
    -w) shift 2 ;;
    -*) shift ;;
    *) url=$1; shift ;;
  esac
done
printf "curl %s\\n" "$url" >> "$CALLS"
case $url in
  */latest) printf 'https://github.com/lgse/strata/releases/tag/v1.0.0' ;;
  *.sha256) cp "$FIXTURE_ARCHIVE.sha256" "$output" ;;
  *) cp "$FIXTURE_ARCHIVE" "$output" ;;
esac
''')
        self.command("pacman", '''if [[ $1 == -Q ]]; then
  [[ $2 != gtksourceview5 && $2 != dcraw && $2 != gvfs-smb ]]
else
  printf "pacman %s\\n" "$*" >> "$CALLS"
fi
''')
        self.command("sudo", '''printf "sudo %s\\n" "$*" >> "$CALLS"
[[ ${1:-} != -n ]] || shift
exec "$@"
''')
        if github_cli != "missing":
            authenticated = 0 if github_cli == "authenticated" else 1
            self.command("gh", f'''[[ $1 != auth ]] || exit {authenticated}
printf "gh %s\\n" "$*" >> "$CALLS"
printf 'fixture verification details\\n'
exit "$VERIFY_RESULT"
''')
        self.command("xdg-mime", '''printf "xdg-mime %s\\n" "$*" >> "$CALLS"
[[ $1 != query ]] || printf 'io.github.lgse.Strata.desktop\\n'
''')
        for name in ("gdbus", "update-desktop-database", "gtk-update-icon-cache"):
            self.command(name, f'printf "{name} %s\\n" "$*" >> "$CALLS"\n')
        if omarchy:
            self.command("omarchy", "printf '4.0.4\\n'\n")

    def command(self, name: str, body: str) -> None:
        path = self.commands / name
        path.write_text("#!/bin/bash\nset -euo pipefail\n" + body, encoding="utf-8")
        path.chmod(0o755)

    def run(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [BASH, str(self.installer), *arguments], env=self.environment,
            check=False, capture_output=True, text=True, timeout=20,
        )

    def terminal(self, stdout: pathlib.Path | None = None, *, piped: bool = False) -> InstallerTerminal:
        terminal = InstallerTerminal(self.installer, self.environment, stdout, piped=piped)
        self.terminals.append(terminal)
        return terminal

    def recorded_calls(self) -> list[str]:
        return self.calls.read_text().splitlines() if self.calls.exists() else []

    def recorded_application_calls(self) -> list[str]:
        return [call for call in self.recorded_calls() if call.startswith(str(self.root))]

    def __enter__(self):
        return self

    def __exit__(self, *_):
        for terminal in self.terminals:
            terminal.close()
        self.directory.cleanup()


class InstallerTests(unittest.TestCase):
    def test_installer_has_valid_bash_syntax(self) -> None:
        result = subprocess.run(
            ["bash", "-n", str(INSTALLER)], capture_output=True, text=True, check=False
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_volume_monitor_is_required_but_smb_is_optional(self) -> None:
        result = bash('printf "%s\\n" "${REQUIRED_PACKAGES[@]}"')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("gvfs", result.stdout.splitlines())
        self.assertNotIn("gvfs-smb", result.stdout.splitlines())
        self.assertNotIn("github-cli", result.stdout.splitlines())

    def test_redirected_output_has_no_terminal_controls(self) -> None:
        result = bash(
            'setup_terminal; show_banner; info complete; warn warning; die failed',
            env={"NO_COLOR": "1"},
        )
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("\033", result.stdout + result.stderr)

    def test_provenance_verification_is_optional_without_github_cli(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result = bash(
                'PATH="$EMPTY_PATH"; verify_provenance /tmp/strata.tar.gz',
                env={"EMPTY_PATH": directory},
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("GitHub CLI is unavailable", result.stderr)

    def test_provenance_verification_is_optional_without_github_authentication(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fake_gh = pathlib.Path(directory) / "gh"
            fake_gh.write_text(
                '#!/bin/sh\nprintf "%s\\n" "$*" >> "$CALLS"\nexit 1\n', encoding="utf-8"
            )
            fake_gh.chmod(0o755)
            calls = pathlib.Path(directory) / "calls"
            result = bash(
                'PATH="$FAKE_PATH"; verify_provenance /tmp/strata.tar.gz',
                env={"FAKE_PATH": directory, "CALLS": str(calls)},
            )
            recorded_calls = calls.read_text().splitlines()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(recorded_calls, ["auth status --hostname github.com"])
        self.assertIn("GitHub CLI is not authenticated", result.stderr)

    def test_authenticated_provenance_verification_remains_mandatory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fake_gh = pathlib.Path(directory) / "gh"
            fake_gh.write_text(
                '#!/bin/sh\nprintf "%s\\n" "$*" >> "$CALLS"\n'
                'case "$*" in "auth status"*) exit 0;; *) exit "$VERIFY_RESULT";; esac\n',
                encoding="utf-8",
            )
            fake_gh.chmod(0o755)
            calls = pathlib.Path(directory) / "calls"
            result = bash(
                'PATH="$FAKE_PATH"; verify_provenance /tmp/strata.tar.gz',
                env={"FAKE_PATH": directory, "CALLS": str(calls), "VERIFY_RESULT": "9"},
            )
            recorded_calls = calls.read_text().splitlines()
        self.assertEqual(result.returncode, 9, result.stderr)
        self.assertEqual(
            recorded_calls,
            [
                "auth status --hostname github.com",
                "attestation verify /tmp/strata.tar.gz --repo lgse/strata",
            ],
        )

    def test_unattended_flags_select_only_requested_integrations(self) -> None:
        result = bash(
            "parse_args --with-folder-association --with-raw; "
            'printf "%s %s %s %s %s %s" "$NON_INTERACTIVE" "$WITH_SMB" '
            '"$WITH_RAW" "$WITH_DESKTOP_ENTRY" "$WITH_FOLDER_ASSOCIATION" '
            '"$WITH_OMARCHY_KEYBINDS"'
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "yes ask yes yes yes ask")

    def test_non_interactive_mode_declines_unspecified_options(self) -> None:
        with InstallerFixture() as fixture:
            result = fixture.run("--non-interactive")
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(fixture.recorded_application_calls(), [])
            self.assertFalse(any(call.startswith(("xdg-mime", "gdbus")) for call in fixture.recorded_calls()))

    def test_non_interactive_pacman_disables_sudo_and_pacman_prompts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fake_sudo = pathlib.Path(directory) / "sudo"
            fake_sudo.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
            fake_sudo.chmod(0o755)
            result = bash(
                "NON_INTERACTIVE=yes; run_pacman gvfs-smb",
                env={"PATH": f"{directory}:{os.environ['PATH']}"},
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout.splitlines(),
            ["-n", "pacman", "-S", "--needed", "--noconfirm", "--", "gvfs-smb"],
        )

    def test_interactive_pacman_reads_from_the_terminal_not_stdin(self) -> None:
        source = INSTALLER.read_text(encoding="utf-8")
        self.assertIn('sudo pacman -S --needed -- "$@" </dev/tty', source)

    def test_github_cli_requirement_is_skipped_when_gh_is_already_on_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fake_pacman = pathlib.Path(directory) / "pacman"
            fake_pacman.write_text('#!/bin/sh\n[ "$1" = "-Q" ] && exit 1\nexit 0\n')
            fake_pacman.chmod(0o755)
            result = bash(
                "REQUIRED_PACKAGES=(github-cli); NON_INTERACTIVE=yes; "
                "gh() { :; }; "
                'run_pacman() { printf "run_pacman called: %s\\n" "$*" >&2; }; '
                "install_arch_dependencies",
                env={"PATH": directory},
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("already installed", result.stdout)
        self.assertNotIn("run_pacman called", result.stderr)

    def test_github_cli_requirement_still_installs_when_gh_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            fake_pacman = pathlib.Path(directory) / "pacman"
            fake_pacman.write_text('#!/bin/sh\n[ "$1" = "-Q" ] && exit 1\nexit 0\n')
            fake_pacman.chmod(0o755)
            result = bash(
                "REQUIRED_PACKAGES=(github-cli); NON_INTERACTIVE=yes; "
                'run_pacman() { printf "run_pacman called: %s\\n" "$*" >&2; }; '
                "install_arch_dependencies",
                env={"PATH": directory},
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("run_pacman called: github-cli", result.stderr)

    def test_unsupported_omarchy_flag_fails_before_installing_anything(self) -> None:
        with InstallerFixture() as fixture:
            result = fixture.run("--with-omarchy-keybinds")
            self.assertEqual(fixture.recorded_calls(), [], result.stdout + result.stderr)
            self.assertFalse((fixture.home / ".local/bin/strata").exists())
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires Omarchy 3 or 4", result.stderr)

    def test_recommended_setup_installs_launcher_without_changing_defaults(self) -> None:
        for github_cli, provenance_note in (
            ("authenticated", None),
            ("unauthenticated", "GitHub CLI is not authenticated"),
            ("missing", "GitHub CLI is unavailable"),
        ):
            with self.subTest(github_cli=github_cli), InstallerFixture(github_cli=github_cli) as fixture:
                terminal = fixture.terminal(piped=True)
                terminal.expect("Enter continue")
                terminal.send(b"\r")
                self.assertEqual(terminal.finish(), 0, terminal.output)
                self.assertTrue((fixture.home / ".local/bin/strata").is_file())
                self.assertTrue((fixture.home / "data/applications/io.github.lgse.Strata.desktop").is_file())
                calls = fixture.recorded_calls()
                self.assertFalse(any(call.startswith(("xdg-mime", "gdbus")) for call in calls), calls)
                self.assertEqual(fixture.recorded_application_calls(), [])
                self.assertNotIn("fixture verification details", terminal.output)
                if provenance_note:
                    self.assertIn(provenance_note, terminal.output)
                self.assertIn("Ready to explore", terminal.output)
                self.assertEqual(list((fixture.root / "tmp").glob("*.log")), [])

    def test_custom_setup_applies_selected_integrations_and_their_dependencies(self) -> None:
        with InstallerFixture() as fixture:
            terminal = fixture.terminal()
            terminal.expect("Enter continue")
            terminal.send(b"\033[B\r")
            terminal.expect("Space toggle")
            terminal.send(b"\033[B \r")
            self.assertEqual(terminal.finish(), 0, terminal.output)
            calls = fixture.recorded_calls()
            self.assertIn("xdg-mime default io.github.lgse.Strata.desktop inode/directory", calls)
            self.assertEqual(fixture.recorded_application_calls(), [])
            self.assertTrue((fixture.home / "data/applications/io.github.lgse.Strata.desktop").is_file())

    def test_deselecting_a_required_launcher_clears_the_folder_association(self) -> None:
        with InstallerFixture() as fixture:
            terminal = fixture.terminal()
            terminal.expect("Enter continue")
            terminal.send(b"\033[B\r")
            terminal.expect("Space toggle")
            terminal.send(b"\033[B \033[A \r")
            self.assertEqual(terminal.finish(), 0, terminal.output)
            self.assertFalse((fixture.home / "data/applications/io.github.lgse.Strata.desktop").exists())
            self.assertFalse(any(call.startswith("xdg-mime") for call in fixture.recorded_calls()))

    def test_custom_setup_offers_omarchy_only_when_supported(self) -> None:
        for omarchy in (False, True):
            with self.subTest(omarchy=omarchy), InstallerFixture(omarchy=omarchy) as fixture:
                terminal = fixture.terminal()
                terminal.expect("Enter continue")
                terminal.send(b"\033[B\r")
                terminal.expect("Space toggle")
                self.assertEqual("Omarchy file-manager shortcuts" in terminal.output, omarchy)
                terminal.send(b"q")
                self.assertEqual(terminal.finish(), 0, terminal.output)
                self.assertEqual(fixture.recorded_calls(), [])

    def test_plain_setup_can_install_without_a_launcher(self) -> None:
        with InstallerFixture() as fixture:
            fixture.environment["TERM"] = "dumb"
            terminal = fixture.terminal()
            terminal.expect("Choose [1]")
            terminal.send(b"2\n")
            terminal.expect("Toggle an option by number")
            terminal.send(b"1\n\n")
            self.assertEqual(terminal.finish(), 0, terminal.output)
            self.assertTrue((fixture.home / ".local/bin/strata").is_file())
            self.assertFalse((fixture.home / "data/applications/io.github.lgse.Strata.desktop").exists())
            self.assertNotIn("\033", terminal.output)

    def test_redirected_installation_keeps_choices_on_the_terminal(self) -> None:
        with InstallerFixture() as fixture:
            output = fixture.root / "output"
            terminal = fixture.terminal(stdout=output)
            terminal.expect("Customize installation")
            terminal.send(b"\n")
            self.assertEqual(terminal.finish(), 0, terminal.output + output.read_text())
            self.assertTrue((fixture.home / ".local/bin/strata").is_file())
            self.assertNotIn("\033", output.read_text())

    def test_download_shows_real_curl_progress_before_installing(self) -> None:
        with InstallerFixture() as fixture:
            body = pathlib.Path(fixture.environment["FIXTURE_ARCHIVE"]).read_bytes()

            class ArchiveHandler(http.server.BaseHTTPRequestHandler):
                def do_GET(self):
                    self.send_response(200)
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    chunk_size = max(1, len(body) // 60)
                    for start in range(0, len(body), chunk_size):
                        self.wfile.write(body[start:start + chunk_size])
                        self.wfile.flush()
                        time.sleep(0.035)

                def log_message(self, *_):
                    pass

            with http.server.ThreadingHTTPServer(("127.0.0.1", 0), ArchiveHandler) as server:
                worker = threading.Thread(target=server.serve_forever, daemon=True)
                fixture.environment["REAL_CURL"] = shutil.which("curl")
                fixture.environment["DOWNLOAD_URL"] = f"http://127.0.0.1:{server.server_port}/archive"
                curl = fixture.commands / "curl"
                curl.write_text(curl.read_text().replace(
                    '*) cp "$FIXTURE_ARCHIVE" "$output" ;;',
                    '*) exec "$REAL_CURL" --noproxy "*" --fail --progress-bar --output "$output" "$DOWNLOAD_URL" ;;',
                ))
                terminal = fixture.terminal()
                worker.start()
                try:
                    terminal.expect("Enter continue")
                    terminal.send(b"\r")
                    terminal.expect("%")
                    self.assertFalse((fixture.home / ".local/bin/strata").exists())
                    self.assertEqual(terminal.finish(), 0, terminal.output)
                    self.assertTrue((fixture.home / ".local/bin/strata").is_file())
                finally:
                    server.shutdown()
                    worker.join(timeout=5)

    def test_escape_keeps_an_existing_install_untouched(self) -> None:
        with InstallerFixture() as fixture:
            binary = fixture.home / ".local/bin/strata"
            binary.parent.mkdir(parents=True)
            binary.write_text("previous version", encoding="utf-8")
            terminal = fixture.terminal()
            terminal.expect("Enter continue")
            terminal.send(b"\033")
            self.assertEqual(terminal.finish(), 0, terminal.output)
            self.assertEqual(binary.read_text(), "previous version")
            self.assertEqual(fixture.recorded_calls(), [])

    def test_no_color_keeps_terminal_navigation_without_color_sequences(self) -> None:
        with InstallerFixture() as fixture:
            fixture.environment["NO_COLOR"] = "1"
            terminal = fixture.terminal()
            terminal.expect("Enter continue")
            terminal.send(b"\r")
            self.assertEqual(terminal.finish(), 0, terminal.output)
            self.assertNotRegex(terminal.output, r"\x1b\[[0-9;]*m")
            self.assertTrue((fixture.home / ".local/bin/strata").is_file())

    def test_interrupt_stops_a_download_before_changing_the_machine(self) -> None:
        with InstallerFixture() as fixture:
            child_pid = fixture.root / "download-pid"
            fixture.environment["DOWNLOAD_PID"] = str(child_pid)
            fixture.command("curl", '''case "$*" in
  */latest*) printf 'https://github.com/lgse/strata/releases/tag/v1.0.0' ;;
  *) printf '%s' "$$" > "$DOWNLOAD_PID"; exec sleep 30 ;;
esac
''')
            terminal = fixture.terminal()
            terminal.expect("Enter continue")
            terminal.send(b"\r")
            terminal.expect("Download Strata 1.0.0")
            deadline = time.monotonic() + 3
            while not child_pid.exists() and time.monotonic() < deadline:
                terminal.read()
            self.assertTrue(child_pid.exists(), terminal.output)
            terminal.send(b"\x03")
            self.assertEqual(terminal.finish(), 130, terminal.output)
            with self.assertRaises(ProcessLookupError):
                os.kill(int(child_pid.read_text()), 0)
            self.assertFalse((fixture.home / ".local/bin/strata").exists())
            self.assertFalse(any(call.startswith("sudo") for call in fixture.recorded_calls()))

    def test_dependencies_share_one_transaction_with_selected_optional_support(self) -> None:
        with InstallerFixture() as fixture:
            result = fixture.run("--with-raw", "--with-smb")
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            calls = fixture.recorded_calls()
            transactions = [call for call in calls if call.startswith("pacman -S")]
            self.assertEqual(len(transactions), 1, calls)
            self.assertIn("gtksourceview5", transactions[0])
            self.assertIn("dcraw", transactions[0])
            self.assertIn("gvfs-smb", transactions[0])
            self.assertEqual(fixture.recorded_application_calls(), [])
            self.assertLess(
                next(i for i, call in enumerate(calls) if call.startswith("gh attestation verify")),
                calls.index(transactions[0]),
            )

    def test_failed_verification_leaves_the_machine_unchanged(self) -> None:
        with InstallerFixture(verify_status=9) as fixture:
            result = fixture.run("--non-interactive")
            self.assertEqual(result.returncode, 9, result.stdout + result.stderr)
            self.assertFalse((fixture.home / ".local/bin/strata").exists())
            self.assertFalse(any(call.startswith("sudo") for call in fixture.recorded_calls()))
            self.assertIn("fixture verification details", result.stderr)
            self.assertIn("Detailed output:", result.stderr)

    def test_integration_failure_reports_that_the_application_is_installed(self) -> None:
        with InstallerFixture() as fixture:
            fixture.command("xdg-mime", "exit 17\n")
            result = fixture.run("--with-folder-association")
            self.assertEqual(result.returncode, 17, result.stdout + result.stderr)
            self.assertTrue((fixture.home / ".local/bin/strata").is_file())
            self.assertIn("Strata is installed, but setup did not finish.", result.stderr)

    def test_package_failure_is_in_the_retained_log(self) -> None:
        with InstallerFixture() as fixture:
            fixture.command("pacman", '''if [[ $1 == -Q ]]; then exit 1; fi
printf 'fixture package download failed\\n' >&2
exit 23
''')
            result = fixture.run("--non-interactive")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("fixture package download failed", result.stdout + result.stderr)
            logs = list((fixture.root / "tmp").glob("*.log"))
            self.assertEqual(len(logs), 1)
            self.assertIn("fixture package download failed", logs[0].read_text())
            self.assertFalse((fixture.home / ".local/bin/strata").exists())

    def test_package_logging_preserves_terminal_prompts_without_recording_passwords(self) -> None:
        with InstallerFixture() as fixture:
            fixture.command("sudo", '''printf 'Fixture sudo password: ' >/dev/tty
IFS= read -rs password </dev/tty
[[ $password == fixture-secret ]] || exit 19
exec "$@"
''')
            fixture.command("pacman", '''if [[ $1 == -Q ]]; then exit 1; fi
printf 'Proceed with fixture packages? [Y/n] '
IFS= read -r answer
[[ $answer == n ]] || exit 18
printf 'fixture package operation cancelled\\n' >&2
exit 23
''')
            terminal = fixture.terminal(piped=True)
            terminal.expect("Enter continue")
            terminal.send(b"\r")
            terminal.expect("Fixture sudo password:")
            terminal.send(b"fixture-secret\n")
            terminal.expect("Proceed with fixture packages?")
            terminal.send(b"n\n")
            self.assertEqual(terminal.finish(), 23, terminal.output)
            logs = list((fixture.root / "tmp").glob("*.log"))
            self.assertEqual(len(logs), 1)
            self.assertIn("Proceed with fixture packages?", logs[0].read_text())
            self.assertIn("fixture package operation cancelled", logs[0].read_text())
            self.assertNotIn("fixture-secret", logs[0].read_text() + terminal.output)
            self.assertFalse((fixture.home / ".local/bin/strata").exists())

    def test_reinstall_without_consent_fails_before_downloading(self) -> None:
        with InstallerFixture() as fixture:
            binary = fixture.home / ".local/bin/strata"
            binary.parent.mkdir(parents=True)
            binary.write_text("previous version", encoding="utf-8")
            result = fixture.run("--non-interactive")
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(binary.read_text(), "previous version")
            self.assertEqual(fixture.recorded_calls(), [])

    def test_installer_messages_follow_locale_precedence_and_keep_paths_literal(self) -> None:
        path = "/tmp/literal & %{version} [file]"
        for language, locale, expected in (
            ("zz:fr", "C.UTF-8", f"Installer dans {path}"),
            ("fr", "C", f"Install to {path}"),
            ("de", "C.UTF-8", f"Installation nach {path}"),
            ("es", "C.UTF-8", f"Instalar en {path}"),
            ("ja", "C.UTF-8", f"インストール先: {path}"),
            ("pt_PT", "C.UTF-8", f"Instalar em {path}"),
            ("ko", "C.UTF-8", f"설치 위치: {path}"),
            ("vi", "C.UTF-8", f"Cài đặt vào {path}"),
            ("it", "C.UTF-8", f"Installa in {path}"),
            ("ru", "C.UTF-8", f"Установить в {path}"),
            ("zh_CN", "C.UTF-8", f"安装到 {path}"),
            ("zh_TW", "C.UTF-8", f"安裝到 {path}"),
            ("zh-Hant-HK", "C.UTF-8", f"安裝到 {path}"),
        ):
            with self.subTest(language=language, locale=locale):
                result = bash(
                    'text destination "path=$VALUE"',
                    env={"LANGUAGE": language, "LC_ALL": locale, "VALUE": path},
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)

    def test_embedded_translations_match_the_shared_catalog(self) -> None:
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/installer_messages.py")],
            check=False, capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_unknown_installer_option_is_rejected(self) -> None:
        result = bash("parse_args --definitely-unknown")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Unknown option", result.stderr)

    def test_version_comparison_accepts_minimum_and_newer(self) -> None:
        for version in ("2.39", "2.39.1", "2.40"):
            with self.subTest(version=version):
                result = bash(f'version_at_least "{version}" 2.39')
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_version_comparison_rejects_older_glibc(self) -> None:
        result = bash('version_at_least "2.38" 2.39')
        self.assertNotEqual(result.returncode, 0)

    def test_detects_omarchy_major_from_command(self) -> None:
        with self.subTest(major="3"):
            result = bash(
                "detect_omarchy_major",
                env={"PATH": f"{ROOT / 'scripts' / 'testdata' / 'omarchy3'}:{os.environ['PATH']}"},
            )
            self.assertEqual(result.stdout.strip(), "3")
        with self.subTest(major="4"):
            result = bash(
                "detect_omarchy_major",
                env={"PATH": f"{ROOT / 'scripts' / 'testdata' / 'omarchy4'}:{os.environ['PATH']}"},
            )
            self.assertEqual(result.stdout.strip(), "4")

    def test_omarchy_major_from_requires_a_whole_version_token(self) -> None:
        cases = [
            ("3.8.5", "3"),
            ("1:4.0.0-1", "4"),
            ("4.0.0-1", "4"),
            ("4.0.0.alpha", "4"),
            ("Omarchy 2.3.1", ""),
            ("5.4.0", ""),
            ("dev (b280f130)", ""),
        ]
        for output, expected in cases:
            with self.subTest(output=output):
                result = bash(f'omarchy_major_from "{output}"')
                self.assertEqual(result.stdout.strip(), expected)
                if expected:
                    self.assertEqual(result.returncode, 0, result.stderr)
                else:
                    self.assertEqual(result.returncode, 1, result.stderr)

    def test_omarchy_dev_hash_is_not_a_major(self) -> None:
        with tempfile.TemporaryDirectory() as home:
            result = bash(
                "detect_omarchy_major",
                env={
                    "HOME": home,
                    "PATH": f"{ROOT / 'scripts' / 'testdata' / 'omarchy-dev'}:{os.environ['PATH']}",
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "")

    def test_omarchy_2_3_1_command_is_not_major_3(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            home = pathlib.Path(directory) / "home"
            home.mkdir()
            fake = pathlib.Path(directory) / "omarchy"
            fake.write_text("#!/bin/sh\nprintf '%s\\n' 'Omarchy 2.3.1'\n", encoding="utf-8")
            fake.chmod(0o755)
            result = bash(
                "detect_omarchy_major",
                env={"HOME": str(home), "PATH": f"{directory}:{os.environ['PATH']}"},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "")

    def test_omarchy_dev_command_falls_back_to_version_file(self) -> None:
        with tempfile.TemporaryDirectory() as home:
            version = pathlib.Path(home) / ".local" / "share" / "omarchy" / "version"
            version.parent.mkdir(parents=True)
            version.write_text("4.0.0.alpha\n", encoding="utf-8")
            result = bash(
                "detect_omarchy_major",
                env={
                    "HOME": home,
                    "PATH": f"{ROOT / 'scripts' / 'testdata' / 'omarchy-dev'}:{os.environ['PATH']}",
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "4")

    def test_detected_omarchy_4_from_version_file_writes_lua_bindings(self) -> None:
        with tempfile.TemporaryDirectory() as home:
            version = pathlib.Path(home) / ".local" / "share" / "omarchy" / "version"
            version.parent.mkdir(parents=True)
            version.write_text("4.0.0.alpha\n", encoding="utf-8")
            result = bash(
                'BIN_PATH="$HOME/.local/bin/strata"; '
                "major=$(detect_omarchy_major); "
                'configure_omarchy_bindings "$major"',
                env={
                    "HOME": home,
                    "HYPRLAND_INSTANCE_SIGNATURE": "",
                    "PATH": f"{ROOT / 'scripts' / 'testdata' / 'omarchy-dev'}:{os.environ['PATH']}",
                },
            )
            lua = pathlib.Path(home) / ".config" / "hypr" / "bindings.lua"
            conf = pathlib.Path(home) / ".config" / "hypr" / "bindings.conf"
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(lua.is_file())
            self.assertFalse(conf.exists())
            self.assertIn("strata-installer: file-manager start", lua.read_text())
            self.assertIn("Omarchy 4 file-manager shortcuts now open Strata.", result.stdout)

    def test_omarchy_detection_without_command_is_not_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as home:
            result = bash(
                'PATH="$HOME"; value=$(detect_omarchy_major); printf "%s" "$value"',
                env={"HOME": home},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "")

    def test_generated_omarchy_bindings_are_idempotent(self) -> None:
        for major, suffix in (("3", "conf"), ("4", "lua")):
            with self.subTest(major=major), tempfile.TemporaryDirectory() as home:
                result = bash(
                    f'BIN_PATH="$HOME/.local/bin/strata"; '
                    f"configure_omarchy_bindings {major}; "
                    f"configure_omarchy_bindings {major}",
                    env={"HOME": home, "HYPRLAND_INSTANCE_SIGNATURE": ""},
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                bindings = pathlib.Path(home) / ".config" / "hypr" / f"bindings.{suffix}"
                contents = bindings.read_text()
                self.assertEqual(contents.count("strata-installer: file-manager start"), 1)
                self.assertIn(f"{home}/.local/bin/strata", contents)
                if major == "4":
                    self.assertIn(
                        f'"uwsm-app -- {home}/.local/bin/strata '
                        '\\"$(omarchy-cmd-terminal-cwd)\\""',
                        contents,
                    )


if __name__ == "__main__":
    unittest.main()
