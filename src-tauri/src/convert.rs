//! Format conversion: decides which engine handles a file and target format.

use crate::engines::{archive, docs, image, media, pdf, subtitles};
use crate::jobs::{Ctx, JobOutcome, Span};
use crate::naming;
use crate::registry::{Fmt, Kind, is_animated_gif, kind_of};
use crate::settings::Settings;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Where outputs go: the folder chosen in Settings, or beside the original when that is
/// writable.
pub fn out_dir(settings: &Settings, input: &Path) -> Option<PathBuf> {
    if let Some(dir) = settings.output_folder.as_deref().filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    let parent = input.parent()?;
    if naming::dir_writable(parent) {
        None
    } else {
        // Read-only folders (a camera card, Program Files): fall back to Downloads.
        dirs_downloads()
    }
}

fn dirs_downloads() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads")).filter(|p| p.is_dir())
}

pub fn run(ctx: &Ctx, inputs: &[PathBuf], to: Fmt) -> Result<JobOutcome> {
    let settings = ctx.app().state::<crate::settings::SettingsStore>().get();
    let span = ctx.span();

    // Several inputs into one archive.
    if matches!(to, Fmt::Zip | Fmt::Tar | Fmt::Tgz) && (inputs.len() > 1 || !matches!(kind_of(&inputs[0]), Kind::Archive)) {
        let first = &inputs[0];
        let dir = out_dir(&settings, first).or_else(|| first.parent().map(Path::to_path_buf)).unwrap_or_default();
        let base = naming::combined_base(inputs, "Archive");
        let out = naming::unique(&dir, &base, to.ext());
        return Ok(JobOutcome::files(vec![archive::create(&span, inputs, to, &out)?]));
    }

    let n = inputs.len();
    let mut outputs = Vec::new();
    let mut failures = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        ctx.check()?;
        if n > 1 {
            ctx.stage(format!("{} of {n}", i + 1));
        }
        let part = span.part(i, n);
        let dir = out_dir(&settings, input);
        match convert_one(&part, input, to, &settings, dir.as_deref()) {
            Ok(mut out) => outputs.append(&mut out),
            Err(e) if e.is::<crate::jobs::Cancelled>() => return Err(e),
            Err(e) if n == 1 => return Err(e),
            Err(e) => failures.push(format!("{}: {e:#}", input.file_name().unwrap_or_default().to_string_lossy())),
        }
    }
    if outputs.is_empty() && !failures.is_empty() {
        bail!("{}", failures.join("\n"));
    }
    let text = (!failures.is_empty()).then(|| format!("Some files were skipped:\n{}", failures.join("\n")));
    Ok(JobOutcome { outputs, text })
}

pub fn convert_one(span: &Span, input: &Path, to: Fmt, settings: &Settings, dir: Option<&Path>) -> Result<Vec<PathBuf>> {
    let kind = kind_of(input);
    let one = |r: Result<PathBuf>| r.map(|p| vec![p]);
    match (kind, to) {
        (Kind::Image, Fmt::Pdf) => {
            let out = naming::output_for(input, dir, "", "pdf");
            one(pdf::images_to_pdf(span, &[input.to_path_buf()], settings, &out))
        }
        (Kind::Image, Fmt::Docx) => one(docs::image_to_docx(span, input, settings, dir)),
        (Kind::Image, Fmt::Mp4 | Fmt::Webm) | (Kind::Image, Fmt::Webp) if is_animated_gif(input) => {
            one(media::convert(span, input, to, settings, dir))
        }
        (Kind::Image, _) => one(image::convert(span, input, to, settings, dir)),
        (Kind::Video | Kind::Audio, _) => one(media::convert(span, input, to, settings, dir)),
        (Kind::Pdf, Fmt::Png | Fmt::Jpg | Fmt::Webp | Fmt::Tiff) => pdf::to_images(span, input, to, settings, dir),
        (Kind::Pdf, Fmt::Txt) => one(pdf::to_text(span, input, dir)),
        (Kind::Pdf, Fmt::Docx) => one(docs::pdf_to_docx(span, input, settings, dir)),
        (Kind::Docx, Fmt::Pdf) => one(docs::docx_to_pdf(span, input, settings, dir)),
        (Kind::Docx, Fmt::Txt) => one(docs::docx_to_text(span, input, dir)),
        (Kind::Text, Fmt::Pdf) => one(docs::text_to_pdf(span, input, settings, dir)),
        (Kind::Text, Fmt::Docx) => one(docs::text_to_docx(span, input, settings, dir)),
        (Kind::Subtitle, _) => one(subtitles::convert(span, input, to, dir)),
        (Kind::Archive, Fmt::Extract) => one(archive::extract(span, input, dir)),
        (Kind::Archive, Fmt::Zip | Fmt::Tar | Fmt::Tgz) => one(archive::repack(span, input, to, dir)),
        (_, Fmt::Gz) => one(archive::gzip(span, input, dir)),
        _ => bail!("KiwiConvert can't convert this file to {}.", to.label()),
    }
}
