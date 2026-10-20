// SPDX-License-Identifier: MIT

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

#[test]
fn open_selection_validates_kind_cardinality_and_locality() {
    let current = Location::local("/tmp");
    let file = entry(Path::new("/tmp/file"), false);
    let folder = entry(Path::new("/tmp/folder"), true);
    assert_eq!(
        open_selection(std::slice::from_ref(&folder), &current).expect("single folder"),
        PathBuf::from("/tmp/folder")
    );
    assert!(open_selection(std::slice::from_ref(&file), &current).is_err());
    assert!(open_selection(&[folder.clone(), folder.clone()], &current).is_err());
    assert_eq!(
        open_selection(&[], &current).expect("current folder"),
        PathBuf::from("/tmp")
    );
    let remote = FileEntry {
        location: Location::uri("smb://server/share/file"),
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        ..folder
    };
    assert!(open_selection(&[remote], &current).is_err());
}
