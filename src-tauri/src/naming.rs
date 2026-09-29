//! Output file naming. Converted copies are saved beside the original, never over it.

use std::path::{Path, PathBuf};

/// File name without the (possibly double) extension: `a.tar.gz` -> `a`, `photo.jpg` -> `photo`.
pub fn stem(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let lower = name.to_ascii_lowercase();
    for double in [".tar.gz", ".tar.bz2", ".tar.xz"] {
        if lower.ends_with(double) && name.len() > double.len() {
            return name[..name.len() - double.len()].to_string();
        }
    }
    if path.is_dir() {
        return name;
    }
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name,
    }
}

/// Returns `dir/base.ext`, or `dir/base (1).ext`, `dir/base (2).ext`... if the name is taken.
pub fn unique(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let make = |n: usize| {
        let name = match (n, ext.is_empty()) {
            (0, true) => base.to_string(),
            (0, false) => format!("{base}.{ext}"),
            (n, true) => format!("{base} ({n})"),
            (n, false) => format!("{base} ({n}).{ext}"),
        };
        dir.join(name)
    };
    (0..)
        .map(make)
        .find(|p| !p.exists())
        .expect("an unused name always exists")
}

/// Output path for a converted or processed copy of `input`.
/// `suffix` is appended to the stem, e.g. `-compressed`.
pub fn output_for(input: &Path, out_dir: Option<&Path>, suffix: &str, ext: &str) -> PathBuf {
    let dir = out_dir
        .map(Path::to_path_buf)
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    unique(&dir, &format!("{}{}", stem(input), suffix), ext)
}

/// Name for an output that combines several inputs, e.g. merged PDFs.
pub fn combined_base(inputs: &[PathBuf], fallback: &str) -> String {
    match inputs {
        [] => fallback.to_string(),
        [one] => stem(one),
        [first, rest @ ..] => format!("{} and {} more", stem(first), rest.len()),
    }
}

/// Splits a file name into base and extension, keeping `.tar.gz` together.
fn split_name(name: &str) -> (String, String) {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") && name.len() > 7 {
        return (name[..name.len() - 7].to_string(), name[name.len() - 6..].to_string());
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i + 1..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// A temporary sibling path used while writing, renamed into place when complete so a
/// half-written file never appears under its final name.
pub fn partial_path(final_path: &Path) -> PathBuf {
    let dir = final_path.parent().unwrap_or(Path::new("."));
    let name = final_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (_, ext) = split_name(&name);
    let suffix = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    dir.join(format!(".~kiwi-{}{}", uuid::Uuid::new_v4().simple(), suffix))
}

/// Moves a finished partial file to its final name, picking a new unique name if the final
/// name was taken while the job ran.
pub fn finalize(partial: &Path, final_path: &Path) -> std::io::Result<PathBuf> {
    let target = if final_path.exists() {
        let dir = final_path.parent().unwrap_or(Path::new("."));
        let name = final_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (base, ext) = split_name(&name);
        unique(dir, &base, &ext)
    } else {
        final_path.to_path_buf()
    };
    std::fs::rename(partial, &target)?;
    Ok(target)
}

/// True when a file can be created in `dir`.
pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".~kiwi-probe-{}", uuid::Uuid::new_v4().simple()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems() {
        assert_eq!(stem(Path::new("C:/a/photo.jpg")), "photo");
        assert_eq!(stem(Path::new("C:/a/pack.tar.gz")), "pack");
        assert_eq!(stem(Path::new("C:/a/.hidden")), ".hidden");
        assert_eq!(stem(Path::new("C:/a/my.report.v2.pdf")), "my.report.v2");
    }

    #[test]
    fn unique_names_do_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("kiwi-naming-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique(&dir, "photo", "webp");
        assert_eq!(first.file_name().unwrap(), "photo.webp");
        std::fs::write(&first, b"x").unwrap();
        let second = unique(&dir, "photo", "webp");
        assert_eq!(second.file_name().unwrap(), "photo (1).webp");
        std::fs::write(&second, b"x").unwrap();
        assert_eq!(unique(&dir, "photo", "webp").file_name().unwrap(), "photo (2).webp");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finalize_moves_partial_into_place() {
        let dir = std::env::temp_dir().join(format!("kiwi-final-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("clip.mp4");
        let partial = partial_path(&target);
        assert!(partial.file_name().unwrap().to_string_lossy().ends_with(".mp4"));
        std::fs::write(&partial, b"data").unwrap();
        std::fs::write(&target, b"taken").unwrap();
        let done = finalize(&partial, &target).unwrap();
        assert_eq!(done.file_name().unwrap(), "clip (1).mp4");
        assert!(!partial.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn combined_names() {
        let v = vec![PathBuf::from("a/one.pdf"), PathBuf::from("a/two.pdf"), PathBuf::from("a/3.pdf")];
        assert_eq!(combined_base(&v, "Merged"), "one and 2 more");
        assert_eq!(combined_base(&v[..1], "Merged"), "one");
    }
}
