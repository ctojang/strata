// SPDX-License-Identifier: MIT

use std::{fs, os::unix::fs::PermissionsExt};

use super::super::{restart, restart_waiter};

const RESTART_CHILD: &str = "STRATA_TEST_RESTART_WITHOUT_APPLICATION";
const RESTART_RETURNED: &str = "restart without an application returned";

/// Runs in a child process, since a regression would exit the process
/// instead of failing an assertion.
#[test]
fn restart_without_an_application_leaves_the_process_running() {
    if std::env::var_os(RESTART_CHILD).is_some() {
        restart(None);
        println!("{RESTART_RETURNED}");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "ui::settings::tests::restart::restart_without_an_application_leaves_the_process_running",
            "--nocapture",
        ])
        .env(RESTART_CHILD, "1")
        .output()
        .expect("run the restart child");
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(RESTART_RETURNED),
        "{output:?}"
    );
}

#[test]
fn restart_waiter_relaunches_the_executable_once_the_parent_is_gone() {
    let directory = tempfile::Builder::new()
        .prefix("strata restart ")
        .tempdir()
        .expect("restart fixture");
    let binary = directory.path().join("strata");
    fs::write(
        &binary,
        "#!/bin/sh\nprintf 'relaunched' > \"$(dirname -- \"$0\")/result\"\n",
    )
    .expect("executable fixture");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).expect("executable mode");

    let command = restart_waiter(&binary, u32::MAX).expect("restart command");
    let status = std::process::Command::new(command.get_program())
        .args(command.get_args())
        .status()
        .expect("run restart waiter");

    assert!(status.success());
    assert_eq!(
        fs::read_to_string(directory.path().join("result")).expect("launched executable"),
        "relaunched"
    );
}
