//! Converts coordinates between rendered pixels and PDF page space — the
//! round trip an OCR pipeline needs.
//!
//! The story: you render a page to a bitmap, hand the bitmap to an OCR
//! engine, and get boxes back in *pixel* coordinates (origin top-left,
//! y-down). To store or overlay those boxes you need them in *page* space
//! (points, origin bottom-left, y-up). `RenderedPage::transform()` is the
//! bridge: it is derived from PDFium's own device-to-page mapping at
//! render time and is plain data, so the mapping stays valid long after
//! the page and document are gone.
//!
//! This example uses the bundled `red_rect.pdf` fixture — a 200x100 pt
//! page with an opaque red rectangle at exactly (50,25)-(150,75) in page
//! space — and plays the OCR role itself by scanning the rendered pixels
//! for the red region.
//!
//! Run with:
//!
//! ```text
//! cargo run --example coordinates
//! ```

use std::path::Path;

use firecrawl_pdfium::{PagePoint, PageRect, Pdfium, PixelRect, RenderConfig};

/// Ground truth baked into the fixture (see tests/fixtures/README.md).
const TRUTH: PageRect = PageRect {
    left: 50.0,
    bottom: 25.0,
    right: 150.0,
    top: 75.0,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/red_rect.pdf");
    let pdfium = Pdfium::load()?;
    let bytes =
        std::fs::read(&fixture).map_err(|e| format!("cannot read {}: {e}", fixture.display()))?;
    let doc = pdfium.load_document(bytes, None)?;
    let page = doc.page(0)?;
    println!("page: {} x {} pt", page.width(), page.height());

    // Render at scale 2 (144 DPI): 200x100 pt becomes a 400x200 px bitmap.
    let rendered = page.render(&RenderConfig::new().scale(2.0))?;
    println!(
        "rendered at scale 2: {} x {} px\n",
        rendered.width(),
        rendered.height()
    );

    // --- Step 1: "OCR" the owned buffer -------------------------------
    // Scan the pixels for the red region and record its bounding box in
    // pixel coordinates, exactly like an OCR engine reporting a word box.
    // The default format is Bgra8, so each pixel is [blue, green, red,
    // alpha].
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..rendered.height() {
        for (x, px) in rendered.row(y).chunks_exact(4).enumerate() {
            let is_red = px[2] > 200 && px[1] < 60 && px[0] < 60;
            if is_red {
                let x = x as u32;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if min_x > max_x {
        return Err("no red pixels found in the render; fixture geometry has changed".into());
    }
    println!("pixel bounding box of the red region (inclusive pixel indices):");
    println!("  x: {min_x}..={max_x}, y: {min_y}..={max_y}");

    // --- Step 2: map the pixel box back to page space -----------------
    // The detected region covers the continuous pixel area
    // [min_x, max_x+1) x [min_y, max_y+1); pixel *indices* address a
    // pixel's top-left corner, so the far edges are max+1.
    let pixel_rect = PixelRect::new(
        f64::from(min_x),
        f64::from(min_y),
        f64::from(max_x - min_x + 1),
        f64::from(max_y - min_y + 1),
    );
    let measured = rendered.transform().pixel_rect_to_page(pixel_rect);
    println!("mapped back to page space (points, origin bottom-left, y-up):");
    println!(
        "  ({:.2}, {:.2}) - ({:.2}, {:.2})",
        measured.left, measured.bottom, measured.right, measured.top
    );
    println!(
        "known ground truth from the fixture: ({:.0}, {:.0}) - ({:.0}, {:.0})\n",
        TRUTH.left, TRUTH.bottom, TRUTH.right, TRUTH.top
    );
    for (name, got, want) in [
        ("left", measured.left, TRUTH.left),
        ("bottom", measured.bottom, TRUTH.bottom),
        ("right", measured.right, TRUTH.right),
        ("top", measured.top, TRUTH.top),
    ] {
        if (got - want).abs() > 1.0 {
            return Err(format!(
                "{name} edge off by more than 1 pt: measured {got:.3}, expected {want:.3}"
            )
            .into());
        }
    }
    println!("round trip agrees with the ground truth within 1 pt\n");

    // --- Step 3: the opposite direction with page_to_pixel ------------
    // Given page-space geometry (say, from stored annotations), project it
    // into this render's pixels — for example to crop or highlight it.
    // Note the y-flip: the rectangle's *top* in page space becomes the
    // *smaller* pixel y.
    let t = rendered.transform();
    let top_left = t.page_to_pixel(PagePoint::new(TRUTH.left, TRUTH.top));
    let bottom_right = t.page_to_pixel(PagePoint::new(TRUTH.right, TRUTH.bottom));
    println!("projecting the rectangle's page corners into this render:");
    println!(
        "  page ({:.0}, {:.0}) -> pixel ({:.1}, {:.1})",
        TRUTH.left, TRUTH.top, top_left.x, top_left.y
    );
    println!(
        "  page ({:.0}, {:.0}) -> pixel ({:.1}, {:.1})\n",
        TRUTH.right, TRUTH.bottom, bottom_right.x, bottom_right.y
    );

    // --- Step 4: plan a render without rendering ----------------------
    // `PdfPage::transform_for` computes the same transform for any render
    // configuration without producing pixels — useful to lay out a 300 DPI
    // OCR pass (or a render happening on another machine) ahead of time.
    let plan = page.transform_for(&RenderConfig::new().dpi(300.0))?;
    println!(
        "hypothetical 300 DPI render (no pixels produced): {} x {} px",
        plan.pixel_width(),
        plan.pixel_height()
    );
    let projected = plan.page_rect_to_pixel(TRUTH);
    println!(
        "  the rectangle would occupy x {:.1}..{:.1}, y {:.1}..{:.1}",
        projected.x,
        projected.x + projected.width,
        projected.y,
        projected.y + projected.height
    );
    Ok(())
}
