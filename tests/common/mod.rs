//! Shared helpers for the integration test suite.
//!
//! Included per test binary via `mod common;` (files inside `tests/`
//! subdirectories are not themselves test binaries).

#![allow(dead_code)] // Not every binary uses every helper.

use firecrawl_pdfium::Pdfium;
use std::path::PathBuf;

/// Loads the process-wide PDFium instance, with a helpful message when the
/// native library has not been fetched yet.
pub fn pdfium() -> Pdfium {
    Pdfium::load().expect(
        "failed to load PDFium; run `cargo xtask fetch-pdfium` from the repo \
         root (or set PDFIUM_LIB_PATH) so target/pdfium/<platform>/lib \
         contains the shared library",
    )
}

/// Absolute path to a file in `tests/fixtures/`.
pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Reads a fixture from `tests/fixtures/` into memory.
pub fn fixture(name: &str) -> Vec<u8> {
    let path = fixture_path(name);
    std::fs::read(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}
