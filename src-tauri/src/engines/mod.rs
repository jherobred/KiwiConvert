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
            if access_denied(&e) {
                let dir = final_path.parent().unwrap_or(final_path);
                bail!(
                    "Windows didn't let KiwiConvert save in {}. If Controlled folder access is on, \
                     allow KiwiConvert.exe and ffmpeg.exe from KiwiConvert's install folder in \
                     Windows Security > Virus & threat protection > Ransomware protection.",
                    dir.display()
                );
            }
            Err(e)
        }
    }
}

/// True when writing failed because Windows refused access, which is what Controlled folder
/// access does to apps it doesn't trust. FFmpeg reports it as text.
fn access_denied(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
            || cause.to_string().contains("Permission denied")
    })
}
