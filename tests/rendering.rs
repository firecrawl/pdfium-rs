//! Rendering: pixel-exact checks on known geometry, size selector math,
//! backgrounds, buffer layout invariants, rotation, and format conversion.
//!
//! Geometry key (tests/fixtures/README.md): red_rect.pdf is a 200x100 pt
//! page with an opaque red rectangle spanning (50,25)-(150,75) in page
//! space; at scale 2 that is device x 100..300, y 50..150.

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::{
    Color, PagePoint, PdfDocument, PixelFormat, RenderConfig, RenderedPage, Rotation,
};

fn red_rect_doc() -> PdfDocument {
    pdfium()
        .load_document(fixture("red_rect.pdf"), None)
        .expect("open red_rect.pdf")
}

#[test]
fn red_rect_pixel_exact_in_every_format() {
    let doc = red_rect_doc();
    let page = doc.page(0).expect("page");

    // (center, corner) byte patterns for pure red on white, per format.
    let cases: [(PixelFormat, &[u8], &[u8]); 4] = [
        (PixelFormat::Bgra8, &[0, 0, 255, 255], &[255, 255, 255, 255]),
        (PixelFormat::Rgba8, &[255, 0, 0, 255], &[255, 255, 255, 255]),
        (PixelFormat::Bgr8, &[0, 0, 255], &[255, 255, 255]),
        // Rec.601 luma of pure red: 0.299 * 255 = 76.
        (PixelFormat::Gray8, &[76], &[255]),
    ];
    for (format, red, white) in cases {
        let r = page
            .render(&RenderConfig::new().scale(2.0).pixel_format(format))
            .expect("render");
        assert_eq!((r.width(), r.height()), (400, 200), "{format:?}");
        assert_eq!(r.format(), format);
        assert_eq!(
            r.pixel(200, 100),
            red,
            "{format:?}: center of the rectangle is red"
        );
        assert_eq!(
            r.pixel(5, 5),
            white,
            "{format:?}: page corner is white background"
        );
        // Just inside each rectangle edge stays red (edges are on exact
        // device integers at scale 2, so there is no AA fringe inside).
        assert_eq!(r.pixel(101, 100), red, "{format:?}: left edge");
        assert_eq!(r.pixel(298, 100), red, "{format:?}: right edge");
        // Just outside is white.
        assert_eq!(r.pixel(98, 100), white, "{format:?}: left of rectangle");
        assert_eq!(r.pixel(200, 48), white, "{format:?}: above rectangle");
    }
}

#[test]
fn size_selector_math_against_real_pages() {
    let doc = red_rect_doc();
    let page = doc.page(0).expect("page"); // 200x100 pt

    let dims = |cfg: &RenderConfig| {
        let r = page.render(cfg).expect("render");
        (r.width(), r.height())
    };
    assert_eq!(
        dims(&RenderConfig::new()),
        (200, 100),
        "default is scale 1.0"
    );
    assert_eq!(dims(&RenderConfig::new().scale(2.0)), (400, 200));
    assert_eq!(dims(&RenderConfig::new().scale(3.7)), (740, 370));
    assert_eq!(
        dims(&RenderConfig::new().dpi(144.0)),
        (400, 200),
        "144 DPI == scale 2"
    );
    assert_eq!(
        dims(&RenderConfig::new().width(400)),
        (400, 200),
        "width keeps aspect"
    );
    assert_eq!(
        dims(&RenderConfig::new().height(50)),
        (100, 50),
        "height keeps aspect"
    );
    assert_eq!(
        dims(&RenderConfig::new().fit(1000, 300)),
        (600, 300),
        "height binds the fit"
    );
    assert_eq!(
        dims(&RenderConfig::new().fit(300, 1000)),
        (300, 150),
        "width binds the fit"
    );
    assert_eq!(
        dims(&RenderConfig::new().exact(37, 91)),
        (37, 91),
        "exact may distort"
    );

    // A US-letter page at 300 DPI: 612x792 pt * 300/72.
    let letter = pdfium()
        .load_document(fixture("valid.pdf"), None)
        .expect("open");
    let r = letter
        .page(0)
        .expect("page")
        .render(&RenderConfig::new().dpi(300.0))
        .expect("render");
    assert_eq!((r.width(), r.height()), (2550, 3300));
}

#[test]
fn background_colors_encode_per_format() {
    let p = pdfium();
    let doc = p.load_document(fixture("valid.pdf"), None).expect("open");
    let page = doc.page(0).expect("page"); // blank page: everything stays background

    let bg = Color::rgba(1, 2, 3, 255);
    let cases: [(PixelFormat, &[u8]); 4] = [
        (PixelFormat::Bgra8, &[3, 2, 1, 255]),
        (PixelFormat::Rgba8, &[1, 2, 3, 255]),
        (PixelFormat::Bgr8, &[3, 2, 1]),
        // Rec.601 luma of (1,2,3): 0.299 + 1.174 + 0.342 = 1.815 -> 2.
        (PixelFormat::Gray8, &[2]),
    ];
    for (format, expected) in cases {
        let r = page
            .render(
                &RenderConfig::new()
                    .scale(0.25)
                    .pixel_format(format)
                    .background(bg),
            )
            .expect("render");
        assert_eq!(r.pixel(0, 0), expected, "{format:?} corner");
        assert_eq!(
            r.pixel(r.width() / 2, r.height() / 2),
            expected,
            "{format:?} center"
        );
    }
}

#[test]
fn transparent_background_has_zero_alpha() {
    let p = pdfium();
    let doc = p.load_document(fixture("valid.pdf"), None).expect("open");
    let page = doc.page(0).expect("page");
    let r = page
        .render(
            &RenderConfig::new()
                .scale(0.25)
                .pixel_format(PixelFormat::Bgra8)
                .background(Color::TRANSPARENT),
        )
        .expect("render");
    assert_eq!(
        r.pixel(0, 0),
        &[0, 0, 0, 0],
        "corner stays fully transparent"
    );
    assert_eq!(
        r.pixel(r.width() / 2, r.height() / 2),
        &[0, 0, 0, 0],
        "a blank page paints nothing over the transparent background"
    );
}

#[test]
fn buffer_layout_is_dense_for_every_format() {
    let doc = red_rect_doc();
    let page = doc.page(0).expect("page");
    for format in [
        PixelFormat::Bgra8,
        PixelFormat::Rgba8,
        PixelFormat::Bgr8,
        PixelFormat::Gray8,
    ] {
        let r = page
            .render(&RenderConfig::new().scale(1.0).pixel_format(format))
            .expect("render");
        let bpp = format.bytes_per_pixel();
        assert_eq!(
            r.stride(),
            r.width() as usize * bpp,
            "{format:?}: dense stride"
        );
        assert_eq!(
            r.pixels().len(),
            r.stride() * r.height() as usize,
            "{format:?}: buffer is exactly stride * height"
        );
        assert_eq!(r.row(0).len(), r.width() as usize * bpp);
        // Last row and last pixel are addressable.
        let _ = r.row(r.height() - 1);
        let _ = r.pixel(r.width() - 1, r.height() - 1);
    }
}

#[test]
#[should_panic(expected = "row 100 out of bounds")]
fn row_out_of_bounds_panics() {
    let doc = red_rect_doc();
    let r = doc
        .page(0)
        .expect("page")
        .render(&RenderConfig::new())
        .expect("render");
    let _ = r.row(r.height()); // height 100 => first invalid row index
}

#[test]
#[should_panic(expected = "column 200 out of bounds")]
fn pixel_x_out_of_bounds_panics() {
    let doc = red_rect_doc();
    let r = doc
        .page(0)
        .expect("page")
        .render(&RenderConfig::new())
        .expect("render");
    let _ = r.pixel(r.width(), 0); // width 200 => first invalid column
}

#[test]
#[should_panic(expected = "out of bounds")]
fn pixel_y_out_of_bounds_panics() {
    let doc = red_rect_doc();
    let r = doc
        .page(0)
        .expect("page")
        .render(&RenderConfig::new())
        .expect("render");
    let _ = r.pixel(0, r.height());
}

/// Extra Clockwise90 rotation swaps the output dimensions, and the
/// transform proves the rectangle landed where PDFium says it should:
/// mapping the rectangle's page-space center through the render's own
/// transform yields a pixel that must be red. Self-verifying — no
/// hand-computed pixel coordinates.
#[test]
fn extra_rotation_swaps_dims_and_moves_content_consistently() {
    let doc = red_rect_doc();
    let page = doc.page(0).expect("page");

    let r = page
        .render(&RenderConfig::new().scale(1.0).rotate(Rotation::Clockwise90))
        .expect("render");
    assert_eq!(
        (r.width(), r.height()),
        (100, 200),
        "90 degrees swaps 200x100 to 100x200"
    );

    let t = r.transform();
    // Rectangle center (100, 50) must land inside the bitmap on a red pixel.
    let center = t.page_to_pixel(PagePoint::new(100.0, 50.0));
    assert!(center.x >= 0.0 && center.x < r.width() as f64);
    assert!(center.y >= 0.0 && center.y < r.height() as f64);
    assert_eq!(
        r.pixel(center.x as u32, center.y as u32),
        &[0, 0, 255, 255],
        "rectangle center is red after rotation"
    );
    // A point well outside the rectangle maps to white.
    let outside = t.page_to_pixel(PagePoint::new(10.0, 90.0));
    assert_eq!(
        r.pixel(outside.x as u32, outside.y as u32),
        &[255, 255, 255, 255]
    );

    // All four rectangle corners, nudged inward, are red; nudged outward,
    // white. This pins the whole rectangle location, not just its center.
    for (cx, cy) in [(50.0, 25.0), (150.0, 25.0), (50.0, 75.0), (150.0, 75.0)] {
        let inward = |v: f64, toward: f64| v + (toward - v).signum() * 2.0;
        let inside = t.page_to_pixel(PagePoint::new(inward(cx, 100.0), inward(cy, 50.0)));
        assert_eq!(
            r.pixel(inside.x as u32, inside.y as u32),
            &[0, 0, 255, 255],
            "just inside corner ({cx},{cy})"
        );
        let outward = |v: f64, toward: f64| v - (toward - v).signum() * 2.0;
        let outside = t.page_to_pixel(PagePoint::new(outward(cx, 100.0), outward(cy, 50.0)));
        assert_eq!(
            r.pixel(outside.x as u32, outside.y as u32),
            &[255, 255, 255, 255],
            "just outside corner ({cx},{cy})"
        );
    }

    // Rotate180 keeps dimensions.
    let r180 = page
        .render(&RenderConfig::new().scale(1.0).rotate(Rotation::Rotate180))
        .expect("render");
    assert_eq!((r180.width(), r180.height()), (200, 100));
    let center = r180.transform().page_to_pixel(PagePoint::new(100.0, 50.0));
    assert_eq!(
        r180.pixel(center.x as u32, center.y as u32),
        &[0, 0, 255, 255]
    );
}

/// `to_rgba8()` converges every format onto the same RGBA bytes: the three
/// color formats are byte-identical, and Gray8 (a genuinely different,
/// luma-only rendering) converts to gray RGBA with opaque alpha fill-ins.
#[test]
fn to_rgba8_is_equivalent_across_formats() {
    let doc = red_rect_doc();
    let page = doc.page(0).expect("page");
    let render = |f: PixelFormat| {
        page.render(&RenderConfig::new().scale(1.0).pixel_format(f))
            .expect("render")
    };

    let bgra = render(PixelFormat::Bgra8);
    let rgba = render(PixelFormat::Rgba8);
    let bgr = render(PixelFormat::Bgr8);
    let gray = render(PixelFormat::Gray8);

    let from_bgra = bgra.to_rgba8();
    assert_eq!(from_bgra.len(), 200 * 100 * 4);
    assert_eq!(
        from_bgra,
        rgba.to_rgba8(),
        "Bgra8 and Rgba8 renders convert identically"
    );
    // Bgr8 has no alpha channel; to_rgba8 fills alpha with 255, which is
    // exactly what the opaque-background BGRA render carries anyway.
    assert_eq!(
        from_bgra,
        bgr.to_rgba8(),
        "Bgr8 converts to the same RGBA (alpha filled 255)"
    );

    // Gray8: every output pixel is r==g==b with alpha 255, and the known
    // regions carry the pinned luma values.
    let from_gray = gray.to_rgba8();
    assert_eq!(from_gray.len(), 200 * 100 * 4);
    for px in from_gray.chunks_exact(4) {
        assert!(
            px[0] == px[1] && px[1] == px[2],
            "gray conversion must stay gray: {px:?}"
        );
        assert_eq!(px[3], 255, "alpha fill-in");
    }
    let at = |buf: &[u8], x: usize, y: usize| -> [u8; 4] {
        let i = (y * 200 + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    assert_eq!(at(&from_gray, 100, 50), [76, 76, 76, 255], "red luma");
    assert_eq!(at(&from_gray, 5, 5), [255, 255, 255, 255], "white");
    assert_eq!(at(&from_bgra, 100, 50), [255, 0, 0, 255], "red");
}

#[test]
fn mixed_pages_dimensions_and_rotation() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("mixed_pages.pdf"), None)
        .expect("open");
    assert_eq!(doc.page_count(), 3);

    // A4 portrait; US-letter with /Rotate 90 (so post-rotate landscape);
    // 200x100 pt page.
    let expected = [(595.0, 842.0), (792.0, 612.0), (200.0, 100.0)];
    for (i, (w, h)) in expected.iter().enumerate() {
        let size = doc.page_size(i).expect("page_size");
        assert_eq!((size.width, size.height), (*w, *h), "page_size({i})");
        let page = doc.page(i).expect("page");
        assert_eq!(
            (page.width(), page.height()),
            (*w, *h),
            "page({i}).size() agrees with page_size({i}), including the rotated page"
        );
    }
    assert_eq!(doc.page(0).expect("page").rotation(), Rotation::None);
    assert_eq!(doc.page(1).expect("page").rotation(), Rotation::Clockwise90);
    assert_eq!(doc.page(2).expect("page").rotation(), Rotation::None);

    // Rendering the rotated page honors its post-/Rotate dimensions.
    let r = doc
        .page(1)
        .expect("page")
        .render(&RenderConfig::new())
        .expect("render");
    assert_eq!((r.width(), r.height()), (792, 612));
}

/// Rendering results are independent snapshots: rendering the same page
/// twice with the same config yields byte-identical buffers.
#[test]
fn repeated_renders_are_deterministic() {
    let doc = red_rect_doc();
    let page = doc.page(0).expect("page");
    let cfg = RenderConfig::new().scale(1.5);
    let a: RenderedPage = page.render(&cfg).expect("render");
    let b: RenderedPage = page.render(&cfg).expect("render");
    assert_eq!(a.pixels(), b.pixels());
    assert_eq!(a.transform(), b.transform());
}
