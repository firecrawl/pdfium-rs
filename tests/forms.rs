//! AcroForm detection and form-field rendering.
//!
//! form_text_field.pdf (tests/fixtures/README.md): minimal AcroForm with a
//! text field `name`, value `initial`, widget rect (50,80)-(250,120) on a
//! 300x200 pt page.

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::{FormType, PdfDocument, RenderConfig, RenderedPage};

fn count_diff(a: &RenderedPage, b: &RenderedPage) -> usize {
    assert_eq!(a.pixels().len(), b.pixels().len());
    a.pixels()
        .iter()
        .zip(b.pixels())
        .filter(|(x, y)| x != y)
        .count()
}

/// Byte indices of differing pixels, mapped to (x, y) pixel coordinates.
fn diff_pixels(a: &RenderedPage, b: &RenderedPage) -> Vec<(u32, u32)> {
    let bpp = a.format().bytes_per_pixel();
    let mut out = Vec::new();
    for (i, (x, y)) in a.pixels().iter().zip(b.pixels()).enumerate() {
        if x != y {
            let px = (i / bpp) as u32;
            out.push((px % a.width(), px / a.width()));
        }
    }
    out.dedup();
    out
}

#[test]
fn form_type_detection() {
    let p = pdfium();
    let cases = [
        ("form_text_field.pdf", FormType::AcroForm),
        ("pdfium/text_form.pdf", FormType::AcroForm),
        ("valid.pdf", FormType::None),
        ("red_rect.pdf", FormType::None),
    ];
    for (name, expected) in cases {
        let doc = p.load_document(fixture(name), None).expect("open");
        assert_eq!(doc.form_type(), expected, "{name}");
        assert_eq!(
            doc.form_type() == FormType::AcroForm,
            doc.form_type().is_renderable()
        );
    }
}

#[test]
fn enable_form_rendering_is_idempotent() {
    let p = pdfium();
    let doc: PdfDocument = p
        .load_document(fixture("form_text_field.pdf"), None)
        .expect("open");
    assert!(!doc.forms_enabled(), "forms start disabled");
    doc.enable_form_rendering().expect("first enable");
    assert!(doc.forms_enabled());
    doc.enable_form_rendering()
        .expect("second enable is a no-op");
    assert!(doc.forms_enabled());
    // Still fully functional afterwards.
    let r = doc
        .page(0)
        .expect("page")
        .render(&RenderConfig::new())
        .expect("render");
    assert_eq!((r.width(), r.height()), (300, 200));
}

#[test]
fn form_layer_changes_pixels_inside_the_widget_rect() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("form_text_field.pdf"), None)
        .expect("open");
    doc.enable_form_rendering().expect("enable");
    let page = doc.page(0).expect("page");

    let base = page
        .render(&RenderConfig::new().scale(1.0).form_fields(false))
        .expect("render");
    let with_forms = page
        .render(&RenderConfig::new().scale(1.0).form_fields(true))
        .expect("render");

    let diff = count_diff(&base, &with_forms);
    assert!(diff > 0, "the form layer must draw the field value");
    // Pinned against the pinned PDFium build (chromium/7988); the smoke run
    // observed exactly 651 differing bytes at scale 1. Keep the strong
    // assertion loose enough to survive AA-level changes but tight enough
    // to notice the layer disappearing.
    assert!(
        (100..10_000).contains(&diff),
        "expected roughly 651 differing bytes, got {diff}"
    );

    // Every differing pixel lies inside the widget rect (50,80)-(250,120)
    // in page space. At scale 1 on the 300x200 pt page, pixel x = page x
    // and pixel y = 200 - page y, so the widget occupies x in [50, 250],
    // y in [80, 120]. Allow 2px of AA slack.
    for (x, y) in diff_pixels(&base, &with_forms) {
        assert!(
            (48..=252).contains(&x) && (78..=122).contains(&y),
            "changed pixel ({x},{y}) is outside the widget rectangle"
        );
    }
}

/// Pages opened BEFORE enable_form_rendering() still render fine. The
/// render path consults the document's form environment at *render* time,
/// so such pages draw the form layer too — behavior verified against the
/// pinned PDFium build (renders are byte-identical to pages opened after
/// enabling). What matters contractually: no panic, no error, usable
/// output.
#[test]
fn page_opened_before_enable_still_renders() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("form_text_field.pdf"), None)
        .expect("open");

    let before_page = doc.page(0).expect("page opened before forms are enabled");
    doc.enable_form_rendering().expect("enable");
    let after_page = doc.page(0).expect("page opened after forms are enabled");

    let base = after_page
        .render(&RenderConfig::new().form_fields(false))
        .expect("render");
    let with_after = after_page
        .render(&RenderConfig::new().form_fields(true))
        .expect("render");
    let with_before = before_page
        .render(&RenderConfig::new().form_fields(true))
        .expect("render must not fail");

    assert_eq!((with_before.width(), with_before.height()), (300, 200));
    // Documented current behavior: the early-opened page picks up the form
    // layer as well, identically to the late-opened page.
    assert_eq!(
        with_before.pixels(),
        with_after.pixels(),
        "form layer participation is decided at render time, not page-open time"
    );
    assert!(count_diff(&base, &with_before) > 0);
}

#[test]
fn vendored_text_form_renders_with_forms() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("pdfium/text_form.pdf"), None)
        .expect("open");
    assert_eq!(doc.form_type(), FormType::AcroForm);
    doc.enable_form_rendering().expect("enable");
    let page = doc.page(0).expect("page");
    let r = page
        .render(&RenderConfig::new().scale(1.0).form_fields(true))
        .expect("render");
    assert_eq!((r.width(), r.height()), (300, 300));
    assert_eq!(r.pixels().len(), r.stride() * r.height() as usize);
}

/// form_fields(true) on a document whose forms were never enabled is a
/// silent no-op: no error, and the output matches a form_fields(false)
/// render exactly.
#[test]
fn form_fields_without_enable_is_a_noop() {
    let p = pdfium();
    let doc = p.load_document(fixture("valid.pdf"), None).expect("open");
    assert!(!doc.forms_enabled());
    let page = doc.page(0).expect("page");
    let with = page
        .render(&RenderConfig::new().scale(0.5).form_fields(true))
        .expect("render");
    let without = page
        .render(&RenderConfig::new().scale(0.5).form_fields(false))
        .expect("render");
    assert_eq!(with.pixels(), without.pixels());
}
