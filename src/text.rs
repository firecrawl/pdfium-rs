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
/// PDFium's text engine emits UCS-2: characters outside the Basic
/// Multilingual Plane are not representable in [`text`](PageText::text)
/// (PDFium substitutes or drops them), and each [`chars`](PageText::chars)
/// entry carries the raw code in [`PageChar::code`]. Indexing also
/// differs between the two views — correlate through geometry rather than
/// assuming 1:1 index equality.
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

/// Default ceiling for [`PdfPage::text`]: one million characters. Real
/// pages hold a few thousand; the ceiling exists because a small hostile
/// PDF can claim an enormous character count and this crate allocates
/// roughly 90 bytes per character during extraction.
pub const DEFAULT_MAX_TEXT_CHARS: usize = 1_000_000;

impl<'doc> PdfPage<'doc> {
    /// Extracts the page's text with per-character geometry, limited to
    /// [`DEFAULT_MAX_TEXT_CHARS`] characters.
    ///
    /// Extraction is eager: the returned [`PageText`] holds no PDFium
    /// resources and stays valid after the page is dropped.
    pub fn text(&self) -> Result<PageText> {
        self.text_with_limit(DEFAULT_MAX_TEXT_CHARS)
    }

    /// Like [`text`](Self::text) with an explicit character ceiling.
    ///
    /// Pages reporting more than `max_chars` characters fail with
    /// [`Error::TextTooLarge`] *before* any allocation, mirroring how
    /// [`RenderConfig::max_output_bytes`](crate::RenderConfig::max_output_bytes)
    /// bounds rendering.
    pub fn text_with_limit(&self, max_chars: usize) -> Result<PageText> {
        let index = self.index();
        self.ffi(|b| -> Result<PageText> {
            // SAFETY: live page handle.
            let text_page = unsafe { b.FPDFText_LoadPage(self.handle()) };
            if text_page.is_null() {
                return Err(Error::TextLoadFailed { index });
            }
            // Ensure FPDFText_ClosePage runs on every path below.
            let result = extract(b, text_page, max_chars);
            // SAFETY: live text page handle, closed exactly once.
            unsafe { b.FPDFText_ClosePage(text_page) };
            match result {
                Ok(text) => Ok(text),
                Err(ExtractFail::CountFailed) => Err(Error::TextLoadFailed { index }),
                Err(ExtractFail::TooLarge { chars }) => Err(Error::TextTooLarge {
                    chars,
                    limit: max_chars,
                }),
            }
        })
    }
}

enum ExtractFail {
    /// `FPDFText_CountChars` returned -1.
    CountFailed,
    /// The reported character count exceeds the caller's ceiling.
    TooLarge { chars: usize },
}

/// Reads everything out of a live text page.
fn extract(
    b: &sys::Bindings,
    text_page: sys::FPDF_TEXTPAGE,
    max_chars: usize,
) -> std::result::Result<PageText, ExtractFail> {
    // SAFETY (all calls below): live text page handle; indices are within
    // [0, count); out-pointers are valid locals.
    let count = unsafe { b.FPDFText_CountChars(text_page) };
    if count < 0 {
        return Err(ExtractFail::CountFailed);
    }
    if count == 0 {
        return Ok(PageText::default());
    }
    // The count is attacker-controlled input (a hostile PDF can claim
    // near-i32::MAX characters); enforce the ceiling before allocating.
    if count as usize > max_chars {
        return Err(ExtractFail::TooLarge {
            chars: count as usize,
        });
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

    // Reserve conservatively: the ceiling already bounds the worst case,
    // but a lying count should not pre-allocate gigabytes either.
    let mut chars = Vec::with_capacity((count as usize).min(65_536));
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

    Ok(PageText { text, chars })
}
