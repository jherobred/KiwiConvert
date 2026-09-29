//! Tool jobs: compress, trim, merge and the rest. Options arrive as JSON from the tool
//! windows (or are empty for tools started straight from the wheel).

use crate::convert::out_dir;
use crate::engines::{image, media, metadata, pdf, qr};
use crate::jobs::{Ctx, JobOutcome};
use crate::naming;
use crate::registry::{Kind, Tool, kind_of};
use anyhow::{Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::Manager;

fn opts<T: DeserializeOwned + Default>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

pub fn describe(tool: Tool, _options: &Value) -> String {
    match tool {
        Tool::Compress => "Compressing",
        Tool::Resize => "Resizing",
        Tool::Crop => "Cropping",
        Tool::Adjust | Tool::Annotate | Tool::Redact => "Saving edits",
        Tool::Metadata => "Updating metadata",
        Tool::ReadQr => "Reading QR code",
        Tool::MakePdf => "Making a PDF",
        Tool::Collage => "Making a collage",
        Tool::Trim => "Trimming",
        Tool::Speed => "Changing speed",
        Tool::Split => "Splitting",
        Tool::Join => "Joining",
        Tool::Snapshot => "Saving snapshot",
        Tool::Normalize => "Normalizing loudness",
        Tool::Bleep => "Bleeping",
        Tool::Channels => "Changing channels",
        Tool::MergePdf => "Merging PDFs",
        Tool::SplitPdf => "Splitting PDF",
        Tool::OrganizePdf => "Saving pages",
    }
    .to_string()
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct TrimOptions {
    start: f64,
    end: f64,
    precise: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SpeedOptions {
    factor: f64,
    keep_pitch: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct PointsOptions {
    points: Vec<f64>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ChannelOptions {
    mode: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct MetadataOptions {
    edits: BTreeMap<String, String>,
    mode: Option<metadata::Mode>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SplitPdfOptions {
    /// 1-based inclusive ranges. Empty means one file per page.
    ranges: Vec<(usize, usize)>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct OrganizeOptions {
    pages: Vec<pdf::PageRef>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct CropOptions {
    rect: Option<media::CropRect>,
}

/// Runs `f` for every input, collecting outputs and continuing past per-file failures.
fn each(ctx: &Ctx, inputs: &[PathBuf], mut f: impl FnMut(&crate::jobs::Span, &Path, Option<PathBuf>) -> Result<Vec<PathBuf>>) -> Result<JobOutcome> {
    let settings = ctx.app().state::<crate::settings::SettingsStore>().get();
    let span = ctx.span();
    let n = inputs.len();
    let mut outputs = Vec::new();
    let mut failures = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        ctx.check()?;
        if n > 1 {
            ctx.stage(format!("{} of {n}", i + 1));
        }
        match f(&span.part(i, n), input, out_dir(&settings, input)) {
            Ok(mut out) => outputs.append(&mut out),
            Err(e) if e.is::<crate::jobs::Cancelled>() || n == 1 => return Err(e),
            Err(e) => failures.push(format!("{}: {e:#}", input.file_name().unwrap_or_default().to_string_lossy())),
        }
    }
    if outputs.is_empty() && !failures.is_empty() {
        bail!("{}", failures.join("\n"));
    }
    let text = (!failures.is_empty()).then(|| format!("Some files were skipped:\n{}", failures.join("\n")));
    Ok(JobOutcome { outputs, text })
}

pub fn run(ctx: &Ctx, tool: Tool, inputs: &[PathBuf], options: &Value) -> Result<JobOutcome> {
    let settings = ctx.app().state::<crate::settings::SettingsStore>().get();
    let first = inputs.first().cloned().unwrap_or_default();
    let first_dir = out_dir(&settings, &first).or_else(|| first.parent().map(Path::to_path_buf)).unwrap_or_default();
    match tool {
        Tool::Compress => each(ctx, inputs, |span, input, dir| {
            let dir = dir.as_deref();
            Ok(vec![match kind_of(input) {
                Kind::Image => image::compress(span, input, &opts(options), &settings, dir)?,
                Kind::Video => media::compress_video(span, input, &opts(options), dir)?,
                Kind::Audio => media::compress_audio(span, input, &opts(options), dir)?,
                Kind::Pdf => pdf::compress(span, input, &opts(options), dir)?,
                _ => bail!("KiwiConvert can't compress this file type."),
            }])
        }),
        Tool::Resize => each(ctx, inputs, |span, input, dir| Ok(vec![image::resize(span, input, &opts(options), &settings, dir.as_deref())?])),
        Tool::Crop => {
            let o: CropOptions = opts(options);
            let Some(rect) = o.rect else { bail!("Choose an area to keep first.") };
            each(ctx, inputs, |span, input, dir| Ok(vec![media::crop(span, input, rect, &settings, dir.as_deref())?]))
        }
        Tool::Trim => {
            let o: TrimOptions = opts(options);
            each(ctx, inputs, |span, input, dir| Ok(vec![media::trim(span, input, o.start, o.end, o.precise, &settings, dir.as_deref())?]))
        }
        Tool::Speed => {
            let o: SpeedOptions = opts(options);
            let factor = if o.factor > 0.0 { o.factor } else { 2.0 };
            each(ctx, inputs, |span, input, dir| Ok(vec![media::speed(span, input, factor, o.keep_pitch, &settings, dir.as_deref())?]))
        }
        Tool::Split => {
            let o: PointsOptions = opts(options);
            each(ctx, inputs, |span, input, dir| media::split(span, input, &o.points, dir.as_deref()))
        }
        Tool::Snapshot => {
            let o: PointsOptions = opts(options);
            let times = if o.points.is_empty() { vec![0.0] } else { o.points };
            each(ctx, inputs, |span, input, dir| {
                times
                    .iter()
                    .enumerate()
                    .map(|(i, t)| media::snapshot(&span.part(i, times.len()), input, *t, dir.as_deref()))
                    .collect()
            })
        }
        Tool::Join => {
            let mut sorted = inputs.to_vec();
            if !options.get("keepOrder").and_then(Value::as_bool).unwrap_or(false) {
                sorted.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));
            }
            let dir = out_dir(&settings, &first);
            Ok(JobOutcome::files(vec![media::join(&ctx.span(), &sorted, &settings, dir.as_deref())?]))
        }
        Tool::Normalize => each(ctx, inputs, |span, input, dir| Ok(vec![media::normalize(span, input, &opts(options), &settings, dir.as_deref())?])),
        Tool::Bleep => each(ctx, inputs, |span, input, dir| Ok(vec![media::bleep(span, input, &opts(options), &settings, dir.as_deref())?])),
        Tool::Channels => {
            let o: ChannelOptions = opts(options);
            each(ctx, inputs, |span, input, dir| Ok(vec![media::channels(span, input, &o.mode, &settings, dir.as_deref())?]))
        }
        Tool::Metadata => {
            let o: MetadataOptions = opts(options);
            let mode = o.mode.unwrap_or(metadata::Mode::Edit);
            each(ctx, inputs, |span, input, dir| Ok(vec![metadata::apply(span, input, &o.edits, mode, dir.as_deref())?]))
        }
        Tool::ReadQr => {
            let mut lines = Vec::new();
            for input in inputs {
                ctx.check()?;
                lines.extend(qr::read(input)?);
            }
            if lines.is_empty() {
                bail!("No QR code was found in this image.");
            }
            Ok(JobOutcome { outputs: vec![], text: Some(lines.join("\n")) })
        }
        Tool::MakePdf => {
            let mut sorted = inputs.to_vec();
            sorted.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));
            let out = naming::unique(&first_dir, &naming::combined_base(&sorted, "Images"), "pdf");
            Ok(JobOutcome::files(vec![pdf::images_to_pdf(&ctx.span(), &sorted, &settings, &out)?]))
        }
        Tool::MergePdf => {
            let dir = out_dir(&settings, &first);
            Ok(JobOutcome::files(vec![pdf::merge(&ctx.span(), inputs, dir.as_deref())?]))
        }
        Tool::SplitPdf => {
            let o: SplitPdfOptions = opts(options);
            each(ctx, inputs, |span, input, dir| pdf::split(span, input, &o.ranges, dir.as_deref()))
        }
        Tool::OrganizePdf => {
            let o: OrganizeOptions = opts(options);
            let base = format!("{} (organized)", naming::combined_base(inputs, "Pages"));
            let out = naming::unique(&first_dir, &base, "pdf");
            ctx.progress(0.2);
            Ok(JobOutcome::files(vec![pdf::assemble(inputs, &o.pages, &out)?]))
        }
        Tool::Adjust | Tool::Annotate | Tool::Redact | Tool::Collage => {
            bail!("This tool saves from its editor window.")
        }
    }
}
