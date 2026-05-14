#![cfg(feature = "integration")]

use std::path::PathBuf;

use fefits::card::CardValue;
use fefits::fits::Fits;
use rstest::rstest;

#[rstest]
fn test_mandatory_headers(#[files("tests/fixtures/*.fits")] path: PathBuf) {
    let mut fits = Fits::open(&path).expect("failed to open file");
    let filename = path.file_name().unwrap().to_string_lossy();
    let header = fits
        .read_header(0)
        .expect("failed to read primary HDU header");
    assert_eq!(
        header.get_value("SIMPLE"),
        Some(&CardValue::Logical(true)),
        "{filename}: SIMPLE must be T"
    );
    header
        .bitpix()
        .unwrap_or_else(|e| panic!("{filename}: {e}"));
    let naxis = header.naxis().unwrap_or_else(|e| panic!("{filename}: {e}"));

    for n in 1..=naxis {
        header
            .naxisn(n)
            .unwrap_or_else(|e| panic!("{filename}: NAXIS{n} missing or invalid: {e}"));
    }
}
