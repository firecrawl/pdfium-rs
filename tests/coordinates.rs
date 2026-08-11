//! Coordinate transforms: roundtrip precision, agreement between
//! `transform_for` and rendered transforms, /Rotate handling, and ties
//! between text geometry and rendered rasters.

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::{PagePoint, PixelFormat, PixelPoint, RenderConfig, Rotation};

const SCALES: [f32; 4] = [0.5, 1.0, 2.0, 3.7];
const ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Rotate180,
    Rotation::Clockwise270,
];

/// Every loadable fixture participates in the roundtrip sweep.
const FIXTURES: [&str; 6] = [
    "valid.pdf",
    "red_rect.pdf",
    "hello_text.pdf",
    "form_text_field.pdf",
    "mixed_pages.pdf",
    "large_page.pdf",
];

/// pixel -> page -> pixel and page -> pixel -> page roundtrips stay within
/// 1e-6 across all fixtures, scales, and extra rotations. Uses
/// `transform_for` (no rasterization), so even large_page at high scales is
/// cheap; the byte cap is lifted because no allocation ever happens here.
#[test]
fn roundtrips_across_fixtures_scales_and_rotations() {
    let p = pdfium();
    for name in FIXTURES {
        let doc = p.load_document(fixture(name), None).expect("open");
        for index in 0..doc.page_count() {
            let page = doc.page(index).expect("page");
            for scale in SCALES {
                for rotation in ROTATIONS {
                    let cfg = RenderConfig::new()
                        .scale(scale)
                        .rotate(rotation)
                        .max_output_bytes(u64::MAX);
                    let t = page.transform_for(&cfg).expect("transform_for");
                    let (w, h) = (f64::from(t.pixel_width()), f64::from(t.pixel_height()));

                    // Pixel-space samples: corners, center, fractional interior.
                    let pixel_samples = [
                        (0.0, 0.0),
                        (w, 0.0),
                        (0.0, h),
                        (w, h),
                        (w / 2.0, h / 2.0),
                        (w * 0.123, h * 0.877),
                        (1.5, h - 0.25),
                    ];
                    for (x, y) in pixel_samples {
                        let px = PixelPoint::new(x, y);
                        let back = t.page_to_pixel(t.pixel_to_page(px));
                        assert!(
                            (back.x - px.x).abs() < 1e-6 && (back.y - px.y).abs() < 1e-6,
                            "{name} p{index} scale {scale} {rotation:?}: pixel ({x},{y}) \
                             roundtripped to ({}, {})",
                            back.x,
                            back.y
                        );
                    }

                    // Page-space samples (affine transforms are global, so
                    // points need not lie on the page to roundtrip).
                    let (pw, ph) = (f64::from(page.width()), f64::from(page.height()));
                    let page_samples = [
                        (0.0, 0.0),
                        (pw, ph),
                        (pw * 0.3, ph * 0.7),
                        (12.34, 56.78),
                        (-5.0, 3.25),
                    ];
                    for (x, y) in page_samples {
                        let pt = PagePoint::new(x, y);
                        let back = t.pixel_to_page(t.page_to_pixel(pt));
                        assert!(
                            (back.x - pt.x).abs() < 1e-6 && (back.y - pt.y).abs() < 1e-6,
                            "{name} p{index} scale {scale} {rotation:?}: page ({x},{y}) \
                             roundtripped to ({}, {})",
                            back.x,
                            back.y
                        );
                    }
                }
            }
        }
    }
}

/// Scan the rendered red_rect bitmap for the red bounding box, map its
/// pixel corners back to page space, and land on the authored rectangle
/// (50,25)-(150,75) within one pixel-equivalent.
#[test]
fn red_rect_scan_maps_back_to_authored_geometry() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("red_rect.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");

    for scale in [1.0f32, 2.0, 3.0] {
        let r = page
            .render(&RenderConfig::new().scale(scale))
            .expect("render");
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in 0..r.height() {
            for x in 0..r.width() {
                if r.pixel(x, y) == [0, 0, 255, 255] {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        assert!(
            min_x < max_x && min_y < max_y,
            "scale {scale}: no red pixels found"
        );

        // Pixel box corners: top-left of the first red pixel, and the far
        // corner of the last red pixel (+1 in each axis).
        let t = r.transform();
        let top_left = t.pixel_to_page(PixelPoint::new(f64::from(min_x), f64::from(min_y)));
        let bottom_right =
            t.pixel_to_page(PixelPoint::new(f64::from(max_x + 1), f64::from(max_y + 1)));

        // One pixel-equivalent in page units.
        let tol = 1.0 / f64::from(scale);
        assert!(
            (top_left.x - 50.0).abs() <= tol,
            "scale {scale}: left {}",
            top_left.x
        );
        assert!(
            (top_left.y - 75.0).abs() <= tol,
            "scale {scale}: top {}",
            top_left.y
        );
        assert!(
            (bottom_right.x - 150.0).abs() <= tol,
            "scale {scale}: right {}",
            bottom_right.x
        );
        assert!(
            (bottom_right.y - 25.0).abs() <= tol,
            "scale {scale}: bottom {}",
            bottom_right.y
        );
    }
}

/// `transform_for(config)` must equal `render(config).transform()` exactly:
/// both are derived from the same three FPDF_DeviceToPage corner queries.
#[test]
fn transform_for_matches_render_transform() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("mixed_pages.pdf"), None)
        .expect("open");
    for index in 0..doc.page_count() {
        let page = doc.page(index).expect("page");
        for cfg in [
            RenderConfig::new(),
            RenderConfig::new().scale(2.0),
            RenderConfig::new().dpi(144.0).rotate(Rotation::Clockwise90),
            RenderConfig::new().exact(123, 77),
            RenderConfig::new().rotate(Rotation::Clockwise270),
        ] {
            let planned = page.transform_for(&cfg).expect("transform_for");
            let rendered = page.render(&cfg).expect("render");
            assert_eq!(
                planned,
                *rendered.transform(),
                "page {index}: planned and rendered transforms must be identical"
            );
        }
    }
}

/// On the /Rotate 90 page of mixed_pages.pdf, the four page-space corners
/// map into the rendered bitmap's bounds. Page space stays the *unrotated*
/// user space (612x792 for the letter page), while the bitmap is the
/// post-rotate 792x612 — pinned corner images verified against PDFium's own
/// DeviceToPage: (0,0)->(0,0), (612,0)->(0,612), (0,792)->(792,0),
/// (612,792)->(792,612).
#[test]
fn rotated_page_corners_land_inside_bitmap() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("mixed_pages.pdf"), None)
        .expect("open");
    let page = doc.page(1).expect("page");
    assert_eq!(page.rotation(), Rotation::Clockwise90);
    let r = page
        .render(&RenderConfig::new().scale(1.0))
        .expect("render");
    assert_eq!((r.width(), r.height()), (792, 612));
    let t = r.transform();

    // Recover the page-space extent from the bitmap corners themselves
    // (self-verifying), then check the four page corners land in bounds.
    let (mut pw, mut ph) = (0.0f64, 0.0f64);
    for (dx, dy) in [(0.0, 0.0), (792.0, 0.0), (0.0, 612.0), (792.0, 612.0)] {
        let pt = t.pixel_to_page(PixelPoint::new(dx, dy));
        pw = pw.max(pt.x);
        ph = ph.max(pt.y);
    }
    assert!(
        (pw - 612.0).abs() < 1e-6,
        "page space is the unrotated 612pt width, got {pw}"
    );
    assert!(
        (ph - 792.0).abs() < 1e-6,
        "page space is the unrotated 792pt height, got {ph}"
    );

    let corners = [(0.0, 0.0), (pw, 0.0), (0.0, ph), (pw, ph)];
    let expected_pixels = [(0.0, 0.0), (0.0, 612.0), (792.0, 0.0), (792.0, 612.0)];
    for ((px, py), (ex, ey)) in corners.iter().zip(expected_pixels) {
        let d = t.page_to_pixel(PagePoint::new(*px, *py));
        assert!(
            d.x >= -1e-6 && d.x <= 792.0 + 1e-6 && d.y >= -1e-6 && d.y <= 612.0 + 1e-6,
            "page corner ({px},{py}) mapped outside the bitmap: ({}, {})",
            d.x,
            d.y
        );
        assert!(
            (d.x - ex).abs() < 1e-6 && (d.y - ey).abs() < 1e-6,
            "page corner ({px},{py}) expected at pixel ({ex},{ey}), got ({}, {})",
            d.x,
            d.y
        );
    }
}

/// Ties text geometry to raster reality: the 'H' glyph's page-space bounds
/// from text(), mapped through the render transform, select a pixel region
/// that actually contains dark glyph pixels in a Gray8 render at 144 DPI.
#[test]
fn char_bounds_map_onto_rendered_glyph() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("hello_text.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");

    let text = page.text().expect("text");
    let h = text.chars()[0];
    assert_eq!(h.unicode, Some('H'));

    let r = page
        .render(
            &RenderConfig::new()
                .dpi(144.0)
                .pixel_format(PixelFormat::Gray8),
        )
        .expect("render");
    let rect = r.transform().page_rect_to_pixel(h.bounds);
    assert!(rect.width > 0.0 && rect.height > 0.0);

    let x0 = rect.x.floor().max(0.0) as u32;
    let y0 = rect.y.floor().max(0.0) as u32;
    let x1 = ((rect.x + rect.width).ceil() as u32).min(r.width());
    let y1 = ((rect.y + rect.height).ceil() as u32).min(r.height());
    assert!(x0 < x1 && y0 < y1, "glyph rect must intersect the bitmap");

    let mut darkest = u8::MAX;
    for y in y0..y1 {
        for x in x0..x1 {
            darkest = darkest.min(r.pixel(x, y)[0]);
        }
    }
    assert!(
        darkest < 128,
        "the glyph region must contain dark pixels (darkest found: {darkest})"
    );

    // Control: a same-sized region in the blank page center is pure white.
    let (cx, cy) = (r.width() / 2, r.height() / 2);
    let mut control_darkest = u8::MAX;
    for y in cy..(cy + (y1 - y0)).min(r.height()) {
        for x in cx..(cx + (x1 - x0)).min(r.width()) {
            control_darkest = control_darkest.min(r.pixel(x, y)[0]);
        }
    }
    assert_eq!(control_darkest, 255, "the empty page center stays white");
}

/// The raw PDFium conversions (`PdfPage::device_to_page` / `page_to_device`)
/// agree with the derived affine `PageTransform` at interior sample points
/// (corners agree by construction; interior points prove the affine model).
/// PDFium quantizes device coordinates to integers, hence the 1px tolerance.
#[test]
fn raw_conversions_agree_with_derived_transform() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("red_rect.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");

    for rotation in [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Rotate180,
        Rotation::Clockwise270,
    ] {
        let cfg = RenderConfig::new().scale(2.5).rotate(rotation);
        let t = page.transform_for(&cfg).expect("transform");

        // Interior device points (integers, so quantization is exact on
        // the way in).
        for (dx, dy) in [(37.0, 19.0), (211.0, 83.0), (123.0, 157.0_f64)] {
            if dx >= f64::from(t.pixel_width()) || dy >= f64::from(t.pixel_height()) {
                continue;
            }
            let via_ffi = page
                .device_to_page(&cfg, PixelPoint::new(dx, dy))
                .expect("device_to_page");
            let via_affine = t.pixel_to_page(PixelPoint::new(dx, dy));
            // PDFium computes DeviceToPage in f32 internally; allow its
            // rounding noise (observed ~1.5e-6) while staying far below a
            // pixel.
            assert!(
                (via_ffi.x - via_affine.x).abs() < 1e-4 && (via_ffi.y - via_affine.y).abs() < 1e-4,
                "{rotation:?} device({dx},{dy}): ffi ({},{}) vs affine ({},{})",
                via_ffi.x,
                via_ffi.y,
                via_affine.x,
                via_affine.y,
            );
        }

        // Interior page points; PDFium returns integer device coords, so
        // agreement is within one pixel of the continuous mapping.
        for (px, py) in [(13.5, 22.25), (101.0, 51.0), (177.75, 88.5_f64)] {
            let via_ffi = page
                .page_to_device(&cfg, PagePoint::new(px, py))
                .expect("page_to_device");
            let via_affine = t.page_to_pixel(PagePoint::new(px, py));
            assert!(
                (via_ffi.x - via_affine.x).abs() <= 1.0 && (via_ffi.y - via_affine.y).abs() <= 1.0,
                "{rotation:?} page({px},{py}): ffi ({},{}) vs affine ({},{})",
                via_ffi.x,
                via_ffi.y,
                via_affine.x,
                via_affine.y,
            );
        }
    }
}
