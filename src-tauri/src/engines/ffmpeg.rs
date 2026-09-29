//! Running the bundled FFmpeg: locating it, probing media, and running encodes with live
//! progress and cancellation.

use crate::jobs::{Cancelled, Span};
use anyhow::{Context, Result, anyhow, bail};
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    encoders: HashSet<String>,
}

static TOOLS: OnceLock<Option<Tools>> = OnceLock::new();

/// Finds FFmpeg next to the app (installed builds) or in `vendor/` (development).
pub fn init(resource_dir: Option<PathBuf>) {
    TOOLS.get_or_init(|| {
        let mut candidates = Vec::new();
        if let Some(dir) = resource_dir {
            candidates.push(dir.join("ffmpeg"));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("ffmpeg"));
            }
        }
        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vendor/ffmpeg"));
        let dir = candidates.into_iter().find(|d| d.join("ffmpeg.exe").is_file())?;
        let ffmpeg = dir.join("ffmpeg.exe");
        let encoders = list_encoders(&ffmpeg);
        log::info!("using FFmpeg at {} ({} encoders)", ffmpeg.display(), encoders.len());
        Some(Tools {
            ffprobe: dir.join("ffprobe.exe"),
            ffmpeg,
            encoders,
        })
    });
}

pub fn tools() -> Result<&'static Tools> {
    TOOLS
        .get()
        .and_then(|t| t.as_ref())
        .ok_or_else(|| anyhow!("FFmpeg is missing from this installation. Reinstall KiwiConvert to restore it."))
}

pub fn has_encoder(name: &str) -> bool {
    tools().map(|t| t.encoders.contains(name)).unwrap_or(false)
}

fn list_encoders(ffmpeg: &Path) -> HashSet<String> {
    let Ok(out) = command(ffmpeg).args(["-hide_banner", "-encoders"]).output() else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let flags = parts.next()?;
            // Encoder lines start with a 6 character capability column such as "V....D".
            (flags.len() == 6 && !flags.contains('=')).then(|| parts.next().map(str::to_string))?
        })
        .collect()
}

/// A process that never flashes a console window.
pub fn command(bin: &Path) -> Command {
    let mut c = Command::new(bin);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

// ---------------------------------------------------------------------------------------
// Probing
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Probe {
    #[serde(default)]
    pub format: ProbeFormat,
    #[serde(default)]
    pub streams: Vec<ProbeStream>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProbeFormat {
    pub duration: Option<String>,
    pub format_name: Option<String>,
    pub bit_rate: Option<String>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProbeStream {
    pub index: u32,
    #[serde(default)]
    pub codec_type: String,
    pub codec_name: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub avg_frame_rate: Option<String>,
    pub sample_rate: Option<String>,
    pub channels: Option<u32>,
    pub bits_per_raw_sample: Option<String>,
    pub sample_fmt: Option<String>,
    pub color_transfer: Option<String>,
    pub duration: Option<String>,
    #[serde(default)]
    pub disposition: HashMap<String, i32>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl ProbeStream {
    pub fn codec(&self) -> &str {
        self.codec_name.as_deref().unwrap_or("")
    }

    pub fn is_cover_art(&self) -> bool {
        self.disposition.get("attached_pic").copied().unwrap_or(0) == 1
    }

    pub fn fps(&self) -> f64 {
        let r = self.avg_frame_rate.as_deref().unwrap_or("0/1");
        let mut parts = r.split('/');
        let n: f64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let d: f64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        if d > 0.0 { n / d } else { 0.0 }
    }

    pub fn is_hdr(&self) -> bool {
        matches!(self.color_transfer.as_deref(), Some("smpte2084" | "arib-std-b67"))
    }
}

impl Probe {
    pub fn duration(&self) -> f64 {
        self.format
            .duration
            .as_deref()
            .and_then(|d| d.parse().ok())
            .or_else(|| {
                self.streams
                    .iter()
                    .filter_map(|s| s.duration.as_deref()?.parse::<f64>().ok())
                    .reduce(f64::max)
            })
            .unwrap_or(0.0)
    }

    /// The main video stream, ignoring embedded cover art.
    pub fn video(&self) -> Option<&ProbeStream> {
        self.streams
            .iter()
            .find(|s| s.codec_type == "video" && !s.is_cover_art())
    }

    pub fn audio(&self) -> Option<&ProbeStream> {
        self.streams.iter().find(|s| s.codec_type == "audio")
    }

    pub fn cover_art(&self) -> Option<&ProbeStream> {
        self.streams.iter().find(|s| s.is_cover_art())
    }

    pub fn subtitles(&self) -> impl Iterator<Item = &ProbeStream> {
        self.streams.iter().filter(|s| s.codec_type == "subtitle")
    }
}

pub fn probe(path: &Path) -> Result<Probe> {
    let t = tools()?;
    let out = command(&t.ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()
        .context("could not start ffprobe")?;
    if !out.status.success() {
        bail!("{}", friendly_error(&String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).context("could not read the media information")
}

// ---------------------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------------------

/// Runs FFmpeg with `args`, reporting progress against `duration` seconds.
/// Returns FFmpeg's stderr, which some filters (loudnorm) use for their results.
pub fn run(span: &Span, args: Vec<OsString>, duration: f64) -> Result<String> {
    let t = tools()?;
    let mut cmd = command(&t.ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-y", "-progress", "pipe:1", "-nostats"])
        .args(["-stats_period", "0.25"])
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    log::debug!("ffmpeg {:?}", args);
    let mut child = cmd.spawn().context("could not start FFmpeg")?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let child = Mutex::new(child);
    let log_lines = Mutex::new(VecDeque::<String>::new());
    let full_log = Mutex::new(String::new());
    let done = AtomicBool::new(false);
    let killed = AtomicBool::new(false);

    let status = std::thread::scope(|s| {
        // Collect stderr on its own thread so FFmpeg never blocks on a full pipe.
        s.spawn(|| {
            let mut reader = BufReader::new(stderr);
            let mut buf = Vec::new();
            while reader.read_until(b'\n', &mut buf).map(|n| n > 0).unwrap_or(false) {
                let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                buf.clear();
                let mut full = full_log.lock();
                if full.len() < 256 * 1024 {
                    full.push_str(&line);
                    full.push('\n');
                }
                let mut lines = log_lines.lock();
                lines.push_back(line);
                if lines.len() > 40 {
                    lines.pop_front();
                }
            }
        });

        // Kill FFmpeg promptly when the job is cancelled.
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if span.cancelled() {
                    killed.store(true, Ordering::Relaxed);
                    let _ = child.lock().kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });

        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(v) = line
                .strip_prefix("out_time_us=")
                .or_else(|| line.strip_prefix("out_time_ms="))
            {
                if let Ok(us) = v.trim().parse::<f64>() {
                    if duration > 0.0 {
                        span.progress((us / 1_000_000.0 / duration) as f32);
                    }
                }
            }
        }
        let status = child.lock().wait();
        done.store(true, Ordering::Relaxed);
        status
    });

    if killed.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    let status = status.context("FFmpeg stopped unexpectedly")?;
    let log = full_log.lock().clone();
    if !status.success() {
        let tail: Vec<String> = log_lines.lock().iter().cloned().collect();
        log::warn!("ffmpeg failed: {}", tail.join(" | "));
        bail!("{}", friendly_error(&tail.join("\n")));
    }
    span.progress(1.0);
    Ok(log)
}

/// Runs FFmpeg and returns its stdout bytes (for frames, thumbnails, and audio samples).
pub fn capture(args: Vec<OsString>) -> Result<Vec<u8>> {
    let t = tools()?;
    let mut child: Child = command(&t.ffmpeg)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error"])
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not start FFmpeg")?;
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let mut out = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout is piped")
        .read_to_end(&mut out)?;
    let status = child.wait()?;
    let err = err_thread.join().unwrap_or_default();
    if !status.success() && out.is_empty() {
        bail!("{}", friendly_error(&err));
    }
    Ok(out)
}

/// Turns FFmpeg's last error lines into a sentence a person can act on.
pub fn friendly_error(log: &str) -> String {
    let lower = log.to_ascii_lowercase();
    if lower.contains("invalid data found") || lower.contains("moov atom not found") {
        return "This file looks damaged, or it isn't a media file FFmpeg can read.".into();
    }
    if lower.contains("does not contain any stream") || lower.contains("output file is empty") {
        return "There was nothing to convert in this file.".into();
    }
    if lower.contains("matches no streams") {
        return "This file doesn't have the kind of track this needs (for example, no audio).".into();
    }
    if lower.contains("permission denied") {
        return "Windows didn't allow writing the output here. Try another folder in Settings.".into();
    }
    if lower.contains("no space left") {
        return "The disk is full.".into();
    }
    log.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| format!("FFmpeg: {l}"))
        .unwrap_or_else(|| "FFmpeg could not process this file.".into())
}

/// Convenience for building argument lists from string slices and paths.
#[macro_export]
macro_rules! args {
    ($($x:expr),* $(,)?) => {{
        let mut v: Vec<std::ffi::OsString> = Vec::new();
        $( $crate::engines::ffmpeg::push_arg(&mut v, $x); )*
        v
    }};
}

pub trait IntoArgs {
    fn push_into(self, v: &mut Vec<OsString>);
}

impl IntoArgs for &str {
    fn push_into(self, v: &mut Vec<OsString>) {
        v.push(self.into());
    }
}

impl IntoArgs for String {
    fn push_into(self, v: &mut Vec<OsString>) {
        v.push(self.into());
    }
}

impl IntoArgs for &String {
    fn push_into(self, v: &mut Vec<OsString>) {
        v.push(self.into());
    }
}

impl IntoArgs for &Path {
    fn push_into(self, v: &mut Vec<OsString>) {
        v.push(self.as_os_str().to_owned());
    }
}

impl IntoArgs for &PathBuf {
    fn push_into(self, v: &mut Vec<OsString>) {
        v.push(self.as_os_str().to_owned());
    }
}

impl IntoArgs for Vec<String> {
    fn push_into(self, v: &mut Vec<OsString>) {
        v.extend(self.into_iter().map(OsString::from));
    }
}

impl IntoArgs for &[&str] {
    fn push_into(self, v: &mut Vec<OsString>) {
        v.extend(self.iter().map(OsString::from));
    }
}

pub fn push_arg<T: IntoArgs>(v: &mut Vec<OsString>, x: T) {
    x.push_into(v);
}

/// Seconds as FFmpeg's `HH:MM:SS.mmm`.
pub fn ts(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let h = (s / 3600.0).floor();
    let m = ((s - h * 3600.0) / 60.0).floor();
    let sec = s - h * 3600.0 - m * 60.0;
    format!("{:02}:{:02}:{:06.3}", h as u64, m as u64, sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(ts(0.0), "00:00:00.000");
        assert_eq!(ts(83.5), "00:01:23.500");
        assert_eq!(ts(3723.25), "01:02:03.250");
    }

    #[test]
    fn friendly_errors() {
        assert!(friendly_error("[mov] moov atom not found").contains("damaged"));
        assert_eq!(friendly_error("\nfoo\nbar baz\n"), "FFmpeg: bar baz");
    }
}
