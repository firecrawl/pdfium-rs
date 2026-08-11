//! Fuzz target: document loading and cheap whole-document inspection.
//!
//! Feeds arbitrary bytes to `Pdfium::load_document` and, when a document
//! opens, walks the inspection surface: page count, per-page sizes, form
//! type, and Title metadata. Encryption errors trigger one retry with a
//! throwaway password so the password-decoding paths run on encrypted
//! input too. Nothing is asserted about outcomes — any input is allowed to
//! fail with a typed error; only crashes, sanitizer reports, and hangs
//! count as findings.

#![no_main]

use std::sync::OnceLock;

use firecrawl_pdfium::{MetadataTag, Pdfium};
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

    let doc = match pdfium.load_document(data.to_vec(), None) {
        Ok(doc) => Some(doc),
        // Retry once with a throwaway password: exercises the password
        // handling (PDFium tries UTF-8 then Latin-1) on encrypted input.
        Err(e) if e.is_encryption_error() => pdfium.load_document(data.to_vec(), Some("a")).ok(),
        Err(_) => None,
    };
    let Some(doc) = doc else { return };

    for index in 0..doc.page_count() {
        let _ = doc.page_size(index);
    }
    let _ = doc.form_type();
    let _ = doc.metadata(MetadataTag::Title);
});
