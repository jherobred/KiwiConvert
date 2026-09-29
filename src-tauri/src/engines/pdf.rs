//! PDF work. PDFium renders pages, extracts text, and moves pages between documents.
//! lopdf builds PDFs from images, recompresses embedded images, and edits metadata.

use super::image::{self as img, Meta};
use super::write_atomic;
use crate::jobs::Span;
use crate::naming;
use crate::registry::Fmt;
use crate::settings::Settings;
use anyhow::{Context, Result, anyhow, bail};
use image::{DynamicImage, GenericImageView};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use parking_lot::Mutex;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static PDFIUM: OnceLock<Option<Pdfium>> = OnceLock::new();
/// PDFium is not thread safe. Every PDF operation holds this lock.
static LOCK: Mutex<()> = parking_lot::const_mutex(());

pub fn init(resource_dir: Option<PathBuf>) {
    PDFIUM.get_or_init(|| {
        let mut dirs = Vec::new();
        if let Some(d) = resource_dir {
            dirs.push(d.join("pdfium"));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(d) = exe.parent() {
                dirs.push(d.join("pdfium"));
            }
        }
        dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vendor/pdfium"));
        for d in dirs {
            let lib = Pdfium::pdfium_platform_library_name_at_path(&d);
            if lib.is_file() {
                match Pdfium::bind_to_library(&lib) {
                    Ok(bindings) => {
                        log::info!("using PDFium at {}", lib.display());
                        return Some(Pdfium::new(bindings));
                    }
                    Err(e) => log::warn!("could not load {}: {e}", lib.display()),
                }
            }
        }
        None
    });
}

fn pdfium() -> Result<&'static Pdfium> {
    PDFIUM
        .get()
        .and_then(|p| p.as_ref())
        .ok_or_else(|| anyhow!("The PDF engine is missing from this installation. Reinstall KiwiConvert to restore it."))
}

fn open<'a>(p: &'a Pdfium, path: &Path) -> Result<PdfDocument<'a>> {
    p.load_pdf_from_file(path, None).map_err(|e| match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            anyhow!("This PDF is password protected. Remove the password and try again.")
        }
        other => anyhow!("This PDF couldn't be opened: {other}"),
    })
}

fn render_page(page: &PdfPage, dpi: f32) -> Result<DynamicImage> {
    let scale = dpi / 72.0;
    let w = (page.width().value * scale).round().max(1.0) as i32;
    let h = (page.height().value * scale).round().max(1.0) as i32;
    // Keep very large pages within a sane pixel budget.
    let budget = 12_000.0 * 12_000.0;
    let factor = ((budget / (w as f64 * h as f64)).sqrt()).min(1.0);
    let config = PdfRenderConfig::new()
        .set_target_width((w as f64 * factor) as i32)
        .set_maximum_height((h as f64 * factor) as i32)
        .render_form_data(true)
        .render_annotations(true);
    let bitmap = page.render_with_config(&config)?;
    Ok(bitmap.as_image()?)
}

/// Renders every page to an image. One page becomes one file beside the PDF; several
/// pages go into a folder named after the PDF. TIFF output is a single multi-page file.
pub fn to_images(span: &Span, input: &Path, to: Fmt, settings: &Settings, out_dir: Option<&Path>) -> Result<Vec<PathBuf>> {
    let _guard = LOCK.lock();
    let p = pdfium()?;
    let doc = open(p, input)?;
    let pages = doc.pages();
    let n = pages.len() as usize;
    if n == 0 {
        bail!("This PDF has no pages.");
    }
    let dpi = settings.pdf_dpi.clamp(36, 600) as f32;
    let meta = Meta { icc: None, exif: None };
    let quality = img::quality_for(to, settings);

    if to == Fmt::Tiff {
        let mut rendered = Vec::with_capacity(n);
        for (i, page) in pages.iter().enumerate() {
            span.check()?;
            rendered.push(DynamicImage::ImageRgb8(render_page(&page, dpi)?.to_rgb8()));
            span.progress((i + 1) as f32 / n as f32 * 0.9);
        }
        let out = naming::output_for(input, out_dir, "", "tiff");
        return Ok(vec![write_atomic(&out, |tmp| {
            img::tiff_write(std::io::BufWriter::new(std::fs::File::create(tmp)?), &rendered, None)
        })?]);
    }

    let dir = if n == 1 {
        out_dir.map(Path::to_path_buf).or_else(|| input.parent().map(Path::to_path_buf)).unwrap_or_default()
    } else {
        let parent = out_dir.map(Path::to_path_buf).or_else(|| input.parent().map(Path::to_path_buf)).unwrap_or_default();
        let folder = naming::unique(&parent, &format!("{} pages", naming::stem(input)), "");
        std::fs::create_dir_all(&folder)?;
        folder
    };
    let digits = n.to_string().len().max(2);
    let mut outputs = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        span.check()?;
        let image = render_page(&page, dpi)?;
        let bytes = img::encode(&image, to, quality, &meta)?;
        let name = if n == 1 {
            naming::stem(input)
        } else {
            format!("page {:0digits$}", i + 1)
        };
        let out = naming::unique(&dir, &name, to.ext());
        std::fs::write(&out, bytes)?;
        if n == 1 {
            outputs.push(out);
        }
        span.progress((i + 1) as f32 / n as f32);
    }
    if n > 1 {
        outputs.push(dir);
    }
    Ok(outputs)
}

/// Text of each page.
pub fn page_texts(input: &Path) -> Result<Vec<String>> {
    let _guard = LOCK.lock();
    let p = pdfium()?;
    let doc = open(p, input)?;
    let mut out = Vec::new();
    for page in doc.pages().iter() {
        out.push(page.text().map(|t| t.all()).unwrap_or_default());
    }
    Ok(out)
}

pub fn to_text(span: &Span, input: &Path, out_dir: Option<&Path>) -> Result<PathBuf> {
    let texts = page_texts(input)?;
    span.progress(0.8);
    let body = texts
        .iter()
        .map(|t| t.replace("\r\n", "\n").trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n\n\u{000C}\n\n");
    let out = naming::output_for(input, out_dir, "", "txt");
    write_atomic(&out, |tmp| Ok(std::fs::write(tmp, body.as_bytes())?))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub doc: usize,
    pub index: usize,
    pub width: f32,
    pub height: f32,
}

pub fn page_infos(paths: &[PathBuf]) -> Result<Vec<PageInfo>> {
    let _guard = LOCK.lock();
    let p = pdfium()?;
    let mut out = Vec::new();
    for (d, path) in paths.iter().enumerate() {
        let doc = open(p, path)?;
        for (i, page) in doc.pages().iter().enumerate() {
            out.push(PageInfo {
                doc: d,
                index: i,
                width: page.width().value,
                height: page.height().value,
            });
        }
    }
    Ok(out)
}

/// A small JPEG of one page, for the organizer grid.
pub fn thumbnail(path: &Path, index: usize, max_side: u32) -> Result<Vec<u8>> {
    let _guard = LOCK.lock();
    let p = pdfium()?;
    let doc = open(p, path)?;
    let page = doc.pages().get(index as PdfPageIndex)?;
    let (w, h) = (page.width().value, page.height().value);
    let scale = max_side as f32 / w.max(h).max(1.0);
    let config = PdfRenderConfig::new()
        .set_target_width((w * scale).round().max(1.0) as i32)
        .set_maximum_height((h * scale).round().max(1.0) as i32)
        .render_form_data(true);
    let image = page.render_with_config(&config)?.as_image()?;
    img::jpeg_bytes(&image, 82, &Meta { icc: None, exif: None })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRef {
    pub doc: usize,
    pub index: usize,
    /// Clockwise quarter turns added to the page's own rotation.
    #[serde(default)]
    pub rotate: i32,
}

fn rotation_from_degrees(deg: i32) -> PdfPageRenderRotation {
    match deg.rem_euclid(360) {
        90 => PdfPageRenderRotation::Degrees90,
        180 => PdfPageRenderRotation::Degrees180,
        270 => PdfPageRenderRotation::Degrees270,
        _ => PdfPageRenderRotation::None,
    }
}

fn degrees(r: PdfPageRenderRotation) -> i32 {
    match r {
        PdfPageRenderRotation::Degrees90 => 90,
        PdfPageRenderRotation::Degrees180 => 180,
        PdfPageRenderRotation::Degrees270 => 270,
        PdfPageRenderRotation::None => 0,
    }
}

/// Builds a new PDF from pages of `sources`, in order, applying rotations.
pub fn assemble(sources: &[PathBuf], pages: &[PageRef], out: &Path) -> Result<PathBuf> {
    if pages.is_empty() {
        bail!("There are no pages to save.");
    }
    let _guard = LOCK.lock();
    let p = pdfium()?;
    let docs: Vec<PdfDocument> = sources.iter().map(|s| open(p, s)).collect::<Result<_>>()?;
    let mut new_doc = p.create_new_pdf()?;
    for (i, r) in pages.iter().enumerate() {
        let src = docs.get(r.doc).context("unknown source document")?;
        new_doc
            .pages_mut()
            .copy_page_from_document(src, r.index as PdfPageIndex, i as PdfPageIndex)?;
        if r.rotate % 4 != 0 {
            let mut page = new_doc.pages().get(i as PdfPageIndex)?;
            let current = page.rotation().map(degrees).unwrap_or(0);
            page.set_rotation(rotation_from_degrees(current + r.rotate * 90));
        }
    }
    write_atomic(out, |tmp| {
        new_doc.save_to_file(tmp)?;
        Ok(())
    })
}

pub fn merge(span: &Span, inputs: &[PathBuf], out_dir: Option<&Path>) -> Result<PathBuf> {
    let mut sorted = inputs.to_vec();
    sorted.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));
    let infos = page_infos(&sorted)?;
    span.progress(0.3);
    let pages: Vec<PageRef> = infos.iter().map(|i| PageRef { doc: i.doc, index: i.index, rotate: 0 }).collect();
    let dir = out_dir.map(Path::to_path_buf).or_else(|| sorted[0].parent().map(Path::to_path_buf)).unwrap_or_default();
    let out = naming::unique(&dir, &format!("{} (merged)", naming::combined_base(&sorted, "Merged")), "pdf");
    assemble(&sorted, &pages, &out)
}

/// Splits one PDF into files by page ranges (1-based, inclusive). An empty list means
/// every page on its own.
pub fn split(span: &Span, input: &Path, ranges: &[(usize, usize)], out_dir: Option<&Path>) -> Result<Vec<PathBuf>> {
    let infos = page_infos(&[input.to_path_buf()])?;
    let n = infos.len();
    let ranges: Vec<(usize, usize)> = if ranges.is_empty() {
        (1..=n).map(|i| (i, i)).collect()
    } else {
        ranges.iter().map(|(a, b)| ((*a).clamp(1, n), (*b).clamp(1, n))).filter(|(a, b)| a <= b).collect()
    };
    if ranges.is_empty() {
        bail!("None of the page ranges are inside this PDF ({n} pages).");
    }
    let parent = out_dir.map(Path::to_path_buf).or_else(|| input.parent().map(Path::to_path_buf)).unwrap_or_default();
    let dir = if ranges.len() > 3 {
        let folder = naming::unique(&parent, &format!("{} split", naming::stem(input)), "");
        std::fs::create_dir_all(&folder)?;
        folder
    } else {
        parent.clone()
    };
    let mut outputs = Vec::new();
    for (i, (a, b)) in ranges.iter().enumerate() {
        span.check()?;
        let pages: Vec<PageRef> = (*a..=*b).map(|p| PageRef { doc: 0, index: p - 1, rotate: 0 }).collect();
        let label = if a == b { format!("p{a}") } else { format!("p{a}-{b}") };
        let out = naming::unique(&dir, &format!("{}-{label}", naming::stem(input)), "pdf");
        outputs.push(assemble(&[input.to_path_buf()], &pages, &out)?);
        span.progress((i + 1) as f32 / ranges.len() as f32);
    }
    if dir != parent {
        return Ok(vec![dir]);
    }
    Ok(outputs)
}

// ---------------------------------------------------------------------------------------
// Building PDFs from images (lopdf)
// ---------------------------------------------------------------------------------------

/// Page size in points for the "page size" setting. `None` means "match the image".
fn paper(settings: &Settings) -> Option<(f32, f32)> {
    match settings.page_size.as_str() {
        "letter" => Some((612.0, 792.0)),
        "fit" => None,
        _ => Some((595.28, 841.89)),
    }
}

/// Adds an image XObject. JPEG files are embedded as-is; everything else is stored
/// losslessly with a soft mask for transparency.
fn add_image(doc: &mut Document, path: &Path) -> Result<(ObjectId, u32, u32)> {
    let is_jpeg = matches!(Fmt::from_ext(&crate::registry::ext_of(path)), Some(Fmt::Jpg));
    let loaded = img::load(path)?;
    let (w, h) = loaded.img.dimensions();
    if is_jpeg && !needs_rotation(path) {
        let data = std::fs::read(path)?;
        let cs = if loaded.img.color().channel_count() == 1 { "DeviceGray" } else { "DeviceRGB" };
        let stream = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                "ColorSpace" => cs, "BitsPerComponent" => 8, "Filter" => "DCTDecode",
            },
            data,
        );
        return Ok((doc.add_object(stream), w, h));
    }
    let rgb = loaded.img.to_rgb8();
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        },
        rgb.into_raw(),
    );
    stream.compress()?;
    if loaded.img.color().has_alpha() {
        let alpha: Vec<u8> = loaded.img.to_rgba8().pixels().map(|p| p[3]).collect();
        let mut mask = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            alpha,
        );
        mask.compress()?;
        let mask_id = doc.add_object(mask);
        stream.dict.set("SMask", mask_id);
    }
    Ok((doc.add_object(stream), w, h))
}

/// JPEGs with an EXIF rotation can't be embedded byte for byte.
fn needs_rotation(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else { return false };
    let mut reader = std::io::BufReader::new(file);
    exif::Reader::new()
        .read_from_container(&mut reader)
        .ok()
        .and_then(|e| e.get_field(exif::Tag::Orientation, exif::In::PRIMARY).and_then(|f| f.value.get_uint(0)))
        .is_some_and(|o| o != 1)
}

pub fn images_to_pdf(span: &Span, images: &[PathBuf], settings: &Settings, out: &Path) -> Result<PathBuf> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for (i, path) in images.iter().enumerate() {
        span.check()?;
        let (image_id, w, h) = add_image(&mut doc, path)?;
        let (pw, ph, dw, dh, x, y) = match paper(settings) {
            Some((a, b)) => {
                // Orient the page like the image, then fit the image with a small margin.
                let (pw, ph) = if w > h { (b, a) } else { (a, b) };
                let margin = 24.0;
                let s = ((pw - 2.0 * margin) / w as f32).min((ph - 2.0 * margin) / h as f32);
                let (dw, dh) = (w as f32 * s, h as f32 * s);
                (pw, ph, dw, dh, (pw - dw) / 2.0, (ph - dh) / 2.0)
            }
            None => {
                // One pixel per point at 96 DPI.
                let s = 72.0 / 96.0;
                let (pw, ph) = (w as f32 * s, h as f32 * s);
                (pw, ph, pw, ph, 0.0, 0.0)
            }
        };
        let content = format!("q {dw:.3} 0 0 {dh:.3} {x:.3} {y:.3} cm /Im0 Do Q");
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), Object::Real(pw), Object::Real(ph)],
            "Contents" => content_id,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        kids.push(page_id.into());
        span.progress((i + 1) as f32 / images.len() as f32 * 0.95);
    }
    let count = kids.len() as i64;
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let info_id = doc.add_object(dictionary! { "Producer" => Object::string_literal("KiwiConvert") });
    doc.trailer.set("Info", info_id);
    write_atomic(out, |tmp| {
        doc.save(tmp)?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------------------
// Compression (lopdf)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompressOptions {
    /// "small", "balanced", or "high".
    pub level: String,
    /// Target size in megabytes.
    pub target_mb: Option<f64>,
}

/// Re-encodes large embedded images as JPEG at a lower resolution. Text and vector
/// graphics are untouched, so the document stays sharp and searchable.
fn recompress_images(doc: &mut Document, max_side: u32, quality: u8) -> Result<usize> {
    let ids: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter_map(|(id, obj)| match obj {
            Object::Stream(s) if s.dict.get(b"Subtype").and_then(|v| v.as_name()).ok() == Some(b"Image".as_slice()) => Some(*id),
            _ => None,
        })
        .collect();
    let mut changed = 0;
    for id in ids {
        let Some(Object::Stream(stream)) = doc.objects.get(&id) else { continue };
        let dict = &stream.dict;
        // Leave masks, 1-bit images, and exotic color spaces alone.
        if dict.has(b"SMask") || dict.has(b"Mask") || dict.get(b"ImageMask").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let bpc = dict.get(b"BitsPerComponent").and_then(|v| v.as_i64()).unwrap_or(8);
        if bpc != 8 {
            continue;
        }
        let cs = dict.get(b"ColorSpace").and_then(|v| v.as_name()).map(|n| n.to_vec()).unwrap_or_default();
        let channels = match cs.as_slice() {
            b"DeviceRGB" => 3,
            b"DeviceGray" => 1,
            _ => continue,
        };
        let w = dict.get(b"Width").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
        let h = dict.get(b"Height").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
        if w < 64 || h < 64 {
            continue;
        }
        let filters: Vec<Vec<u8>> = match dict.get(b"Filter") {
            Ok(Object::Name(n)) => vec![n.clone()],
            Ok(Object::Array(a)) => a.iter().filter_map(|o| o.as_name().ok().map(|n| n.to_vec())).collect(),
            _ => vec![],
        };
        let original_len = stream.content.len();
        let decoded: DynamicImage = match filters.iter().map(|f| f.as_slice()).collect::<Vec<_>>().as_slice() {
            [b"DCTDecode"] => match image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg) {
                Ok(i) => i,
                Err(_) => continue,
            },
            [] | [b"FlateDecode"] => {
                let raw = match stream.decompressed_content() {
                    Ok(r) => r,
                    Err(_) if filters.is_empty() => stream.content.clone(),
                    Err(_) => continue,
                };
                if raw.len() < (w * h * channels) as usize {
                    continue;
                }
                let raw = raw[..(w * h * channels) as usize].to_vec();
                match channels {
                    3 => DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, raw).context("bad image")?),
                    _ => DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, raw).context("bad image")?),
                }
            }
            _ => continue,
        };
        let resized = if w.max(h) > max_side {
            decoded.resize(max_side, max_side, image::imageops::FilterType::Lanczos3)
        } else {
            decoded
        };
        let jpeg = if channels == 1 {
            let mut out = Vec::new();
            let gray = resized.to_luma8();
            let mut enc = jpeg_encoder::Encoder::new(&mut out, quality);
            enc.set_optimized_huffman_tables(true);
            enc.encode(gray.as_raw(), gray.width() as u16, gray.height() as u16, jpeg_encoder::ColorType::Luma)?;
            out
        } else {
            img::jpeg_bytes(&resized, quality, &Meta { icc: None, exif: None })?
        };
        if jpeg.len() >= original_len {
            continue;
        }
        let (nw, nh) = resized.dimensions();
        if let Some(Object::Stream(s)) = doc.objects.get_mut(&id) {
            s.dict.set("Filter", "DCTDecode");
            s.dict.remove(b"DecodeParms");
            s.dict.set("Width", nw as i64);
            s.dict.set("Height", nh as i64);
            s.set_content(jpeg);
            s.allows_compression = false;
            changed += 1;
        }
    }
    Ok(changed)
}

fn compressed_bytes(input: &Path, max_side: u32, quality: u8) -> Result<Vec<u8>> {
    let mut doc = Document::load(input).context("This PDF couldn't be read")?;
    if doc.is_encrypted() {
        bail!("This PDF is encrypted. Remove the password and try again.");
    }
    recompress_images(&mut doc, max_side, quality)?;
    doc.delete_zero_length_streams();
    doc.prune_objects();
    doc.compress();
    let mut out = Vec::new();
    doc.save_to(&mut out)?;
    Ok(out)
}

pub fn compress(span: &Span, input: &Path, o: &CompressOptions, out_dir: Option<&Path>) -> Result<PathBuf> {
    let original = std::fs::metadata(input)?.len();
    let bytes = if let Some(mb) = o.target_mb.filter(|m| *m > 0.0) {
        let target = (mb * 1024.0 * 1024.0) as usize;
        let steps: [(u32, u8); 6] = [(2400, 80), (2000, 70), (1600, 60), (1200, 50), (1000, 40), (800, 30)];
        let mut best = None;
        for (i, (side, q)) in steps.iter().enumerate() {
            span.check()?;
            let b = compressed_bytes(input, *side, *q)?;
            span.progress((i + 1) as f32 / steps.len() as f32);
            let fits = b.len() <= target;
            best = Some(b);
            if fits {
                break;
            }
        }
        best.context("compression failed")?
    } else {
        let (side, q) = match o.level.as_str() {
            "small" => (1200, 50),
            "high" => (2400, 82),
            _ => (1800, 68),
        };
        compressed_bytes(input, side, q)?
    };
    span.progress(0.98);
    if bytes.len() as u64 >= original {
        bail!("This PDF is already as small as KiwiConvert can make it.");
    }
    let out = naming::output_for(input, out_dir, "-compressed", "pdf");
    write_atomic(&out, |tmp| Ok(std::fs::write(tmp, &bytes)?))
}

// ---------------------------------------------------------------------------------------
// Metadata (lopdf)
// ---------------------------------------------------------------------------------------

fn pdf_text(obj: &Object) -> Option<String> {
    match obj {
        Object::String(bytes, _) => {
            if bytes.starts_with(&[0xFE, 0xFF]) {
                let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                Some(String::from_utf16_lossy(&units))
            } else {
                Some(bytes.iter().map(|b| *b as char).collect())
            }
        }
        _ => None,
    }
}

fn info_dict(doc: &Document) -> Option<Dictionary> {
    let info = doc.trailer.get(b"Info").ok()?;
    let dict = match info {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok()?.clone(),
        Object::Dictionary(d) => d.clone(),
        _ => return None,
    };
    Some(dict)
}

pub fn read_info(input: &Path) -> Result<(Vec<(String, String)>, usize, bool)> {
    let doc = Document::load(input).context("This PDF couldn't be read")?;
    let mut fields = Vec::new();
    if let Some(dict) = info_dict(&doc) {
        for (k, v) in dict.iter() {
            if let Some(text) = pdf_text(v) {
                fields.push((String::from_utf8_lossy(k).into_owned(), text));
            }
        }
    }
    let xmp = doc
        .catalog()
        .ok()
        .map(|c| c.has(b"Metadata"))
        .unwrap_or(false);
    Ok((fields, doc.get_pages().len(), xmp))
}

fn encode_text(s: &str) -> Object {
    if s.is_ascii() {
        Object::string_literal(s)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(bytes, lopdf::StringFormat::Hexadecimal)
    }
}

pub fn write_info(span: &Span, input: &Path, edits: &[(String, String)], strip: bool, out_dir: Option<&Path>) -> Result<PathBuf> {
    let mut doc = Document::load(input).context("This PDF couldn't be read")?;
    if doc.is_encrypted() {
        bail!("This PDF is encrypted. Remove the password and try again.");
    }
    span.progress(0.4);
    if strip {
        doc.trailer.remove(b"Info");
        if let Ok(catalog) = doc.catalog_mut() {
            catalog.remove(b"Metadata");
        }
        doc.prune_objects();
    } else {
        let mut dict = info_dict(&doc).unwrap_or_default();
        for (k, v) in edits {
            if v.is_empty() {
                dict.remove(k.as_bytes());
            } else {
                dict.set(k.as_bytes().to_vec(), encode_text(v));
            }
        }
        let id = doc.add_object(dict);
        doc.trailer.set("Info", id);
    }
    let suffix = if strip { "-clean" } else { "-edited" };
    let out = naming::output_for(input, out_dir, suffix, "pdf");
    write_atomic(&out, |tmp| {
        doc.save(tmp)?;
        Ok(())
    })
}
