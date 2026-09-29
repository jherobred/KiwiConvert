//! End-to-end checks of every conversion the wheel offers and every tool, run against
//! generated sample files. They need the real engines, so they are ignored by default:
//!
//!     npm run fetch:binaries
//!     cargo test -- --ignored
//!
//! DOCX, TXT and Markdown to PDF print through WebView2 and are exercised in the app instead.

use super::{archive, docs, ffmpeg, image as img, pdf};
use crate::args;
use crate::convert::convert_one;
use crate::jobs::Ctx;
use crate::registry::{Action, Fmt, Kind, Mode, kind_of, wheel_options};
use crate::settings::Settings;
use anyhow::{Context, Result, ensure};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use std::path::{Path, PathBuf};

fn engines() {
    ffmpeg::init(None);
    pdf::init(None);
    assert!(ffmpeg::tools().is_ok(), "FFmpeg not found. Run `npm run fetch:binaries` first.");
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kiwi-{tag}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn gradient(w: u32, h: u32, alpha: bool) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
        let a = if alpha && x < w / 5 { 0 } else { 255 };
        Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 140, a])
    }))
}

fn ff(args: Vec<std::ffi::OsString>) {
    ffmpeg::capture(args).expect("ffmpeg failed to make a sample");
}

/// One sample of every input kind KiwiConvert handles.
fn make_samples(dir: &Path) -> Vec<PathBuf> {
    let p = |n: &str| dir.join(n);
    gradient(320, 200, true).save(p("photo.png")).unwrap();
    gradient(300, 180, false).to_rgb8().save(p("shot.jpg")).unwrap();
    {
        let file = std::fs::File::create(p("anim.gif")).unwrap();
        let mut enc = image::codecs::gif::GifEncoder::new(file);
        for shade in [40u8, 200] {
            let frame = RgbaImage::from_pixel(64, 48, Rgba([shade, 120, 60, 255]));
            let delay = image::Delay::from_numer_denom_ms(600, 1);
            enc.encode_frame(image::Frame::from_parts(frame, 0, 0, delay)).unwrap();
        }
    }
    std::fs::write(p("logo.svg"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><rect width="120" height="80" fill="#7cc23a"/><circle cx="60" cy="40" r="25" fill="#1d1812"/></svg>"##).unwrap();
    ff(args![
        "-y", "-f", "lavfi", "-i", "testsrc2=size=320x240:rate=25:duration=2", "-f", "lavfi", "-i", "sine=frequency=440:duration=2",
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", &p("clip.mp4")
    ]);
    ff(args!["-y", "-f", "lavfi", "-i", "sine=frequency=330:duration=2", "-ac", "2", &p("song.wav")]);
    std::fs::write(p("notes.txt"), "First line\r\nSecond line with café\r\n").unwrap();
    std::fs::write(p("readme.md"), "# Title\n\nSome **bold** text.\n").unwrap();
    std::fs::write(p("subs.srt"), "1\r\n00:00:01,000 --> 00:00:02,000\r\nHello\r\n").unwrap();
    std::fs::write(p("subs.vtt"), "WEBVTT\n\n00:01.000 --> 00:02.000\nHello\n").unwrap();
    docs::write_docx(&p("doc.docx"), "Doc", &[docs::Block::Heading("Title".into()), docs::Block::Paragraph("Body text.".into())], false).unwrap();
    let ctx = Ctx::detached();
    pdf::images_to_pdf(&ctx.span(), &[p("shot.jpg"), p("photo.png")], &Settings::default(), &p("doc.pdf")).unwrap();
    text_pdf(&p("text.pdf"));
    archive::create(&ctx.span(), &[p("notes.txt"), p("subs.srt")], Fmt::Zip, &p("pack.zip")).unwrap();
    archive::create(&ctx.span(), &[p("notes.txt")], Fmt::Tgz, &p("pack.tar.gz")).unwrap();
    std::fs::write(p("data.bin"), [7u8; 1000]).unwrap();
    [
        "photo.png", "shot.jpg", "anim.gif", "logo.svg", "clip.mp4", "song.wav", "notes.txt", "readme.md", "subs.srt", "subs.vtt",
        "doc.docx", "doc.pdf", "text.pdf", "pack.zip", "pack.tar.gz", "data.bin",
    ]
    .iter()
    .map(|n| p(n))
    .collect()
}

/// A one-page PDF with real text, written directly with lopdf.
fn text_pdf(path: &Path) {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let content = b"BT /F1 18 Tf 72 720 Td (Hello from KiwiConvert) Tj 0 -24 Td (Second line of text.) Tj ET".to_vec();
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
    });
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

/// Conversions that are expected to refuse a sample.
fn expected_refusal(input: &Path, to: Fmt) -> bool {
    // doc.pdf is made of images only, so it has no text to extract.
    input.file_name().is_some_and(|n| n == "doc.pdf") && matches!(to, Fmt::Txt | Fmt::Docx)
}

/// Checks that a produced file is readable as what it claims to be.
fn verify(path: &Path, to: Fmt) -> Result<()> {
    if to == Fmt::Extract {
        ensure!(path.is_dir() && std::fs::read_dir(path)?.next().is_some(), "extraction produced an empty folder");
        return Ok(());
    }
    if path.is_dir() {
        // Multi-page PDF to images goes into a folder.
        for entry in std::fs::read_dir(path)? {
            verify(&entry?.path(), to)?;
        }
        return Ok(());
    }
    ensure!(std::fs::metadata(path)?.len() > 0, "empty output");
    match to {
        Fmt::Jpg | Fmt::Png | Fmt::Webp | Fmt::Gif | Fmt::Bmp | Fmt::Tiff | Fmt::Ico => {
            // Animated WebP from GIF is checked by FFmpeg instead.
            if to == Fmt::Webp && image::open(path).is_err() {
                ffmpeg::probe(path)?;
            } else {
                image::open(path).with_context(|| format!("{} doesn't decode", path.display()))?;
            }
        }
        Fmt::Heic | Fmt::Avif => {
            let loaded = img::load(path)?;
            ensure!(loaded.img.width() > 8, "decoded {} is too small", to.label());
        }
        Fmt::Svg => {
            let data = std::fs::read(path)?;
            resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).context("bad SVG")?;
        }
        Fmt::Pdf => ensure!(!pdf::page_infos(&[path.to_path_buf()])?.is_empty(), "PDF has no pages"),
        Fmt::Docx => {
            docs::read_docx(path)?;
        }
        Fmt::Txt | Fmt::Srt | Fmt::Vtt => ensure!(!std::fs::read_to_string(path)?.trim().is_empty(), "empty text"),
        Fmt::Zip | Fmt::Tar | Fmt::Tgz => {
            let ctx = Ctx::detached();
            let dest = temp_dir("verify");
            archive::extract_into(&ctx.span(), path, &dest)?;
            ensure!(std::fs::read_dir(&dest)?.next().is_some(), "archive is empty");
        }
        Fmt::Gz => ensure!(std::fs::metadata(path)?.len() > 10, "gzip too small"),
        Fmt::Mp4 | Fmt::Mov | Fmt::Mkv | Fmt::Avi | Fmt::Wmv | Fmt::Webm | Fmt::Mp3 | Fmt::M4a | Fmt::Wav | Fmt::Flac | Fmt::Ogg | Fmt::Opus | Fmt::Aiff => {
            let p = ffmpeg::probe(path)?;
            ensure!(p.duration() > 0.5, "media is too short ({}s)", p.duration());
            let wants_video = matches!(to, Fmt::Mp4 | Fmt::Mov | Fmt::Mkv | Fmt::Avi | Fmt::Wmv | Fmt::Webm);
            ensure!(!wants_video || p.video().is_some(), "no video stream");
            ensure!(wants_video || p.audio().is_some(), "no audio stream");
        }
        Fmt::Extract => unreachable!(),
    }
    Ok(())
}

/// Conversions that print through WebView2 and need the running app.
fn needs_webview(input: &Path, to: Fmt) -> bool {
    to == Fmt::Pdf && matches!(kind_of(input), Kind::Docx | Kind::Text)
}

#[test]
#[ignore = "needs vendor/ffmpeg and vendor/pdfium"]
fn every_wheel_conversion_produces_a_readable_file() {
    engines();
    let dir = temp_dir("matrix");
    let samples = make_samples(&dir);
    let ctx = Ctx::detached();
    let settings = Settings { heic_quality: 60, avif_quality: 50, ..Settings::default() };
    let mut failures = Vec::new();
    let mut count = 0;
    for input in &samples {
        for option in wheel_options(std::slice::from_ref(input), Mode::Convert) {
            let Action::Convert { to } = option.action else { continue };
            if needs_webview(input, to) || expected_refusal(input, to) {
                continue;
            }
            let name = input.file_name().unwrap().to_string_lossy().replace('.', "_");
            let out = dir.join(format!("out_{name}_{}", to.label().replace('.', "_")));
            std::fs::create_dir_all(&out).unwrap();
            count += 1;
            let result = convert_one(&ctx.span(), input, to, &settings, Some(&out))
                .and_then(|outputs| outputs.iter().try_for_each(|o| verify(o, to)));
            if let Err(e) = result {
                failures.push(format!("{} -> {}: {e:#}", input.file_name().unwrap().to_string_lossy(), to.label()));
            }
        }
    }
    println!("{count} conversions checked");
    assert!(failures.is_empty(), "{} of {count} conversions failed:\n{}", failures.len(), failures.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "needs vendor/ffmpeg"]
fn heic_round_trip_keeps_size_color_and_exif() {
    engines();
    let dir = temp_dir("heic");
    let src = dir.join("in.png");
    // Odd dimensions exercise the padding and clean-aperture crop.
    gradient(101, 67, false).save(&src).unwrap();
    let mut loaded = img::load(&src).unwrap();
    loaded.exif = Some(b"MM\0*\0\0\0\x08\0\0\0\0\0\0".to_vec());
    let out = dir.join("out.heic");
    img::save(&loaded, Fmt::Heic, 90, true, &out).unwrap();
    let back = img::load(&out).unwrap();
    // FFmpeg rounds the clean-aperture crop of odd sizes; other decoders apply it exactly.
    let (w, h) = back.img.dimensions();
    assert!((100..=102).contains(&w) && (66..=68).contains(&h), "got {w}x{h}");
    let a = loaded.img.to_rgb8().get_pixel(50, 30).0;
    let b = back.img.to_rgb8().get_pixel(50, 30).0;
    for c in 0..3 {
        assert!((a[c] as i32 - b[c] as i32).abs() < 20, "color drifted: {a:?} vs {b:?}");
    }
    assert!(back.exif.is_some(), "EXIF was not carried into the HEIC");
    let _ = std::fs::remove_dir_all(&dir);
}

fn qr_png(path: &Path, text: &str) {
    let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
    let modules = code.width() as u32;
    let scale = 8;
    let quiet = 4;
    let side = (modules + quiet * 2) * scale;
    let colors = code.to_colors();
    let img = image::GrayImage::from_fn(side, side, |x, y| {
        let (mx, my) = (x / scale, y / scale);
        if mx < quiet || my < quiet || mx >= modules + quiet || my >= modules + quiet {
            return image::Luma([255]);
        }
        let dark = colors[((my - quiet) * modules + (mx - quiet)) as usize] == qrcode::Color::Dark;
        image::Luma([if dark { 0 } else { 255 }])
    });
    img.save(path).unwrap();
}

#[test]
#[ignore = "needs vendor/ffmpeg and vendor/pdfium"]
fn tools_produce_the_expected_files() {
    engines();
    let dir = temp_dir("tools");
    let samples = make_samples(&dir);
    let p = |n: &str| dir.join(n);
    let ctx = Ctx::detached();
    let span = ctx.span();
    let s = Settings::default();
    let out = Some(dir.as_path());
    let mut failures: Vec<String> = Vec::new();
    let mut check = |name: &str, r: Result<Vec<PathBuf>>| match r {
        Ok(files) if files.iter().all(|f| f.exists()) && !files.is_empty() => {}
        Ok(files) => failures.push(format!("{name}: missing outputs {files:?}")),
        Err(e) => failures.push(format!("{name}: {e:#}")),
    };
    use super::media;
    let one = |r: Result<PathBuf>| r.map(|p| vec![p]);

    // Compress, including exact sizes.
    check("compress image", one(img::compress(&span, &p("photo.png"), &img::CompressOptions { level: "small".into(), ..Default::default() }, &s, out)));
    check("compress image to 20 KB", one(img::compress(&span, &p("shot.jpg"), &img::CompressOptions { target_kb: Some(20.0), ..Default::default() }, &s, out)).and_then(|f| {
        let len = std::fs::metadata(&f[0])?.len();
        ensure!(len <= 20 * 1024, "{len} bytes is over the 20 KB target");
        Ok(f)
    }));
    check("compress video", one(media::compress_video(&span, &p("clip.mp4"), &media::CompressOptions { level: "small".into(), ..Default::default() }, out)));
    check("compress video to 0.2 MB", one(media::compress_video(&span, &p("clip.mp4"), &media::CompressOptions { target_mb: Some(0.2), ..Default::default() }, out)).and_then(|f| {
        let len = std::fs::metadata(&f[0])?.len();
        ensure!(len as f64 <= 0.2 * 1024.0 * 1024.0 * 1.1, "{len} bytes is over the 0.2 MB target");
        Ok(f)
    }));
    check("compress audio", one(media::compress_audio(&span, &p("song.wav"), &media::CompressOptions { level: "small".into(), ..Default::default() }, out)));
    check("compress pdf", one(pdf::compress(&span, &p("doc.pdf"), &pdf::CompressOptions { level: "small".into(), ..Default::default() }, out)));

    // Images.
    check("resize", one(img::resize(&span, &p("photo.png"), &img::ResizeOptions { percent: Some(50.0), ..Default::default() }, &s, out)).and_then(|f| {
        ensure!(image::image_dimensions(&f[0])? == (160, 100), "wrong size");
        Ok(f)
    }));
    qr_png(&p("qr.png"), "https://example.com/kiwi");
    check("read QR", super::qr::read(&p("qr.png")).and_then(|t| {
        ensure!(t == vec!["https://example.com/kiwi".to_string()], "read {t:?}");
        Ok(vec![p("qr.png")])
    }));
    let jpgs = vec![p("shot.jpg"), p("photo.png")];
    check("make pdf", one(pdf::images_to_pdf(&span, &jpgs, &s, &p("made.pdf"))));

    // Video and audio.
    check("trim", one(media::trim(&span, &p("clip.mp4"), 0.5, 1.5, false, &s, out)));
    check("trim precise", one(media::trim(&span, &p("clip.mp4"), 0.5, 1.5, true, &s, out)));
    check("crop", one(media::crop(&span, &p("clip.mp4"), media::CropRect { x: 10, y: 10, w: 101, h: 81 }, &s, out)));
    check("speed", one(media::speed(&span, &p("clip.mp4"), 2.0, true, &s, out)));
    check("speed audio", one(media::speed(&span, &p("song.wav"), 0.5, false, &s, out)));
    check("split", media::split(&span, &p("clip.mp4"), &[1.0], out).and_then(|f| {
        ensure!(f.len() == 2, "expected 2 parts, got {}", f.len());
        Ok(f)
    }));
    check("join", one(media::join(&span, &[p("clip.mp4"), p("clip.mp4")], &s, out)));
    check("join audio", one(media::join(&span, &[p("song.wav"), p("song.wav")], &s, out)));
    check("snapshot", one(media::snapshot(&span, &p("clip.mp4"), 1.0, out)));
    check("normalize", one(media::normalize(&span, &p("song.wav"), &Default::default(), &s, out)));
    check("bleep", one(media::bleep(&span, &p("song.wav"), &media::BleepOptions { ranges: vec![media::Range { start: 0.5, end: 1.0 }], style: "tone".into() }, &s, out)));
    check("channels", one(media::channels(&span, &p("song.wav"), "mono", &s, out)));

    // PDFs.
    check("merge", one(pdf::merge(&span, &[p("doc.pdf"), p("made.pdf")], out)));
    check("split pdf", pdf::split(&span, &p("doc.pdf"), &[], out));
    check("organize", one(pdf::assemble(&[p("doc.pdf")], &[pdf::PageRef { doc: 0, index: 1, rotate: 1 }, pdf::PageRef { doc: 0, index: 0, rotate: 0 }], &p("organized.pdf"))));

    // Metadata.
    use super::metadata::{self, Mode as MetaMode};
    let mut edits = std::collections::BTreeMap::new();
    edits.insert("Artist".to_string(), "Kiwi Tester".to_string());
    check("metadata edit jpg", one(metadata::apply(&span, &p("shot.jpg"), &edits, MetaMode::Edit, out)).and_then(|f| {
        let report = metadata::read(&f[0])?;
        ensure!(report.fields.iter().any(|x| x.key == "Artist" && x.value.contains("Kiwi Tester")), "artist not saved");
        Ok(f)
    }));
    check("metadata strip jpg", one(metadata::apply(&span, &p("shot.jpg"), &Default::default(), MetaMode::Strip, out)));
    let mut av = std::collections::BTreeMap::new();
    av.insert("title".to_string(), "Kiwi Song".to_string());
    check("metadata edit audio", one(metadata::apply(&span, &p("song.wav"), &av, MetaMode::Edit, out)));
    check("metadata strip pdf", one(metadata::apply(&span, &p("doc.pdf"), &Default::default(), MetaMode::Strip, out)));
    let mut dx = std::collections::BTreeMap::new();
    dx.insert("dc:creator".to_string(), "Someone".to_string());
    check("metadata edit docx", one(metadata::apply(&span, &p("doc.docx"), &dx, MetaMode::Edit, out)).and_then(|f| {
        docs::read_docx(&f[0])?;
        let report = metadata::read(&f[0])?;
        ensure!(report.fields.iter().any(|x| x.key == "dc:creator" && x.value == "Someone"), "author not saved");
        Ok(f)
    }));

    drop(check);
    assert!(samples.iter().all(|s| s.exists()), "an input was modified or removed");
    assert!(failures.is_empty(), "{} tool checks failed:\n{}", failures.len(), failures.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_rejects_empty_files() {
    let dir = temp_dir("verify");
    let f = dir.join("empty.png");
    std::fs::write(&f, b"").unwrap();
    assert!(verify(&f, Fmt::Png).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
