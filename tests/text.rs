//! Text extraction: content, per-character geometry, and invariants.
//!
//! hello_text.pdf (tests/fixtures/README.md): "Hello, PDFium!" in Helvetica
//! 24pt with its origin at (50, 700) on a US-letter page.

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::{Error, PageText};

fn hello_text() -> PageText {
    let p = pdfium();
    let doc = p
        .load_document(fixture("hello_text.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");
    page.text().expect("text")
}

#[test]
fn extracts_exact_text_and_char_count() {
    let text = hello_text();
    assert_eq!(text.text(), "Hello, PDFium!");
    assert_eq!(text.len(), 14, "one PDFium character per glyph");
    assert!(!text.is_empty());
}

#[test]
fn per_char_unicode_sequence() {
    let text = hello_text();
    let extracted: String = text
        .chars()
        .iter()
        .map(|c| c.unicode.expect("BMP char"))
        .collect();
    assert_eq!(extracted, "Hello, PDFium!");
    // The raw codes agree with the chars.
    for c in text.chars() {
        assert_eq!(char::from_u32(c.code), c.unicode);
    }
    // First char pinned: 'H' with its baseline origin at the authored
    // text position.
    let h = text.chars()[0];
    assert_eq!(h.unicode, Some('H'));
    assert_eq!(h.code, u32::from('H'));
    assert!((h.origin.x - 50.0).abs() < 1.0, "H origin.x {}", h.origin.x);
    assert!(
        (h.origin.y - 700.0).abs() < 1.0,
        "H origin.y {}",
        h.origin.y
    );
}

#[test]
fn char_bounds_are_ordered() {
    let text = hello_text();
    for (i, c) in text.chars().iter().enumerate() {
        assert!(
            c.bounds.left < c.bounds.right,
            "char {i} {:?}: left {} !< right {}",
            c.unicode,
            c.bounds.left,
            c.bounds.right
        );
        assert!(
            c.bounds.bottom < c.bounds.top,
            "char {i} {:?}: bottom {} !< top {}",
            c.unicode,
            c.bounds.bottom,
            c.bounds.top
        );
        assert!(
            c.loose_bounds.left < c.loose_bounds.right,
            "char {i}: loose left/right"
        );
        assert!(
            c.loose_bounds.bottom < c.loose_bounds.top,
            "char {i}: loose bottom/top"
        );
    }
}

#[test]
fn origins_increase_monotonically_on_single_line() {
    let text = hello_text();
    let origins: Vec<f64> = text.chars().iter().map(|c| c.origin.x).collect();
    for w in origins.windows(2) {
        assert!(
            w[0] < w[1],
            "single-line origins must increase: {origins:?}"
        );
    }
    // All chars share the same baseline.
    for c in text.chars() {
        assert!((c.origin.y - 700.0).abs() < 1.0);
    }
}

#[test]
fn loose_bounds_contain_tight_bounds() {
    // Loose boxes cover the full advance/line metrics, so they contain the
    // tight glyph boxes (small float slack for PDFium's f32 rects).
    const SLACK: f64 = 0.05;
    let text = hello_text();
    for (i, c) in text.chars().iter().enumerate() {
        assert!(
            c.loose_bounds.left <= c.bounds.left + SLACK,
            "char {i} {:?}: loose.left {} > bounds.left {}",
            c.unicode,
            c.loose_bounds.left,
            c.bounds.left
        );
        assert!(
            c.loose_bounds.right >= c.bounds.right - SLACK,
            "char {i} {:?}: loose.right {} < bounds.right {}",
            c.unicode,
            c.loose_bounds.right,
            c.bounds.right
        );
        assert!(
            c.loose_bounds.bottom <= c.bounds.bottom + SLACK,
            "char {i} {:?}: loose.bottom {} > bounds.bottom {}",
            c.unicode,
            c.loose_bounds.bottom,
            c.bounds.bottom
        );
        assert!(
            c.loose_bounds.top >= c.bounds.top - SLACK,
            "char {i} {:?}: loose.top {} < bounds.top {}",
            c.unicode,
            c.loose_bounds.top,
            c.bounds.top
        );
    }
}

#[test]
fn blank_page_has_empty_text() {
    let p = pdfium();
    let doc = p.load_document(fixture("valid.pdf"), None).expect("open");
    let text = doc.page(0).expect("page").text().expect("text");
    assert!(text.is_empty());
    assert_eq!(text.len(), 0);
    assert_eq!(text.text(), "");
    assert!(text.chars().is_empty());
}

#[test]
fn vendored_hello_world_extracts() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("pdfium/hello_world.pdf"), None)
        .expect("open");
    let text = doc.page(0).expect("page").text().expect("text");
    // The vendored fixture has two text objects; PDFium separates them
    // with a generated line break.
    assert!(
        text.text().contains("Hello, world!"),
        "unexpected extraction: {:?}",
        text.text()
    );
    assert!(text.text().contains("Goodbye, world!"));
    assert!(!text.is_empty());
}

/// PDFium synthesizes the CRLF that separates the vendored fixture's two
/// text objects. The indices are pinned against PDFium's own
/// `FPDFTextEmbedderTest.IsGenerated` (fpdfsdk/fpdf_text_embeddertest.cpp),
/// which asserts the same values on this exact file. If a future PDFium
/// changes what it synthesizes, this test will flag the behavior change.
#[test]
fn generated_chars_are_flagged() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("pdfium/hello_world.pdf"), None)
        .expect("open");
    let text = doc.page(0).expect("page").text().expect("text");

    assert_eq!(text.text(), "Hello, world!\r\nGoodbye, world!");
    assert_eq!(text.len(), 30);

    let chars = text.chars();
    assert_eq!(chars[13].unicode, Some('\r'));
    assert_eq!(chars[14].unicode, Some('\n'));
    // Only the separator is synthesized; everything else comes from the two
    // text objects, including the literal space at index 6.
    for (i, c) in chars.iter().enumerate() {
        let synthesized = i == 13 || i == 14;
        assert_eq!(
            c.generated,
            Some(synthesized),
            "char {i} {:?}: expected generated == {synthesized}",
            c.unicode
        );
    }
}

/// A single-`Tj` page has nothing for PDFium to synthesize, so no character
/// is generated — including the space, which is real content here.
#[test]
fn single_line_text_has_no_generated_chars() {
    let text = hello_text();
    for (i, c) in text.chars().iter().enumerate() {
        assert_eq!(
            c.generated,
            Some(false),
            "char {i} {:?} comes from the content stream",
            c.unicode
        );
    }
}

/// The text-extraction character ceiling rejects before allocating, with a
/// typed error carrying both numbers; a generous ceiling still extracts.
#[test]
fn text_limit_is_enforced_and_typed() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("hello_text.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");

    // 14 characters on the page; a ceiling of 5 must refuse.
    match page.text_with_limit(5) {
        Err(Error::TextTooLarge { chars, limit }) => {
            assert_eq!(chars, 14);
            assert_eq!(limit, 5);
        }
        other => panic!("expected TextTooLarge, got {other:?}"),
    }
    // Exactly at the count succeeds, as does the default.
    assert_eq!(page.text_with_limit(14).expect("at limit").len(), 14);
    assert_eq!(page.text().expect("default limit").len(), 14);
}
