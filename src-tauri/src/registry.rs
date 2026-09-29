//! What KiwiConvert can do with a file: its kind, the formats it converts to, and the tools
//! that apply to it. The wheel is built from this module, so it is the single source of truth.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Video,
    Audio,
    Pdf,
    Docx,
    Text,
    Subtitle,
    Archive,
    Folder,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fmt {
    Jpg,
    Png,
    Webp,
    Heic,
    Avif,
    Gif,
    Bmp,
    Tiff,
    Ico,
    Svg,
    Pdf,
    Docx,
    Txt,
    Mp4,
    Mov,
    Mkv,
    Avi,
    Wmv,
    Webm,
    Mp3,
    M4a,
    Wav,
    Flac,
    Ogg,
    Opus,
    Aiff,
    Srt,
    Vtt,
    Zip,
    Tar,
    Tgz,
    Gz,
    Extract,
}

impl Fmt {
    /// File extension written for this format (without the dot).
    pub fn ext(self) -> &'static str {
        match self {
            Fmt::Jpg => "jpg",
            Fmt::Png => "png",
            Fmt::Webp => "webp",
            Fmt::Heic => "heic",
            Fmt::Avif => "avif",
            Fmt::Gif => "gif",
            Fmt::Bmp => "bmp",
            Fmt::Tiff => "tiff",
            Fmt::Ico => "ico",
            Fmt::Svg => "svg",
            Fmt::Pdf => "pdf",
            Fmt::Docx => "docx",
            Fmt::Txt => "txt",
            Fmt::Mp4 => "mp4",
            Fmt::Mov => "mov",
            Fmt::Mkv => "mkv",
            Fmt::Avi => "avi",
            Fmt::Wmv => "wmv",
            Fmt::Webm => "webm",
            Fmt::Mp3 => "mp3",
            Fmt::M4a => "m4a",
            Fmt::Wav => "wav",
            Fmt::Flac => "flac",
            Fmt::Ogg => "ogg",
            Fmt::Opus => "opus",
            Fmt::Aiff => "aiff",
            Fmt::Srt => "srt",
            Fmt::Vtt => "vtt",
            Fmt::Zip => "zip",
            Fmt::Tar => "tar",
            Fmt::Tgz => "tar.gz",
            Fmt::Gz => "gz",
            Fmt::Extract => "",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Fmt::Jpg => "JPG",
            Fmt::Png => "PNG",
            Fmt::Webp => "WEBP",
            Fmt::Heic => "HEIC",
            Fmt::Avif => "AVIF",
            Fmt::Gif => "GIF",
            Fmt::Bmp => "BMP",
            Fmt::Tiff => "TIFF",
            Fmt::Ico => "ICO",
            Fmt::Svg => "SVG",
            Fmt::Pdf => "PDF",
            Fmt::Docx => "DOCX",
            Fmt::Txt => "TXT",
            Fmt::Mp4 => "MP4",
            Fmt::Mov => "MOV",
            Fmt::Mkv => "MKV",
            Fmt::Avi => "AVI",
            Fmt::Wmv => "WMV",
            Fmt::Webm => "WEBM",
            Fmt::Mp3 => "MP3",
            Fmt::M4a => "M4A",
            Fmt::Wav => "WAV",
            Fmt::Flac => "FLAC",
            Fmt::Ogg => "OGG",
            Fmt::Opus => "OPUS",
            Fmt::Aiff => "AIFF",
            Fmt::Srt => "SRT",
            Fmt::Vtt => "VTT",
            Fmt::Zip => "ZIP",
            Fmt::Tar => "TAR",
            Fmt::Tgz => "TAR.GZ",
            Fmt::Gz => "GZIP",
            Fmt::Extract => "EXTRACT",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Fmt::Jpg => "Universal photo format",
            Fmt::Png => "Lossless, keeps transparency",
            Fmt::Webp => "Small modern web image",
            Fmt::Heic => "High efficiency photo",
            Fmt::Avif => "Smallest modern image",
            Fmt::Gif => "Plays everywhere",
            Fmt::Bmp => "Uncompressed bitmap",
            Fmt::Tiff => "Print and archive quality",
            Fmt::Ico => "Windows icon, all sizes",
            Fmt::Svg => "Traced vector artwork",
            Fmt::Pdf => "Portable document",
            Fmt::Docx => "Editable Word document",
            Fmt::Txt => "Plain text",
            Fmt::Mp4 => "Plays on every device",
            Fmt::Mov => "QuickTime movie",
            Fmt::Mkv => "Flexible Matroska video",
            Fmt::Avi => "Classic video for older players",
            Fmt::Wmv => "Windows Media video",
            Fmt::Webm => "Open web video",
            Fmt::Mp3 => "Universal audio",
            Fmt::M4a => "AAC audio, great quality",
            Fmt::Wav => "Uncompressed audio",
            Fmt::Flac => "Lossless, compressed",
            Fmt::Ogg => "Open Vorbis audio",
            Fmt::Opus => "Efficient voice and music",
            Fmt::Aiff => "Uncompressed, Apple friendly",
            Fmt::Srt => "SubRip subtitles",
            Fmt::Vtt => "Web subtitles",
            Fmt::Zip => "Compressed archive",
            Fmt::Tar => "Tape archive",
            Fmt::Tgz => "Gzipped tape archive",
            Fmt::Gz => "Gzip compressed",
            Fmt::Extract => "Unpack into a folder",
        }
    }

    pub fn from_ext(ext: &str) -> Option<Fmt> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" | "jpe" | "jfif" => Fmt::Jpg,
            "png" => Fmt::Png,
            "webp" => Fmt::Webp,
            "heic" | "heif" | "hif" => Fmt::Heic,
            "avif" => Fmt::Avif,
            "gif" => Fmt::Gif,
            "bmp" | "dib" => Fmt::Bmp,
            "tif" | "tiff" => Fmt::Tiff,
            "ico" => Fmt::Ico,
            "svg" => Fmt::Svg,
            "pdf" => Fmt::Pdf,
            "docx" => Fmt::Docx,
            "txt" | "md" | "markdown" | "log" | "csv" => Fmt::Txt,
            "mp4" | "m4v" => Fmt::Mp4,
            "mov" | "qt" => Fmt::Mov,
            "mkv" => Fmt::Mkv,
            "avi" => Fmt::Avi,
            "wmv" | "asf" => Fmt::Wmv,
            "webm" => Fmt::Webm,
            "mp3" => Fmt::Mp3,
            "m4a" | "aac" => Fmt::M4a,
            "wav" => Fmt::Wav,
            "flac" => Fmt::Flac,
            "ogg" | "oga" => Fmt::Ogg,
            "opus" => Fmt::Opus,
            "aif" | "aiff" | "aifc" => Fmt::Aiff,
            "srt" => Fmt::Srt,
            "vtt" => Fmt::Vtt,
            "zip" => Fmt::Zip,
            "tar" => Fmt::Tar,
            "tgz" => Fmt::Tgz,
            "gz" => Fmt::Gz,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    Compress,
    Resize,
    Crop,
    Adjust,
    Annotate,
    Redact,
    Metadata,
    ReadQr,
    MakePdf,
    Collage,
    Trim,
    Speed,
    Split,
    Join,
    Snapshot,
    Normalize,
    Bleep,
    Channels,
    MergePdf,
    SplitPdf,
    OrganizePdf,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Compress => "COMPRESS",
            Tool::Resize => "RESIZE",
            Tool::Crop => "CROP",
            Tool::Adjust => "ADJUST",
            Tool::Annotate => "ANNOTATE",
            Tool::Redact => "REDACT",
            Tool::Metadata => "METADATA",
            Tool::ReadQr => "READ QR",
            Tool::MakePdf => "MAKE PDF",
            Tool::Collage => "COLLAGE",
            Tool::Trim => "TRIM",
            Tool::Speed => "SPEED",
            Tool::Split => "SPLIT",
            Tool::Join => "JOIN",
            Tool::Snapshot => "SNAPSHOT",
            Tool::Normalize => "NORMALIZE",
            Tool::Bleep => "BLEEP",
            Tool::Channels => "CHANNELS",
            Tool::MergePdf => "MERGE",
            Tool::SplitPdf => "SPLIT",
            Tool::OrganizePdf => "ORGANIZE",
        }
    }

    /// Icon name understood by the frontend (a lucide icon id).
    pub fn icon(self) -> &'static str {
        match self {
            Tool::Compress => "shrink",
            Tool::Resize => "scaling",
            Tool::Crop => "crop",
            Tool::Adjust => "sliders-horizontal",
            Tool::Annotate => "pen-line",
            Tool::Redact => "eye-off",
            Tool::Metadata => "tags",
            Tool::ReadQr => "qr-code",
            Tool::MakePdf => "file-stack",
            Tool::Collage => "layout-grid",
            Tool::Trim => "scissors",
            Tool::Speed => "gauge",
            Tool::Split => "split",
            Tool::Join => "merge",
            Tool::Snapshot => "camera",
            Tool::Normalize => "audio-lines",
            Tool::Bleep => "bell-off",
            Tool::Channels => "headphones",
            Tool::MergePdf => "files",
            Tool::SplitPdf => "split",
            Tool::OrganizePdf => "layout-panel-left",
        }
    }

    fn hint(self, kind: Kind) -> &'static str {
        match (self, kind) {
            (Tool::Compress, Kind::Video) => "Smaller video or an exact size",
            (Tool::Compress, Kind::Audio) => "Lower bitrate or an exact size",
            (Tool::Compress, Kind::Pdf) => "Shrink embedded images",
            (Tool::Compress, _) => "Smaller file or an exact size",
            (Tool::Resize, _) => "Scale to new dimensions",
            (Tool::Crop, Kind::Video) => "Cut away the edges of the frame",
            (Tool::Crop, _) => "Crop, rotate, and straighten",
            (Tool::Adjust, _) => "Exposure, color, and light",
            (Tool::Annotate, _) => "Draw, point, and add text",
            (Tool::Redact, _) => "Hide private details",
            (Tool::Metadata, _) => "View, edit, or remove metadata",
            (Tool::ReadQr, _) => "Read the QR code in this image",
            (Tool::MakePdf, _) => "Combine images into one PDF",
            (Tool::Collage, _) => "Arrange images in a grid",
            (Tool::Trim, _) => "Keep only the part you need",
            (Tool::Speed, _) => "Speed up or slow down",
            (Tool::Split, _) => "Cut into separate clips",
            (Tool::Join, _) => "Join into one file",
            (Tool::Snapshot, _) => "Save a frame as an image",
            (Tool::Normalize, _) => "Even out loudness",
            (Tool::Bleep, _) => "Bleep or mute sections",
            (Tool::Channels, _) => "Mono, stereo, or swap sides",
            (Tool::MergePdf, _) => "Merge into one PDF",
            (Tool::SplitPdf, _) => "Split pages into files",
            (Tool::OrganizePdf, _) => "Reorder, rotate, delete pages",
        }
    }

    /// Tools that start a job straight from the wheel, without opening a window first.
    pub fn is_instant(self) -> bool {
        matches!(
            self,
            Tool::ReadQr | Tool::MakePdf | Tool::MergePdf | Tool::Join | Tool::Normalize
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    Convert { to: Fmt },
    Tool { tool: Tool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Convert,
    Tools,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelOption {
    pub label: &'static str,
    pub icon: Option<&'static str>,
    pub hint: String,
    pub action: Action,
}

pub fn kind_of(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Folder;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".rar") {
        return Kind::Archive;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "jpe" | "jfif" | "png" | "webp" | "heic" | "heif" | "hif" | "avif"
        | "gif" | "bmp" | "dib" | "tif" | "tiff" | "ico" | "svg" | "tga" | "qoi" | "pnm"
        | "ppm" | "pgm" | "pbm" => Kind::Image,
        "mp4" | "m4v" | "mov" | "qt" | "mkv" | "avi" | "wmv" | "asf" | "webm" | "flv" | "mpg"
        | "mpeg" | "m2ts" | "mts" | "ts" | "3gp" | "3g2" | "ogv" | "vob" => Kind::Video,
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "oga" | "opus" | "aif" | "aiff"
        | "aifc" | "wma" | "alac" | "amr" | "ac3" | "mka" => Kind::Audio,
        "pdf" => Kind::Pdf,
        "docx" => Kind::Docx,
        "txt" | "md" | "markdown" | "log" | "csv" => Kind::Text,
        "srt" | "vtt" => Kind::Subtitle,
        "zip" | "tar" | "gz" | "tgz" | "rar" => Kind::Archive,
        _ => Kind::Other,
    }
}

/// True when a GIF has more than one frame. Walks the block structure without decoding pixels.
pub fn is_animated_gif(path: &Path) -> bool {
    std::fs::read(path).map(|b| gif_frame_count(&b) > 1).unwrap_or(false)
}

fn gif_frame_count(b: &[u8]) -> usize {
    if b.len() < 13 || &b[0..3] != b"GIF" {
        return 0;
    }
    let mut i = 13;
    if b[10] & 0x80 != 0 {
        i += 3 * (1 << ((b[10] & 0x07) + 1));
    }
    let skip_sub_blocks = |mut i: usize| -> usize {
        while i < b.len() {
            let n = b[i] as usize;
            i += 1;
            if n == 0 {
                break;
            }
            i += n;
        }
        i
    };
    let mut frames = 0;
    while i < b.len() {
        match b[i] {
            0x21 => i = skip_sub_blocks(i + 2),
            0x2C => {
                frames += 1;
                if frames > 1 || i + 10 > b.len() {
                    return frames;
                }
                let packed = b[i + 9];
                i += 10;
                if packed & 0x80 != 0 {
                    i += 3 * (1 << ((packed & 0x07) + 1));
                }
                i = skip_sub_blocks(i + 1);
            }
            _ => break,
        }
    }
    frames
}

pub fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// The format a file already is, used to hide "convert to itself" options.
fn source_fmt(path: &Path) -> Option<Fmt> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return Some(Fmt::Tgz);
    }
    Fmt::from_ext(&ext_of(path))
}

fn convert_targets(path: &Path) -> Vec<Fmt> {
    use Fmt::*;
    let kind = kind_of(path);
    let mut list: Vec<Fmt> = match kind {
        Kind::Image => match ext_of(path).as_str() {
            "svg" => vec![Png, Jpg, Webp, Pdf, Ico, Avif, Gif, Tiff, Bmp],
            "gif" if is_animated_gif(path) => {
                vec![Mp4, Webm, Webp, Png, Jpg, Avif, Pdf, Tiff, Bmp, Ico]
            }
            _ => vec![Jpg, Png, Webp, Heic, Avif, Pdf, Gif, Tiff, Bmp, Ico, Svg, Docx],
        },
        Kind::Video => vec![Mp4, Mov, Mkv, Webm, Avi, Wmv, Gif, Mp3, M4a, Wav],
        Kind::Audio => vec![Mp3, M4a, Wav, Flac, Ogg, Opus, Aiff],
        Kind::Pdf => vec![Png, Jpg, Webp, Tiff, Txt, Docx],
        Kind::Docx => vec![Pdf, Txt],
        Kind::Text => vec![Pdf, Docx],
        Kind::Subtitle => vec![Srt, Vtt, Txt],
        Kind::Archive => vec![Extract, Zip, Tar, Tgz],
        Kind::Folder => vec![Zip, Tar, Tgz],
        Kind::Other => vec![Zip, Tar, Tgz, Gz],
    };
    if let Some(src) = source_fmt(path) {
        list.retain(|f| *f != src);
    }
    list
}

fn tools_for(kind: Kind, count: usize) -> Vec<Tool> {
    use Tool::*;
    let multi = count > 1;
    match kind {
        Kind::Image if multi => vec![Compress, Resize, MakePdf, Collage, Metadata],
        Kind::Image => vec![
            Compress, Resize, Crop, Adjust, Annotate, Redact, Metadata, ReadQr,
        ],
        Kind::Video if multi => vec![Compress, Join, Metadata],
        Kind::Video => vec![Compress, Trim, Crop, Speed, Split, Snapshot, Metadata],
        Kind::Audio if multi => vec![Compress, Join, Normalize, Metadata],
        Kind::Audio => vec![Compress, Trim, Normalize, Bleep, Channels, Speed, Metadata],
        Kind::Pdf if multi => vec![MergePdf, Compress, Metadata],
        Kind::Pdf => vec![Compress, OrganizePdf, SplitPdf, Metadata],
        Kind::Docx => vec![Metadata],
        _ => vec![],
    }
}

/// Builds the wheel for a set of dragged files.
pub fn wheel_options(paths: &[PathBuf], mode: Mode) -> Vec<WheelOption> {
    if paths.is_empty() {
        return vec![];
    }
    let kinds: Vec<Kind> = paths.iter().map(|p| kind_of(p)).collect();
    let same_kind = kinds.iter().all(|k| *k == kinds[0]);
    match mode {
        Mode::Convert => {
            let mut common = convert_targets(&paths[0]);
            for p in &paths[1..] {
                let t = convert_targets(p);
                common.retain(|f| t.contains(f));
            }
            // Several files of different kinds can always be archived together.
            if paths.len() > 1 && !same_kind {
                for f in [Fmt::Zip, Fmt::Tar, Fmt::Tgz] {
                    if !common.contains(&f) {
                        common.push(f);
                    }
                }
            }
            common.truncate(12);
            common
                .into_iter()
                .map(|f| WheelOption {
                    label: f.label(),
                    icon: None,
                    hint: f.hint().to_string(),
                    action: Action::Convert { to: f },
                })
                .collect()
        }
        Mode::Tools => {
            if !same_kind {
                return vec![];
            }
            tools_for(kinds[0], paths.len())
                .into_iter()
                .map(|t| WheelOption {
                    label: t.label(),
                    icon: Some(t.icon()),
                    hint: t.hint(kinds[0]).to_string(),
                    action: Action::Tool { tool: t },
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(opts: &[WheelOption]) -> Vec<&str> {
        opts.iter().map(|o| o.label).collect()
    }

    #[test]
    fn image_wheel_hides_its_own_format() {
        let opts = wheel_options(&[PathBuf::from("C:/x/photo.PNG")], Mode::Convert);
        let l = labels(&opts);
        assert!(!l.contains(&"PNG"));
        assert!(l.contains(&"JPG") && l.contains(&"HEIC") && l.contains(&"WEBP"));
    }

    #[test]
    fn jpeg_aliases_map_to_jpg() {
        assert_eq!(Fmt::from_ext("JPEG"), Some(Fmt::Jpg));
        let opts = wheel_options(&[PathBuf::from("a.jpeg")], Mode::Convert);
        assert!(!labels(&opts).contains(&"JPG"));
    }

    #[test]
    fn video_audio_and_documents() {
        let v = labels(&wheel_options(&[PathBuf::from("clip.mov")], Mode::Convert)).join(",");
        assert!(v.contains("MP4") && !v.contains("MOV") && v.contains("GIF") && v.contains("MP3"));
        let a = labels(&wheel_options(&[PathBuf::from("song.flac")], Mode::Convert)).join(",");
        assert_eq!(a, "MP3,M4A,WAV,OGG,OPUS,AIFF");
        let d = labels(&wheel_options(&[PathBuf::from("doc.docx")], Mode::Convert)).join(",");
        assert_eq!(d, "PDF,TXT");
        let s = labels(&wheel_options(&[PathBuf::from("subs.srt")], Mode::Convert)).join(",");
        assert_eq!(s, "VTT,TXT");
    }

    #[test]
    fn archives_offer_extract_and_repack() {
        let a = labels(&wheel_options(&[PathBuf::from("pack.tar.gz")], Mode::Convert)).join(",");
        assert_eq!(a, "EXTRACT,ZIP,TAR");
        let r = labels(&wheel_options(&[PathBuf::from("pack.rar")], Mode::Convert)).join(",");
        assert_eq!(r, "EXTRACT,ZIP,TAR,TAR.GZ");
    }

    #[test]
    fn mixed_selection_falls_back_to_archives() {
        let opts = wheel_options(
            &[PathBuf::from("a.jpg"), PathBuf::from("b.mp3")],
            Mode::Convert,
        );
        assert_eq!(labels(&opts), vec!["ZIP", "TAR", "TAR.GZ"]);
        assert!(wheel_options(&[PathBuf::from("a.jpg"), PathBuf::from("b.mp3")], Mode::Tools)
            .is_empty());
    }

    #[test]
    fn same_kind_selection_intersects_targets() {
        let opts = wheel_options(
            &[PathBuf::from("a.jpg"), PathBuf::from("b.png")],
            Mode::Convert,
        );
        let l = labels(&opts);
        assert!(!l.contains(&"JPG") && !l.contains(&"PNG") && l.contains(&"WEBP"));
    }

    #[test]
    fn tools_depend_on_count() {
        let one = labels(&wheel_options(&[PathBuf::from("a.pdf")], Mode::Tools)).join(",");
        assert_eq!(one, "COMPRESS,ORGANIZE,SPLIT,METADATA");
        let two = labels(&wheel_options(
            &[PathBuf::from("a.pdf"), PathBuf::from("b.pdf")],
            Mode::Tools,
        ))
        .join(",");
        assert_eq!(two, "MERGE,COMPRESS,METADATA");
    }

    #[test]
    fn wheel_never_exceeds_twelve_segments() {
        for name in ["a.jpg", "a.svg", "a.gif", "a.mp4", "a.wav", "a.pdf", "a.bin"] {
            assert!(wheel_options(&[PathBuf::from(name)], Mode::Convert).len() <= 12);
        }
    }
}
