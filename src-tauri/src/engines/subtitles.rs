//! SubRip (SRT) and WebVTT subtitles.

use super::write_atomic;
use crate::jobs::Span;
use crate::naming;
use crate::registry::Fmt;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Parses `HH:MM:SS,mmm`, `HH:MM:SS.mmm`, or `MM:SS.mmm`.
fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim().replace(',', ".");
    let (hms, ms) = s.split_once('.').unwrap_or((s.as_str(), "0"));
    let parts: Vec<u64> = hms.split(':').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (*h, *m, *s),
        [m, s] => (0, *m, *s),
        _ => return None,
    };
    let ms: u64 = format!("{:0<3}", &ms[..ms.len().min(3)]).parse().ok()?;
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

fn fmt_time(ms: u64, sep: char) -> String {
    let h = ms / 3_600_000;
    let m = ms / 60_000 % 60;
    let s = ms / 1000 % 60;
    format!("{h:02}:{m:02}:{s:02}{sep}{:03}", ms % 1000)
}

/// Parses SRT or WebVTT. Both are blocks separated by blank lines with a timing line.
pub fn parse(content: &str) -> Vec<Cue> {
    let content = content.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in content.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(timing_at) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let timing = lines[timing_at];
        let Some((a, rest)) = timing.split_once("-->") else { continue };
        // WebVTT cue settings follow the end time.
        let b = rest.split_whitespace().next().unwrap_or("");
        let (Some(start_ms), Some(end_ms)) = (parse_time(a), parse_time(b)) else { continue };
        let text = lines[timing_at + 1..].join("\n");
        cues.push(Cue { start_ms, end_ms, text });
    }
    cues
}

/// Removes WebVTT markup that SRT players don't understand, keeping b/i/u.
fn srt_safe(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        match rest[i..].find('>') {
            Some(j) => {
                let tag = &rest[i + 1..i + j];
                let name = tag.trim_start_matches('/').split(['.', ' ']).next().unwrap_or("");
                if matches!(name, "b" | "i" | "u") {
                    out.push('<');
                    out.push_str(tag.split(['.', ' ']).next().unwrap_or(""));
                    out.push('>');
                }
                rest = &rest[i + j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn plain(text: &str) -> String {
    let no_tags = srt_safe(text)
        .replace("<b>", "")
        .replace("</b>", "")
        .replace("<i>", "")
        .replace("</i>", "")
        .replace("<u>", "")
        .replace("</u>", "");
    no_tags.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ")
}

pub fn to_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\r\n{} --> {}\r\n{}\r\n\r\n",
            i + 1,
            fmt_time(c.start_ms, ','),
            fmt_time(c.end_ms, ','),
            srt_safe(&c.text).replace('\n', "\r\n")
        ));
    }
    out
}

pub fn to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for c in cues {
        out.push_str(&format!("{} --> {}\n{}\n\n", fmt_time(c.start_ms, '.'), fmt_time(c.end_ms, '.'), c.text));
    }
    out
}

pub fn to_text(cues: &[Cue]) -> String {
    let mut out = String::new();
    let mut last = String::new();
    for c in cues {
        let t = plain(&c.text).replace('\n', " ");
        // Rolling captions repeat lines; keep each line once.
        if t != last {
            out.push_str(&t);
            out.push_str("\r\n");
            last = t;
        }
    }
    out
}

pub fn convert(span: &Span, input: &Path, to: Fmt, out_dir: Option<&Path>) -> Result<PathBuf> {
    let content = super::docs::read_text(input)?;
    let cues = parse(&content);
    if cues.is_empty() {
        bail!("No subtitles were found in this file.");
    }
    span.progress(0.5);
    let body = match to {
        Fmt::Srt => to_srt(&cues),
        Fmt::Vtt => to_vtt(&cues),
        Fmt::Txt => to_text(&cues),
        _ => bail!("Subtitles can't be converted to {}.", to.label()),
    };
    let out = naming::output_for(input, out_dir, "", to.ext());
    write_atomic(&out, |tmp| Ok(std::fs::write(tmp, body.as_bytes())?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\r\n00:00:01,500 --> 00:00:03,000\r\nHello <i>world</i>\r\n\r\n2\r\n00:01:02,003 --> 00:01:04,250\r\nTwo\r\nlines\r\n";

    #[test]
    fn srt_to_vtt_and_back() {
        let cues = parse(SRT);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1500);
        assert_eq!(cues[1].end_ms, 64_250);
        let vtt = to_vtt(&cues);
        assert!(vtt.starts_with("WEBVTT\n\n00:00:01.500 --> 00:00:03.000\nHello <i>world</i>"));
        let again = parse(&vtt);
        assert_eq!(again, cues);
        assert!(to_srt(&again).contains("2\r\n00:01:02,003 --> 00:01:04,250\r\nTwo\r\nlines"));
    }

    #[test]
    fn vtt_settings_and_tags() {
        let vtt = "WEBVTT\n\nNOTE hi\n\nintro\n00:05.000 --> 00:06.000 line:0 position:50%\n<v Bob>Hi <c.loud>there</c> <b>you</b></v>\n";
        let cues = parse(vtt);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start_ms, 5000);
        assert_eq!(srt_safe(&cues[0].text), "Hi there <b>you</b>");
        assert_eq!(to_text(&cues), "Hi there you\r\n");
    }
}
