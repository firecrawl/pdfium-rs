//! Uses the API correctly in a concurrent application, with two patterns:
//!
//! **Pattern A — one shared document.** A single `Arc<PdfDocument>` is
//! shared by several worker threads, each rendering different pages at
//! different scales. This is *safe by construction*: every PDFium call in
//! this crate serializes through one process-wide mutex, so the handle
//! types are `Send + Sync` and threads can never corrupt PDFium state.
//! What the mutex buys is correctness and convenience, **not
//! parallelism** — calls take turns. For CPU-bound throughput, shard work
//! across processes (PDFium upstream's own recommendation).
//!
//! **Pattern B — one document per thread.** Each thread opens its own
//! `PdfDocument` from the same shared bytes. The lifetimes are fully
//! independent, but the calls still serialize on the same global lock.
//!
//! In both patterns the results (`RenderedPage`) are plain owned data
//! with no PDFium attachments: they travel through an `mpsc` channel and
//! remain valid even after the documents are dropped.
//!
//! Run with:
//!
//! ```text
//! cargo run --example concurrent [-- /path/to/document.pdf]
//! ```
//!
//! Without an argument it uses the bundled `mixed_pages.pdf` fixture.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

use firecrawl_pdfium::{Pdfium, RenderConfig, RenderedPage};

const WORKERS: usize = 4;
const RENDERS_PER_WORKER: usize = 4;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed_pages.pdf")
        });
    let pdfium = Pdfium::load()?;
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    println!("{} ({} bytes)", path.display(), bytes.len());
    let started = Instant::now();

    // ---- Pattern A: one shared Arc<PdfDocument> -----------------------
    // This load also validates the input once, up front; the worker
    // threads below only touch a document that is known to open.
    let doc = Arc::new(pdfium.load_document(bytes.clone(), None)?);
    println!("\npattern A: {WORKERS} threads sharing one Arc<PdfDocument>");

    let (tx, rx) = mpsc::channel::<(usize, RenderedPage)>();
    let mut handles = Vec::with_capacity(WORKERS);
    for worker in 0..WORKERS {
        let doc = Arc::clone(&doc);
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            for round in 0..RENDERS_PER_WORKER {
                // Each worker walks different pages at its own scale. Every
                // page(), render(), etc. takes the global FFI mutex
                // internally; no external locking is needed (or possible).
                let page_index = (worker + round) % doc.page_count();
                let scale = 1.0 + worker as f32 * 0.5;
                let page = doc
                    .page(page_index)
                    .expect("open a page of the probed document");
                let rendered = page
                    .render(&RenderConfig::new().scale(scale))
                    .expect("render a page of the probed document");
                // RenderedPage is plain data: moving it to another thread
                // involves no PDFium resources at all.
                tx.send((worker, rendered))
                    .expect("main thread outlives the workers");
            }
        }));
    }
    drop(tx); // The receive loop below ends once every worker is done.

    let results: Vec<(usize, RenderedPage)> = rx.into_iter().collect();
    for handle in handles {
        handle
            .join()
            .map_err(|_| "a pattern A worker thread panicked")?;
    }

    // The renders outlive the document that produced them.
    drop(doc);
    let total_pixels: u64 = results
        .iter()
        .map(|(_, r)| u64::from(r.width()) * u64::from(r.height()))
        .sum();
    for (worker, rendered) in &results {
        println!(
            "  worker {worker}: page {} -> {} x {} px",
            rendered.page_index(),
            rendered.width(),
            rendered.height(),
        );
    }
    println!(
        "  {} renders, {total_pixels} pixels total (documents already dropped; \
         results are still valid)",
        results.len()
    );

    // ---- Pattern B: one document per thread ---------------------------
    println!("\npattern B: {WORKERS} threads, each with its own document from shared bytes");
    let shared: Arc<[u8]> = bytes.into();
    let (tx, rx) = mpsc::channel::<(usize, usize, u64)>();
    let mut handles = Vec::with_capacity(WORKERS);
    for worker in 0..WORKERS {
        let shared = Arc::clone(&shared);
        let tx = tx.clone();
        handles.push(thread::spawn(move || {
            // `load_document` takes ownership of a Vec, so each thread
            // copies the shared bytes into its own document. These bytes
            // already opened successfully in pattern A.
            let doc = pdfium
                .load_document(shared.to_vec(), None)
                .expect("reload bytes that opened successfully before");
            let mut pages_rendered = 0usize;
            let mut pixel_bytes = 0u64;
            for page in doc.pages() {
                let page = page.expect("open a page of the probed document");
                let rendered = page
                    .render(&RenderConfig::new().dpi(96.0))
                    .expect("render a page of the probed document");
                pages_rendered += 1;
                pixel_bytes += rendered.pixels().len() as u64;
            }
            tx.send((worker, pages_rendered, pixel_bytes))
                .expect("main thread outlives the workers");
        }));
    }
    drop(tx);

    for (worker, pages_rendered, pixel_bytes) in rx {
        println!("  worker {worker}: rendered {pages_rendered} page(s), {pixel_bytes} pixel bytes");
    }
    for handle in handles {
        handle
            .join()
            .map_err(|_| "a pattern B worker thread panicked")?;
    }

    println!("\ntotal elapsed: {:?}", started.elapsed());
    Ok(())
}
