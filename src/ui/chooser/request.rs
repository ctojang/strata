// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::model::{FileEntry, Location};

pub(crate) const CHOOSER_APPLICATION_ID: &str = "io.github.lgse.Strata.FileChooser";

pub(crate) fn open_selection(
    entries: &[FileEntry],
    current: &Location,
) -> Result<PathBuf, &'static str> {
    let path = match entries {
        [] => {
            return current
                .native_path()
                .map(Path::to_path_buf)
                .ok_or("Choose a local folder");
        }
        [entry] if !entry.is_directory() => return Err("Choose folders only"),
        [entry] => entry.location.native_path(),
        _ => return Err("Choose one item"),
    };
    path.map(Path::to_path_buf).ok_or("Choose local items only")
}
