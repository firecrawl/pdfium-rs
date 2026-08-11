//! Text extraction with per-character geometry.

use crate::coords::{PagePoint, PageRect};
use crate::error::{Error, Result};
use crate::page::PdfPage;
use crate::sys;

/// One character as reported by PDFium's text engine, with its geometry in
/// page space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageChar {
    /// The Unicode scalar, or `None` when PDFium reports a code point Rust
    /// cannot represent as `char` (rare; see [`PageChar::code`]).
    pub unicode: Option<char>,
    /// The raw code from `FPDFText_GetUnicode`.
    pub code: u32,
    /// Tight glyph bounding box in page space.
    pub bounds: PageRect,
    /// Loose bounding box (full advance/line metrics) in page space —
    /// usually what you want for hit-testing and highlight rectangles.
    pub loose_bounds: PageRect,
    /// Glyph origin (baseline start) in page space.
    pub origin: PagePoint,
}

/// Text content of one page: the extracted string plus per-character
/// geometry. Plain owned data — no PDFium resources, `Send + Sync`,
/// outlives page and document.
///
/// Note that [`text`](PageText::text) (UTF-16 derived, may contain
/// surrogate-paired characters) and [`chars`](PageText::chars) (one entry
/// per PDFium character index) are indexed differently; correlate through
/// geometry rather than assuming 1:1 index equality.
#[derive(Debug, Clone, Default)]
pub struct PageText {
    text: String,
    chars: Vec<PageChar>,
}

impl PageText {
    /// The page's text in reading order, as extracted by PDFium
    /// (includes generated whitespace/newlines).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Per-character data, indexed by PDFium character index.
    pub fn chars(&self) -> &[PageChar] {
        &self.chars
    }

    /// Number of PDFium characters on the page.
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// True when the page has no text.
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
}

impl<'doc> PdfPage<'doc> {
    /// Extracts the page's text with per-character geometry.
    ///
    /// Extraction is eager: the returned [`PageText`] holds no PDFium
    /// resources and stays valid after the page is dropped.
    pub fn text(&self) -> Result<PageText> {
        let index = self.index();
        self.ffi(|b| -> Result<PageText> {
            // SAFETY: live page handle.
            let text_page = unsafe { b.FPDFText_LoadPage(self.handle()) };
            if text_page.is_null() {
                return Err(Error::TextLoadFailed { index });
            }
            // Ensure FPDFText_ClosePage runs on every path below.
            let result = extract(b, text_page);
            // SAFETY: live text page handle, closed exactly once.
            unsafe { b.FPDFText_ClosePage(text_page) };
            result.ok_or(Error::TextLoadFailed { index })
        })
    }
}

/// Reads everything out of a live text page. Returns `None` on hard
/// errors (`FPDFText_CountChars == -1`).
fn extract(b: &sys::Bindings, text_page: sys::FPDF_TEXTPAGE) -> Option<PageText> {
    // SAFETY (all calls below): live text page handle; indices are within
    // [0, count); out-pointers are valid locals.
    let count = unsafe { b.FPDFText_CountChars(text_page) };
    if count < 0 {
        return None;
    }
    if count == 0 {
        return Some(PageText::default());
    }

    // Full text: `count + 1` UTF-16 units (PDFium appends a NUL and counts
    // it in the return value).
    let mut units = vec![0u16; count as usize + 1];
    let written = unsafe { b.FPDFText_GetText(text_page, 0, count, units.as_mut_ptr()) };
    let text_units = if written > 0 {
        &units[..(written as usize - 1)]
    } else {
        &[][..]
    };
    let text = String::from_utf16_lossy(text_units);

    let mut chars = Vec::with_capacity(count as usize);
    for i in 0..count {
        let code = unsafe { b.FPDFText_GetUnicode(text_page, i) };

        let (mut left, mut right, mut bottom, mut top) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        // Note PDFium's parameter order here: left, right, bottom, top.
        let have_box = unsafe {
            b.FPDFText_GetCharBox(text_page, i, &mut left, &mut right, &mut bottom, &mut top)
        } != 0;
        let bounds = if have_box {
            PageRect::new(left, bottom, right, top)
        } else {
            PageRect::new(0.0, 0.0, 0.0, 0.0)
        };

        let mut loose = sys::FS_RECTF::default();
        let have_loose = unsafe { b.FPDFText_GetLooseCharBox(text_page, i, &mut loose) } != 0;
        let loose_bounds = if have_loose {
            // FS_RECTF carries top-left/bottom-right corners; PageRect is
            // (left, bottom, right, top).
            PageRect::new(
                loose.left as f64,
                loose.bottom as f64,
                loose.right as f64,
                loose.top as f64,
            )
        } else {
            bounds
        };

        let (mut ox, mut oy) = (0.0f64, 0.0f64);
        let have_origin = unsafe { b.FPDFText_GetCharOrigin(text_page, i, &mut ox, &mut oy) } != 0;
        let origin = if have_origin {
            PagePoint::new(ox, oy)
        } else {
            PagePoint::new(bounds.left, bounds.bottom)
        };

        chars.push(PageChar {
            unicode: char::from_u32(code),
            code,
            bounds,
            loose_bounds,
            origin,
        });
    }

    Some(PageText { text, chars })
}
