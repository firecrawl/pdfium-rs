//! Concurrency: hammer the process-wide FFI serialization from many
//! threads. Every handle type is Send + Sync; correctness (not parallelism)
//! is the contract under test.

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::{PdfDocument, RenderConfig, RenderedPage};
use std::sync::{mpsc, Arc};
use std::thread;

const THREADS: usize = 8;

/// One shared document, 8 threads, 20 renders each at scale 1.5 — all 160
/// outputs must be byte-identical.
#[test]
fn shared_document_renders_identically_across_threads() {
    let p = pdfium();
    let doc: Arc<PdfDocument> = Arc::new(
        p.load_document(fixture("red_rect.pdf"), None)
            .expect("open"),
    );

    let reference = doc
        .page(0)
        .expect("page")
        .render(&RenderConfig::new().scale(1.5))
        .expect("render");
    assert_eq!((reference.width(), reference.height()), (300, 150));

    let mut handles = Vec::new();
    for t in 0..THREADS {
        let doc = Arc::clone(&doc);
        let expected = reference.pixels().to_vec();
        handles.push(thread::spawn(move || {
            for i in 0..20 {
                let page = doc.page(0).expect("page");
                let r = page
                    .render(&RenderConfig::new().scale(1.5))
                    .expect("render");
                assert_eq!(
                    r.pixels(),
                    &expected[..],
                    "thread {t} render {i} differed from the reference"
                );
            }
        }));
    }
    for h in handles {
        h.join().expect("no thread may panic");
    }
}

/// Each thread opens its own document from the same shared bytes and
/// renders it.
#[test]
fn independent_documents_from_shared_bytes() {
    let p = pdfium();
    let bytes: Arc<Vec<u8>> = Arc::new(fixture("red_rect.pdf"));

    let mut handles = Vec::new();
    for t in 0..THREADS {
        let bytes = Arc::clone(&bytes);
        handles.push(thread::spawn(move || {
            let doc = p.load_document(bytes.as_ref().clone(), None).expect("open");
            assert_eq!(doc.page_count(), 1, "thread {t}");
            let r = doc
                .page(0)
                .expect("page")
                .render(&RenderConfig::new())
                .expect("render");
            assert_eq!(
                r.pixel(100, 50),
                &[0, 0, 255, 255],
                "thread {t}: red center"
            );
        }));
    }
    for h in handles {
        h.join().expect("no thread may panic");
    }
}

/// Different operation kinds interleave on one shared document: text
/// extraction, rendering, dimension surveys, and metadata reads.
#[test]
fn interleaved_operations_on_shared_document() {
    let p = pdfium();
    let doc: Arc<PdfDocument> = Arc::new(
        p.load_document(fixture("hello_text.pdf"), None)
            .expect("open"),
    );

    thread::scope(|s| {
        for _ in 0..2 {
            let doc = &doc;
            s.spawn(move || {
                for _ in 0..15 {
                    let text = doc.page(0).expect("page").text().expect("text");
                    assert_eq!(text.text(), "Hello, PDFium!");
                }
            });
            s.spawn(move || {
                for _ in 0..15 {
                    let r = doc
                        .page(0)
                        .expect("page")
                        .render(&RenderConfig::new().scale(0.5));
                    assert_eq!(r.expect("render").width(), 306);
                }
            });
            s.spawn(move || {
                for _ in 0..50 {
                    let size = doc.page_size(0).expect("page_size");
                    assert_eq!((size.width, size.height), (612.0, 792.0));
                }
            });
            s.spawn(move || {
                for _ in 0..50 {
                    assert_eq!(doc.metadata(firecrawl_pdfium::MetadataTag::Title), None);
                    assert_eq!(doc.pdf_version(), Some(17));
                    assert!(doc.permissions().can_copy());
                }
            });
        }
    });
}

/// Open/drop churn from many threads on one shared document (drop-order
/// stress on FPDF_LoadPage/FPDF_ClosePage under the FFI lock).
#[test]
fn page_open_drop_stress() {
    let p = pdfium();
    let doc: Arc<PdfDocument> = Arc::new(
        p.load_document(fixture("mixed_pages.pdf"), None)
            .expect("open"),
    );

    thread::scope(|s| {
        for t in 0..THREADS {
            let doc = &doc;
            s.spawn(move || {
                for i in 0..100 {
                    let index = (t + i) % doc.page_count();
                    let page = doc.page(index).expect("page");
                    assert!(page.width() > 0.0);
                    // Hold two pages at once on some iterations to vary
                    // drop ordering.
                    if i % 3 == 0 {
                        let other = doc.page((index + 1) % doc.page_count()).expect("page");
                        drop(page);
                        assert!(other.height() > 0.0);
                    }
                }
            });
        }
    });
}

/// RenderedPage is plain owned data: send it through a channel, drop the
/// source page and document, and read the pixels afterwards.
#[test]
fn rendered_page_travels_across_channels_and_outlives_document() {
    let p = pdfium();
    let (tx, rx) = mpsc::channel::<RenderedPage>();

    let producer = thread::spawn(move || {
        let doc = p
            .load_document(fixture("red_rect.pdf"), None)
            .expect("open");
        for _ in 0..5 {
            let r = doc
                .page(0)
                .expect("page")
                .render(&RenderConfig::new().scale(2.0));
            tx.send(r.expect("render")).expect("send");
        }
        // doc (and its pages) fully dropped here, before consumers finish.
    });
    producer.join().expect("producer");

    let mut received = 0;
    while let Ok(r) = rx.recv() {
        received += 1;
        // Source document is long gone; the render stays fully usable.
        assert_eq!((r.width(), r.height()), (400, 200));
        assert_eq!(r.pixel(200, 100), &[0, 0, 255, 255]);
        let pt = r.transform().pixel_to_page((100.0, 50.0).into());
        assert!((pt.x - 50.0).abs() < 0.01 && (pt.y - 75.0).abs() < 0.01);
    }
    assert_eq!(received, 5);
}
