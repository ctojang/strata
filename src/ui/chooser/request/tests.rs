// SPDX-License-Identifier: MIT

use std::{future::Future, os::unix::ffi::OsStringExt as _};

use super::*;
use crate::model::{EntryKind, MetadataValue};

fn entry(path: &Path, directory: bool) -> FileEntry {
    FileEntry {
        location: Location::local(path),
        native_name: path.file_name().unwrap_or_default().to_owned(),
        thumbnail_path: None,
        display_name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        kind: if directory {
            EntryKind::Directory
        } else {
            EntryKind::File
        },
        recent_unix_seconds: MetadataValue::Unknown,
        is_hidden: false,
        mode: MetadataValue::Unknown,
        size: MetadataValue::Unknown,
        modified_unix_seconds: MetadataValue::Unknown,
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        recent_uri: None,
    }
}

fn run_async<T>(future: impl Future<Output = T>) -> T {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| context.block_on(future))
        .expect("test main context")
}

#[test]
fn local_uri_preserves_non_utf8_path_bytes() {
    let path = PathBuf::from("/tmp").join(OsString::from_vec(vec![b'n', 0xff]));
    let uri = local_uri(&path).expect("local URI");
    assert!(uri.as_str().starts_with("file://"));
    assert!(uri.as_str().to_ascii_uppercase().contains("%FF"));
}

#[test]
fn local_uri_uses_the_file_scheme() {
    for path in [Path::new("/tmp/a"), Path::new("/tmp/a b")] {
        assert!(
            local_uri(path)
                .expect("valid URI")
                .as_str()
                .starts_with("file://")
        );
    }
}

#[test]
fn save_filenames_must_be_safe_basenames() {
    for unsafe_name in [
        OsStr::new(""),
        OsStr::new("."),
        OsStr::new(".."),
        OsStr::new("a/b"),
    ] {
        assert!(!safe_filename(unsafe_name), "{unsafe_name:?}");
    }
    assert!(safe_filename(OsStr::new("report.txt")));
    assert!(safe_filename(&OsString::from_vec(vec![b'n', 0xff])));
    assert!(validate_save_filenames(&[]).is_err());
    assert!(validate_save_filenames(&[OsString::from("../bad")]).is_err());
    let filenames = vec![OsString::from("file"); MAX_SAVE_FILES + 1];
    assert!(validate_save_filenames(&filenames).is_err());
    assert!(
        validate_save_filenames(&[OsString::from("x".repeat(MAX_FILENAME_BYTES + 1))]).is_err()
    );
}

#[test]
fn save_files_preserve_order_and_report_collisions_once() {
    let folder = tempfile::tempdir().expect("destination");
    std::fs::write(folder.path().join("second"), b"existing").expect("collision");
    let names = vec![OsString::from("first"), OsString::from("second")];
    let checked = run_async(check_destinations(folder.path(), &names)).expect("safe destinations");
    assert_eq!(
        checked.paths,
        vec![folder.path().join("first"), folder.path().join("second")]
    );
    assert!(checked.existing_files);
}

#[test]
fn save_files_block_directory_collisions() {
    let folder = tempfile::tempdir().expect("destination");
    std::fs::create_dir(folder.path().join("reserved")).expect("collision directory");
    assert!(
        run_async(check_destinations(
            folder.path(),
            &[OsString::from("reserved")],
        ))
        .expect_err("directory collision")
        .contains("folder")
    );
}

#[test]
fn open_selection_validates_kind_cardinality_and_locality() {
    let current = Location::local("/tmp");
    let file = entry(Path::new("/tmp/file"), false);
    let folder = entry(Path::new("/tmp/folder"), true);
    assert_eq!(
        open_selection(std::slice::from_ref(&file), &current, false, false).expect("single file"),
        [PathBuf::from("/tmp/file")]
    );
    assert!(open_selection(&[file.clone(), folder.clone()], &current, false, true).is_err());
    assert!(open_selection(&[file.clone(), file], &current, false, false).is_err());
    assert_eq!(
        open_selection(&[], &current, true, false).expect("current folder"),
        [PathBuf::from("/tmp")]
    );
    let remote = FileEntry {
        location: Location::uri("smb://server/share/file"),
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        ..folder
    };
    assert!(open_selection(&[remote], &current, true, false).is_err());
}
