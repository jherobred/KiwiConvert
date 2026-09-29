//! Reading QR codes from images.

use anyhow::Result;
use image::{DynamicImage, GrayImage};
use std::path::Path;

fn scan(gray: &GrayImage) -> Vec<String> {
    let mut prepared = rqrr::PreparedImage::prepare(gray.clone());
    prepared
        .detect_grids()
        .into_iter()
        .filter_map(|g| g.decode().ok().map(|(_, text)| text))
        .collect()
}

/// Every QR code found in the image, in reading order of detection.
pub fn read(path: &Path) -> Result<Vec<String>> {
    let img: DynamicImage = super::image::load(path)?.img;
    let gray = img.to_luma8();
    let mut found = scan(&gray);
    if found.is_empty() {
        // Phone photos are large and noisy; tiny screenshots are too small. Try other scales.
        let (w, h) = gray.dimensions();
        let side = w.max(h);
        for target in [1400u32, 900, 2400] {
            if (side as i64 - target as i64).abs() < 200 {
                continue;
            }
            let scaled = image::imageops::resize(&gray, w * target / side, h * target / side, image::imageops::FilterType::Triangle);
            found = scan(&scaled);
            if !found.is_empty() {
                break;
            }
        }
    }
    found.dedup();
    Ok(found)
}
