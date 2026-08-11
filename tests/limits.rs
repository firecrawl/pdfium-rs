//! Resource limits and configuration validation: the output-byte cap, the
//! i32 bitmap guards, and InvalidConfig classification.
//!
//! large_page.pdf is 14400x14400 pt — PDF's maximum page size.

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::{Error, RenderConfig};

/// scale 10 => 144000x144000 px * 4 B = 82,944,000,000 bytes, refused
/// against the default 1 GiB cap before any allocation.
#[test]
fn oversized_render_hits_default_cap() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("large_page.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");
    assert_eq!((page.width(), page.height()), (14400.0, 14400.0));

    match page.render(&RenderConfig::new().scale(10.0)) {
        Err(Error::RenderTooLarge {
            required_bytes,
            limit,
        }) => {
            assert_eq!(required_bytes, 144_000u64 * 144_000 * 4);
            assert_eq!(limit, RenderConfig::DEFAULT_MAX_OUTPUT_BYTES);
            assert_eq!(limit, 1 << 30);
            assert!(required_bytes > limit);
        }
        other => panic!("expected RenderTooLarge, got {other:?}"),
    }
}

/// With the byte cap disabled, PDFium's hard i32 bitmap-API guards still
/// refuse the render as a typed error: at scale 50,000 the 720,000,000 px
/// width fits i32 but width*bpp (2.88e9) exceeds i32::MAX. No allocation,
/// no crash.
///
/// (Even more extreme scales saturate the dimension casts; the requirement
/// math runs in u128, pinned by `extreme_scale_saturates_instead_of_
/// overflowing` below.)
#[test]
fn uncapped_render_hits_i32_guards() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("large_page.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");

    match page.render(
        &RenderConfig::new()
            .scale(50_000.0)
            .max_output_bytes(u64::MAX),
    ) {
        Err(Error::RenderTooLarge {
            required_bytes,
            limit,
        }) => {
            assert_eq!(required_bytes, 720_000_000u64 * 720_000_000 * 4);
            assert_eq!(limit, u64::MAX);
        }
        other => panic!("expected RenderTooLarge from the i32 guards, got {other:?}"),
    }
}

/// A render just under (at) the cap succeeds; one byte less refuses it.
/// scale 0.05 => 720x720 px * 4 B = 2,073,600 bytes (~2 MB).
#[test]
fn cap_boundary_is_exact() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("large_page.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");
    const REQUIRED: u64 = 720 * 720 * 4;

    // Under the default cap: succeeds.
    let r = page
        .render(&RenderConfig::new().scale(0.05))
        .expect("~2 MB render succeeds");
    assert_eq!((r.width(), r.height()), (720, 720));
    assert_eq!(r.pixels().len() as u64, REQUIRED);

    // Cap set exactly at the requirement: still succeeds.
    let r = page
        .render(&RenderConfig::new().scale(0.05).max_output_bytes(REQUIRED))
        .expect("required == limit is allowed");
    assert_eq!(r.pixels().len() as u64, REQUIRED);

    // One byte below: typed refusal carrying both numbers.
    match page.render(
        &RenderConfig::new()
            .scale(0.05)
            .max_output_bytes(REQUIRED - 1),
    ) {
        Err(Error::RenderTooLarge {
            required_bytes,
            limit,
        }) => {
            assert_eq!(required_bytes, REQUIRED);
            assert_eq!(limit, REQUIRED - 1);
        }
        other => panic!("expected RenderTooLarge, got {other:?}"),
    }
}

#[test]
fn degenerate_render_configs_are_invalid_config() {
    let p = pdfium();
    let doc = p.load_document(fixture("valid.pdf"), None).expect("open");
    let page = doc.page(0).expect("page");

    let cases: [(&str, RenderConfig); 5] = [
        ("scale(0.0)", RenderConfig::new().scale(0.0)),
        ("scale(-1.0)", RenderConfig::new().scale(-1.0)),
        ("scale(NaN)", RenderConfig::new().scale(f32::NAN)),
        ("exact(0, h)", RenderConfig::new().exact(0, 100)),
        ("exact(w, 0)", RenderConfig::new().exact(100, 0)),
    ];
    for (label, cfg) in cases {
        match page.render(&cfg) {
            Err(Error::InvalidConfig(msg)) => {
                assert!(
                    !msg.is_empty(),
                    "{label}: message should explain the problem"
                );
            }
            other => panic!("{label}: expected InvalidConfig, got {other:?}"),
        }
        // transform_for shares the validation.
        assert!(
            matches!(page.transform_for(&cfg), Err(Error::InvalidConfig(_))),
            "{label}"
        );
    }
}

#[test]
fn password_with_interior_nul_is_invalid_config() {
    let p = pdfium();
    match p.load_document(fixture("valid.pdf"), Some("pass\0word")) {
        Err(Error::InvalidConfig(msg)) => {
            assert!(
                msg.contains("NUL"),
                "message should name the NUL problem: {msg}"
            );
        }
        other => panic!("expected InvalidConfig for interior NUL, got {other:?}"),
    }
    // The error is a config error, not an encryption error.
    let err = p
        .load_document(fixture("valid.pdf"), Some("a\0b"))
        .expect_err("NUL");
    assert!(!err.is_encryption_error());
}

/// Regression test: extreme scales used to saturate the width/height casts
/// to u32::MAX and then overflow u64 when multiplying w * h * bpp (a panic
/// in debug builds). The requirement math now runs in u128 and reports a
/// saturated `required_bytes`.
#[test]
fn extreme_scale_saturates_instead_of_overflowing() {
    let p = pdfium();
    let doc = p
        .load_document(fixture("large_page.pdf"), None)
        .expect("open");
    let page = doc.page(0).expect("page");
    let cfg = RenderConfig::new().scale(1e6).max_output_bytes(u64::MAX);
    match page.render(&cfg) {
        Err(Error::RenderTooLarge {
            required_bytes,
            limit,
        }) => {
            // 14400pt * 1e6 saturates both dimensions to u32::MAX; the true
            // requirement exceeds u64, so the report saturates.
            assert_eq!(required_bytes, u64::MAX);
            assert_eq!(limit, u64::MAX);
        }
        other => panic!("expected RenderTooLarge, got {other:?}"),
    }
}
