//! Commands the frontend can invoke. Anything slow runs on a blocking thread so the UI
//! thread never stalls.

use crate::engines::{ffmpeg, image as img, media, metadata, pdf};
use crate::jobs::{JobManager, JobOutcome, JobView};
use crate::registry::{Fmt, Mode, Tool, WheelOption};
use crate::settings::{Settings, SettingsStore};
use crate::ui::ToolSession;
use crate::wheel::WheelPayload;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State, WebviewWindow};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(err)?
        .map_err(|e| format!("{e:#}"))
}

/// Paths from the frontend must be absolute and exist.
fn checked(paths: &[String]) -> CmdResult<Vec<PathBuf>> {
    paths
        .iter()
        .map(|p| {
            let path = PathBuf::from(p);
            if path.is_absolute() && path.exists() {
                Ok(path)
            } else {
                Err(format!("File not found: {p}"))
            }
        })
        .collect()
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

// --- settings and app ---------------------------------------------------------------

#[tauri::command]
pub fn get_settings(store: State<SettingsStore>) -> Settings {
    store.get()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, store: State<SettingsStore>, settings: Settings) -> CmdResult<()> {
    store.set(&app, settings).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    ffmpeg: bool,
    pdf: bool,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        ffmpeg: ffmpeg::tools().is_ok(),
        pdf: pdf::page_infos(&[]).is_ok(),
    }
}

#[tauri::command]
pub fn set_paused(paused: bool) {
    crate::PAUSED.store(paused, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

// --- wheel ---------------------------------------------------------------------------

#[tauri::command]
pub fn wheel_snapshot() -> WheelPayload {
    crate::wheel::snapshot()
}

#[tauri::command]
pub fn wheel_choose(app: AppHandle, index: i32) {
    crate::wheel::choose(&app, index);
}

#[tauri::command]
pub fn wheel_close(app: AppHandle, reason: String) {
    crate::wheel::close(&app, &reason);
}

#[tauri::command]
pub fn wheel_hidden(app: AppHandle, generation: u64) {
    crate::wheel::hide_now(&app, generation);
}

#[tauri::command]
pub fn wheel_toggle_mode(app: AppHandle) {
    crate::wheel::toggle_mode(&app);
}

/// Opens the wheel for files chosen in the hub, centered on a screen point.
#[tauri::command]
pub fn open_wheel(app: AppHandle, paths: Vec<String>, x: i32, y: i32) -> CmdResult<()> {
    let paths = checked(&paths)?;
    for p in &paths {
        let _ = app.asset_protocol_scope().allow_file(p);
    }
    crate::wheel::open(&app, x, y, Mode::Convert, true, paths);
    Ok(())
}

#[tauri::command]
pub fn wheel_options(paths: Vec<String>, mode: Mode) -> Vec<WheelOption> {
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    crate::registry::wheel_options(&paths, mode)
}

/// The Explorer thumbnail of a file as a data URL.
#[tauri::command]
pub async fn thumbnail(path: String, size: u32) -> CmdResult<Option<String>> {
    blocking(move || {
        Ok(crate::platform::thumbnail_png(Path::new(&path), size.clamp(16, 512) as i32).map(|png| data_url("image/png", &png)))
    })
    .await
}

// --- jobs ----------------------------------------------------------------------------

#[tauri::command]
pub fn jobs_active(jobs: State<JobManager>) -> Vec<JobView> {
    jobs.active()
}

#[tauri::command]
pub fn jobs_history(jobs: State<JobManager>) -> Vec<JobView> {
    jobs.history()
}

#[tauri::command]
pub fn job_cancel(jobs: State<JobManager>, id: String) {
    jobs.cancel(&id);
}

#[tauri::command]
pub fn job_dismiss(jobs: State<JobManager>, id: String) {
    jobs.dismiss(&id);
}

#[tauri::command]
pub fn history_clear(app: AppHandle, jobs: State<JobManager>) {
    jobs.clear_history();
    let _ = tauri::Emitter::emit(&app, "history://changed", ());
}

#[tauri::command]
pub fn run_convert(app: AppHandle, paths: Vec<String>, to: Fmt) -> CmdResult<String> {
    Ok(crate::actions::start_convert(&app, checked(&paths)?, to))
}

#[tauri::command]
pub fn run_tool(app: AppHandle, tool: Tool, paths: Vec<String>, options: serde_json::Value) -> CmdResult<String> {
    Ok(crate::actions::start_tool(&app, tool, checked(&paths)?, options))
}

#[tauri::command]
pub fn open_tool(app: AppHandle, tool: Tool, paths: Vec<String>) -> CmdResult<()> {
    crate::ui::open_tool(&app, tool, checked(&paths)?);
    Ok(())
}

// --- windows -------------------------------------------------------------------------

#[tauri::command]
pub fn activity_resize(app: AppHandle, height: f64) {
    crate::ui::resize_activity(&app, height);
}

#[tauri::command]
pub fn activity_hide(app: AppHandle) {
    crate::ui::hide_activity(&app);
}

#[tauri::command]
pub fn hub_hide(app: AppHandle) {
    log::debug!("hub hidden by the frontend");
    if let Some(hub) = app.get_webview_window("hub") {
        let _ = hub.hide();
    }
}

/// Tool windows start hidden and call this once their first frame is painted.
#[tauri::command]
pub fn window_ready(window: WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

#[tauri::command]
pub fn tool_session(window: WebviewWindow) -> Option<ToolSession> {
    crate::ui::tool_session(window.label())
}

#[tauri::command]
pub fn tool_closed(window: WebviewWindow) {
    crate::ui::close_tool_session(window.label());
    let _ = window.destroy();
}

#[tauri::command]
pub fn reveal(paths: Vec<String>) {
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).filter(|p| p.exists()).collect();
    crate::platform::reveal(&paths);
}

#[tauri::command]
pub fn open_file(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("That file no longer exists.".into());
    }
    tauri_plugin_opener::open_path(p, None::<&str>).map_err(err)
}

/// Opens a web link from a QR code in the default browser. Only http(s) is allowed.
#[tauri::command]
pub fn open_link(url: String) -> CmdResult<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("Only web links can be opened.".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(err)
}

/// Lets a window load a local file through the asset protocol.
#[tauri::command]
pub fn allow_file(app: AppHandle, path: String) -> CmdResult<()> {
    let p = checked(&[path])?.remove(0);
    app.asset_protocol_scope().allow_file(&p).map_err(err)
}

// --- previews for the editors --------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    duration: f64,
    width: u32,
    height: u32,
    fps: f64,
    has_video: bool,
    has_audio: bool,
    video_codec: String,
    audio_codec: String,
    channels: u32,
}

#[tauri::command]
pub async fn media_info(path: String) -> CmdResult<MediaInfo> {
    blocking(move || {
        let p = ffmpeg::probe(Path::new(&path))?;
        let v = p.video();
        let a = p.audio();
        Ok(MediaInfo {
            duration: p.duration(),
            width: v.and_then(|v| v.width).unwrap_or(0),
            height: v.and_then(|v| v.height).unwrap_or(0),
            fps: v.map(|v| v.fps()).unwrap_or(0.0),
            has_video: v.is_some(),
            has_audio: a.is_some(),
            video_codec: v.map(|v| v.codec().to_string()).unwrap_or_default(),
            audio_codec: a.map(|a| a.codec().to_string()).unwrap_or_default(),
            channels: a.and_then(|a| a.channels).unwrap_or(0),
        })
    })
    .await
}

#[tauri::command]
pub async fn video_frame(path: String, time: f64, width: u32) -> CmdResult<String> {
    blocking(move || Ok(data_url("image/jpeg", &media::frame_jpeg(Path::new(&path), time, width.clamp(32, 1920))?))).await
}

#[tauri::command]
pub async fn video_strip(path: String, count: u32, width: u32) -> CmdResult<Vec<String>> {
    blocking(move || {
        let p = Path::new(&path);
        let duration = ffmpeg::probe(p)?.duration();
        let n = count.clamp(1, 40);
        (0..n)
            .map(|i| {
                let t = duration * (i as f64 + 0.5) / n as f64;
                media::frame_jpeg(p, t, width.clamp(32, 480)).map(|b| data_url("image/jpeg", &b))
            })
            .collect()
    })
    .await
}

#[derive(Serialize)]
pub struct Peaks {
    peaks: Vec<f32>,
    duration: f64,
}

#[tauri::command]
pub async fn audio_peaks(path: String, buckets: usize) -> CmdResult<Peaks> {
    blocking(move || {
        let (peaks, duration) = media::audio_peaks(Path::new(&path), buckets)?;
        Ok(Peaks { peaks, duration })
    })
    .await
}

/// A version of a media file the WebView can play. Returns its path, allowed for loading.
#[tauri::command]
pub async fn preview_proxy(app: AppHandle, path: String, video: bool) -> CmdResult<String> {
    let proxy = blocking(move || media::preview_proxy(Path::new(&path), video)).await?;
    app.asset_protocol_scope().allow_file(&proxy).map_err(err)?;
    Ok(proxy.to_string_lossy().into_owned())
}

#[derive(Serialize)]
pub struct ImagePreview {
    path: String,
    width: u32,
    height: u32,
}

/// Formats the WebView can't decode (HEIC, TIFF...) are converted to a temporary PNG.
#[tauri::command]
pub async fn image_preview(app: AppHandle, path: String) -> CmdResult<ImagePreview> {
    let preview = blocking(move || {
        let src = PathBuf::from(&path);
        let native = matches!(
            crate::registry::ext_of(&src).as_str(),
            "jpg" | "jpeg" | "jpe" | "jfif" | "png" | "webp" | "gif" | "bmp" | "svg" | "avif" | "ico"
        );
        if native {
            let (w, h) = image::image_dimensions(&src).unwrap_or((0, 0));
            return Ok(ImagePreview { path, width: w, height: h });
        }
        let loaded = img::load(&src)?;
        let dir = std::env::temp_dir().join("KiwiConvert").join("previews");
        std::fs::create_dir_all(&dir)?;
        let out = dir.join(format!("{}.png", uuid::Uuid::new_v4().simple()));
        let bytes = img::png_bytes(&loaded.img, &img::Meta { icc: None, exif: None }, false)?;
        std::fs::write(&out, bytes)?;
        Ok(ImagePreview {
            path: out.to_string_lossy().into_owned(),
            width: loaded.img.width(),
            height: loaded.img.height(),
        })
    })
    .await?;
    app.asset_protocol_scope().allow_file(&preview.path).map_err(err)?;
    Ok(preview)
}

#[tauri::command]
pub async fn pdf_pages(paths: Vec<String>) -> CmdResult<Vec<pdf::PageInfo>> {
    let paths = checked(&paths)?;
    blocking(move || pdf::page_infos(&paths)).await
}

#[tauri::command]
pub async fn pdf_thumb(path: String, index: usize, size: u32) -> CmdResult<String> {
    blocking(move || Ok(data_url("image/jpeg", &pdf::thumbnail(Path::new(&path), index, size.clamp(48, 800))?))).await
}

#[tauri::command]
pub async fn metadata_read(path: String) -> CmdResult<metadata::Report> {
    blocking(move || metadata::read(Path::new(&path))).await
}

#[tauri::command]
pub async fn compress_estimate(path: String, options: img::CompressOptions) -> CmdResult<u64> {
    blocking(move || img::estimate(Path::new(&path), &options)).await
}

#[tauri::command]
pub fn file_size(path: String) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

// --- saving images rendered in the editors -------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveMeta {
    /// The image the edit started from; the output goes beside it.
    source: String,
    /// Appended to the source name, e.g. "-edited". Ignored when `name` is set.
    suffix: Option<String>,
    /// A complete base name, used for collages.
    name: Option<String>,
    width: u32,
    height: u32,
    /// "keep" (the source format when possible), "png", "jpg", or "webp".
    format: Option<String>,
    title: Option<String>,
}

/// Receives raw RGBA pixels from an editor and saves them in the background.
#[tauri::command]
pub async fn save_rendered_image(app: AppHandle, request: tauri::ipc::Request<'_>) -> CmdResult<String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("expected raw pixel data".into());
    };
    let meta_header = request
        .headers()
        .get("x-kiwi-meta")
        .and_then(|v| v.to_str().ok())
        .ok_or("missing image description")?;
    let meta: SaveMeta = serde_json::from_str(&urlencoding_decode(meta_header)).map_err(err)?;
    if data.len() != (meta.width as usize) * (meta.height as usize) * 4 {
        return Err("pixel data doesn't match the image size".into());
    }
    let source = checked(&[meta.source.clone()])?.remove(0);
    let pixels = data.clone();
    let title = meta.title.clone().unwrap_or_else(|| "Saving image".into());
    let manager = app.state::<JobManager>();
    let id = manager.spawn(&app, title, "Saving".into(), Some(source.clone()), false, move |ctx| {
        let settings = ctx.app().state::<SettingsStore>().get();
        let rgba = image::RgbaImage::from_raw(meta.width, meta.height, pixels).ok_or_else(|| anyhow::anyhow!("bad pixels"))?;
        let opaque = rgba.pixels().all(|p| p[3] == 255);
        let image = if opaque {
            image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(rgba).to_rgb8())
        } else {
            image::DynamicImage::ImageRgba8(rgba)
        };
        ctx.progress(0.3);
        let src_fmt = Fmt::from_ext(&crate::registry::ext_of(&source));
        let fmt = match meta.format.as_deref() {
            Some("png") => Fmt::Png,
            Some("jpg") => Fmt::Jpg,
            Some("webp") => Fmt::Webp,
            _ => match src_fmt {
                Some(f @ (Fmt::Jpg | Fmt::Png | Fmt::Webp | Fmt::Heic | Fmt::Avif | Fmt::Tiff | Fmt::Bmp)) => {
                    // Transparent edits can't go into JPEG.
                    if !opaque && matches!(f, Fmt::Jpg | Fmt::Bmp | Fmt::Heic) { Fmt::Png } else { f }
                }
                _ => Fmt::Png,
            },
        };
        // Keep the source's EXIF and color profile; the pixels are already upright.
        let (icc, exif) = source_meta(&source);
        let loaded = img::Loaded { img: image, icc, exif };
        let dir = crate::convert::out_dir(&settings, &source)
            .or_else(|| source.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let out = match &meta.name {
            Some(name) => crate::naming::unique(&dir, name, fmt.ext()),
            None => crate::naming::output_for(&source, Some(&dir), meta.suffix.as_deref().unwrap_or("-edited"), fmt.ext()),
        };
        let quality = img::quality_for(fmt, &settings).max(90);
        let saved = img::save(&loaded, fmt, quality, settings.keep_metadata, &out)?;
        Ok(JobOutcome::files(vec![saved]))
    });
    Ok(id)
}

/// The color profile and EXIF of an image file, without decoding its pixels.
fn source_meta(path: &Path) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    use image::ImageDecoder;
    if matches!(crate::registry::ext_of(path).as_str(), "heic" | "heif" | "hif" | "avif") {
        if let Ok(meta) = std::fs::read(path).map_err(anyhow::Error::from).and_then(|d| crate::engines::heif::read_meta(&d)) {
            let exif = meta.exif.map(|mut e| {
                img::reset_orientation(&mut e);
                e
            });
            return (meta.icc, exif);
        }
        return (None, None);
    }
    let Ok(reader) = image::ImageReader::open(path).and_then(|r| r.with_guessed_format()) else { return (None, None) };
    let Ok(mut decoder) = reader.into_decoder() else { return (None, None) };
    let icc = decoder.icc_profile().ok().flatten();
    let exif = decoder.exif_metadata().ok().flatten().map(|mut e| {
        img::reset_orientation(&mut e);
        e
    });
    (icc, exif)
}

/// Headers are ASCII, so the frontend percent-encodes the JSON description.
fn urlencoding_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_decoding() {
        assert_eq!(super::urlencoding_decode("%7B%22a%22%3A1%7D"), "{\"a\":1}");
        assert_eq!(super::urlencoding_decode("caf%C3%A9"), "café");
        assert_eq!(super::urlencoding_decode("100%"), "100%");
    }
}
