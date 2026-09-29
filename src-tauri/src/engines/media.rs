//! Video and audio conversion and tools, all through FFmpeg.

use super::ffmpeg::{self, Probe, ProbeStream, has_encoder, ts};
use super::write_atomic;
use crate::args;
use crate::jobs::Span;
use crate::naming;
use crate::registry::Fmt;
use crate::settings::Settings;
use anyhow::{Result, bail};
use serde::Deserialize;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn muxer(fmt: Fmt) -> &'static str {
    match fmt {
        Fmt::Mp4 => "mp4",
        Fmt::Mov => "mov",
        Fmt::Mkv => "matroska",
        Fmt::Webm => "webm",
        Fmt::Avi => "avi",
        Fmt::Wmv => "asf",
        Fmt::Gif => "gif",
        Fmt::Mp3 => "mp3",
        Fmt::M4a => "ipod",
        Fmt::Wav => "wav",
        Fmt::Flac => "flac",
        Fmt::Ogg => "ogg",
        Fmt::Opus => "opus",
        Fmt::Aiff => "aiff",
        Fmt::Srt => "srt",
        Fmt::Vtt => "webvtt",
        Fmt::Webp => "webp",
        _ => "",
    }
}

fn fmt_of(path: &Path) -> Option<Fmt> {
    Fmt::from_ext(&crate::registry::ext_of(path))
}

fn source_bits(a: &ProbeStream) -> u32 {
    a.bits_per_raw_sample
        .as_deref()
        .and_then(|b| b.parse().ok())
        .filter(|b| *b > 0)
        .unwrap_or_else(|| match a.sample_fmt.as_deref() {
            Some("s32" | "s32p" | "flt" | "fltp" | "dbl" | "dblp") => 24,
            _ => 16,
        })
}

fn audio_codec(to: Fmt, a: &ProbeStream) -> Vec<String> {
    let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let deep = source_bits(a) > 16;
    match to {
        Fmt::Mp3 => v(&["-c:a", "libmp3lame", "-q:a", "2"]),
        Fmt::M4a => v(&["-c:a", "aac", "-b:a", "256k"]),
        Fmt::Wav if deep => v(&["-c:a", "pcm_s24le"]),
        Fmt::Wav => v(&["-c:a", "pcm_s16le"]),
        Fmt::Flac => v(&["-c:a", "flac", "-compression_level", "8"]),
        Fmt::Ogg => v(&["-c:a", "libvorbis", "-q:a", "6"]),
        Fmt::Opus => v(&["-c:a", "libopus", "-b:a", "160k"]),
        Fmt::Aiff if deep => v(&["-c:a", "pcm_s24be"]),
        Fmt::Aiff => v(&["-c:a", "pcm_s16be"]),
        _ => vec![],
    }
}

/// x264 speed and quality for the "video preset" setting.
fn h264_args(settings: &Settings, crf_override: Option<u32>) -> Vec<String> {
    let (preset, crf) = match settings.video_preset.as_str() {
        "fast" => ("veryfast", 21),
        "quality" => ("slow", 18),
        _ => ("faster", 20),
    };
    let crf = crf_override.unwrap_or(crf);
    if has_encoder("libx264") {
        ["-c:v", "libx264", "-preset", preset, "-crf", &crf.to_string(), "-pix_fmt", "yuv420p"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        // Windows' built-in encoder, always present, lower quality per bit.
        ["-c:v", "h264_mf", "-b:v", "8M", "-pix_fmt", "yuv420p"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

/// Filters every transcode needs: HDR to SDR tone mapping and even frame sizes.
fn base_filters(v: &ProbeStream) -> Vec<String> {
    let mut vf = Vec::new();
    if v.is_hdr() {
        vf.push(
            "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p"
                .to_string(),
        );
    }
    let odd = v.width.unwrap_or(0) % 2 == 1 || v.height.unwrap_or(0) % 2 == 1;
    if odd {
        vf.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".into());
    }
    vf
}

fn video_codec(to: Fmt, settings: &Settings, v: &ProbeStream) -> Vec<String> {
    let s = |x: &[&str]| x.iter().map(|v| v.to_string()).collect::<Vec<_>>();
    match to {
        Fmt::Webm => {
            let cpu = if settings.video_preset == "quality" { "2" } else { "5" };
            let mut a = s(&["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-row-mt", "1", "-deadline", "good"]);
            a.extend(s(&["-cpu-used", cpu, "-pix_fmt", "yuv420p"]));
            a
        }
        Fmt::Avi => s(&["-c:v", "mpeg4", "-vtag", "xvid", "-q:v", "3"]),
        Fmt::Wmv => {
            let px = v.width.unwrap_or(1280) as f64 * v.height.unwrap_or(720) as f64;
            let fps = v.fps().clamp(1.0, 120.0);
            let kbps = (px * fps * 0.12 / 1000.0).clamp(1500.0, 16000.0) as u32;
            s(&["-c:v", "wmv2", "-b:v", &format!("{kbps}k")])
        }
        _ => h264_args(settings, None),
    }
}

fn audio_for_video(to: Fmt) -> Vec<String> {
    let s = |x: &[&str]| x.iter().map(|v| v.to_string()).collect::<Vec<_>>();
    match to {
        Fmt::Webm => s(&["-c:a", "libopus", "-b:a", "128k"]),
        Fmt::Avi => s(&["-c:a", "libmp3lame", "-q:a", "3"]),
        Fmt::Wmv => s(&["-c:a", "wmav2", "-b:a", "192k"]),
        _ => s(&["-c:a", "aac", "-b:a", "192k"]),
    }
}

/// True when the streams can be copied into `to` without re-encoding.
fn can_remux(p: &Probe, to: Fmt) -> bool {
    let Some(v) = p.video() else { return false };
    let vc = v.codec();
    let ac = p.audio().map(|a| a.codec());
    let v_ok = match to {
        Fmt::Mp4 => matches!(vc, "h264" | "hevc" | "av1" | "mpeg4"),
        Fmt::Mov => matches!(vc, "h264" | "hevc" | "mpeg4" | "prores"),
        Fmt::Mkv => true,
        Fmt::Webm => matches!(vc, "vp8" | "vp9" | "av1"),
        _ => false,
    };
    let a_ok = match to {
        Fmt::Mp4 | Fmt::Mov => matches!(ac, None | Some("aac" | "mp3" | "alac" | "ac3" | "eac3")),
        Fmt::Mkv => true,
        Fmt::Webm => matches!(ac, None | Some("opus" | "vorbis")),
        _ => false,
    };
    v_ok && a_ok
}

fn text_subtitle(s: &ProbeStream) -> bool {
    matches!(s.codec(), "subrip" | "ass" | "ssa" | "mov_text" | "webvtt" | "text")
}

/// Converts a video or audio file to `to` and returns the new file.
pub fn convert(span: &Span, input: &Path, to: Fmt, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    let dur = p.duration();
    let out = naming::output_for(input, out_dir, "", to.ext());
    match to {
        Fmt::Mp3 | Fmt::M4a | Fmt::Wav | Fmt::Flac | Fmt::Ogg | Fmt::Opus | Fmt::Aiff => {
            let Some(a) = p.audio() else {
                bail!("This file has no audio to convert.");
            };
            write_atomic(&out, |tmp| {
                let mut v = args!["-i", input, "-map", "0:a:0"];
                let cover = p.cover_art().filter(|_| matches!(to, Fmt::Mp3 | Fmt::M4a | Fmt::Flac));
                match cover {
                    Some(c) => v.extend(args![
                        "-map",
                        format!("0:{}", c.index),
                        "-c:v",
                        "copy",
                        "-disposition:v:0",
                        "attached_pic"
                    ]),
                    None => v.extend(args!["-vn"]),
                }
                v.extend(args![audio_codec(to, a), "-map_metadata", "0"]);
                if to == Fmt::Mp3 {
                    v.extend(args!["-id3v2_version", "3"]);
                }
                v.extend(args!["-f", muxer(to), tmp]);
                ffmpeg::run(span, v, dur).map(|_| ())
            })
        }
        Fmt::Gif => to_gif(span, input, &p, &out),
        Fmt::Webp => {
            // Animated GIF to animated WebP.
            write_atomic(&out, |tmp| {
                let codec = if has_encoder("libwebp_anim") { "libwebp_anim" } else { "libwebp" };
                let v = args!["-i", input, "-c:v", codec, "-lossless", "0", "-quality", "82", "-loop", "0", "-f", "webp", tmp];
                ffmpeg::run(span, v, dur).map(|_| ())
            })
        }
        Fmt::Mp4 | Fmt::Mov | Fmt::Mkv | Fmt::Webm | Fmt::Avi | Fmt::Wmv => {
            if p.video().is_none() {
                bail!("This file has no video track.");
            }
            if can_remux(&p, to) {
                let remuxed = write_atomic(&out, |tmp| {
                    let mut v = args!["-i", input];
                    if to == Fmt::Mkv {
                        v.extend(args!["-map", "0", "-c", "copy"]);
                    } else {
                        v.extend(args!["-map", "0:v:0", "-map", "0:a?", "-c", "copy"]);
                        if p.video().map(|s| s.codec()) == Some("hevc") {
                            v.extend(args!["-tag:v", "hvc1"]);
                        }
                    }
                    if matches!(to, Fmt::Mp4 | Fmt::Mov) {
                        v.extend(args!["-movflags", "+faststart"]);
                    }
                    v.extend(args!["-f", muxer(to), tmp]);
                    ffmpeg::run(span, v, dur).map(|_| ())
                });
                match remuxed {
                    Ok(done) => return Ok(done),
                    Err(e) if e.is::<crate::jobs::Cancelled>() => return Err(e),
                    Err(e) => log::info!("remux failed, transcoding instead: {e:#}"),
                }
            }
            transcode(span, input, &p, to, settings, &out)
        }
        _ => bail!("KiwiConvert can't turn this file into {}.", to.label()),
    }
}

fn transcode(span: &Span, input: &Path, p: &Probe, to: Fmt, settings: &Settings, out: &Path) -> Result<PathBuf> {
    let v = p.video().expect("checked by caller");
    write_atomic(out, |tmp| {
        let mut a = args!["-i", input, "-map", "0:v:0", "-map", "0:a?"];
        let vf = base_filters(v);
        if !vf.is_empty() {
            a.extend(args!["-vf", vf.join(",")]);
        }
        a.extend(args![video_codec(to, settings, v), audio_for_video(to)]);
        match to {
            Fmt::Mp4 | Fmt::Mov => {
                let text_subs: Vec<u32> = p.subtitles().filter(|s| text_subtitle(s)).map(|s| s.index).collect();
                for i in &text_subs {
                    a.extend(args!["-map", format!("0:{i}")]);
                }
                if !text_subs.is_empty() {
                    a.extend(args!["-c:s", "mov_text"]);
                }
                a.extend(args!["-movflags", "+faststart"]);
            }
            Fmt::Mkv => {
                let text_subs: Vec<u32> = p.subtitles().filter(|s| text_subtitle(s)).map(|s| s.index).collect();
                for i in &text_subs {
                    a.extend(args!["-map", format!("0:{i}")]);
                }
                if !text_subs.is_empty() {
                    a.extend(args!["-c:s", "srt"]);
                }
            }
            _ => {}
        }
        a.extend(args!["-f", muxer(to), tmp]);
        ffmpeg::run(span, a, p.duration()).map(|_| ())
    })
}

fn to_gif(span: &Span, input: &Path, p: &Probe, out: &Path) -> Result<PathBuf> {
    let dur = p.duration();
    let (fps, width) = if dur > 30.0 {
        (8, 360)
    } else if dur > 10.0 {
        (10, 420)
    } else {
        (15, 480)
    };
    let filter = format!(
        "fps={fps},scale='min({width},iw)':-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=256:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle"
    );
    write_atomic(out, |tmp| {
        let v = args!["-i", input, "-filter_complex", filter, "-loop", "0", "-f", "gif", tmp];
        ffmpeg::run(span, v, dur).map(|_| ())
    })
}

// ---------------------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompressOptions {
    /// "small", "balanced", or "high".
    pub level: String,
    /// Target size in megabytes. Overrides `level` when set.
    pub target_mb: Option<f64>,
    /// Longest side limit for video, in pixels.
    pub max_height: Option<u32>,
    /// Audio bitrate for audio-only files.
    pub audio_kbps: Option<u32>,
}

fn scale_filter(v: &ProbeStream, max_h: Option<u32>) -> Option<String> {
    let max_h = max_h?;
    let h = v.height.unwrap_or(0);
    (h > max_h).then(|| format!("scale=-2:{max_h}:flags=lanczos"))
}

pub fn compress_video(span: &Span, input: &Path, o: &CompressOptions, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    let Some(v) = p.video() else { bail!("This file has no video track.") };
    let dur = p.duration();
    let keep = matches!(fmt_of(input), Some(Fmt::Mp4 | Fmt::Mov | Fmt::Mkv));
    let to = if keep { fmt_of(input).unwrap_or(Fmt::Mp4) } else { Fmt::Mp4 };
    let out = naming::output_for(input, out_dir, "-compressed", to.ext());
    let mut vf = base_filters(v);

    if let Some(mb) = o.target_mb.filter(|mb| *mb > 0.0) {
        if dur <= 0.0 {
            bail!("Couldn't read this video's length, so an exact size isn't possible.");
        }
        let total_kbps = mb * 8.0 * 1024.0 * 1024.0 / 1000.0 / dur * 0.97;
        let audio_kbps = if p.audio().is_some() { (total_kbps * 0.12).clamp(32.0, 128.0) } else { 0.0 };
        let video_kbps = total_kbps - audio_kbps;
        if video_kbps < 40.0 {
            bail!("{mb} MB is too small for a video this long. Try a larger size.");
        }
        // Shrink the frame when the bitrate can't carry the original resolution.
        let h = v.height.unwrap_or(1080);
        let auto_h = if video_kbps < 400.0 { Some(360) } else if video_kbps < 900.0 { Some(480) } else if video_kbps < 2000.0 { Some(720) } else { None };
        let limit = o.max_height.or(auto_h).filter(|m| *m < h);
        if let Some(f) = scale_filter(v, limit) {
            vf.push(f);
        }
        let dir = std::env::temp_dir().join(format!("kiwi-2pass-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir)?;
        let log = dir.join("pass");
        let result = write_atomic(&out, |tmp| {
            let common = |pass: &str| {
                let mut a = args!["-i", input, "-map", "0:v:0"];
                if !vf.is_empty() {
                    a.extend(args!["-vf", vf.join(",")]);
                }
                a.extend(args![
                    "-c:v", "libx264", "-preset", "medium", "-b:v", format!("{}k", video_kbps as u32),
                    "-pix_fmt", "yuv420p", "-pass", pass, "-passlogfile", &log
                ]);
                a
            };
            let mut first = common("1");
            first.extend(args!["-an", "-f", "null", "NUL"]);
            ffmpeg::run(&span.range(0.0, 0.5), first, dur)?;
            let mut second = common("2");
            if p.audio().is_some() {
                second.extend(args!["-map", "0:a:0", "-c:a", "aac", "-b:a", format!("{}k", audio_kbps as u32)]);
            }
            if matches!(to, Fmt::Mp4 | Fmt::Mov) {
                second.extend(args!["-movflags", "+faststart"]);
            }
            second.extend(args!["-f", muxer(to), tmp]);
            ffmpeg::run(&span.range(0.5, 1.0), second, dur).map(|_| ())
        });
        let _ = std::fs::remove_dir_all(&dir);
        return result;
    }

    let crf = match o.level.as_str() {
        "small" => 30,
        "high" => 21,
        _ => 26,
    };
    if let Some(f) = scale_filter(v, o.max_height) {
        vf.push(f);
    }
    write_atomic(&out, |tmp| {
        let mut a = args!["-i", input, "-map", "0:v:0", "-map", "0:a?"];
        if !vf.is_empty() {
            a.extend(args!["-vf", vf.join(",")]);
        }
        let x264 = has_encoder("libx264");
        if x264 {
            a.extend(args!["-c:v", "libx264", "-preset", "medium", "-crf", crf.to_string(), "-pix_fmt", "yuv420p"]);
        } else {
            a.extend(args![h264_args(&Settings::default(), Some(crf))]);
        }
        let akbps = if o.level == "small" { "96k" } else { "128k" };
        a.extend(args!["-c:a", "aac", "-b:a", akbps]);
        if matches!(to, Fmt::Mp4 | Fmt::Mov) {
            a.extend(args!["-movflags", "+faststart"]);
        }
        a.extend(args!["-f", muxer(to), tmp]);
        ffmpeg::run(span, a, dur).map(|_| ())
    })
}

pub fn compress_audio(span: &Span, input: &Path, o: &CompressOptions, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    let Some(_) = p.audio() else { bail!("This file has no audio track.") };
    let dur = p.duration();
    let to = match fmt_of(input) {
        Some(f @ (Fmt::M4a | Fmt::Ogg | Fmt::Opus)) => f,
        _ => Fmt::Mp3,
    };
    let kbps = if let Some(mb) = o.target_mb.filter(|mb| *mb > 0.0) {
        if dur <= 0.0 {
            bail!("Couldn't read this file's length, so an exact size isn't possible.");
        }
        let k = mb * 8.0 * 1024.0 * 1024.0 / 1000.0 / dur * 0.97;
        if k < 16.0 {
            bail!("{mb} MB is too small for audio this long. Try a larger size.");
        }
        (k as u32).min(320)
    } else {
        o.audio_kbps.unwrap_or(match o.level.as_str() {
            "small" => 64,
            "high" => 192,
            _ => 128,
        })
    };
    let out = naming::output_for(input, out_dir, "-compressed", to.ext());
    write_atomic(&out, |tmp| {
        let codec = match to {
            Fmt::M4a => "aac",
            Fmt::Ogg => "libvorbis",
            Fmt::Opus => "libopus",
            _ => "libmp3lame",
        };
        let mut a = args!["-i", input, "-map", "0:a:0", "-vn", "-c:a", codec, "-b:a", format!("{kbps}k"), "-map_metadata", "0"];
        if kbps < 64 && to == Fmt::Mp3 {
            a.extend(args!["-ar", "22050"]);
        }
        a.extend(args!["-f", muxer(to), tmp]);
        ffmpeg::run(span, a, dur).map(|_| ())
    })
}

/// Encoder arguments that keep a file in its own container when it has to be re-encoded.
struct Codecs {
    to: Fmt,
    /// Video encoder and muxer flags. Empty for audio-only files.
    video: Vec<OsString>,
    audio: Vec<OsString>,
}

fn same_container(input: &Path, p: &Probe, settings: &Settings) -> Codecs {
    let to = fmt_of(input).unwrap_or(Fmt::Mp4);
    if let Some(v) = p.video() {
        let to = if matches!(to, Fmt::Mp4 | Fmt::Mov | Fmt::Mkv | Fmt::Webm | Fmt::Avi | Fmt::Wmv) { to } else { Fmt::Mp4 };
        let mut video = args![video_codec(to, settings, v)];
        if matches!(to, Fmt::Mp4 | Fmt::Mov) {
            video.extend(args!["-movflags", "+faststart"]);
        }
        Codecs { to, video, audio: args![audio_for_video(to)] }
    } else {
        let to = if matches!(to, Fmt::Mp3 | Fmt::M4a | Fmt::Wav | Fmt::Flac | Fmt::Ogg | Fmt::Opus | Fmt::Aiff) { to } else { Fmt::M4a };
        let audio = p.audio().map(|a| args![audio_codec(to, a)]).unwrap_or_default();
        Codecs { to, video: vec![], audio }
    }
}

/// Keeps `start..end` seconds. Stream copy is instant but snaps to keyframes; `precise`
/// re-encodes for frame accuracy.
pub fn trim(span: &Span, input: &Path, start: f64, end: f64, precise: bool, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    if end <= start {
        bail!("The end of the selection must come after its start.");
    }
    let c = same_container(input, &p, settings);
    let to = c.to;
    let out = naming::output_for(input, out_dir, "-trimmed", to.ext());
    write_atomic(&out, |tmp| {
        let mut a = args!["-ss", ts(start), "-to", ts(end), "-i", input, "-map", "0:v:0?", "-map", "0:a?"];
        if precise {
            if let Some(v) = p.video() {
                let vf = base_filters(v);
                if !vf.is_empty() {
                    a.extend(args!["-vf", vf.join(",")]);
                }
            }
            a.extend(c.video.iter().cloned());
            a.extend(c.audio.iter().cloned());
        } else {
            a.extend(args!["-c", "copy", "-avoid_negative_ts", "make_zero"]);
        }
        a.extend(args!["-map_metadata", "0", "-f", muxer(to), tmp]);
        ffmpeg::run(span, a, end - start).map(|_| ())
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub fn crop(span: &Span, input: &Path, r: CropRect, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    let Some(v) = p.video() else { bail!("This file has no video track.") };
    let c = same_container(input, &p, settings);
    let to = c.to;
    let out = naming::output_for(input, out_dir, "-cropped", to.ext());
    let w = (r.w / 2 * 2).max(2);
    let h = (r.h / 2 * 2).max(2);
    let mut vf = Vec::new();
    if v.is_hdr() {
        vf.extend(base_filters(v));
    }
    vf.push(format!("crop={w}:{h}:{}:{}", r.x, r.y));
    write_atomic(&out, |tmp| {
        let mut a = args!["-i", input, "-map", "0:v:0", "-map", "0:a?", "-vf", vf.join(",")];
        a.extend(c.video.iter().cloned());
        a.extend(args!["-c:a", "copy", "-map_metadata", "0", "-f", muxer(to), tmp]);
        ffmpeg::run(span, a, p.duration()).map(|_| ())
    })
}

fn atempo_chain(factor: f64) -> String {
    let mut f = factor;
    let mut parts = Vec::new();
    while f > 2.0 {
        parts.push("atempo=2.0".to_string());
        f /= 2.0;
    }
    while f < 0.5 {
        parts.push("atempo=0.5".to_string());
        f /= 0.5;
    }
    parts.push(format!("atempo={f:.6}"));
    parts.join(",")
}

pub fn speed(span: &Span, input: &Path, factor: f64, keep_pitch: bool, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    if !(0.1..=16.0).contains(&factor) {
        bail!("Speed must be between 0.1x and 16x.");
    }
    let p = ffmpeg::probe(input)?;
    let c = same_container(input, &p, settings);
    let to = c.to;
    let label = format!("-{factor}x");
    let out = naming::output_for(input, out_dir, &label, to.ext());
    let audio_filter = match p.audio() {
        Some(a) if !keep_pitch => {
            let sr = a.sample_rate.as_deref().and_then(|s| s.parse::<f64>().ok()).unwrap_or(48000.0);
            Some(format!("asetrate={},aresample={}", (sr * factor).round(), sr))
        }
        Some(_) => Some(atempo_chain(factor)),
        None => None,
    };
    write_atomic(&out, |tmp| {
        let mut a = args!["-i", input];
        if let Some(v) = p.video() {
            let mut vf = base_filters(v);
            vf.push(format!("setpts=PTS/{factor}"));
            a.extend(args!["-map", "0:v:0", "-vf", vf.join(",")]);
        }
        if let Some(af) = &audio_filter {
            a.extend(args!["-map", "0:a:0", "-af", af]);
        }
        a.extend(c.video.iter().cloned());
        a.extend(c.audio.iter().cloned());
        a.extend(args!["-map_metadata", "0", "-f", muxer(to), tmp]);
        ffmpeg::run(span, a, p.duration() / factor).map(|_| ())
    })
}

/// Cuts the file at each point in `points` (seconds), producing one file per piece.
pub fn split(span: &Span, input: &Path, points: &[f64], out_dir: Option<&Path>) -> Result<Vec<PathBuf>> {
    let p = ffmpeg::probe(input)?;
    let dur = p.duration();
    let mut cuts: Vec<f64> = points.iter().copied().filter(|t| *t > 0.05 && *t < dur - 0.05).collect();
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    cuts.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    if cuts.is_empty() {
        bail!("Add at least one split point inside the clip.");
    }
    let mut bounds = vec![0.0];
    bounds.extend(cuts);
    bounds.push(dur);
    let to = fmt_of(input).unwrap_or(Fmt::Mp4);
    let n = bounds.len() - 1;
    let mut outputs = Vec::new();
    for i in 0..n {
        span.check()?;
        let (s, e) = (bounds[i], bounds[i + 1]);
        let out = naming::output_for(input, out_dir, &format!("-part{}", i + 1), to.ext());
        let piece = span.part(i, n);
        outputs.push(write_atomic(&out, |tmp| {
            let a = args![
                "-ss", ts(s), "-to", ts(e), "-i", input, "-map", "0", "-c", "copy",
                "-avoid_negative_ts", "make_zero", "-map_metadata", "0", "-f", muxer(to), tmp
            ];
            ffmpeg::run(&piece, a, e - s).map(|_| ())
        })?);
    }
    Ok(outputs)
}

/// Joins clips end to end. Identical formats are joined without re-encoding.
pub fn join(span: &Span, inputs: &[PathBuf], settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let probes: Vec<Probe> = inputs.iter().map(|p| ffmpeg::probe(p)).collect::<Result<_>>()?;
    let total: f64 = probes.iter().map(|p| p.duration()).sum();
    let first = &inputs[0];
    let to = fmt_of(first).unwrap_or(Fmt::Mp4);
    let base = naming::combined_base(inputs, "Joined");
    let dir = out_dir.map(Path::to_path_buf).or_else(|| first.parent().map(Path::to_path_buf)).unwrap_or_default();
    let out = naming::unique(&dir, &format!("{base} (joined)"), to.ext());
    let has_video = probes[0].video().is_some();

    let signature = |p: &Probe| {
        let v = p.video().map(|v| (v.codec().to_string(), v.width, v.height));
        let a = p.audio().map(|a| (a.codec().to_string(), a.sample_rate.clone(), a.channels));
        (v, a)
    };
    let same = probes.iter().all(|p| signature(p) == signature(&probes[0]));

    if same {
        let list = std::env::temp_dir().join(format!("kiwi-join-{}.txt", uuid::Uuid::new_v4().simple()));
        let body: String = inputs
            .iter()
            .map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\'', "'\\''")))
            .collect();
        std::fs::write(&list, body)?;
        let r = write_atomic(&out, |tmp| {
            let a = args!["-f", "concat", "-safe", "0", "-i", &list, "-map", "0", "-c", "copy", "-f", muxer(to), tmp];
            ffmpeg::run(span, a, total).map(|_| ())
        });
        let _ = std::fs::remove_file(&list);
        match r {
            Ok(p) => return Ok(p),
            Err(e) if e.is::<crate::jobs::Cancelled>() => return Err(e),
            Err(e) => log::info!("lossless join failed, re-encoding: {e:#}"),
        }
    }

    // Re-encode through the concat filter. Every clip is fitted into the first clip's frame.
    let mut a: Vec<OsString> = Vec::new();
    for p in inputs {
        a.extend(args!["-i", p]);
    }
    let mut graph = String::new();
    let n = inputs.len();
    if has_video {
        let v0 = probes[0].video().expect("has video");
        let (w, h) = (v0.width.unwrap_or(1280) / 2 * 2, v0.height.unwrap_or(720) / 2 * 2);
        let fps = v0.fps().clamp(1.0, 120.0);
        for (i, p) in probes.iter().enumerate() {
            graph.push_str(&format!(
                "[{i}:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps:.3},format=yuv420p[v{i}];"
            ));
            if p.audio().is_some() {
                graph.push_str(&format!("[{i}:a:0]aformat=sample_rates=48000:channel_layouts=stereo[a{i}];"));
            } else {
                graph.push_str(&format!(
                    "anullsrc=r=48000:cl=stereo,atrim=duration={:.3}[a{i}];",
                    p.duration()
                ));
            }
        }
        for i in 0..n {
            graph.push_str(&format!("[v{i}][a{i}]"));
        }
        graph.push_str(&format!("concat=n={n}:v=1:a=1[v][a]"));
        let to = if matches!(to, Fmt::Mp4 | Fmt::Mov | Fmt::Mkv | Fmt::Webm) { to } else { Fmt::Mp4 };
        let out = naming::unique(&dir, &format!("{base} (joined)"), to.ext());
        write_atomic(&out, |tmp| {
            a.extend(args!["-filter_complex", &graph, "-map", "[v]", "-map", "[a]"]);
            a.extend(args![video_codec(to, settings, v0), audio_for_video(to)]);
            if matches!(to, Fmt::Mp4 | Fmt::Mov) {
                a.extend(args!["-movflags", "+faststart"]);
            }
            a.extend(args!["-f", muxer(to), tmp]);
            ffmpeg::run(span, a, total).map(|_| ())
        })
    } else {
        for (i, p) in probes.iter().enumerate() {
            if p.audio().is_none() {
                bail!("{} has no audio.", inputs[i].display());
            }
            graph.push_str(&format!("[{i}:a:0]aformat=sample_rates=48000:channel_layouts=stereo[a{i}];"));
        }
        for i in 0..n {
            graph.push_str(&format!("[a{i}]"));
        }
        graph.push_str(&format!("concat=n={n}:v=0:a=1[a]"));
        let a0 = probes[0].audio().expect("checked");
        write_atomic(&out, |tmp| {
            a.extend(args!["-filter_complex", &graph, "-map", "[a]", audio_codec(to, a0), "-f", muxer(to), tmp]);
            ffmpeg::run(span, a, total).map(|_| ())
        })
    }
}

/// Saves the frame at `t` seconds as a PNG beside the video.
pub fn snapshot(span: &Span, input: &Path, t: f64, out_dir: Option<&Path>) -> Result<PathBuf> {
    let secs = t.max(0.0);
    let label = format!("-{:02}m{:02}s", (secs / 60.0) as u64, (secs % 60.0) as u64);
    let out = naming::output_for(input, out_dir, &label, "png");
    write_atomic(&out, |tmp| {
        let a = args!["-ss", ts(secs), "-i", input, "-frames:v", "1", "-update", "1", "-f", "image2", "-c:v", "png", tmp];
        ffmpeg::run(span, a, 0.0).map(|_| ())
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NormalizeOptions {
    /// Integrated loudness target in LUFS. Streaming services use about -14.
    pub target: Option<f64>,
}

pub fn normalize(span: &Span, input: &Path, o: &NormalizeOptions, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    if p.audio().is_none() {
        bail!("This file has no audio to normalize.");
    }
    let target = o.target.unwrap_or(-14.0).clamp(-40.0, -5.0);
    let dur = p.duration();
    let spec = format!("loudnorm=I={target}:TP=-1.0:LRA=11");
    let log = ffmpeg::run(
        &span.range(0.0, 0.5),
        args!["-i", input, "-map", "0:a:0", "-af", format!("{spec}:print_format=json"), "-f", "null", "NUL"],
        dur,
    )?;
    let json_start = log.rfind('{').unwrap_or(0);
    let json_end = log.rfind('}').map(|i| i + 1).unwrap_or(log.len());
    let m: serde_json::Value = serde_json::from_str(&log[json_start..json_end]).unwrap_or_default();
    let get = |k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("0").to_string();
    let second = format!(
        "{spec}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
        get("input_i"),
        get("input_tp"),
        get("input_lra"),
        get("input_thresh"),
        get("target_offset")
    );
    let c = same_container(input, &p, settings);
    let to = c.to;
    let out = naming::output_for(input, out_dir, "-normalized", to.ext());
    write_atomic(&out, |tmp| {
        let mut a = args!["-i", input];
        if p.video().is_some() {
            a.extend(args!["-map", "0:v:0", "-c:v", "copy"]);
        }
        a.extend(args!["-map", "0:a:0", "-af", format!("{second},aresample=48000")]);
        a.extend(c.audio.iter().cloned());
        a.extend(args!["-map_metadata", "0", "-f", muxer(to), tmp]);
        ffmpeg::run(&span.range(0.5, 1.0), a, dur).map(|_| ())
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct Range {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BleepOptions {
    pub ranges: Vec<Range>,
    /// "tone" or "mute".
    pub style: String,
}

pub fn bleep(span: &Span, input: &Path, o: &BleepOptions, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    let Some(au) = p.audio() else { bail!("This file has no audio.") };
    if o.ranges.is_empty() {
        bail!("Mark at least one section to bleep.");
    }
    let expr = o
        .ranges
        .iter()
        .map(|r| format!("between(t,{:.3},{:.3})", r.start.min(r.end), r.start.max(r.end)))
        .collect::<Vec<_>>()
        .join("+");
    let sr = au.sample_rate.clone().unwrap_or_else(|| "48000".into());
    let layout = if au.channels.unwrap_or(2) == 1 { "mono" } else { "stereo" };
    let graph = if o.style == "mute" {
        format!("[0:a:0]volume=0:enable='{expr}'[out]")
    } else {
        format!(
            "[0:a:0]volume=0:enable='{expr}',aformat=sample_rates={sr}:channel_layouts={layout}[muted];\
             sine=frequency=1000:sample_rate={sr},volume=0.25,aformat=sample_rates={sr}:channel_layouts={layout},volume=0:enable='not({expr})'[tone];\
             [muted][tone]amix=inputs=2:duration=first:normalize=0[out]"
        )
    };
    let c = same_container(input, &p, settings);
    let to = c.to;
    let out = naming::output_for(input, out_dir, "-bleeped", to.ext());
    write_atomic(&out, |tmp| {
        let mut a = args!["-i", input, "-filter_complex", &graph];
        if p.video().is_some() {
            a.extend(args!["-map", "0:v:0", "-c:v", "copy"]);
        }
        a.extend(args!["-map", "[out]"]);
        a.extend(c.audio.iter().cloned());
        a.extend(args!["-map_metadata", "0", "-f", muxer(to), tmp]);
        ffmpeg::run(span, a, p.duration()).map(|_| ())
    })
}

pub fn channels(span: &Span, input: &Path, mode: &str, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    if p.audio().is_none() {
        bail!("This file has no audio.");
    }
    let (filter, suffix) = match mode {
        "mono" => ("pan=mono|c0=0.5*c0+0.5*c1", "-mono"),
        "stereo" => ("aformat=channel_layouts=stereo", "-stereo"),
        "left" => ("pan=stereo|c0=c0|c1=c0", "-left"),
        "right" => ("pan=stereo|c0=c1|c1=c1", "-right"),
        "swap" => ("pan=stereo|c0=c1|c1=c0", "-swapped"),
        _ => bail!("Unknown channel option."),
    };
    let c = same_container(input, &p, settings);
    let to = c.to;
    let out = naming::output_for(input, out_dir, suffix, to.ext());
    write_atomic(&out, |tmp| {
        let mut a = args!["-i", input];
        if p.video().is_some() {
            a.extend(args!["-map", "0:v:0", "-c:v", "copy"]);
        }
        a.extend(args!["-map", "0:a:0", "-af", filter]);
        a.extend(c.audio.iter().cloned());
        a.extend(args!["-map_metadata", "0", "-f", muxer(to), tmp]);
        ffmpeg::run(span, a, p.duration()).map(|_| ())
    })
}

/// Copies the file with metadata changed (`edits`) or removed entirely (`strip`).
pub fn write_metadata(span: &Span, input: &Path, edits: &[(String, String)], strip: bool, out_dir: Option<&Path>) -> Result<PathBuf> {
    let p = ffmpeg::probe(input)?;
    let to = fmt_of(input).unwrap_or(Fmt::Mp4);
    let suffix = if strip { "-clean" } else { "-edited" };
    let out = naming::output_for(input, out_dir, suffix, &crate::registry::ext_of(input));
    write_atomic(&out, |tmp| {
        let mut a = args!["-i", input];
        if strip {
            a.extend(args!["-map", "0:V?", "-map", "0:a?", "-map", "0:s?", "-map_metadata", "-1", "-map_chapters", "-1", "-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact"]);
        } else {
            a.extend(args!["-map", "0", "-map_metadata", "0"]);
            for (k, v) in edits {
                a.extend(args!["-metadata", format!("{k}={v}")]);
            }
        }
        a.extend(args!["-c", "copy"]);
        if to == Fmt::Mp3 {
            a.extend(args!["-id3v2_version", "3", "-write_id3v1", "0"]);
        }
        let mux = muxer(to);
        if !mux.is_empty() {
            a.extend(args!["-f", mux]);
        }
        a.extend(args![tmp]);
        ffmpeg::run(span, a, p.duration()).map(|_| ())
    })
}

// ---------------------------------------------------------------------------------------
// Editor helpers
// ---------------------------------------------------------------------------------------

/// A JPEG of the frame at `t` seconds, `width` pixels wide.
pub fn frame_jpeg(input: &Path, t: f64, width: u32) -> Result<Vec<u8>> {
    ffmpeg::capture(args![
        "-ss", ts(t), "-i", input, "-frames:v", "1", "-vf", format!("scale={width}:-2"),
        "-q:v", "4", "-f", "image2pipe", "-c:v", "mjpeg", "pipe:1"
    ])
}

/// Peak amplitude per bucket, 0..1, for drawing a waveform.
pub fn audio_peaks(input: &Path, buckets: usize) -> Result<(Vec<f32>, f64)> {
    let p = ffmpeg::probe(input)?;
    let dur = p.duration();
    let raw = ffmpeg::capture(args!["-i", input, "-map", "0:a:0", "-ac", "1", "-ar", "4000", "-f", "s16le", "-c:a", "pcm_s16le", "pipe:1"])?;
    let samples: Vec<i16> = raw.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    let buckets = buckets.clamp(16, 20_000);
    if samples.is_empty() {
        return Ok((vec![0.0; buckets], dur));
    }
    let per = (samples.len() as f64 / buckets as f64).max(1.0);
    let peaks = (0..buckets)
        .map(|i| {
            let s = (i as f64 * per) as usize;
            let e = (((i + 1) as f64 * per) as usize).min(samples.len());
            samples[s.min(samples.len())..e]
                .iter()
                .map(|v| (*v as f32).abs() / 32768.0)
                .fold(0.0f32, f32::max)
        })
        .collect();
    Ok((peaks, dur))
}

/// A copy the WebView can play, for codecs it can't (HEVC, WMV, AIFF...).
pub fn preview_proxy(input: &Path, video: bool) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("KiwiConvert").join("previews");
    std::fs::create_dir_all(&dir)?;
    let key = format!("{:x}", {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        input.hash(&mut h);
        std::fs::metadata(input).and_then(|m| m.modified()).ok().hash(&mut h);
        h.finish()
    });
    let out = dir.join(format!("{key}.{}", if video { "mp4" } else { "m4a" }));
    if out.exists() {
        return Ok(out);
    }
    let tmp = dir.join(format!("{key}.partial.{}", if video { "mp4" } else { "m4a" }));
    let a = if video {
        args![
            "-y", "-i", input, "-map", "0:v:0", "-map", "0:a?", "-vf", "scale=-2:'min(720,ih)'",
            "-c:v", "libx264", "-preset", "ultrafast", "-crf", "26", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "128k", "-f", "mp4", &tmp
        ]
    } else {
        args!["-y", "-i", input, "-map", "0:a:0", "-c:a", "aac", "-b:a", "160k", "-f", "ipod", &tmp]
    };
    ffmpeg::capture(a)?;
    std::fs::rename(&tmp, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atempo_chains_stay_in_range() {
        assert_eq!(atempo_chain(1.5), "atempo=1.500000");
        assert_eq!(atempo_chain(4.0), "atempo=2.0,atempo=2.000000");
        assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.500000");
    }
}
