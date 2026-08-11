//! Fuzz target: first-page text extraction and rendering.
//!
//! For any input that opens as a document with at least one page, this
//! loads page 0, extracts its text, and renders it under tiny caps (fit
//! within 64×64 pixels, 1 MiB output ceiling) so iterations stay fast and
//! memory stays bounded no matter what page geometry the mutated input
//! declares. It also derives the render transform without rendering and
//! runs one pixel→page→pixel roundtrip. Nothing is asserted about
//! outcomes — only crashes, sanitizer reports, and hangs count.

#![no_main]

use std::sync::OnceLock;

use firecrawl_pdfium::{Pdfium, PixelPoint, RenderConfig};
use libfuzzer_sys::fuzz_target;

/// Loads PDFium exactly once for the whole fuzz run.
///
/// Fuzzing without the native library would exercise nothing but the
/// load-error path on every input, so an unavailable library is a loud
/// startup panic rather than a silent no-op run.
fn pdfium() -> &'static Pdfium {
    static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
    PDFIUM.get_or_init(|| {
        Pdfium::load()
            .or_else(|_| {
                // cargo-fuzz may execute the binary from a directory where
                // the cwd-relative discovery step misses; fall back to the
                // checkout location `cargo xtask fetch-pdfium` populates.
                let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../target/pdfium")
                    .join(firecrawl_pdfium::platform_slug())
                    .join("lib");
                Pdfium::load_from_directory(dir)
            })
            .unwrap_or_else(|e| {
                panic!(
                    "PDFium library not available ({e}); fuzzing is meaningless without it. \
                     Run `cargo xtask fetch-pdfium` from the repository root first."
                )
            })
    })
}

fuzz_target!(|data: &[u8]| {
    let pdfium = pdfium();

    let Ok(doc) = pdfium.load_document(data.to_vec(), None) else {
        return;
    };
    if doc.page_count() == 0 {
        return;
    }
    let Ok(page) = doc.page(0) else { return };

    let _ = page.text();

    // Tiny caps: any page collapses to at most 64×64 pixels, and the
    // output allocation is bounded at 1 MiB regardless of what the
    // mutated input claims about its geometry.
    let config = RenderConfig::new().fit(64, 64).max_output_bytes(1 << 20);
    let _ = page.transform_for(&config);
    if let Ok(rendered) = page.render(&config) {
        let transform = rendered.transform();
        let center = PixelPoint::new(
            f64::from(rendered.width()) / 2.0,
            f64::from(rendered.height()) / 2.0,
        );
        let _ = transform.page_to_pixel(transform.pixel_to_page(center));
    }
});
