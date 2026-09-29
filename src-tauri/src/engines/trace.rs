//! Raster to SVG tracing with visioncortex.
//!
//! Adapted from VTracer's color converter (MIT OR Apache-2.0, Copyright 2023 Tsang Hao Fung),
//! trimmed to the color mode KiwiConvert uses.

use anyhow::{Result, anyhow};
use image::{DynamicImage, GenericImageView};
use std::fmt::Write;
use visioncortex::color_clusters::{HIERARCHICAL_MAX, KeyingAction, Runner, RunnerConfig};
use visioncortex::{Color, ColorImage, PathSimplifyMode, PointF64};

/// Largest side traced. Photos above this are downscaled first; tracing time grows fast.
const MAX_SIDE: u32 = 1600;

fn deg2rad(deg: f64) -> f64 {
    deg / 180.0 * std::f64::consts::PI
}

pub fn to_svg(img: &DynamicImage) -> Result<String> {
    let img = if img.width().max(img.height()) > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        img.clone()
    };
    let (w, h) = img.dimensions();
    let rgba = img.to_rgba8();
    let mut color_image = ColorImage {
        pixels: rgba.into_raw(),
        width: w as usize,
        height: h as usize,
    };

    // Transparent regions are replaced by an unused key color and dropped from the output.
    let transparent = color_image.pixels.chunks_exact(4).filter(|p| p[3] == 0).count();
    let key_color = if transparent > (w as usize * h as usize) / 50 {
        let key = unused_color(&color_image).ok_or_else(|| anyhow!("could not trace this image"))?;
        for y in 0..color_image.height {
            for x in 0..color_image.width {
                if color_image.get_pixel(x, y).a == 0 {
                    color_image.set_pixel(x, y, &key);
                }
            }
        }
        key
    } else {
        Color::default()
    };

    // VTracer's "photo" preset for photographs, "poster" for flat artwork.
    let photo = is_photo(&img);
    let (filter_speckle, color_precision, layer_difference, corner_threshold) = if photo {
        (10usize, 8i32, 48i32, 180.0)
    } else {
        (4, 8, 16, 60.0)
    };

    let runner = Runner::new(
        RunnerConfig {
            diagonal: layer_difference == 0,
            hierarchical: HIERARCHICAL_MAX,
            batch_size: 25600,
            good_min_area: filter_speckle * filter_speckle,
            good_max_area: w as usize * h as usize,
            is_same_color_a: 8 - color_precision,
            is_same_color_b: 1,
            deepen_diff: layer_difference,
            hollow_neighbours: 1,
            key_color,
            keying_action: KeyingAction::Discard,
        },
        color_image,
    );
    let clusters = runner.run();
    let view = clusters.view();

    let mut svg = String::new();
    let _ = writeln!(svg, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        svg,
        r#"<svg version="1.1" xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#
    );
    for &index in view.clusters_output.iter().rev() {
        let cluster = view.get_cluster(index);
        let path = cluster.to_compound_path(
            &view,
            false,
            PathSimplifyMode::Spline,
            deg2rad(corner_threshold),
            4.0,
            10,
            deg2rad(45.0),
        );
        let (d, offset) = path.to_svg_string(true, PointF64::default(), Some(2));
        let _ = writeln!(
            svg,
            r#"<path d="{d}" fill="{}" transform="translate({},{})"/>"#,
            cluster.residue_color().to_hex_string(),
            offset.x,
            offset.y
        );
    }
    svg.push_str("</svg>\n");
    Ok(svg)
}

fn unused_color(img: &ColorImage) -> Option<Color> {
    let candidates = [
        Color::new(255, 0, 255),
        Color::new(0, 255, 0),
        Color::new(0, 255, 255),
        Color::new(255, 255, 0),
        Color::new(1, 2, 3),
        Color::new(254, 1, 253),
    ];
    candidates.into_iter().find(|c| {
        !img.pixels
            .chunks_exact(4)
            .any(|p| p[0] == c.r && p[1] == c.g && p[2] == c.b)
    })
}

/// Rough test: photographs have many distinct colors, artwork has few.
fn is_photo(img: &DynamicImage) -> bool {
    let small = img.thumbnail(96, 96).to_rgb8();
    let mut seen = std::collections::HashSet::new();
    for p in small.pixels() {
        seen.insert((p[0] >> 3, p[1] >> 3, p[2] >> 3));
    }
    seen.len() > 900
}
