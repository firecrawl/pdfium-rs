//! Malformed and hostile inputs: every corrupt fixture maps to a typed
//! error (or to PDFium's documented lenient recovery), and no byte pattern
//! panics.

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::Error;

#[test]
fn malformed_fixtures_are_invalid_pdf() {
    let p = pdfium();
    // bad_xref.pdf is deliberately absent here: see
    // bad_xref_opens_via_parser_recovery below.
    for name in [
        "empty.pdf",
        "header_only.pdf",
        "truncated.pdf",
        "garbage.bin",
    ] {
        match p.load_document(fixture(name), None) {
            Err(Error::InvalidPdf) => {}
            other => panic!("{name}: expected InvalidPdf, got {other:?}"),
        }
    }
}

#[test]
fn invalid_pdf_is_not_an_encryption_error() {
    let p = pdfium();
    let err = p
        .load_document(fixture("garbage.bin"), None)
        .expect_err("garbage must not open");
    assert!(!err.is_encryption_error());
}

/// PDFium's parser is famously lenient: a corrupted `startxref` keyword
/// triggers cross-reference reconstruction, so `bad_xref.pdf` opens
/// successfully with its single page intact (verified empirically against
/// the pinned PDFium build). If a future PDFium becomes stricter this test
/// will flag the behavior change.
#[test]
fn bad_xref_opens_via_parser_recovery() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("bad_xref.pdf"), None)
        .expect("PDFium reconstructs the xref table and opens the document");
    assert_eq!(doc.page_count(), 1);
    assert_eq!(doc.pdf_version(), Some(17));
    // The recovered document is fully usable, not just openable.
    let page = doc.page(0).expect("page loads from the recovered document");
    assert_eq!((page.width(), page.height()), (612.0, 792.0));
}

#[test]
fn empty_and_tiny_inputs_are_invalid_pdf() {
    let p = pdfium();
    match p.load_document(Vec::new(), None) {
        Err(Error::InvalidPdf) => {}
        other => panic!("empty Vec: expected InvalidPdf, got {other:?}"),
    }
    match p.load_document(vec![b'%'], None) {
        Err(Error::InvalidPdf) => {}
        other => panic!("1-byte input: expected InvalidPdf, got {other:?}"),
    }
}

/// Loading arbitrary slices of a real PDF must never panic — every outcome
/// is either a document or a typed error. Deterministic LCG so failures
/// reproduce.
#[test]
fn random_slices_of_valid_pdf_never_panic() {
    let p = pdfium();
    let bytes = fixture("valid.pdf");
    assert!(!bytes.is_empty());

    let mut state: u64 = 0x5DEECE66D;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };

    for i in 0..300 {
        let a = next() % (bytes.len() + 1);
        let b = next() % (bytes.len() + 1);
        let (start, end) = (a.min(b), a.max(b));
        let slice = bytes[start..end].to_vec();
        // Any result is acceptable; a panic is not. Documents that do open
        // from a lucky slice must still be safe to inspect.
        if let Ok(doc) = p.load_document(slice, None) {
            let _ = doc.page_count();
            let _ = doc.form_type();
            if doc.page_count() > 0 {
                let _ = doc.page_size(0);
            }
        } else {
            // Typed error: fine.
        }
        // Prefix truncations hit different parser stages than mid slices.
        if i < 50 {
            let cut = next() % (bytes.len() + 1);
            let _ = p.load_document(bytes[..cut].to_vec(), None);
        }
    }
}
