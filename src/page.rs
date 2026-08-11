//! Pages: metrics, rotation, and access to rendering/text.

use crate::document::PdfDocument;
use crate::error::{Error, Result};
use crate::sys;

/// Page dimensions in points (1/72 inch), after applying the page's
/// `/Rotate` entry (PDFium's `FPDF_GetPageWidthF`/`HeightF` semantics: a
/// portrait page with `/Rotate 90` reports landscape dimensions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    /// Width in points (1/72 inch).
    pub width: f32,
    /// Height in points (1/72 inch).
    pub height: f32,
}

/// A rotation in 90° clockwise increments — used both for a page's own
/// `/Rotate` entry and for extra rotation applied at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    /// No rotation.
    #[default]
    None,
    /// 90° clockwise.
    Clockwise90,
    /// 180°.
    Rotate180,
    /// 270° clockwise (90° counter-clockwise).
    Clockwise270,
}

impl Rotation {
    pub(crate) fn from_raw(raw: i32) -> Rotation {
        match raw.rem_euclid(4) {
            1 => Rotation::Clockwise90,
            2 => Rotation::Rotate180,
            3 => Rotation::Clockwise270,
            _ => Rotation::None,
        }
    }

    pub(crate) fn as_raw(self) -> i32 {
        match self {
            Rotation::None => 0,
            Rotation::Clockwise90 => 1,
            Rotation::Rotate180 => 2,
            Rotation::Clockwise270 => 3,
        }
    }

    /// Whether this rotation swaps width and height.
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Clockwise90 | Rotation::Clockwise270)
    }
}

/// An open page of a [`PdfDocument`].
///
/// Borrows its document, so the borrow checker statically prevents pages
/// from outliving it. Dimension and rotation getters are cached at open
/// time and take no lock.
///
/// `Send + Sync` like the document; all PDFium access is serialized.
pub struct PdfPage<'doc> {
    doc: &'doc PdfDocument,
    handle: sys::FPDF_PAGE,
    index: usize,
    size: PageSize,
    rotation: Rotation,
    forms_active: bool,
}

// SAFETY: `handle` is only passed to PDFium under the process-wide FFI
// lock; cached fields are immutable after construction. See PdfDocument.
unsafe impl Send for PdfPage<'_> {}
// SAFETY: all `&self` methods that touch PDFium acquire the FFI lock.
unsafe impl Sync for PdfPage<'_> {}

impl std::fmt::Debug for PdfPage<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfPage")
            .field("index", &self.index)
            .field("size", &self.size)
            .field("rotation", &self.rotation)
            .finish_non_exhaustive()
    }
}

impl<'doc> PdfPage<'doc> {
    pub(crate) fn open(doc: &'doc PdfDocument, index: usize) -> Result<PdfPage<'doc>> {
        let forms_active = doc.form_env().is_some();
        let loaded = doc.pdfium().ffi(|b| {
            // SAFETY: valid document handle; index bounds-checked by caller.
            let handle = unsafe { b.FPDF_LoadPage(doc.handle(), index as i32) };
            if handle.is_null() {
                return None;
            }
            if forms_active {
                if let Some(env) = doc.form_env() {
                    // SAFETY: live page + form handles. Header: "Should be
                    // invoked after user successfully loaded a PDF page, and
                    // FPDFDOC_InitFormFillEnvironment() has been invoked."
                    unsafe { b.FORM_OnAfterLoadPage(handle, env.handle()) };
                }
            }
            // SAFETY: live page handle for all three calls.
            let width = unsafe { b.FPDF_GetPageWidthF(handle) };
            let height = unsafe { b.FPDF_GetPageHeightF(handle) };
            let rotation = unsafe { b.FPDFPage_GetRotation(handle) };
            Some((handle, width, height, rotation))
        });

        match loaded {
            Some((handle, width, height, rotation)) => Ok(PdfPage {
                doc,
                handle,
                index,
                size: PageSize { width, height },
                rotation: Rotation::from_raw(rotation),
                forms_active,
            }),
            None => Err(Error::PageLoadFailed { index }),
        }
    }

    /// This page's 0-based index in the document.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Page size in points, post-`/Rotate` (cached; no FFI call).
    pub fn size(&self) -> PageSize {
        self.size
    }

    /// Width in points, post-`/Rotate` (cached).
    pub fn width(&self) -> f32 {
        self.size.width
    }

    /// Height in points, post-`/Rotate` (cached).
    pub fn height(&self) -> f32 {
        self.size.height
    }

    /// The page's own `/Rotate` entry (cached). Note that [`size`](Self::size)
    /// already reflects this rotation.
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// The page bounding box (intersection of media box and crop box) in
    /// page space.
    pub fn bounding_box(&self) -> Result<crate::PageRect> {
        let mut rect = sys::FS_RECTF::default();
        // SAFETY: live page handle, valid out-pointer.
        let ok = self.ffi(|b| unsafe { b.FPDF_GetPageBoundingBox(self.handle, &mut rect) });
        if ok != 0 {
            Ok(crate::PageRect::new(
                rect.left as f64,
                rect.bottom as f64,
                rect.right as f64,
                rect.top as f64,
            ))
        } else {
            Err(Error::PageLoadFailed { index: self.index })
        }
    }

    /// Whether the page declares transparency.
    pub fn has_transparency(&self) -> bool {
        // SAFETY: live page handle.
        self.ffi(|b| unsafe { b.FPDFPage_HasTransparency(self.handle) }) != 0
    }

    pub(crate) fn document(&self) -> &'doc PdfDocument {
        self.doc
    }

    pub(crate) fn handle(&self) -> sys::FPDF_PAGE {
        self.handle
    }

    pub(crate) fn ffi<R>(&self, f: impl FnOnce(&sys::Bindings) -> R) -> R {
        self.doc.pdfium().ffi(f)
    }
}

impl Drop for PdfPage<'_> {
    fn drop(&mut self) {
        self.ffi(|b| {
            if self.forms_active {
                if let Some(env) = self.doc.form_env() {
                    // SAFETY: live handles; header requires this before
                    // closing a page when a form environment is active.
                    unsafe { b.FORM_OnBeforeClosePage(self.handle, env.handle()) };
                }
            }
            // SAFETY: live page handle, closed exactly once (here).
            unsafe { b.FPDF_ClosePage(self.handle) };
        });
    }
}
