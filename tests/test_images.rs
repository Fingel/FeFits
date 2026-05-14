#![cfg(feature = "integration")]

use std::fs;
use std::path::PathBuf;

use fefits::fits::Fits;
use rstest::rstest;

fn percentile_scale(pixels: &[f64]) -> Vec<u8> {
    const SAMPLE_SIZE: usize = 10_000;
    let stride = (pixels.len() / SAMPLE_SIZE).max(1);
    let mut sample: Vec<f64> = pixels
        .iter()
        .step_by(stride)
        .copied()
        .filter(|v| v.is_finite())
        .collect();
    assert!(!sample.is_empty(), "image contains no finite pixel values");
    sample.sort_unstable_by(f64::total_cmp);
    let lo = sample[(sample.len() as f64 * 0.02) as usize];
    let hi = sample[((sample.len() as f64 * 0.98) as usize).min(sample.len() - 1)];
    if lo >= hi {
        // uniform image, return all grey
        return vec![128u8; pixels.len()];
    }
    let range = hi - lo;
    pixels
        .iter()
        .map(|&v| {
            if !v.is_finite() {
                0u8
            } else {
                ((v - lo) / range * 255.0).clamp(0.0, 255.0) as u8
            }
        })
        .collect()
}

#[rstest]
fn test_image_output(#[files("tests/fixtures/*.fits")] path: PathBuf) {
    let filename = path.file_stem().unwrap().to_string_lossy();
    let mut fits = Fits::open(&path).expect("failed to open file");

    let Some(hdu_idx) = fits.find_image().expect("error scanning HDUs") else {
        eprintln!("{filename}: no 2D image HDU found, skipping");
        return;
    };

    let header = fits.read_header(hdu_idx).expect("failed to read header");
    let width = header.naxisn(1).expect("NAXIS1 missing") as u32;
    let height = header.naxisn(2).expect("NAXIS2 missing") as u32;

    let pixels = percentile_scale(
        &fits
            .read_image(hdu_idx)
            .expect("failed to read image")
            .into_f64(),
    );

    let out_dir = PathBuf::from("tests/output/test_image_output");
    fs::create_dir_all(&out_dir).expect("failed to create output directory");

    let out_path = out_dir.join(format!("{filename}.png"));
    image::GrayImage::from_raw(width, height, pixels)
        .expect("image dimensions do not match pixel count")
        .save(&out_path)
        .expect("failed to save PNG");

    println!("Wrote {}", out_path.display());
}
