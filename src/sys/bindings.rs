//! The PDFium function table, resolved from a dynamically loaded library.
//!
//! Every function bound here was transcribed from the headers shipped in the
//! `pdfium-binaries` archives (see `types.rs` for the same statement about
//! structs). Symbols are resolved eagerly in [`Bindings::load_from_library`]:
//! a library missing any bound symbol is rejected at load time with
//! [`MissingSymbolError`], never at call time.

#![allow(non_snake_case)] // fields/methods intentionally mirror the C API

use std::ffi::{c_int, c_uchar, c_uint, c_ulong, c_ushort, c_void};
use std::fmt;

use super::types::*;

/// Error produced when a symbol required by the bindings table is absent
/// from the loaded library — in practice, a PDFium build older than the
/// oldest version this crate supports.
#[derive(Debug)]
pub struct MissingSymbolError {
    pub symbol: &'static str,
    pub source: libloading::Error,
}

impl fmt::Display for MissingSymbolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PDFium library is missing required symbol `{}` (library too old or not PDFium)",
            self.symbol
        )
    }
}

impl std::error::Error for MissingSymbolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

macro_rules! pdfium_bindings {
    ($( fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?; )*) => {
        /// Table of resolved PDFium function pointers plus the library they
        /// were resolved from.
        ///
        /// # Safety contract
        ///
        /// Every method on this type is `unsafe` and shares two
        /// preconditions beyond the per-function pointer/handle validity
        /// requirements documented by PDFium:
        ///
        /// 1. **Serialization.** PDFium is not thread-safe. The caller must
        ///    guarantee that no two PDFium calls (through any `Bindings`
        ///    instance in the process) execute concurrently. The safe layer
        ///    does this with a process-global mutex.
        /// 2. **Initialization.** `FPDF_InitLibraryWithConfig` must have been
        ///    called (once) before any other function, and
        ///    `FPDF_DestroyLibrary` must not have been called.
        pub struct Bindings {
            $( $name: unsafe extern "C" fn($($ty),*) $(-> $ret)?, )*
            /// Keeps the shared library mapped. Field order is irrelevant
            /// for safety here because `Bindings` is never dropped while a
            /// function pointer is in flight (the safe layer holds it in a
            /// process-lifetime static), but the library is declared last so
            /// pointers never outlive it even in a hypothetical drop.
            _library: libloading::Library,
        }

        impl Bindings {
            /// Resolves the complete function table from `library`.
            ///
            /// # Safety
            ///
            /// The library must be PDFium (or ABI-compatible): each resolved
            /// symbol is blindly trusted to have the C signature transcribed
            /// in this file. Loading symbols from an unrelated library that
            /// happens to export these names is undefined behavior.
            pub unsafe fn load_from_library(
                library: libloading::Library,
            ) -> Result<Box<Self>, MissingSymbolError> {
                Ok(Box::new(Bindings {
                    $(
                        // SAFETY: symbol type is the signature transcribed
                        // from the PDFium headers; caller guarantees the
                        // library is PDFium (this function's contract).
                        $name: *unsafe {
                            library.get(concat!(stringify!($name), "\0").as_bytes())
                        }
                        .map_err(|source| MissingSymbolError {
                            symbol: stringify!($name),
                            source,
                        })?,
                    )*
                    _library: library,
                }))
            }

            $(
                /// Direct call to the PDFium function of the same name.
                ///
                /// # Safety
                ///
                /// See the type-level safety contract, plus the PDFium
                /// documentation for this function's own preconditions.
                #[allow(non_snake_case, clippy::too_many_arguments)]
                #[inline]
                pub unsafe fn $name(&self, $($arg: $ty),*) $(-> $ret)? {
                    // SAFETY: forwarded verbatim; the caller upholds this
                    // method's documented preconditions.
                    unsafe { (self.$name)($($arg),*) }
                }
            )*
        }
    };
}

pdfium_bindings! {
    // --- Library lifecycle (fpdfview.h) ---
    fn FPDF_InitLibraryWithConfig(config: *const FPDF_LIBRARY_CONFIG);
    fn FPDF_DestroyLibrary();
    fn FPDF_GetLastError() -> c_ulong;

    // --- Document (fpdfview.h) ---
    fn FPDF_LoadMemDocument64(
        data_buf: *const c_void,
        size: usize,
        password: FPDF_BYTESTRING,
    ) -> FPDF_DOCUMENT;
    fn FPDF_CloseDocument(document: FPDF_DOCUMENT);
    fn FPDF_GetPageCount(document: FPDF_DOCUMENT) -> c_int;
    fn FPDF_GetDocPermissions(document: FPDF_DOCUMENT) -> c_ulong;
    fn FPDF_GetSecurityHandlerRevision(document: FPDF_DOCUMENT) -> c_int;
    fn FPDF_GetFileVersion(doc: FPDF_DOCUMENT, fileVersion: *mut c_int) -> FPDF_BOOL;
    fn FPDF_GetPageSizeByIndexF(
        document: FPDF_DOCUMENT,
        page_index: c_int,
        size: *mut FS_SIZEF,
    ) -> FPDF_BOOL;

    // --- Page (fpdfview.h / fpdf_edit.h) ---
    fn FPDF_LoadPage(document: FPDF_DOCUMENT, page_index: c_int) -> FPDF_PAGE;
    fn FPDF_ClosePage(page: FPDF_PAGE);
    fn FPDF_GetPageWidthF(page: FPDF_PAGE) -> f32;
    fn FPDF_GetPageHeightF(page: FPDF_PAGE) -> f32;
    fn FPDF_GetPageBoundingBox(page: FPDF_PAGE, rect: *mut FS_RECTF) -> FPDF_BOOL;
    fn FPDFPage_GetRotation(page: FPDF_PAGE) -> c_int;
    fn FPDFPage_HasTransparency(page: FPDF_PAGE) -> FPDF_BOOL;

    // --- Rendering (fpdfview.h) ---
    fn FPDF_RenderPageBitmap(
        bitmap: FPDF_BITMAP,
        page: FPDF_PAGE,
        start_x: c_int,
        start_y: c_int,
        size_x: c_int,
        size_y: c_int,
        rotate: c_int,
        flags: c_int,
    );
    fn FPDF_RenderPageBitmapWithMatrix(
        bitmap: FPDF_BITMAP,
        page: FPDF_PAGE,
        matrix: *const FS_MATRIX,
        clipping: *const FS_RECTF,
        flags: c_int,
    );

    // --- Coordinate transforms (fpdfview.h) ---
    fn FPDF_DeviceToPage(
        page: FPDF_PAGE,
        start_x: c_int,
        start_y: c_int,
        size_x: c_int,
        size_y: c_int,
        rotate: c_int,
        device_x: c_int,
        device_y: c_int,
        page_x: *mut f64,
        page_y: *mut f64,
    ) -> FPDF_BOOL;
    fn FPDF_PageToDevice(
        page: FPDF_PAGE,
        start_x: c_int,
        start_y: c_int,
        size_x: c_int,
        size_y: c_int,
        rotate: c_int,
        page_x: f64,
        page_y: f64,
        device_x: *mut c_int,
        device_y: *mut c_int,
    ) -> FPDF_BOOL;

    // --- Bitmaps (fpdfview.h) ---
    fn FPDFBitmap_CreateEx(
        width: c_int,
        height: c_int,
        format: c_int,
        first_scan: *mut c_void,
        stride: c_int,
    ) -> FPDF_BITMAP;
    fn FPDFBitmap_FillRect(
        bitmap: FPDF_BITMAP,
        left: c_int,
        top: c_int,
        width: c_int,
        height: c_int,
        color: FPDF_DWORD,
    ) -> FPDF_BOOL;
    fn FPDFBitmap_Destroy(bitmap: FPDF_BITMAP);
    fn FPDFBitmap_GetBuffer(bitmap: FPDF_BITMAP) -> *mut c_void;
    fn FPDFBitmap_GetWidth(bitmap: FPDF_BITMAP) -> c_int;
    fn FPDFBitmap_GetHeight(bitmap: FPDF_BITMAP) -> c_int;
    fn FPDFBitmap_GetStride(bitmap: FPDF_BITMAP) -> c_int;
    fn FPDFBitmap_GetFormat(bitmap: FPDF_BITMAP) -> c_int;

    // --- Metadata (fpdf_doc.h) ---
    fn FPDF_GetMetaText(
        document: FPDF_DOCUMENT,
        tag: FPDF_BYTESTRING,
        buffer: *mut c_void,
        buflen: c_ulong,
    ) -> c_ulong;
    fn FPDF_GetPageLabel(
        document: FPDF_DOCUMENT,
        page_index: c_int,
        buffer: *mut c_void,
        buflen: c_ulong,
    ) -> c_ulong;

    // --- Text extraction (fpdf_text.h) ---
    fn FPDFText_LoadPage(page: FPDF_PAGE) -> FPDF_TEXTPAGE;
    fn FPDFText_ClosePage(text_page: FPDF_TEXTPAGE);
    fn FPDFText_CountChars(text_page: FPDF_TEXTPAGE) -> c_int;
    fn FPDFText_GetText(
        text_page: FPDF_TEXTPAGE,
        start_index: c_int,
        count: c_int,
        result: *mut c_ushort,
    ) -> c_int;
    fn FPDFText_GetUnicode(text_page: FPDF_TEXTPAGE, index: c_int) -> c_uint;
    fn FPDFText_GetCharBox(
        text_page: FPDF_TEXTPAGE,
        index: c_int,
        left: *mut f64,
        right: *mut f64,
        bottom: *mut f64,
        top: *mut f64,
    ) -> FPDF_BOOL;
    fn FPDFText_GetCharOrigin(
        text_page: FPDF_TEXTPAGE,
        index: c_int,
        x: *mut f64,
        y: *mut f64,
    ) -> FPDF_BOOL;
    fn FPDFText_GetLooseCharBox(
        text_page: FPDF_TEXTPAGE,
        index: c_int,
        rect: *mut FS_RECTF,
    ) -> FPDF_BOOL;
    fn FPDFText_CountRects(text_page: FPDF_TEXTPAGE, start_index: c_int, count: c_int) -> c_int;
    fn FPDFText_GetRect(
        text_page: FPDF_TEXTPAGE,
        rect_index: c_int,
        left: *mut f64,
        top: *mut f64,
        right: *mut f64,
        bottom: *mut f64,
    ) -> FPDF_BOOL;

    // --- Forms (fpdf_formfill.h) ---
    fn FPDFDOC_InitFormFillEnvironment(
        document: FPDF_DOCUMENT,
        formInfo: *mut FPDF_FORMFILLINFO,
    ) -> FPDF_FORMHANDLE;
    fn FPDFDOC_ExitFormFillEnvironment(hHandle: FPDF_FORMHANDLE);
    fn FPDF_FFLDraw(
        hHandle: FPDF_FORMHANDLE,
        bitmap: FPDF_BITMAP,
        page: FPDF_PAGE,
        start_x: c_int,
        start_y: c_int,
        size_x: c_int,
        size_y: c_int,
        rotate: c_int,
        flags: c_int,
    );
    fn FORM_OnAfterLoadPage(page: FPDF_PAGE, hHandle: FPDF_FORMHANDLE);
    fn FORM_OnBeforeClosePage(page: FPDF_PAGE, hHandle: FPDF_FORMHANDLE);
    fn FPDF_GetFormType(document: FPDF_DOCUMENT) -> c_int;
    fn FPDF_SetFormFieldHighlightColor(
        hHandle: FPDF_FORMHANDLE,
        fieldType: c_int,
        color: c_ulong,
    );
    fn FPDF_SetFormFieldHighlightAlpha(hHandle: FPDF_FORMHANDLE, alpha: c_uchar);
}

// `FPDF_InitLibrary` (config-less) is intentionally not bound: we always use
// `FPDF_InitLibraryWithConfig` with a version-2 config and null font paths.
// `FPDF_LoadMemDocument` (32-bit size) is superseded by the 64-bit variant.
// Windows-only symbols (`FPDF_RenderPage`, `FPDF_SetPrintMode`) and
// Skia-only symbols must never be added to this table: it is resolved
// eagerly on every platform.
