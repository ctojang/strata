// SPDX-License-Identifier: MIT

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use super::compression::{compress_7z, compress_tar, compress_zip, inspect_archive_sources};
use crate::services::ArchiveFormat;

pub fn write_compression_fixture(
    path: &Path,
    entries: &[PathBuf],
    format: ArchiveFormat,
    password: Option<&str>,
) -> Result<usize, String> {
    // The filesystem seam asserts it runs off the main thread.
    if glib::MainContext::default().is_owner() {
        return std::thread::scope(|scope| {
            scope
                .spawn(|| write_compression_fixture(path, entries, format, password))
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        });
    }
    let file = fs::File::create(path).map_err(|error| error.to_string())?;
    let progress = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(AtomicBool::new(false));
    match format {
        ArchiveFormat::Zip => compress_zip(file, entries, password, &progress, &cancelled),
        ArchiveFormat::SevenZ => compress_7z(file, entries, password, &progress, &cancelled),
        ArchiveFormat::Tar => compress_tar(file, entries, None, &progress, &cancelled),
        ArchiveFormat::TarGz => compress_tar(
            file,
            entries,
            Some(
                inspect_archive_sources(entries, &cancelled)
                    .map_err(|error| error.to_string())?
                    .gzip_level(),
            ),
            &progress,
            &cancelled,
        ),
        ArchiveFormat::Rar => return Err("RAR compression is not supported".to_owned()),
    }
    .map_err(|error| error.to_string())?;
    Ok(progress.load(Ordering::Relaxed))
}
