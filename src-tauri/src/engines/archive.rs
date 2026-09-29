//! ZIP, TAR, and GZIP archives, plus RAR extraction.
//! Every extraction path is checked so an archive can't write outside its folder.

use super::write_atomic;
use crate::jobs::Span;
use crate::naming;
use crate::registry::Fmt;
use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

/// Every file under `inputs` with the name it gets inside the archive.
fn collect(inputs: &[PathBuf]) -> Result<Vec<(PathBuf, String)>> {
    fn walk(path: &Path, name: String, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
        if path.is_dir() {
            out.push((path.to_path_buf(), format!("{name}/")));
            let mut entries: Vec<_> = std::fs::read_dir(path)?.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let child = e.path();
                // Don't follow links: they can point anywhere or loop forever.
                if e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                    continue;
                }
                walk(&child, format!("{name}/{}", e.file_name().to_string_lossy()), out)?;
            }
        } else if path.is_file() {
            out.push((path.to_path_buf(), name));
        }
        Ok(())
    }
    let mut out = Vec::new();
    for p in inputs {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        walk(p, name, &mut out)?;
    }
    Ok(out)
}

fn total_size(entries: &[(PathBuf, String)]) -> u64 {
    entries.iter().filter_map(|(p, _)| std::fs::metadata(p).ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

/// Copies with progress and cancellation checks.
fn copy_counted(span: &Span, mut from: impl Read, to: &mut impl Write, done: &mut u64, total: u64) -> Result<()> {
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&buf[..n])?;
        *done += n as u64;
        if total > 0 {
            span.progress(*done as f32 / total as f32);
        }
        span.check()?;
    }
}

pub fn create(span: &Span, inputs: &[PathBuf], fmt: Fmt, out: &Path) -> Result<PathBuf> {
    let entries = collect(inputs)?;
    if entries.is_empty() {
        bail!("There's nothing to archive.");
    }
    let total = total_size(&entries);
    let mut done = 0u64;
    write_atomic(out, |tmp| {
        let file = BufWriter::new(File::create(tmp)?);
        match fmt {
            Fmt::Zip => {
                let mut zip = zip::ZipWriter::new(file);
                for (path, name) in &entries {
                    let opts = zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated)
                        .large_file(std::fs::metadata(path).map(|m| m.len() > u32::MAX as u64).unwrap_or(false));
                    if name.ends_with('/') {
                        zip.add_directory(name.trim_end_matches('/'), opts)?;
                    } else {
                        zip.start_file(name.as_str(), opts)?;
                        copy_counted(span, BufReader::new(File::open(path)?), &mut zip, &mut done, total)?;
                    }
                }
                zip.finish()?.flush()?;
            }
            Fmt::Tar | Fmt::Tgz => {
                let writer: Box<dyn Write> = if fmt == Fmt::Tgz {
                    Box::new(GzEncoder::new(file, Compression::default()))
                } else {
                    Box::new(file)
                };
                let mut tar = tar::Builder::new(writer);
                tar.follow_symlinks(false);
                for (path, name) in &entries {
                    span.check()?;
                    if name.ends_with('/') {
                        tar.append_dir(name.trim_end_matches('/'), path)?;
                    } else {
                        tar.append_path_with_name(path, name)?;
                        done += std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                        if total > 0 {
                            span.progress(done as f32 / total as f32);
                        }
                    }
                }
                let mut inner = tar.into_inner()?;
                inner.flush()?;
            }
            _ => bail!("{} isn't an archive format.", fmt.label()),
        }
        Ok(())
    })
}

pub fn gzip(span: &Span, input: &Path, out_dir: Option<&Path>) -> Result<PathBuf> {
    if input.is_dir() {
        bail!("GZIP compresses single files. Use TAR.GZ for folders.");
    }
    let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = out_dir.map(Path::to_path_buf).or_else(|| input.parent().map(Path::to_path_buf)).unwrap_or_default();
    let out = naming::unique(&dir, &name, "gz");
    let total = std::fs::metadata(input)?.len();
    let mut done = 0;
    write_atomic(&out, |tmp| {
        let mut enc = GzEncoder::new(BufWriter::new(File::create(tmp)?), Compression::default());
        copy_counted(span, BufReader::new(File::open(input)?), &mut enc, &mut done, total)?;
        enc.finish()?.flush()?;
        Ok(())
    })
}

/// Joins an archive entry name onto `dest`, refusing absolute paths and `..`.
fn safe_join(dest: &Path, name: &str) -> Option<PathBuf> {
    let rel = Path::new(name);
    let mut out = dest.to_path_buf();
    for c in rel.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (out != dest).then_some(out)
}

fn archive_kind(path: &Path) -> &'static str {
    let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        "tgz"
    } else if name.ends_with(".tar") {
        "tar"
    } else if name.ends_with(".gz") {
        "gz"
    } else if name.ends_with(".rar") {
        "rar"
    } else {
        "zip"
    }
}

/// Unpacks into `dest` (created if needed).
pub fn extract_into(span: &Span, archive: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    match archive_kind(archive) {
        "zip" => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(archive)?)).context("This ZIP file is damaged")?;
            let total: u64 = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.size())).sum();
            let mut done = 0u64;
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i)?;
                let Some(name) = entry.enclosed_name() else { continue };
                let Some(target) = safe_join(dest, &name.to_string_lossy()) else { continue };
                if entry.is_dir() {
                    std::fs::create_dir_all(&target)?;
                    continue;
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out = BufWriter::new(File::create(&target)?);
                copy_counted(span, &mut entry, &mut out, &mut done, total)?;
                out.flush()?;
            }
        }
        "tar" | "tgz" => {
            let file = BufReader::new(File::open(archive)?);
            let reader: Box<dyn Read> = if archive_kind(archive) == "tgz" { Box::new(GzDecoder::new(file)) } else { Box::new(file) };
            let mut tar = tar::Archive::new(reader);
            tar.set_preserve_permissions(false);
            tar.set_unpack_xattrs(false);
            let total = std::fs::metadata(archive)?.len().max(1);
            let mut done = 0u64;
            for entry in tar.entries()? {
                span.check()?;
                let mut entry = entry?;
                done += entry.size();
                // `unpack_in` refuses paths that escape `dest`.
                entry.unpack_in(dest)?;
                span.progress((done as f32 / total as f32).min(0.99));
            }
        }
        "gz" => {
            let name = naming::stem(archive);
            let target = naming::unique(dest, &name, "");
            let total = std::fs::metadata(archive)?.len();
            let mut done = 0;
            let mut out = BufWriter::new(File::create(&target)?);
            copy_counted(span, GzDecoder::new(BufReader::new(File::open(archive)?)), &mut out, &mut done, total * 3)?;
            out.flush()?;
        }
        "rar" => {
            let mut open = unrar::Archive::new(archive)
                .open_for_processing()
                .map_err(|e| anyhow::anyhow!("This RAR file couldn't be opened: {e}"))?;
            let mut count = 0usize;
            while let Some(header) = open.read_header().map_err(|e| anyhow::anyhow!("RAR error: {e}"))? {
                span.check()?;
                let entry = header.entry();
                let name = entry.filename.to_string_lossy().replace('\\', "/");
                let target = safe_join(dest, &name);
                open = match target {
                    Some(t) if entry.is_file() => {
                        if let Some(parent) = t.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        header.extract_to(&t).map_err(|e| anyhow::anyhow!("RAR error: {e}"))?
                    }
                    Some(t) if entry.is_directory() => {
                        std::fs::create_dir_all(&t)?;
                        header.skip().map_err(|e| anyhow::anyhow!("RAR error: {e}"))?
                    }
                    _ => header.skip().map_err(|e| anyhow::anyhow!("RAR error: {e}"))?,
                };
                count += 1;
                span.progress(1.0 - 1.0 / (1.0 + count as f32 / 20.0));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// Extracts into a new folder named after the archive.
pub fn extract(span: &Span, archive: &Path, out_dir: Option<&Path>) -> Result<PathBuf> {
    let parent = out_dir.map(Path::to_path_buf).or_else(|| archive.parent().map(Path::to_path_buf)).unwrap_or_default();
    let dest = naming::unique(&parent, &naming::stem(archive), "");
    let partial = parent.join(format!(".~kiwi-{}", uuid::Uuid::new_v4().simple()));
    match extract_into(span, archive, &partial) {
        Ok(()) => {
            std::fs::rename(&partial, &dest)?;
            Ok(dest)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&partial);
            Err(e)
        }
    }
}

/// Converts one archive format into another through a temporary folder.
pub fn repack(span: &Span, archive: &Path, fmt: Fmt, out_dir: Option<&Path>) -> Result<PathBuf> {
    let tmp = std::env::temp_dir().join(format!("kiwi-repack-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        extract_into(&span.range(0.0, 0.5), archive, &tmp)?;
        let items: Vec<PathBuf> = std::fs::read_dir(&tmp)?.flatten().map(|e| e.path()).collect();
        let out = naming::output_for(archive, out_dir, "", fmt.ext());
        create(&span.range(0.5, 1.0), &items, fmt, &out)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_names_are_rejected() {
        let d = Path::new("C:/out");
        assert!(safe_join(d, "../evil.txt").is_none());
        assert!(safe_join(d, "C:/Windows/evil.txt").is_none());
        assert!(safe_join(d, "/etc/passwd").is_none());
        assert_eq!(safe_join(d, "a/./b.txt"), Some(PathBuf::from("C:/out/a/b.txt")));
    }
}
