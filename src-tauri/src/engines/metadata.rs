//! Reading, editing, and removing metadata for images, media, PDFs, and Word documents.
//! Edits and removals are written to a copy; the original is never modified.

use super::{ffmpeg, heif, write_atomic};
use crate::jobs::Span;
use crate::naming;
use crate::registry::{Kind, ext_of, kind_of};
use anyhow::{Context, Result, bail};
use quick_xml::{Reader, XmlVersion};
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub name: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub key: String,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub kind: Kind,
    pub groups: Vec<Group>,
    pub fields: Vec<Field>,
    pub has_location: bool,
    /// False when this file type can only be viewed and stripped.
    pub editable: bool,
}

fn entry(k: impl Into<String>, v: impl Into<String>) -> Entry {
    Entry { key: k.into(), value: v.into() }
}

fn field(key: &str, label: &str, value: Option<&String>) -> Field {
    Field { key: key.into(), label: label.into(), value: value.cloned().unwrap_or_default() }
}

pub fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..1_048_576 => format!("{:.1} KB", b / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", b / 1_048_576.0),
        _ => format!("{:.2} GB", b / 1_073_741_824.0),
    }
}

fn file_group(path: &Path) -> Group {
    let mut entries = vec![entry("Name", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())];
    if let Ok(m) = std::fs::metadata(path) {
        entries.push(entry("Size", human_size(m.len())));
        let fmt = |t: std::io::Result<std::time::SystemTime>| {
            t.ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| format_unix(d.as_secs() as i64))
                .unwrap_or_default()
        };
        entries.push(entry("Modified", fmt(m.modified())));
        entries.push(entry("Created", fmt(m.created())));
    }
    if let Some(dir) = path.parent() {
        entries.push(entry("Folder", dir.to_string_lossy()));
    }
    Group { name: "File".into(), entries }
}

/// UTC date and time from a Unix timestamp (no time zone database needed).
fn format_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem / 60 % 60)
}

pub fn read(path: &Path) -> Result<Report> {
    let kind = kind_of(path);
    let mut groups = vec![file_group(path)];
    let mut fields = Vec::new();
    let mut has_location = false;
    let mut editable = false;
    match kind {
        Kind::Image => {
            let (g, f, loc, e) = image_meta(path)?;
            groups.extend(g);
            fields = f;
            has_location = loc;
            editable = e;
        }
        Kind::Video | Kind::Audio => {
            let p = ffmpeg::probe(path)?;
            let mut fmt = vec![];
            if let Some(n) = &p.format.format_name {
                fmt.push(entry("Container", n.clone()));
            }
            let d = p.duration();
            if d > 0.0 {
                fmt.push(entry("Duration", format!("{}:{:02}:{:02}", (d / 3600.0) as u64, (d / 60.0) as u64 % 60, d as u64 % 60)));
            }
            if let Some(b) = p.format.bit_rate.as_deref().and_then(|b| b.parse::<f64>().ok()) {
                fmt.push(entry("Bitrate", format!("{:.0} kbps", b / 1000.0)));
            }
            groups.push(Group { name: "Format".into(), entries: fmt });
            for s in &p.streams {
                let mut e = vec![entry("Codec", s.codec())];
                match s.codec_type.as_str() {
                    "video" if !s.is_cover_art() => {
                        e.push(entry("Resolution", format!("{} × {}", s.width.unwrap_or(0), s.height.unwrap_or(0))));
                        e.push(entry("Frame rate", format!("{:.2} fps", s.fps())));
                        if s.is_hdr() {
                            e.push(entry("HDR", "Yes"));
                        }
                    }
                    "audio" => {
                        e.push(entry("Sample rate", format!("{} Hz", s.sample_rate.clone().unwrap_or_default())));
                        e.push(entry("Channels", s.channels.unwrap_or(0).to_string()));
                    }
                    _ => {}
                }
                for (k, v) in &s.tags {
                    e.push(entry(k.clone(), v.clone()));
                }
                let name = if s.is_cover_art() { "Cover art".to_string() } else { format!("{} stream", capitalize(&s.codec_type)) };
                groups.push(Group { name, entries: e });
            }
            let tags: BTreeMap<String, String> = p.format.tags.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.clone())).collect();
            has_location = tags.keys().any(|k| k.contains("location"));
            if !tags.is_empty() {
                groups.push(Group { name: "Tags".into(), entries: tags.iter().map(|(k, v)| entry(k.clone(), v.clone())).collect() });
            }
            let keys: &[(&str, &str)] = if kind == Kind::Audio {
                &[("title", "Title"), ("artist", "Artist"), ("album", "Album"), ("album_artist", "Album artist"), ("date", "Year"), ("genre", "Genre"), ("track", "Track"), ("comment", "Comment")]
            } else {
                &[("title", "Title"), ("artist", "Author"), ("date", "Date"), ("description", "Description"), ("comment", "Comment")]
            };
            fields = keys.iter().map(|(k, l)| field(k, l, tags.get(*k))).collect();
            editable = true;
        }
        Kind::Pdf => {
            let (info, pages, xmp) = super::pdf::read_info(path)?;
            groups.push(Group {
                name: "Document".into(),
                entries: vec![entry("Pages", pages.to_string()), entry("XMP metadata", if xmp { "Yes" } else { "No" })],
            });
            let map: BTreeMap<String, String> = info.iter().cloned().collect();
            groups.push(Group { name: "Info".into(), entries: info.into_iter().map(|(k, v)| entry(k, v)).collect() });
            fields = [("Title", "Title"), ("Author", "Author"), ("Subject", "Subject"), ("Keywords", "Keywords"), ("Creator", "Creator app")]
                .iter()
                .map(|(k, l)| field(k, l, map.get(*k)))
                .collect();
            editable = true;
        }
        Kind::Docx => {
            let core = docx_core(path)?;
            groups.push(Group { name: "Document properties".into(), entries: core.iter().map(|(k, v)| entry(k.clone(), v.clone())).collect() });
            fields = [
                ("dc:title", "Title"),
                ("dc:subject", "Subject"),
                ("dc:creator", "Author"),
                ("cp:keywords", "Keywords"),
                ("dc:description", "Comments"),
                ("cp:lastModifiedBy", "Last modified by"),
            ]
            .iter()
            .map(|(k, l)| field(k, l, core.get(*k)))
            .collect();
            editable = true;
        }
        _ => {}
    }
    Ok(Report { kind, groups, fields, has_location, editable })
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn exif_of(path: &Path) -> Option<exif::Exif> {
    let reader = exif::Reader::new();
    if matches!(ext_of(path).as_str(), "heic" | "heif" | "hif" | "avif") {
        let data = std::fs::read(path).ok()?;
        let raw = heif::read_meta(&data).ok()?.exif?;
        return reader.read_raw(raw).ok();
    }
    let file = std::fs::File::open(path).ok()?;
    reader.read_from_container(&mut std::io::BufReader::new(file)).ok()
}

fn image_meta(path: &Path) -> Result<(Vec<Group>, Vec<Field>, bool, bool)> {
    let mut groups = Vec::new();
    if let Ok((w, h)) = image::image_dimensions(path) {
        groups.push(Group { name: "Image".into(), entries: vec![entry("Dimensions", format!("{w} × {h}")), entry("Megapixels", format!("{:.1}", w as f64 * h as f64 / 1e6))] });
    }
    let mut camera = Vec::new();
    let mut gps = Vec::new();
    let mut values: BTreeMap<&'static str, String> = BTreeMap::new();
    if let Some(ex) = exif_of(path) {
        for f in ex.fields() {
            if f.ifd_num != exif::In::PRIMARY || matches!(f.tag, exif::Tag::MakerNote) {
                continue;
            }
            let value = f.display_value().with_unit(&ex).to_string();
            let value = value.trim_matches('"').to_string();
            if value.len() > 300 {
                continue;
            }
            match f.tag {
                exif::Tag::ImageDescription => { values.insert("ImageDescription", value.clone()); }
                exif::Tag::Artist => { values.insert("Artist", value.clone()); }
                exif::Tag::Copyright => { values.insert("Copyright", value.clone()); }
                exif::Tag::DateTimeOriginal => { values.insert("DateTimeOriginal", value.clone()); }
                _ => {}
            }
            if f.tag.context() == exif::Context::Gps {
                gps.push(entry(f.tag.to_string(), value));
            } else {
                camera.push(entry(f.tag.to_string(), value));
            }
        }
    }
    let has_location = !gps.is_empty();
    if !camera.is_empty() {
        groups.push(Group { name: "EXIF".into(), entries: camera });
    }
    if !gps.is_empty() {
        groups.push(Group { name: "Location (GPS)".into(), entries: gps });
    }
    let editable = matches!(ext_of(path).as_str(), "jpg" | "jpeg" | "jpe" | "jfif" | "png" | "webp" | "tif" | "tiff");
    let get = |k: &str| values.get(k).cloned();
    let fields = if editable {
        vec![
            field("ImageDescription", "Title", get("ImageDescription").as_ref()),
            field("Artist", "Artist", get("Artist").as_ref()),
            field("Copyright", "Copyright", get("Copyright").as_ref()),
            field("DateTimeOriginal", "Date taken", get("DateTimeOriginal").as_ref()),
        ]
    } else {
        vec![]
    };
    Ok((groups, fields, has_location, editable))
}

// ---------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Apply the edits.
    Edit,
    /// Remove all metadata.
    Strip,
    /// Remove only location data.
    Location,
}

pub fn apply(span: &Span, path: &Path, edits: &BTreeMap<String, String>, mode: Mode, out_dir: Option<&Path>) -> Result<PathBuf> {
    let pairs: Vec<(String, String)> = edits.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    match kind_of(path) {
        Kind::Image => match mode {
            Mode::Strip => image_strip(path, out_dir),
            Mode::Location => image_edit(path, &BTreeMap::new(), true, out_dir),
            Mode::Edit => image_edit(path, edits, false, out_dir),
        },
        Kind::Video | Kind::Audio => {
            let pairs = if mode == Mode::Location {
                ["location", "location-eng", "com.apple.quicktime.location.ISO6709"]
                    .iter()
                    .map(|k| (k.to_string(), String::new()))
                    .collect()
            } else {
                pairs
            };
            super::media::write_metadata(span, path, &pairs, mode == Mode::Strip, out_dir)
        }
        Kind::Pdf => super::pdf::write_info(span, path, &pairs, mode != Mode::Edit, out_dir),
        Kind::Docx => docx_write(path, edits, mode != Mode::Edit, out_dir),
        _ => bail!("KiwiConvert can't change the metadata of this file type."),
    }
}

/// Removes EXIF, XMP, IPTC, and comments without re-encoding. The color profile stays.
fn image_strip(path: &Path, out_dir: Option<&Path>) -> Result<PathBuf> {
    let data = std::fs::read(path)?;
    let ext = ext_of(path);
    let cleaned: Vec<u8> = match ext.as_str() {
        "jpg" | "jpeg" | "jpe" | "jfif" => {
            let mut jpeg = img_parts::jpeg::Jpeg::from_bytes(data.into()).context("This JPEG is damaged")?;
            // Keep JFIF (APP0), the color profile (APP2), and Adobe color info (APP14).
            jpeg.segments_mut().retain(|s| {
                let m = s.marker();
                !((0xE1..=0xEF).contains(&m) && m != 0xE2 && m != 0xEE) && m != 0xFE
            });
            let mut out = Vec::new();
            jpeg.encoder().write_to(&mut out)?;
            out
        }
        "png" => {
            let mut png = img_parts::png::Png::from_bytes(data.into()).context("This PNG is damaged")?;
            png.chunks_mut().retain(|c| !matches!(&c.kind(), b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME"));
            let mut out = Vec::new();
            png.encoder().write_to(&mut out)?;
            out
        }
        "webp" => {
            let mut webp = img_parts::webp::WebP::from_bytes(data.into()).context("This WebP is damaged")?;
            use img_parts::ImageEXIF;
            webp.set_exif(None);
            webp.remove_chunks_by_id(img_parts::webp::CHUNK_XMP);
            let mut out = Vec::new();
            webp.encoder().write_to(&mut out)?;
            out
        }
        "heic" | "heif" | "hif" | "avif" => blank_heif_metadata(data)?,
        _ => {
            // Formats without separable metadata blocks are re-encoded losslessly.
            let loaded = super::image::load(path)?;
            let fmt = crate::registry::Fmt::from_ext(&ext).unwrap_or(crate::registry::Fmt::Png);
            let meta = super::image::Meta { icc: loaded.icc.as_deref(), exif: None };
            super::image::encode(&loaded.img, fmt, 95, &meta)?
        }
    };
    let out = naming::output_for(path, out_dir, "-clean", &ext);
    write_atomic(&out, |tmp| Ok(std::fs::write(tmp, &cleaned)?))
}

/// Zeroes the EXIF and XMP items of a HEIF file in place, keeping every offset valid.
fn blank_heif_metadata(mut data: Vec<u8>) -> Result<Vec<u8>> {
    let spans = heif_metadata_spans(&data);
    if spans.is_empty() {
        return Ok(data);
    }
    for (start, end) in spans {
        if let Some(slice) = data.get_mut(start..end) {
            slice.fill(0);
        }
    }
    Ok(data)
}

/// Byte ranges of EXIF and XMP item payloads, found through a fresh metadata read.
fn heif_metadata_spans(data: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if let Ok(meta) = heif::read_meta(data) {
        if let Some(exif) = meta.exif {
            // The EXIF payload is stored verbatim; find it in the file.
            if let Some(pos) = find_subslice(data, &exif) {
                out.push((pos, pos + exif.len()));
            }
        }
    }
    // XMP packets are plain text and self-delimiting.
    let mut from = 0;
    while let Some(start) = find_subslice(&data[from..], b"<x:xmpmeta").map(|p| p + from) {
        let end = find_subslice(&data[start..], b"</x:xmpmeta>").map(|p| start + p + 12).unwrap_or(start);
        if end > start {
            out.push((start, end));
        }
        from = end.max(start + 1);
    }
    out
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn image_edit(path: &Path, edits: &BTreeMap<String, String>, remove_location: bool, out_dir: Option<&Path>) -> Result<PathBuf> {
    use little_exif::exif_tag::ExifTag;
    use little_exif::metadata::Metadata;
    let ext = ext_of(path);
    let out = naming::output_for(path, out_dir, if remove_location { "-no-location" } else { "-edited" }, &ext);
    write_atomic(&out, |tmp| {
        std::fs::copy(path, tmp)?;
        let mut meta = Metadata::new_from_path(tmp).unwrap_or_else(|_| Metadata::new());
        if remove_location {
            for tag in 0x0000u16..=0x001f {
                meta.remove_tag_by_hex_group(tag, little_exif::ifd::ExifTagGroup::GPS);
            }
        }
        for (k, v) in edits {
            let tag = match k.as_str() {
                "ImageDescription" => ExifTag::ImageDescription(v.clone()),
                "Artist" => ExifTag::Artist(v.clone()),
                "Copyright" => ExifTag::Copyright(v.clone()),
                "DateTimeOriginal" => ExifTag::DateTimeOriginal(v.clone()),
                _ => continue,
            };
            if v.is_empty() {
                meta.remove_tag(tag);
            } else {
                meta.set_tag(tag);
            }
        }
        meta.write_to_file(tmp).map_err(|e| anyhow::anyhow!("couldn't write the metadata: {e}"))?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------------------
// Word documents
// ---------------------------------------------------------------------------------------

fn read_zip_entry(path: &Path, name: &str) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut e = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    e.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Core document properties keyed by their qualified element name, e.g. `dc:title`.
fn docx_core(path: &Path) -> Result<BTreeMap<String, String>> {
    let xml = read_zip_entry(path, "docProps/core.xml").unwrap_or_default();
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut r = Reader::from_reader(xml.as_slice());
    let mut buf = Vec::new();
    let mut current: Option<String> = None;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => current = Some(e.name().as_ref().to_string()),
            Ok(Event::Text(t)) => {
                if let Some(k) = &current {
                    map.entry(k.clone()).or_default().push_str(&t.xml_content(XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some(k) = &current {
                    map.entry(k.clone()).or_default().push_str(&super::docs::entity_text(&r));
                }
            }
            Ok(Event::End(_)) => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map.retain(|_, v| !v.trim().is_empty());
    Ok(map)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn docx_write(path: &Path, edits: &BTreeMap<String, String>, strip: bool, out_dir: Option<&Path>) -> Result<PathBuf> {
    let mut props = if strip { BTreeMap::new() } else { docx_core(path)? };
    if !strip {
        for (k, v) in edits {
            if v.is_empty() {
                props.remove(k);
            } else {
                props.insert(k.clone(), v.clone());
            }
        }
    }
    let mut core = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#);
    for (k, v) in &props {
        if !k.contains(':') || k.contains(' ') {
            continue;
        }
        if k.starts_with("dcterms:") {
            core.push_str(&format!("<{k} xsi:type=\"dcterms:W3CDTF\">{}</{k}>", xml_escape(v)));
        } else {
            core.push_str(&format!("<{k}>{}</{k}>", xml_escape(v)));
        }
    }
    core.push_str("</cp:coreProperties>");

    let out = naming::output_for(path, out_dir, if strip { "-clean" } else { "-edited" }, "docx");
    write_atomic(&out, |tmp| {
        let mut src = zip::ZipArchive::new(std::fs::File::open(path)?).context("This isn't a valid .docx file")?;
        let mut dst = zip::ZipWriter::new(std::fs::File::create(tmp)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for i in 0..src.len() {
            let name = src.name_for_index(i).unwrap_or_default().to_string();
            match name.as_str() {
                "docProps/core.xml" => {
                    dst.start_file(name, opts)?;
                    dst.write_all(core.as_bytes())?;
                }
                "docProps/app.xml" if strip => {
                    dst.start_file(name, opts)?;
                    dst.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"/>"#)?;
                }
                "docProps/custom.xml" if strip => {
                    dst.start_file(name, opts)?;
                    dst.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"/>"#)?;
                }
                _ => dst.raw_copy_file(src.by_index_raw(i)?)?,
            }
        }
        dst.finish()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_dates() {
        assert_eq!(format_unix(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_unix(1_700_000_000), "2023-11-14 22:13 UTC");
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 bytes");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1_048_576), "5.0 MB");
    }
}
