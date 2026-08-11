//! Document and page lifecycle: open/close churn, coexistence, owned
//! results outliving their sources, and document-level inspection.

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::{FormType, MetadataTag, PageText, RenderConfig, RenderedPage};

#[test]
fn many_documents_sequentially() {
    let p = pdfium();
    let bytes = fixture("valid.pdf");
    for i in 0..50 {
        let doc = p.load_document(bytes.clone(), None).expect("open");
        assert_eq!(doc.page_count(), 1, "iteration {i}");
        let page = doc.page(0).expect("page");
        assert_eq!(page.width(), 612.0);
        // doc and page drop here, exercising close paths 50 times.
    }
}

#[test]
fn multiple_documents_alive_at_once() {
    let p = pdfium();
    let docs: Vec<_> = (0..10)
        .map(|i| {
            let name = if i % 2 == 0 {
                "valid.pdf"
            } else {
                "red_rect.pdf"
            };
            p.load_document(fixture(name), None).expect("open")
        })
        .collect();
    // All ten handles stay usable while coexisting.
    for (i, doc) in docs.iter().enumerate() {
        assert_eq!(doc.page_count(), 1);
        let expected_width = if i % 2 == 0 { 612.0 } else { 200.0 };
        assert_eq!(doc.page(0).expect("page").width(), expected_width);
    }
}

#[test]
fn many_pages_open_simultaneously_from_one_document() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("mixed_pages.pdf"), None)
        .expect("open");
    assert_eq!(doc.page_count(), 3);

    // All pages at once, several handles per index (PDFium reference-counts
    // repeated FPDF_LoadPage calls on the same index).
    let mut pages = Vec::new();
    for _ in 0..8 {
        for index in 0..doc.page_count() {
            pages.push(doc.page(index).expect("page"));
        }
    }
    assert_eq!(pages.len(), 24);
    for page in &pages {
        assert!(page.width() > 0.0 && page.height() > 0.0);
    }
    // Drop an interleaved half, then keep using the rest.
    let survivors: Vec<_> = pages
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(_, p)| p)
        .collect();
    for page in &survivors {
        let rendered = page
            .render(&RenderConfig::new().scale(0.1))
            .expect("render");
        assert!(rendered.width() > 0);
    }
}

#[test]
fn rendered_page_and_text_outlive_page_and_document() {
    let p = pdfium();
    let (rendered, text): (RenderedPage, PageText) = {
        let doc = p
            .load_document(fixture("hello_text.pdf"), None)
            .expect("open");
        let page = doc.page(0).expect("page");
        let rendered = page
            .render(&RenderConfig::new().scale(1.0))
            .expect("render");
        let text = page.text().expect("text");
        (rendered, text)
        // page, then doc, drop here.
    };

    // Both results are plain owned data and remain fully usable.
    assert_eq!((rendered.width(), rendered.height()), (612, 792));
    assert_eq!(
        rendered.pixels().len(),
        rendered.stride() * rendered.height() as usize
    );
    assert_eq!(rendered.page_index(), 0);
    // The transform embedded in the render also works after the drop.
    let pt = rendered.transform().pixel_to_page((0.0, 0.0).into());
    assert!((pt.x - 0.0).abs() < 1e-9 && (pt.y - 792.0).abs() < 1e-9);

    assert_eq!(text.text(), "Hello, PDFium!");
    assert_eq!(text.len(), 14);

    // The pixel buffer can be taken out entirely.
    let stride = rendered.stride();
    let height = rendered.height() as usize;
    let pixels = rendered.into_pixels();
    assert_eq!(pixels.len(), stride * height);
}

#[test]
fn document_debug_impl() {
    let p = pdfium();
    let doc = p.load_document(fixture("valid.pdf"), None).expect("open");
    let debug = format!("{doc:?}");
    assert!(debug.contains("PdfDocument"), "debug output: {debug}");
    assert!(debug.contains("page_count: 1"), "debug output: {debug}");
    assert!(
        debug.contains("forms_enabled: false"),
        "debug output: {debug}"
    );

    let form_doc = p
        .load_document(fixture("form_text_field.pdf"), None)
        .expect("open");
    form_doc.enable_form_rendering().expect("enable forms");
    let debug = format!("{form_doc:?}");
    assert!(
        debug.contains("forms_enabled: true"),
        "debug output: {debug}"
    );
}

#[test]
fn metadata_and_page_labels_absent_on_generated_fixtures() {
    let p = pdfium();
    let doc = p.load_document(fixture("valid.pdf"), None).expect("open");
    for tag in [
        MetadataTag::Title,
        MetadataTag::Author,
        MetadataTag::Subject,
        MetadataTag::Keywords,
        MetadataTag::Creator,
        MetadataTag::Producer,
        MetadataTag::CreationDate,
        MetadataTag::ModDate,
    ] {
        assert_eq!(
            doc.metadata(tag),
            None,
            "generated fixtures carry no {tag:?}"
        );
    }
    assert_eq!(doc.page_label(0), None, "no page labels are defined");
    assert_eq!(
        doc.page_label(999),
        None,
        "out-of-range label lookup is None, not an error"
    );
}

#[test]
fn pdf_version_is_17_for_generated_fixtures() {
    let p = pdfium();
    for name in [
        "valid.pdf",
        "red_rect.pdf",
        "hello_text.pdf",
        "mixed_pages.pdf",
    ] {
        let doc = p.load_document(fixture(name), None).expect("open");
        assert_eq!(
            doc.pdf_version(),
            Some(17),
            "{name} is generated as PDF 1.7"
        );
    }
}

#[test]
fn unencrypted_documents_grant_everything_and_have_no_security_handler() {
    let p = pdfium();
    let doc = p.load_document(fixture("valid.pdf"), None).expect("open");

    let perms = doc.permissions();
    assert_eq!(
        perms.0, 0xFFFF_FFFF,
        "unencrypted => all permission bits set"
    );
    assert!(perms.can_print());
    assert!(perms.can_modify());
    assert!(perms.can_copy());
    assert!(perms.can_annotate());

    assert_eq!(
        doc.security_handler_revision(),
        None,
        "unencrypted => no security handler"
    );
    assert_eq!(
        doc.form_type(),
        FormType::None,
        "valid.pdf has no interactive form"
    );
    assert!(!doc.forms_enabled());
}

#[test]
fn page_iterator_and_out_of_bounds() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("mixed_pages.pdf"), None)
        .expect("open");

    let sizes: Vec<_> = doc
        .pages()
        .map(|page| page.map(|pg| (pg.index(), pg.width(), pg.height())))
        .collect::<Result<_, _>>()
        .expect("all pages open");
    assert_eq!(
        sizes,
        vec![(0, 595.0, 842.0), (1, 792.0, 612.0), (2, 200.0, 100.0)],
        "pages() yields every page in order with post-/Rotate dimensions"
    );

    match doc.page(3) {
        Err(firecrawl_pdfium::Error::PageIndexOutOfBounds { index: 3, count: 3 }) => {}
        other => panic!("expected PageIndexOutOfBounds, got {other:?}"),
    }
    match doc.page_size(3) {
        Err(firecrawl_pdfium::Error::PageIndexOutOfBounds { index: 3, count: 3 }) => {}
        other => panic!("expected PageIndexOutOfBounds, got {other:?}"),
    }
}
