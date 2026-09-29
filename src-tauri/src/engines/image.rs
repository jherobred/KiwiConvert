//! Still images: decoding with orientation, color profile and EXIF preserved, and encoding
//! to every supported format.

use super::{ffmpeg, heif, trace, write_atomic};
use crate::args;
use crate::jobs::Span;
use crate::naming;
use crate::registry::{Fmt, ext_of};
use crate::settings::Settings;
use anyhow::{Context, Result, bail};
use image::codecs::gif::GifEncoder;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::imageops::FilterType as Resample;
use image::metadata::Orientation;
use image::{DynamicImage, ExtendedColorType, GenericImageView, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, RgbaImage};
use serde::Deserialize;
use std::io::{BufWriter, Cursor, Write};
use std::path::{Path, PathBuf};

pub struct Loaded {
    pub img: DynamicImage,
    pub icc: Option<Vec<u8>>,
    /// TIFF-structured EXIF, with the orientation already applied to the pixels.
    pub exif: Option<Vec<u8>>,
}

fn limits() -> image::Limits {
    let mut l = image::Limits::default();
    l.max_alloc = Some(3 << 30);
    l.max_image_width = Some(50_000);
    l.max_image_height = Some(50_000);
    l
}

pub fn load(path: &Path) -> Result<Loaded> {
    match ext_of(path).as_str() {
        "svg" => Ok(Loaded {
            img: rasterize_svg(&std::fs::read(path)?, 0)?,
            icc: None,
            exif: None,
        }),
        "heic" | "heif" | "hif" | "avif" => load_heif(path),
        _ => {
            let mut reader = ImageReader::open(path)?
                .with_guessed_format()
                .context("could not read the image")?;
            reader.limits(limits());
            let mut decoder = reader
                .into_decoder()
                .context("this image format isn't supported or the file is damaged")?;
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            let icc = decoder.icc_profile().ok().flatten();
            let exif = decoder.exif_metadata().ok().flatten().map(|mut e| {
                reset_orientation(&mut e);
                e
            });
            let mut img = DynamicImage::from_decoder(decoder).context("could not decode the image")?;
            img.apply_orientation(orientation);
            Ok(Loaded { img, icc, exif })
        }
    }
}

fn load_heif(path: &Path) -> Result<Loaded> {
    let data = std::fs::read(path)?;
    let meta = heif::read_meta(&data).unwrap_or_default();
    let png = ffmpeg::capture(args![
        "-noautorotate", "-i", path, "-frames:v", "1", "-f", "image2pipe", "-c:v", "png", "pipe:1"
    ])?;
    let mut img = image::load_from_memory_with_format(&png, ImageFormat::Png)
        .context("FFmpeg could not decode this image")?;
    let rotate = |img: DynamicImage| match meta.rotation {
        1 => img.rotate270(),
        2 => img.rotate180(),
        3 => img.rotate90(),
        _ => img,
    };
    let mirror = |img: DynamicImage| match meta.mirror {
        Some(0) => img.fliph(),
        Some(_) => img.flipv(),
        None => img,
    };
    img = if meta.mirror_first { rotate(mirror(img)) } else { mirror(rotate(img)) };
    let exif = meta.exif.map(|mut e| {
        reset_orientation(&mut e);
        e
    });
    Ok(Loaded { img, icc: meta.icc, exif })
}

/// Renders SVG at its own size, or so its longer side is at least `min_side` pixels.
pub fn rasterize_svg(data: &[u8], min_side: u32) -> Result<DynamicImage> {
    let mut opts = resvg::usvg::Options::default();
    opts.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(data, &opts).context("this SVG couldn't be read")?;
    let size = tree.size();
    let longer = size.width().max(size.height()).max(1.0);
    let target = (min_side.max(1024) as f32).max(longer).min(8192.0);
    let scale = target / longer;
    let w = (size.width() * scale).ceil().max(1.0) as u32;
    let h = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).context("SVG is too large")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied alpha.
    let mut rgba = pixmap.take();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, rgba).context("bad SVG raster")?))
}

/// Sets the EXIF orientation tag to "normal" after the rotation has been applied to pixels,
/// so viewers don't rotate the image a second time.
pub fn reset_orientation(exif: &mut [u8]) {
    let big = match exif.get(0..2) {
        Some(b"MM") => true,
        Some(b"II") => false,
        _ => return,
    };
    let rd16 = |b: &[u8], i: usize| -> Option<u16> {
        let v: [u8; 2] = b.get(i..i + 2)?.try_into().ok()?;
        Some(if big { u16::from_be_bytes(v) } else { u16::from_le_bytes(v) })
    };
    let rd32 = |b: &[u8], i: usize| -> Option<u32> {
        let v: [u8; 4] = b.get(i..i + 4)?.try_into().ok()?;
        Some(if big { u32::from_be_bytes(v) } else { u32::from_le_bytes(v) })
    };
    let Some(ifd) = rd32(exif, 4).map(|v| v as usize) else { return };
    let Some(count) = rd16(exif, ifd) else { return };
    for i in 0..count as usize {
        let e = ifd + 2 + i * 12;
        if rd16(exif, e) == Some(0x0112) {
            let one = if big { [0u8, 1] } else { [1u8, 0] };
            if let Some(slot) = exif.get_mut(e + 8..e + 10) {
                slot.copy_from_slice(&one);
            }
            return;
        }
    }
}

fn flatten(img: &DynamicImage) -> image::RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut out = image::RgbImage::new(w, h);
    for (dst, src) in out.pixels_mut().zip(rgba.pixels()) {
        let a = src[3] as u32;
        for c in 0..3 {
            dst[c] = ((src[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
    }
    out
}

pub struct Meta<'a> {
    pub icc: Option<&'a [u8]>,
    pub exif: Option<&'a [u8]>,
}

pub fn jpeg_bytes(img: &DynamicImage, quality: u8, meta: &Meta) -> Result<Vec<u8>> {
    let rgb = flatten(img);
    let (w, h) = rgb.dimensions();
    if w > 65_535 || h > 65_535 {
        bail!("JPEG can't store images larger than 65535 pixels on a side.");
    }
    let mut out = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100));
    enc.set_progressive(true);
    enc.set_optimized_huffman_tables(true);
    if quality >= 92 {
        enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
    }
    if let Some(icc) = meta.icc {
        let _ = enc.add_icc_profile(icc);
    }
    if let Some(exif) = meta.exif {
        let _ = enc.add_exif_metadata(exif);
    }
    enc.encode(rgb.as_raw(), w as u16, h as u16, jpeg_encoder::ColorType::Rgb)?;
    Ok(out)
}

pub fn png_bytes(img: &DynamicImage, meta: &Meta, best: bool) -> Result<Vec<u8>> {
    let img = match img {
        DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_) | DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) => img.clone(),
        DynamicImage::ImageLuma16(_) | DynamicImage::ImageLumaA16(_) | DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) => img.clone(),
        other if other.color().has_alpha() => DynamicImage::ImageRgba8(other.to_rgba8()),
        other => DynamicImage::ImageRgb8(other.to_rgb8()),
    };
    let mut out = Vec::new();
    let compression = if best { CompressionType::Best } else { CompressionType::Default };
    let mut enc = PngEncoder::new_with_quality(&mut out, compression, FilterType::Adaptive);
    if let Some(icc) = meta.icc {
        let _ = enc.set_icc_profile(icc.to_vec());
    }
    if let Some(exif) = meta.exif {
        let _ = enc.set_exif_metadata(exif.to_vec());
    }
    enc.write_image(img.as_bytes(), img.width(), img.height(), img.color().into())?;
    Ok(out)
}

pub fn webp_bytes(img: &DynamicImage, quality: Option<u8>, meta: &Meta) -> Result<Vec<u8>> {
    let img = if img.color().has_alpha() {
        DynamicImage::ImageRgba8(img.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(img.to_rgb8())
    };
    if img.width() > 16_383 || img.height() > 16_383 {
        bail!("WebP can't store images larger than 16383 pixels on a side.");
    }
    let enc = webp::Encoder::from_image(&img).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mem = match quality {
        Some(q) => enc.encode(q as f32),
        None => enc.encode_lossless(),
    };
    let bytes = mem.to_vec();
    if meta.icc.is_none() && meta.exif.is_none() {
        return Ok(bytes);
    }
    let mut file = img_parts::webp::WebP::from_bytes(bytes.clone().into()).map_err(|e| anyhow::anyhow!("{e}"))?;
    use img_parts::ImageEXIF;
    use img_parts::ImageICC;
    if let Some(icc) = meta.icc {
        file.set_icc_profile(Some(icc.to_vec().into()));
    }
    if let Some(exif) = meta.exif {
        file.set_exif(Some(exif.to_vec().into()));
    }
    let mut out = Vec::new();
    file.encoder().write_to(&mut out)?;
    Ok(out)
}

fn gif_bytes(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new_with_speed(&mut out, 10);
        enc.encode_frame(image::Frame::new(img.to_rgba8()))?;
    }
    Ok(out)
}

fn bmp_bytes(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        image::codecs::bmp::BmpEncoder::new(&mut out).encode(rgba.as_raw(), rgba.width(), rgba.height(), ExtendedColorType::Rgba8)?;
    } else {
        let rgb = img.to_rgb8();
        image::codecs::bmp::BmpEncoder::new(&mut out).encode(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)?;
    }
    Ok(out)
}

/// Writes one or more pages to a Deflate-compressed TIFF.
pub fn tiff_write<W: Write + std::io::Seek>(w: W, pages: &[DynamicImage], icc: Option<&[u8]>) -> Result<()> {
    use tiff::encoder::{Compression, TiffEncoder, colortype, compression::DeflateLevel};
    use tiff::tags::Tag;
    let mut enc = TiffEncoder::new(w)?.with_compression(Compression::Deflate(DeflateLevel::Balanced));
    for page in pages {
        let (width, height) = page.dimensions();
        macro_rules! write {
            ($ct:ty, $data:expr) => {{
                let mut image = enc.new_image::<$ct>(width, height)?;
                if let Some(icc) = icc {
                    image.encoder().write_tag(Tag::Unknown(34675), icc)?;
                }
                image.write_data($data)?;
            }};
        }
        match page {
            DynamicImage::ImageRgb16(i) => write!(colortype::RGB16, i.as_raw()),
            DynamicImage::ImageRgba16(i) => write!(colortype::RGBA16, i.as_raw()),
            DynamicImage::ImageLuma8(i) => write!(colortype::Gray8, i.as_raw()),
            p if p.color().has_alpha() => write!(colortype::RGBA8, p.to_rgba8().as_raw()),
            p => write!(colortype::RGB8, p.to_rgb8().as_raw()),
        }
    }
    Ok(())
}

fn ico_bytes(img: &DynamicImage) -> Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let side = w.max(h);
    let mut square = RgbaImage::new(side, side);
    image::imageops::overlay(&mut square, &img.to_rgba8(), ((side - w) / 2) as i64, ((side - h) / 2) as i64);
    let square = DynamicImage::ImageRgba8(square);
    let mut frames = Vec::new();
    for size in [16u32, 24, 32, 48, 64, 128, 256] {
        let resized = square.resize_exact(size, size, Resample::Lanczos3).to_rgba8();
        frames.push(IcoFrame::as_png(resized.as_raw(), size, size, ExtendedColorType::Rgba8)?);
    }
    let mut out = Vec::new();
    IcoEncoder::new(&mut out).encode_images(&frames)?;
    Ok(out)
}

/// AVIF through FFmpeg's libaom encoder, keeping transparency as an alpha plane.
fn avif_file(img: &DynamicImage, quality: u8, out: &Path) -> Result<()> {
    let tmp = std::env::temp_dir().join(format!("kiwi-{}.png", uuid::Uuid::new_v4().simple()));
    let alpha = img.color().has_alpha();
    std::fs::write(&tmp, png_bytes(img, &Meta { icc: None, exif: None }, false)?)?;
    let crf = (60.0 - quality as f64 * 0.5).clamp(8.0, 55.0).round() as u32;
    let mut a = args!["-y", "-i", &tmp];
    if alpha {
        a.extend(args![
            "-filter_complex", "[0:v]format=rgba,split[c][a];[c]format=yuv420p[main];[a]alphaextract,format=gray[alpha]",
            "-map", "[main]", "-map", "[alpha]"
        ]);
    } else {
        a.extend(args!["-pix_fmt", "yuv420p"]);
    }
    a.extend(args![
        "-c:v", "libaom-av1", "-still-picture", "1", "-crf", crf.to_string(), "-cpu-used", "6",
        "-row-mt", "1", "-f", "avif", out
    ]);
    let result = ffmpeg::capture(a);
    let _ = std::fs::remove_file(&tmp);
    result?;
    if std::fs::metadata(out).map(|m| m.len()).unwrap_or(0) == 0 {
        bail!("the AVIF encoder produced no output");
    }
    Ok(())
}

/// HEIC: x265 encodes the frame, `heif` wraps it with the EXIF and color profile.
fn heic_bytes(img: &DynamicImage, quality: u8, meta: &Meta) -> Result<Vec<u8>> {
    let rgb = flatten(img);
    let (w, h) = rgb.dimensions();
    let (cw, ch) = (w + w % 2, h + h % 2);
    let padded = if (cw, ch) == (w, h) {
        rgb
    } else {
        let mut p = image::RgbImage::new(cw, ch);
        image::imageops::overlay(&mut p, &rgb, 0, 0);
        for y in 0..ch {
            for x in 0..cw {
                if x >= w || y >= h {
                    let src = *p.get_pixel(x.min(w - 1), y.min(h - 1));
                    p.put_pixel(x, y, src);
                }
            }
        }
        p
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let png = std::env::temp_dir().join(format!("kiwi-{id}.png"));
    let mp4 = std::env::temp_dir().join(format!("kiwi-{id}.mp4"));
    DynamicImage::ImageRgb8(padded).save_with_format(&png, ImageFormat::Png)?;
    let crf = (42.0 - quality as f64 * 0.25).clamp(10.0, 40.0).round() as u32;
    let result = ffmpeg::capture(args![
        "-y", "-i", &png, "-frames:v", "1", "-c:v", "libx265", "-preset", "medium", "-crf", crf.to_string(),
        "-x265-params", "log-level=error", "-pix_fmt", "yuv420p", "-colorspace", "smpte170m",
        "-color_primaries", "bt709", "-color_trc", "iec61966-2-1", "-color_range", "tv",
        "-tag:v", "hvc1", "-f", "mp4", &mp4
    ]);
    let _ = std::fs::remove_file(&png);
    result?;
    let data = std::fs::read(&mp4);
    let _ = std::fs::remove_file(&mp4);
    let frame = heif::hevc_from_mp4(&data?)?;
    Ok(heif::build_heic(&frame, cw, ch, (w, h), meta.exif, meta.icc))
}

/// Encodes `img` to `fmt` into memory. AVIF, SVG, PDF, and DOCX are written by other paths.
pub fn encode(img: &DynamicImage, fmt: Fmt, quality: u8, meta: &Meta) -> Result<Vec<u8>> {
    match fmt {
        Fmt::Jpg => jpeg_bytes(img, quality, meta),
        Fmt::Png => png_bytes(img, meta, false),
        Fmt::Webp => webp_bytes(img, Some(quality), meta),
        Fmt::Gif => gif_bytes(img),
        Fmt::Bmp => bmp_bytes(img),
        Fmt::Ico => ico_bytes(img),
        Fmt::Tiff => {
            let mut c = Cursor::new(Vec::new());
            tiff_write(&mut c, std::slice::from_ref(img), meta.icc)?;
            Ok(c.into_inner())
        }
        Fmt::Heic => heic_bytes(img, quality, meta),
        _ => bail!("{} isn't an in-memory image format", fmt.label()),
    }
}

pub fn quality_for(fmt: Fmt, s: &Settings) -> u8 {
    match fmt {
        Fmt::Jpg => s.jpeg_quality,
        Fmt::Webp => s.webp_quality,
        Fmt::Avif => s.avif_quality,
        Fmt::Heic => s.heic_quality,
        _ => 90,
    }
}

/// Writes a decoded image to `out` in `fmt`.
pub fn save(loaded: &Loaded, fmt: Fmt, quality: u8, keep_meta: bool, out: &Path) -> Result<PathBuf> {
    let meta = Meta {
        icc: loaded.icc.as_deref(),
        exif: if keep_meta { loaded.exif.as_deref() } else { None },
    };
    write_atomic(out, |tmp| match fmt {
        Fmt::Avif => avif_file(&loaded.img, quality, tmp),
        Fmt::Svg => {
            let svg = trace::to_svg(&loaded.img)?;
            std::fs::write(tmp, svg)?;
            Ok(())
        }
        _ => {
            let bytes = encode(&loaded.img, fmt, quality, &meta)?;
            let mut f = BufWriter::new(std::fs::File::create(tmp)?);
            f.write_all(&bytes)?;
            f.flush()?;
            Ok(())
        }
    })
}

/// Converts one image file.
pub fn convert(span: &Span, input: &Path, to: Fmt, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    span.progress(0.05);
    let loaded = load(input)?;
    span.check()?;
    span.progress(0.4);
    let out = naming::output_for(input, out_dir, "", to.ext());
    let result = save(&loaded, to, quality_for(to, settings), settings.keep_metadata, &out)?;
    span.progress(1.0);
    Ok(result)
}

// ---------------------------------------------------------------------------------------
// Compress and resize
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompressOptions {
    /// "small", "balanced", or "high".
    pub level: String,
    /// Target size in kilobytes. Overrides `level` when set.
    pub target_kb: Option<f64>,
    /// Longest side limit in pixels.
    pub max_side: Option<u32>,
    /// "keep", "jpg", or "webp".
    pub format: Option<String>,
}

/// The format a compressed copy is written in.
pub fn compress_format(input: &Path, img: &DynamicImage, o: &CompressOptions) -> Fmt {
    match o.format.as_deref() {
        Some("jpg") => return Fmt::Jpg,
        Some("webp") => return Fmt::Webp,
        _ => {}
    }
    match Fmt::from_ext(&ext_of(input)) {
        Some(f @ (Fmt::Jpg | Fmt::Webp | Fmt::Heic | Fmt::Avif | Fmt::Png)) => f,
        // Formats without lossy compression become JPEG (or PNG to keep transparency).
        _ if img.color().has_alpha() => Fmt::Png,
        _ => Fmt::Jpg,
    }
}

fn level_quality(level: &str) -> u8 {
    match level {
        "small" => 55,
        "high" => 86,
        _ => 72,
    }
}

fn limit_side(img: DynamicImage, max_side: Option<u32>) -> DynamicImage {
    match max_side {
        Some(m) if img.width().max(img.height()) > m => img.resize(m, m, Resample::Lanczos3),
        _ => img,
    }
}

/// Reduces a PNG to a 256 color palette. Much smaller for screenshots and graphics.
fn quantized_png(img: &DynamicImage, meta: &Meta) -> Result<Vec<u8>> {
    let rgba = img.to_rgba8();
    let nq = color_quant::NeuQuant::new(10, 256, rgba.as_raw());
    let palette = nq.color_map_rgba();
    let (w, h) = rgba.dimensions();
    let indices: Vec<u8> = rgba.pixels().map(|p| nq.index_of(&p.0) as u8).collect();
    let rgb: Vec<u8> = palette.chunks_exact(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
    let alpha: Vec<u8> = palette.chunks_exact(4).map(|c| c[3]).collect();
    let mut info = png::Info::with_size(w, h);
    info.color_type = png::ColorType::Indexed;
    info.bit_depth = png::BitDepth::Eight;
    info.palette = Some(rgb.into());
    if alpha.iter().any(|a| *a < 255) {
        info.trns = Some(alpha.into());
    }
    info.icc_profile = meta.icc.map(|icc| icc.to_vec().into());
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::with_info(&mut out, info)?;
        enc.set_compression(png::Compression::High);
        let mut writer = enc.write_header()?;
        writer.write_image_data(&indices)?;
    }
    Ok(out)
}

fn encode_for_size(img: &DynamicImage, fmt: Fmt, quality: u8, level: &str, meta: &Meta) -> Result<Vec<u8>> {
    match fmt {
        Fmt::Png if level == "high" => png_bytes(img, meta, true),
        Fmt::Png => quantized_png(img, meta),
        Fmt::Avif => {
            let tmp = std::env::temp_dir().join(format!("kiwi-{}.avif", uuid::Uuid::new_v4().simple()));
            avif_file(img, quality, &tmp)?;
            let bytes = std::fs::read(&tmp);
            let _ = std::fs::remove_file(&tmp);
            Ok(bytes?)
        }
        _ => encode(img, fmt, quality, meta),
    }
}

pub fn compress(span: &Span, input: &Path, o: &CompressOptions, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let loaded = load(input)?;
    span.progress(0.1);
    let fmt = compress_format(input, &loaded.img, o);
    let meta = Meta {
        icc: loaded.icc.as_deref(),
        exif: if settings.keep_metadata { loaded.exif.as_deref() } else { None },
    };
    let mut img = limit_side(loaded.img.clone(), o.max_side);
    let level = if o.level.is_empty() { "balanced" } else { o.level.as_str() };

    let bytes = if let Some(kb) = o.target_kb.filter(|k| *k > 0.0) {
        let target = (kb * 1024.0) as usize;
        let mut best: Option<Vec<u8>> = None;
        // Shrink the image only if even the lowest quality is too large.
        'outer: for round in 0..8 {
            span.check()?;
            let (mut lo, mut hi) = (5u8, 95u8);
            let lossless = fmt == Fmt::Png;
            if lossless {
                let b = encode_for_size(&img, fmt, 0, "balanced", &meta)?;
                if b.len() <= target {
                    best = Some(b);
                    break 'outer;
                }
            } else {
                while lo <= hi {
                    let q = lo + (hi - lo) / 2;
                    let b = encode_for_size(&img, fmt, q, level, &meta)?;
                    span.progress(0.1 + 0.85 * (round as f32 * 7.0 + (95 - (hi - lo)) as f32 / 95.0 * 7.0) / 56.0);
                    if b.len() <= target {
                        best = Some(b);
                        lo = q + 1;
                    } else if q == 0 {
                        break;
                    } else {
                        hi = q - 1;
                    }
                }
                if best.is_some() {
                    break 'outer;
                }
            }
            let (w, h) = img.dimensions();
            if w.max(h) < 64 {
                break;
            }
            img = img.resize((w as f32 * 0.8) as u32, (h as f32 * 0.8) as u32, Resample::Lanczos3);
        }
        best.ok_or_else(|| anyhow::anyhow!("Couldn't get this image under {kb:.0} KB."))?
    } else {
        encode_for_size(&img, fmt, level_quality(level), level, &meta)?
    };
    span.progress(0.97);
    let out = naming::output_for(input, out_dir, "-compressed", fmt.ext());
    write_atomic(&out, |tmp| {
        std::fs::write(tmp, &bytes)?;
        Ok(())
    })
}

/// Encodes with the compress settings and returns only the size, for the live estimate.
pub fn estimate(input: &Path, o: &CompressOptions) -> Result<u64> {
    let loaded = load(input)?;
    let fmt = compress_format(input, &loaded.img, o);
    // Estimate on a reduced copy for speed, then scale by the pixel ratio.
    let img = limit_side(loaded.img, o.max_side);
    let (w, h) = img.dimensions();
    let preview_side = 1600u32;
    let (small, ratio) = if w.max(h) > preview_side {
        let s = img.resize(preview_side, preview_side, Resample::Triangle);
        let r = (w as f64 * h as f64) / (s.width() as f64 * s.height() as f64);
        (s, r)
    } else {
        (img, 1.0)
    };
    let level = if o.level.is_empty() { "balanced" } else { o.level.as_str() };
    let meta = Meta { icc: None, exif: None };
    let bytes = encode_for_size(&small, fmt, level_quality(level), level, &meta)?;
    Ok((bytes.len() as f64 * ratio) as u64)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ResizeOptions {
    /// Scale in percent, used when width and height are not given.
    pub percent: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Keep proportions when both sides are given (fit inside the box).
    pub keep_aspect: bool,
}

pub fn target_size(w: u32, h: u32, o: &ResizeOptions) -> (u32, u32) {
    let (nw, nh) = match (o.width, o.height, o.percent) {
        (Some(tw), Some(th), _) if o.keep_aspect => {
            let s = (tw as f64 / w as f64).min(th as f64 / h as f64);
            ((w as f64 * s).round() as u32, (h as f64 * s).round() as u32)
        }
        (Some(tw), Some(th), _) => (tw, th),
        (Some(tw), None, _) => (tw, (h as f64 * tw as f64 / w as f64).round() as u32),
        (None, Some(th), _) => ((w as f64 * th as f64 / h as f64).round() as u32, th),
        (None, None, Some(p)) => ((w as f64 * p / 100.0).round() as u32, (h as f64 * p / 100.0).round() as u32),
        _ => (w, h),
    };
    (nw.clamp(1, 50_000), nh.clamp(1, 50_000))
}

pub fn resize(span: &Span, input: &Path, o: &ResizeOptions, settings: &Settings, out_dir: Option<&Path>) -> Result<PathBuf> {
    let loaded = load(input)?;
    span.progress(0.3);
    let (w, h) = loaded.img.dimensions();
    let (nw, nh) = target_size(w, h, o);
    let img = loaded.img.resize_exact(nw, nh, Resample::Lanczos3);
    let fmt = match Fmt::from_ext(&ext_of(input)) {
        Some(Fmt::Svg) | None => Fmt::Png,
        Some(f) => f,
    };
    let out = naming::output_for(input, out_dir, &format!("-{nw}x{nh}"), fmt.ext());
    let resized = Loaded { img, icc: loaded.icc, exif: loaded.exif };
    save(&resized, fmt, quality_for(fmt, settings).max(88), settings.keep_metadata, &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_tag_is_reset() {
        // Little-endian TIFF with one IFD0 entry: Orientation (0x0112) SHORT 1 value 6.
        let mut exif = vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0];
        reset_orientation(&mut exif);
        assert_eq!(exif[18], 1);
        assert_eq!(exif[19], 0);
    }

    #[test]
    fn resize_math() {
        let fit = ResizeOptions { width: Some(100), height: Some(100), keep_aspect: true, ..Default::default() };
        assert_eq!(target_size(400, 200, &fit), (100, 50));
        let pct = ResizeOptions { percent: Some(50.0), ..Default::default() };
        assert_eq!(target_size(400, 200, &pct), (200, 100));
        let w_only = ResizeOptions { width: Some(200), ..Default::default() };
        assert_eq!(target_size(400, 300, &w_only), (200, 150));
    }

    #[test]
    fn encoders_round_trip() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_fn(37, 21, |x, y| image::Rgba([x as u8 * 6, y as u8 * 10, 128, if x < 5 { 0 } else { 255 }])));
        let meta = Meta { icc: None, exif: None };
        for fmt in [Fmt::Jpg, Fmt::Png, Fmt::Webp, Fmt::Gif, Fmt::Bmp, Fmt::Tiff, Fmt::Ico] {
            let bytes = encode(&img, fmt, 80, &meta).unwrap();
            let back = image::load_from_memory(&bytes).unwrap_or_else(|e| panic!("{fmt:?}: {e}"));
            if fmt != Fmt::Ico {
                assert_eq!(back.dimensions(), (37, 21), "{fmt:?}");
            }
        }
        let q = quantized_png(&img, &meta).unwrap();
        assert_eq!(image::load_from_memory(&q).unwrap().dimensions(), (37, 21));
    }
}
