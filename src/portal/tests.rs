// SPDX-License-Identifier: MIT

use std::{
    ffi::{OsStr, OsString},
    future::Future,
    os::unix::ffi::OsStringExt as _,
    sync::atomic::Ordering,
};

use ashpd::desktop::{
    HandleToken,
    file_chooser::{Choice, FileFilter, OpenFileOptions},
};

use super::*;

#[test]
fn open_defaults_match_the_portal_contract() {
    let request = run_async(open_request(
        HandleToken::default().to_string(),
        None,
        "",
        OpenFileOptions::default(),
    ))
    .expect("open request");
    assert!(request.modal);
    assert_eq!(request.title, "Open Files");
    assert_eq!(request.accept_label, "Open");
    assert!(matches!(
        request.kind,
        ChooserKind::Open {
            directory: false,
            multiple: false
        }
    ));
}

#[test]
fn folder_hints_override_defaults_and_invalid_hints_preserve_save_names() {
    let current = tempfile::tempdir().expect("current directory");
    for hint in [
        None,
        Some(PathBuf::from("relative")),
        Some(current.path().join("missing")),
        Some(current.path().to_path_buf()),
    ] {
        let expected = if hint.as_deref() == Some(current.path()) {
            current.path().to_path_buf()
        } else {
            crate::ui::default_save_folder()
        };
        assert_eq!(
            run_async(accessible_folder(hint.clone())).expect("open/save-files folder"),
            expected
        );
        assert_eq!(
            run_async(save_file_suggestion(
                None,
                hint,
                Some("report.txt".to_owned()),
            ))
            .expect("save suggestion"),
            (expected, Some(OsString::from("report.txt")))
        );
    }
}

#[test]
fn current_file_takes_precedence_over_folder_and_name() {
    let current = tempfile::tempdir().expect("current directory");
    let ignored = tempfile::tempdir().expect("ignored directory");
    let file = current.path().join("existing.txt");
    std::fs::write(&file, b"data").expect("fixture file");

    let suggestion = run_async(save_file_suggestion(
        Some(file),
        Some(ignored.path().to_path_buf()),
        Some("ignored.txt".to_owned()),
    ))
    .expect("save suggestion");
    assert_eq!(suggestion.0, current.path());
    assert_eq!(suggestion.1.as_deref(), Some(OsStr::new("existing.txt")));
}

#[test]
fn current_file_preserves_a_non_utf8_filename() {
    let current = tempfile::tempdir().expect("current directory");
    let name = OsString::from_vec(vec![b'n', 0xff]);
    let file = current.path().join(&name);
    std::fs::write(&file, b"data").expect("fixture file");

    let suggestion =
        run_async(save_file_suggestion(Some(file), None, None)).expect("save suggestion");
    assert_eq!(suggestion, (current.path().to_path_buf(), Some(name)));
}

#[test]
fn new_current_file_preserves_filename_and_directory() {
    let current = tempfile::tempdir().expect("current directory");
    for name in [
        OsString::from("packing list 格式(2).xls"),
        OsString::from_vec(vec![b'n', 0xff]),
    ] {
        let file = current.path().join(&name);
        let suggestion = run_async(save_file_suggestion(
            Some(file.clone()),
            Some(current.path().to_path_buf()),
            None,
        ))
        .expect("save suggestion");
        assert_eq!(suggestion, (current.path().to_path_buf(), Some(name)));
        assert!(!file.exists(), "suggesting a name must not create a file");
    }
}

#[test]
fn current_file_rejects_directories_and_missing_parents() {
    let current = tempfile::tempdir().expect("current directory");
    for file in [
        current.path().to_path_buf(),
        current.path().join("missing/new.txt"),
    ] {
        let suggestion =
            run_async(save_file_suggestion(Some(file), None, None)).expect("save suggestion");
        assert_eq!(suggestion, (crate::ui::default_save_folder(), None));
    }
}

#[test]
fn invalid_current_file_falls_back_without_using_lower_priority_suggestions() {
    let ignored = tempfile::tempdir().expect("ignored directory");
    let suggestion = run_async(save_file_suggestion(
        Some(PathBuf::from("relative/missing.txt")),
        Some(ignored.path().to_path_buf()),
        Some("ignored.txt".to_owned()),
    ))
    .expect("save suggestion");
    assert_eq!(suggestion, (crate::ui::default_save_folder(), None));
}

#[test]
fn filters_and_choices_keep_input_order_and_current_filter() {
    let images = FileFilter::new("Images")
        .glob("*.png")
        .mimetype("image/jpeg");
    let text = FileFilter::new("Text").glob("*.txt");
    let encoding = Choice::new("encoding", "Encoding", "utf8")
        .insert("utf8", "UTF-8")
        .insert("latin1", "Latin-1");
    let request = run_async(open_request(
        HandleToken::default().to_string(),
        None,
        "Choose",
        OpenFileOptions::default()
            .set_filters([images.clone(), text.clone()])
            .set_current_filter(Some(text.clone()))
            .set_choices([Choice::boolean("readonly", "Read only", true), encoding]),
    ))
    .expect("open request");
    assert_eq!(request.filters, [images, text.clone()]);
    assert_eq!(request.current_filter, Some(text));
    assert_eq!(
        request.choices.iter().map(Choice::id).collect::<Vec<_>>(),
        ["readonly", "encoding"]
    );
}

#[test]
fn cancellation_before_presentation_is_sticky_and_cleanup_is_race_safe() {
    let tracker = Arc::new(RequestTracker::default());
    let first = tracker.begin("same".into()).expect("first request");
    assert!(tracker.begin("same".into()).is_err());
    assert!(tracker.cancel("same"));
    assert!(first.cancelled.load(Ordering::SeqCst));

    drop(first);
    let replacement = tracker.begin("same".into()).expect("replacement request");
    assert!(tracker.cancel("same"));
    assert!(replacement.cancelled.load(Ordering::SeqCst));
    drop(replacement);
    assert!(!tracker.cancel("same"));
}

#[test]
fn backend_idles_only_after_the_last_request_finishes() {
    let tracker = Arc::new(RequestTracker::default());
    let first = tracker.begin("first".into()).expect("first request");
    let second = tracker.begin("second".into()).expect("second request");
    let old = Instant::now() - IDLE_TIMEOUT - Duration::from_secs(1);
    tracker.state.lock().expect("request state").last_activity = old;
    assert!(!tracker.begin_shutdown_if_idle(Instant::now()));
    drop(first);
    assert!(!tracker.begin_shutdown_if_idle(Instant::now()));
    drop(second);
    assert!(!tracker.begin_shutdown_if_idle(Instant::now()));
    tracker.state.lock().expect("request state").last_activity = old;
    let third = tracker
        .begin("third".into())
        .expect("new request resets idle");
    assert!(!tracker.begin_shutdown_if_idle(Instant::now()));
    drop(third);
    assert!(!tracker.begin_shutdown_if_idle(Instant::now()));
    tracker.state.lock().expect("request state").last_activity = old;
    assert!(tracker.begin_shutdown_if_idle(Instant::now()));
    assert!(tracker.begin("too-late".into()).is_err());
}

#[test]
fn untrusted_request_inputs_are_bounded() {
    let too_long = "x".repeat(MAX_STRING_BYTES + 1);
    assert!(
        validate_common_request(&too_long, None, None, &[]).is_err(),
        "titles must be bounded"
    );

    let choices = (0..=MAX_CHOICES)
        .map(|index| Choice::boolean(&format!("choice-{index}"), "Choice", false))
        .collect::<Vec<_>>();
    assert!(validate_choices(&choices).is_err());
    let choice = (0..=MAX_CHOICE_OPTIONS).fold(
        Choice::new("choice", "Choice", "option-0"),
        |choice, index| choice.insert(&format!("option-{index}"), "Option"),
    );
    assert!(validate_choices(&[choice]).is_err());

    let bulky = (0..=FILTER_RULE_WARNING_THRESHOLD)
        .fold(FileFilter::new("GitHub accepted types"), |filter, index| {
            filter.mimetype(&format!("application/x-attachment-{index}"))
        });
    assert!(validate_filters(&[bulky], None).is_ok());
    assert!(
        validate_filters(&[FileFilter::new("Filter").glob("*a*a*a*z")], None).is_err(),
        "backtracking-heavy globs must be rejected"
    );
    assert!(
        validate_filters(&[FileFilter::new("Filter").glob("*a*.txt")], None).is_ok(),
        "two wildcard groups remain supported"
    );
    assert!(
        validate_filters(
            &[FileFilter::new("Filter").glob(&"a".repeat(MAX_GLOB_BYTES + 1))],
            None,
        )
        .is_err(),
        "glob length must be bounded"
    );

    assert!(validate_path(Some(Path::new(&"x".repeat(MAX_STRING_BYTES + 1))), "path").is_err());
}

#[test]
fn active_request_count_is_bounded() {
    let tracker = Arc::new(RequestTracker::default());
    let requests = (0..MAX_ACTIVE_REQUESTS)
        .map(|index| {
            tracker
                .begin(format!("request{index}"))
                .expect("request within limit")
        })
        .collect::<Vec<_>>();
    assert!(tracker.begin("one-too-many".into()).is_err());
    drop(requests);
    assert!(tracker.begin("available-again".into()).is_ok());
}

fn run_async<T>(future: impl Future<Output = T>) -> T {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| context.block_on(future))
        .expect("test main context")
}

#[test]
fn long_filter_lists_are_accepted() {
    let filters = (0..256)
        .map(|index| {
            FileFilter::new(&format!("Filter {index}"))
                .mimetype(&format!("application/x-upload-{index}"))
        })
        .collect::<Vec<_>>();
    assert!(
        validate_filters(&filters, None).is_ok(),
        "a large well-formed filter list must open the chooser"
    );
    let current = FileFilter::new("Selected").mimetype("application/x-selected");
    assert!(
        validate_filters(&filters, Some(&current)).is_ok(),
        "a current filter outside a large list must be accepted"
    );
}
