//! Conversion and processing engines. Each works on local files only.

pub mod archive;
pub mod docs;
pub mod ffmpeg;
pub mod heif;
pub mod image;
pub mod media;
pub mod metadata;
pub mod pdf;
pub mod print;
pub mod qr;
pub mod subtitles;
pub mod trace;

#[cfg(test)]
mod matrix_tests;

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// Writes `final_path` through a hidden temporary sibling that is renamed into place when
/// `write` succeeds and removed when it fails, so a broken file never appears.
pub fn write_atomic(final_path: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<PathBuf> {
    if let Some(dir) = final_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = crate::naming::partial_path(final_path);
    match write(&tmp) {
        Ok(()) => {
            if !tmp.exists() {
                bail!("nothing was written");
            }
            Ok(crate::naming::finalize(&tmp, final_path)?)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}
