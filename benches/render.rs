//! Render-latency benchmarks.
//!
//! Requires a PDFium library discoverable via the normal chain (run
//! `cargo xtask fetch-pdfium` first); benches are skipped with a notice
//! when none is found.
//!
//! Run: `cargo bench --bench render`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use firecrawl_pdfium::{Pdfium, PixelFormat, RenderConfig};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn render_benches(c: &mut Criterion) {
    let pdfium = match Pdfium::load() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("SKIPPING render benches — PDFium not available: {e}");
            return;
        }
    };

    let doc = pdfium
        .load_document(fixture("hello_text.pdf"), None)
        .expect("fixture loads");
    let page = doc.page(0).expect("page 0");

    let mut group = c.benchmark_group("render_letter_page");
    for dpi in [72.0f32, 150.0, 300.0] {
        let config = RenderConfig::new().dpi(dpi);
        let t = page.transform_for(&config).expect("transform");
        let bytes = u64::from(t.pixel_width()) * u64::from(t.pixel_height()) * 4;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(
            BenchmarkId::new("bgra8_dpi", dpi as u32),
            &config,
            |b, cfg| {
                b.iter(|| page.render(cfg).expect("render"));
            },
        );
    }
    group.finish();

    let mut group = c.benchmark_group("render_formats_150dpi");
    for (name, format) in [
        ("bgra8", PixelFormat::Bgra8),
        ("rgba8", PixelFormat::Rgba8),
        ("bgr8", PixelFormat::Bgr8),
        ("gray8", PixelFormat::Gray8),
    ] {
        let config = RenderConfig::new().dpi(150.0).pixel_format(format);
        group.bench_with_input(BenchmarkId::from_parameter(name), &config, |b, cfg| {
            b.iter(|| page.render(cfg).expect("render"));
        });
    }
    group.finish();

    // Open+close a document per iteration: measures full lifecycle cost.
    let bytes = fixture("mixed_pages.pdf");
    c.bench_function("open_inspect_close_3_pages", |b| {
        b.iter(|| {
            let doc = pdfium.load_document(bytes.clone(), None).expect("load");
            let mut area = 0.0f32;
            for i in 0..doc.page_count() {
                let s = doc.page_size(i).expect("size");
                area += s.width * s.height;
            }
            area
        });
    });

    // Text extraction with per-char geometry.
    let text_doc = pdfium
        .load_document(fixture("hello_text.pdf"), None)
        .expect("fixture loads");
    let text_page = text_doc.page(0).expect("page 0");
    c.bench_function("text_extraction_with_geometry", |b| {
        b.iter(|| text_page.text().expect("text"));
    });
}

criterion_group!(benches, render_benches);
criterion_main!(benches);
