# SPDX-License-Identifier: MIT

from pathlib import Path
import sys
import time
import tomllib

import pytest

from harness import tree
from harness.browser import ENTRY_ROLES
from harness.environment import process_environment
from harness.process import ManagedProcess, terminate


def _find_row(chooser, name):
    for role in ENTRY_ROLES:
        node = chooser.find(role=role, name=name)
        if node is not None:
            return node
    return None


def _row(chooser, name):
    return tree.wait_until(lambda: _find_row(chooser, name), message=f"the {name!r} row")


def _copy_to_chooser(strata, name):
    strata.open_context_menu(name)
    strata.choose_menu_item("Copy to…")
    return strata.destination_chooser("Copy to")


def _modes(*modes):
    return [pytest.param(mode, marks=pytest.mark.preferences(browser_mode=mode)) for mode in modes]


@pytest.mark.parametrize("browser_mode", _modes("list", "columns"))
def test_copy_to_uses_the_folder_picked_in_the_chooser(strata, browser_mode):
    destination = strata.fixture.path("documents")
    chooser = _copy_to_chooser(strata, "todo.txt")
    strata.pointer.click(_row(chooser, "documents"))
    strata.confirm_destination(chooser, "Copy here")
    strata.wait(lambda: (destination / "todo.txt").exists(), "the copy in the picked folder")
    assert (destination / "todo.txt").read_text() == "todo\n"
    assert strata.fixture.path("todo.txt").exists()


@pytest.mark.parametrize("browser_mode", _modes("list", "columns", "icons"))
def test_chooser_sidebar_external_drops_survive_shrinking_and_expanding(strata, browser_mode):
    environment = strata.environment
    bookmarks = environment.config_home / "gtk-3.0" / "bookmarks"
    bookmarks.parent.mkdir(parents=True, exist_ok=True)
    destination = strata.fixture.path("pictures")
    remote = "sftp://host/unavailable Remote\n"
    existing = f"{remote}{destination.as_uri()} Destination\n"
    bookmarks.write_text(existing)
    (environment.home / "Desktop").mkdir()
    (environment.config_home / "user-dirs.dirs").write_text('XDG_DESKTOP_DIR="$HOME/Desktop"\n')
    strata.application.stop()
    strata.application.start()
    chooser = _copy_to_chooser(strata, "documents")
    pointer = strata.pointer

    def target():
        return tree.wait_until(
            lambda: chooser.find(role="button", name="Destination"),
            message="chooser sidebar destination",
        )

    def insertion_gap(index):
        return tree.wait_until(
            lambda: chooser.find_all(role="grouping", name="PINNED") or None,
            message="shared chooser pin insertion targets",
        )[index]

    assert chooser.find(role="button", name="Remote") is None
    for name, width, pin in [("readme.md", 400, False), ("todo.txt", 1200, False), ("archive", 400, True)]:
        source = strata.fixture.path(name)
        contents = None if pin else source.read_text()

        def drop_target():
            return insertion_gap(-1) if pin else target()
        drag_environment = {
            **process_environment(), **environment.variables(), **strata.display.environment,
        }
        foreign = ManagedProcess.spawn(
            "chooser-drag-source",
            [sys.executable, str(Path(__file__).parents[1] / "fixtures/file_drag_source.py"), str(source)],
            log_dir=environment.root, env=drag_environment,
        )
        try:
            button = tree.wait_until(
                lambda: tree.desktop().find(role="button", name="Foreign file selection"),
                message="external chooser drag source",
            )
            pointer.drag_points(
                button.screen_bounds().center,
                drop_target().screen_bounds().center,
                release=False,
            )
            try:
                bounds = chooser.window_bounds()
                pointer.connection.resize_surface(bounds.width, bounds.height, width, bounds.height)
                time.sleep(0.5)
                pointer.move_to(*drop_target().screen_bounds().center)
            finally:
                pointer.connection.button(1, False)
            if pin:
                expected = existing + f"{source.as_uri()} {name}\n"
                tree.wait_until(lambda: bookmarks.read_text() == expected, message="folder pinned in chooser")
                assert source.is_dir() and not (destination / name).exists()
            else:
                tree.wait_until(
                    lambda: (destination / name).exists(),
                    message="file copied by resized external chooser drag",
                )
                assert (destination / name).read_text() == contents
                assert source.read_text() == contents
                assert bookmarks.read_text() == existing
            tree.wait_until(
                lambda: tree.desktop().find(role="button", name="Foreign drag finished"),
                message="external chooser drag protocol completed without deletion",
            )
            assert chooser.is_rendered(), "a sidebar drop must not accept or close the chooser"
        finally:
            terminate(foreign.popen)

    pointer.drag(chooser.find(role="button", name="archive"), insertion_gap(0))
    expected = f"{remote}{strata.fixture.path('archive').as_uri()} archive\n{destination.as_uri()} Destination\n"
    tree.wait_until(lambda: bookmarks.read_text() == expected, message="shared chooser pin reordering")
    home = chooser.find(role="button", name="Home").screen_bounds()
    pointer.drag_points(
        chooser.find(role="button", name="Desktop").screen_bounds().center,
        (home.center[0], home.y - 1),
    )
    tree.wait_until(
        lambda: tomllib.loads(environment.settings_path.read_text())["sidebar_order"][:2] == ["desktop", "home"],
        message="chooser place reordering saved by the shared sidebar",
    )
    assert chooser.is_rendered()
    pointer.click(target())
    strata.confirm_destination(chooser, "Copy here")
    strata.wait(
        lambda: (destination / "documents" / "report.md").exists(),
        "the chooser still copies after dragging",
    )
    assert (destination / "documents" / "report.md").read_text() == "# Report\n"
