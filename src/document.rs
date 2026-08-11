//! Documents: loading, inspection, and page access.

use std::ffi::{c_ulong, CString};
use std::path::Path;
use std::sync::OnceLock;

use crate::error::{Error, Result};
use crate::forms::{FormEnv, FormType};
use crate::library::Pdfium;
use crate::page::{PageSize, PdfPage};
use crate::sys;

/// Standard PDF metadata tags accepted by [`PdfDocument::metadata`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MetadataTag {
    /// Document title.
    Title,
    /// Author.
    Author,
    /// Subject.
    Subject,
    /// Keywords.
    Keywords,
    /// Application that created the original document.
    Creator,
    /// Application that produced the PDF.
    Producer,
    /// Creation date (PDF date string, e.g. `D:20260810...`).
    CreationDate,
    /// Last-modified date (PDF date string).
    ModDate,
}

impl MetadataTag {
    fn as_cstr(self) -> &'static std::ffi::CStr {
        match self {
            MetadataTag::Title => c"Title",
            MetadataTag::Author => c"Author",
            MetadataTag::Subject => c"Subject",
            MetadataTag::Keywords => c"Keywords",
            MetadataTag::Creator => c"Creator",
            MetadataTag::Producer => c"Producer",
            MetadataTag::CreationDate => c"CreationDate",
            MetadataTag::ModDate => c"ModDate",
        }
    }
}

/// Document permission flags from the PDF's encryption dictionary
/// (`FPDF_GetDocPermissions`). For unencrypted documents every permission
/// is granted (`0xFFFF_FFFF`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions(pub u64);

impl Permissions {
    fn bit(&self, n: u32) -> bool {
        self.0 & (1 << (n - 1)) != 0
    }

    /// Bit 3: print the document.
    pub fn can_print(&self) -> bool {
        self.bit(3)
    }

    /// Bit 4: modify contents.
    pub fn can_modify(&self) -> bool {
        self.bit(4)
    }

    /// Bit 5: copy or extract text and graphics.
    pub fn can_copy(&self) -> bool {
        self.bit(5)
    }

    /// Bit 6: add or modify annotations / fill form fields.
    pub fn can_annotate(&self) -> bool {
        self.bit(6)
    }
}

/// An open PDF document.
///
/// The document **owns the PDF bytes** for its whole lifetime (PDFium
/// requires the backing buffer to stay valid while the document is open).
/// Dropping the document releases the PDFium handle and, if forms were
/// enabled, the form-fill environment first (in the order PDFium requires).
///
/// # Thread safety
///
/// `PdfDocument` is `Send + Sync`: every method serializes through the
/// process-wide FFI lock. Sharing one document across threads is safe;
/// calls will not run in parallel.
pub struct PdfDocument {
    pdfium: Pdfium,
    handle: sys::FPDF_DOCUMENT,
    /// Backing buffer for `handle`; must outlive it. Boxed slice so the
    /// heap address is stable regardless of moves of `PdfDocument`.
    _bytes: Box<[u8]>,
    forms: OnceLock<FormEnv>,
    page_count: usize,
}

// SAFETY: `handle` (and the handles inside `forms`) are only ever passed to
// PDFium under the process-wide FFI lock; `_bytes` is never written after
// construction. No method provides unsynchronized interior access.
unsafe impl Send for PdfDocument {}
// SAFETY: all `&self` methods acquire the FFI lock before touching PDFium
// state, so concurrent `&self` access from multiple threads is serialized.
unsafe impl Sync for PdfDocument {}

impl std::fmt::Debug for PdfDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfDocument")
            .field("page_count", &self.page_count)
            .field("forms_enabled", &self.forms.get().is_some())
            .finish_non_exhaustive()
    }
}

impl Pdfium {
    /// Opens a PDF from bytes, taking ownership of them.
    ///
    /// `password` unlocks encrypted documents (PDFium tries UTF-8 then
    /// Latin-1 encodings of it). Pass `None` for unencrypted documents;
    /// a `Some` password is ignored by unencrypted documents.
    ///
    /// # Errors
    ///
    /// - [`Error::PasswordRequired`] — encrypted, no password given.
    /// - [`Error::IncorrectPassword`] — encrypted, wrong password.
    /// - [`Error::UnsupportedSecurity`] — unsupported encryption scheme.
    /// - [`Error::InvalidPdf`] — not a PDF / unrecoverably corrupt.
    pub fn load_document(
        &self,
        bytes: impl Into<Vec<u8>>,
        password: Option<&str>,
    ) -> Result<PdfDocument> {
        let bytes: Box<[u8]> = bytes.into().into_boxed_slice();
        let c_password = match password {
            Some(p) => Some(CString::new(p).map_err(|_| {
                Error::InvalidConfig("password must not contain NUL bytes".into())
            })?),
            None => None,
        };

        let (handle, last_error) = self.ffi(|b| {
            // SAFETY: `bytes` is a live allocation of the given length and
            // outlives the handle (owned by the PdfDocument built below;
            // on error the handle is never created). Password pointer is a
            // valid NUL-terminated string or null.
            let handle = unsafe {
                b.FPDF_LoadMemDocument64(
                    bytes.as_ptr().cast(),
                    bytes.len(),
                    c_password.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
                )
            };
            // FPDF_GetLastError is documented as meaningful only right
            // after a failed load; fetch it in the same critical section
            // so another thread's call cannot clobber it.
            let last_error = if handle.is_null() {
                // SAFETY: no preconditions beyond initialization.
                unsafe { b.FPDF_GetLastError() }
            } else {
                sys::FPDF_ERR_SUCCESS
            };
            (handle, last_error)
        });

        if handle.is_null() {
            return Err(map_load_error(last_error, password.is_some()));
        }

        // SAFETY: valid document handle.
        let raw_count = self.ffi(|b| unsafe { b.FPDF_GetPageCount(handle) });
        let page_count = usize::try_from(raw_count).unwrap_or(0);

        Ok(PdfDocument {
            pdfium: *self,
            handle,
            _bytes: bytes,
            forms: OnceLock::new(),
            page_count,
        })
    }

    /// Opens a PDF file from disk (reads it fully into memory first —
    /// PDFium is fastest and simplest with in-memory documents).
    pub fn load_document_from_file(
        &self,
        path: impl AsRef<Path>,
        password: Option<&str>,
    ) -> Result<PdfDocument> {
        let bytes = std::fs::read(path).map_err(Error::Io)?;
        self.load_document(bytes, password)
    }
}

fn map_load_error(code: c_ulong, password_supplied: bool) -> Error {
    match code {
        sys::FPDF_ERR_PASSWORD => {
            if password_supplied {
                Error::IncorrectPassword
            } else {
                Error::PasswordRequired
            }
        }
        sys::FPDF_ERR_SECURITY => Error::UnsupportedSecurity,
        sys::FPDF_ERR_FORMAT | sys::FPDF_ERR_FILE => Error::InvalidPdf,
        // FPDF_ERR_SUCCESS with a null handle should not happen; treat any
        // unexpected code (incl. UNKNOWN and PAGE) as an opaque PDFium error.
        // `c_ulong` is 32-bit on Windows; the widening cast is real there.
        #[allow(clippy::unnecessary_cast)]
        code => Error::Pdfium { code: code as u64 },
    }
}

impl PdfDocument {
    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.page_count
    }

    /// Opens page `index` (0-based).
    pub fn page(&self, index: usize) -> Result<PdfPage<'_>> {
        if index >= self.page_count {
            return Err(Error::PageIndexOutOfBounds { index, count: self.page_count });
        }
        PdfPage::open(self, index)
    }

    /// Iterates over all pages, opening each lazily.
    pub fn pages(&self) -> impl Iterator<Item = Result<PdfPage<'_>>> {
        (0..self.page_count).map(move |i| self.page(i))
    }

    /// Page size in points **without loading the page** — cheap for
    /// dimension surveys of large documents.
    pub fn page_size(&self, index: usize) -> Result<PageSize> {
        if index >= self.page_count {
            return Err(Error::PageIndexOutOfBounds { index, count: self.page_count });
        }
        let mut size = sys::FS_SIZEF::default();
        // SAFETY: valid handle, in-bounds index, valid out-pointer.
        let ok = self.ffi(|b| unsafe {
            b.FPDF_GetPageSizeByIndexF(self.handle, index as i32, &mut size)
        });
        if ok != 0 {
            Ok(PageSize { width: size.width, height: size.height })
        } else {
            Err(Error::PageLoadFailed { index })
        }
    }

    /// The document's interactive form type (cheap; does not initialize
    /// form rendering).
    pub fn form_type(&self) -> FormType {
        // SAFETY: valid handle.
        FormType::from_raw(self.ffi(|b| unsafe { b.FPDF_GetFormType(self.handle) }))
    }

    /// Initializes PDFium's form-fill environment so
    /// [`RenderConfig::form_fields`](crate::RenderConfig::form_fields)
    /// can draw AcroForm field appearances.
    ///
    /// Idempotent. Pages opened **after** this call participate in form
    /// rendering; enable forms before opening pages you intend to render.
    pub fn enable_form_rendering(&self) -> Result<()> {
        if self.forms.get().is_some() {
            return Ok(());
        }
        let env = FormEnv::new(self.pdfium, self.handle)?;
        // A racing second init would leak a FormEnv teardown; set() failing
        // means another thread won — destroy ours cleanly.
        if let Err(mut lost) = self.forms.set(env) {
            self.pdfium.ffi(|b| lost.destroy(b));
        }
        Ok(())
    }

    /// Whether [`enable_form_rendering`](Self::enable_form_rendering) has
    /// been called successfully.
    pub fn forms_enabled(&self) -> bool {
        self.forms.get().is_some()
    }

    /// Document permissions from the encryption dictionary. Unencrypted
    /// documents report all permissions granted.
    pub fn permissions(&self) -> Permissions {
        // SAFETY: valid handle.
        // `c_ulong` is 32-bit on Windows; the widening cast is real there.
        #[allow(clippy::unnecessary_cast)]
        Permissions(self.ffi(|b| unsafe { b.FPDF_GetDocPermissions(self.handle) }) as u64)
    }

    /// Security handler revision (2/3/4/5/6), or `None` for unencrypted
    /// documents.
    pub fn security_handler_revision(&self) -> Option<i32> {
        // SAFETY: valid handle.
        let rev = self.ffi(|b| unsafe { b.FPDF_GetSecurityHandlerRevision(self.handle) });
        (rev != -1).then_some(rev)
    }

    /// PDF file version as reported by the header, times ten
    /// (14 = PDF 1.4, 17 = PDF 1.7, 20 = PDF 2.0). `None` if unavailable.
    pub fn pdf_version(&self) -> Option<i32> {
        let mut version = 0;
        // SAFETY: valid handle and out-pointer.
        let ok = self.ffi(|b| unsafe { b.FPDF_GetFileVersion(self.handle, &mut version) });
        (ok != 0).then_some(version)
    }

    /// A standard metadata field, or `None` when absent/empty.
    pub fn metadata(&self, tag: MetadataTag) -> Option<String> {
        self.ffi(|b| {
            read_utf16le_buffer(|buffer, buflen| {
                // SAFETY: valid handle; tag is a NUL-terminated static;
                // buffer/buflen follow the two-call length protocol.
                unsafe { b.FPDF_GetMetaText(self.handle, tag.as_cstr().as_ptr(), buffer, buflen) }
            })
        })
    }

    /// The page label for `index` (e.g. "iv", "A-2"), or `None` when the
    /// document defines no label for it.
    pub fn page_label(&self, index: usize) -> Option<String> {
        if index >= self.page_count {
            return None;
        }
        self.ffi(|b| {
            read_utf16le_buffer(|buffer, buflen| {
                // SAFETY: valid handle, in-bounds index, two-call protocol.
                unsafe { b.FPDF_GetPageLabel(self.handle, index as i32, buffer, buflen) }
            })
        })
    }

    pub(crate) fn pdfium(&self) -> Pdfium {
        self.pdfium
    }

    pub(crate) fn handle(&self) -> sys::FPDF_DOCUMENT {
        self.handle
    }

    pub(crate) fn form_env(&self) -> Option<&FormEnv> {
        self.forms.get()
    }

    fn ffi<R>(&self, f: impl FnOnce(&sys::Bindings) -> R) -> R {
        self.pdfium.ffi(f)
    }
}

impl Drop for PdfDocument {
    fn drop(&mut self) {
        self.pdfium.ffi(|b| {
            // Teardown order required by PDFium: exit the form-fill
            // environment before closing the document it wraps.
            if let Some(env) = self.forms.get_mut() {
                env.destroy(b);
            }
            // SAFETY: handle is live (created in load_document, closed
            // only here); pages hold `&PdfDocument`, so the borrow checker
            // guarantees none outlive this drop.
            unsafe { b.FPDF_CloseDocument(self.handle) };
        });
    }
}

/// Runs PDFium's two-call UTF-16LE string protocol: first call with a null
/// buffer returns the byte length including the NUL terminator; `0` means
/// "no such value".
fn read_utf16le_buffer(
    mut call: impl FnMut(*mut std::ffi::c_void, c_ulong) -> c_ulong,
) -> Option<String> {
    let byte_len = call(std::ptr::null_mut(), 0);
    if byte_len < 2 {
        return None; // absent, or empty (just the terminator)
    }
    let unit_len = (byte_len as usize) / 2;
    let mut units = vec![0u16; unit_len];
    let written = call(units.as_mut_ptr().cast(), byte_len);
    if written == 0 {
        return None;
    }
    let written_units = (written as usize / 2).min(unit_len);
    // Strip the trailing NUL terminator.
    let text_units = &units[..written_units.saturating_sub(1)];
    if text_units.is_empty() {
        return None;
    }
    Some(String::from_utf16_lossy(text_units))
}
