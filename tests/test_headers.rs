#![cfg(feature = "integration")]

use std::path::PathBuf;
use std::sync::OnceLock;

static FIXTURES: OnceLock<PathBuf> = OnceLock::new();

fn fixtures() -> &'static PathBuf {
    FIXTURES.get_or_init(|| {
        let p = PathBuf::from("tests/fixtures");
        assert!(
            p.exists(),
            "fixtures missing — run `just fetch-test-data` first"
        );
        p
    })
}

#[test]
fn placeholder() {
    let _fixtures = fixtures();
    // TODO: add tests
}
