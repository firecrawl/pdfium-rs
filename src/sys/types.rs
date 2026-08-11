//! C ABI types, structs, and constants for the PDFium public API.
//!
//! Everything in this module is transcribed from the headers shipped inside
//! the `pdfium-binaries` release archives (`include/fpdfview.h`,
//! `include/fpdf_formfill.h`, ...). Field order and types are ABI contracts:
//! do not reorder or retype fields without re-verifying against the headers
//! for the pinned PDFium version (see `PDFIUM_VERSION` in `xtask`).

#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

use std::ffi::{c_char, c_int, c_uint, c_ulong, c_ushort, c_void};

// ---------------------------------------------------------------------------
// Scalar typedefs (fpdfview.h)
// ---------------------------------------------------------------------------

/// `typedef int FPDF_BOOL;` — nonzero is true.
pub type FPDF_BOOL = c_int;
/// `typedef unsigned long FPDF_DWORD;`
///
/// Note: `unsigned long` is 32-bit on Windows and 64-bit on 64-bit Unix.
/// Using `c_ulong` (not `u32`) here is load-bearing for ABI correctness.
pub type FPDF_DWORD = c_ulong;
/// `typedef const char* FPDF_BYTESTRING;` — NUL-terminated byte string.
pub type FPDF_BYTESTRING = *const c_char;
/// `typedef const FPDF_WCHAR* FPDF_WIDESTRING;` — UTF-16LE, NUL-terminated.
pub type FPDF_WIDESTRING = *const c_ushort;
/// `typedef const char* FPDF_STRING;`
pub type FPDF_STRING = *const c_char;

// ---------------------------------------------------------------------------
// Opaque handles
// ---------------------------------------------------------------------------

macro_rules! opaque_handle {
    ($(#[$doc:meta])* $marker:ident, $handle:ident) => {
        $(#[$doc])*
        #[repr(C)]
        #[doc(hidden)]
        pub struct $marker {
            _data: [u8; 0],
            _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
        }
        pub type $handle = *mut $marker;
    };
}

opaque_handle!(/// Opaque document object. `typedef void* FPDF_DOCUMENT;`
    fpdf_document_t__, FPDF_DOCUMENT);
opaque_handle!(/// Opaque page object.
    fpdf_page_t__, FPDF_PAGE);
opaque_handle!(/// Opaque bitmap object.
    fpdf_bitmap_t__, FPDF_BITMAP);
opaque_handle!(/// Opaque text page object.
    fpdf_textpage_t__, FPDF_TEXTPAGE);
opaque_handle!(/// Opaque form-fill environment handle.
    fpdf_formhandle_t__, FPDF_FORMHANDLE);

// ---------------------------------------------------------------------------
// Geometry structs (fpdfview.h)
// ---------------------------------------------------------------------------

/// Rectangle area in device or page coordinates.
///
/// ```c
/// typedef struct _FS_RECTF_ { float left, top, right, bottom; } FS_RECTF;
/// ```
/// `(left, top)` is the top-left corner and `(right, bottom)` the
/// bottom-right corner.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FS_RECTF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// `typedef struct FS_SIZEF_ { float width, height; } FS_SIZEF;`
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FS_SIZEF {
    pub width: f32,
    pub height: f32,
}

/// `typedef struct FS_POINTF_ { float x, y; } FS_POINTF;`
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FS_POINTF {
    pub x: f32,
    pub y: f32,
}

/// 2D transform matrix: `[a b 0; c d 0; e f 1]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FS_MATRIX {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

// ---------------------------------------------------------------------------
// Library configuration (fpdfview.h)
// ---------------------------------------------------------------------------

/// `FPDF_LIBRARY_CONFIG`, passed to `FPDF_InitLibraryWithConfig`.
///
/// We always initialize with `version = 2`, so PDFium reads only the fields
/// through `m_v8EmbedderSlot`; the version 4-6 fields exist to keep the
/// struct at full size for forward compatibility and are zeroed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FPDF_LIBRARY_CONFIG {
    /// Interface version. "Currently must be 2."
    pub version: c_int,
    /// NULL-terminated array of font directory paths, or null for defaults.
    pub m_pUserFontPaths: *const *const c_char,
    /// Version 2: `v8::Isolate*` to use, or null. Unused in non-V8 builds.
    pub m_pIsolate: *mut c_void,
    /// Version 2: embedder data slot in the isolate. 0 is fine.
    pub m_v8EmbedderSlot: c_uint,
    /// Version 3 (experimental): `v8::Platform*`.
    pub m_pPlatform: *mut c_void,
    /// Version 4 (experimental): `FPDF_RENDERER_TYPE` (C enum, int-sized).
    pub m_RendererType: c_int,
    /// Version 5 (experimental): `FPDF_FONT_BACKEND_TYPE` (C enum, int-sized).
    pub m_FontLibraryType: c_int,
    /// Version 6 (experimental): enable /BrotliDecode when built with it.
    pub m_BrotliEnabled: FPDF_BOOL,
}

// ---------------------------------------------------------------------------
// Error codes (fpdfview.h)
// ---------------------------------------------------------------------------

pub const FPDF_ERR_SUCCESS: c_ulong = 0; // No error.
pub const FPDF_ERR_UNKNOWN: c_ulong = 1; // Unknown error.
pub const FPDF_ERR_FILE: c_ulong = 2; // File not found or could not be opened.
pub const FPDF_ERR_FORMAT: c_ulong = 3; // File not in PDF format or corrupted.
pub const FPDF_ERR_PASSWORD: c_ulong = 4; // Password required or incorrect password.
pub const FPDF_ERR_SECURITY: c_ulong = 5; // Unsupported security scheme.
pub const FPDF_ERR_PAGE: c_ulong = 6; // Page not found or content error.

// ---------------------------------------------------------------------------
// Bitmap formats (fpdfview.h)
// ---------------------------------------------------------------------------

pub const FPDFBitmap_Unknown: c_int = 0;
/// Gray scale bitmap, one byte per pixel.
pub const FPDFBitmap_Gray: c_int = 1;
/// 3 bytes per pixel, byte order: blue, green, red.
pub const FPDFBitmap_BGR: c_int = 2;
/// 4 bytes per pixel, byte order: blue, green, red, unused.
pub const FPDFBitmap_BGRx: c_int = 3;
/// 4 bytes per pixel, byte order: blue, green, red, alpha (straight alpha).
pub const FPDFBitmap_BGRA: c_int = 4;
/// 4 bytes per pixel, byte order: blue, green, red, alpha (premultiplied).
pub const FPDFBitmap_BGRA_Premul: c_int = 5;

// ---------------------------------------------------------------------------
// Render flags (fpdfview.h)
// ---------------------------------------------------------------------------

/// Set if annotations are to be rendered.
pub const FPDF_ANNOT: c_int = 0x01;
/// Set if using text rendering optimized for LCD display.
pub const FPDF_LCD_TEXT: c_int = 0x02;
/// Don't use the native text output available on some platforms.
pub const FPDF_NO_NATIVETEXT: c_int = 0x04;
/// Grayscale output.
pub const FPDF_GRAYSCALE: c_int = 0x08;
/// Set whether to render in a reverse Byte order (BGRA -> RGBA).
pub const FPDF_REVERSE_BYTE_ORDER: c_int = 0x10;
/// Set whether fill paths need to be stroked.
pub const FPDF_CONVERT_FILL_TO_STROKE: c_int = 0x20;
/// Set if you want to get some debug info.
pub const FPDF_DEBUG_INFO: c_int = 0x80;
/// Set if you don't want to catch exceptions.
pub const FPDF_NO_CATCH: c_int = 0x100;
/// Limit image cache size.
pub const FPDF_RENDER_LIMITEDIMAGECACHE: c_int = 0x200;
/// Always use halftone for image stretching.
pub const FPDF_RENDER_FORCEHALFTONE: c_int = 0x400;
/// Render for printing.
pub const FPDF_PRINTING: c_int = 0x800;
/// Set to disable anti-aliasing on text.
pub const FPDF_RENDER_NO_SMOOTHTEXT: c_int = 0x1000;
/// Set to disable anti-aliasing on images.
pub const FPDF_RENDER_NO_SMOOTHIMAGE: c_int = 0x2000;
/// Set to disable anti-aliasing on paths.
pub const FPDF_RENDER_NO_SMOOTHPATH: c_int = 0x4000;

// ---------------------------------------------------------------------------
// Form types (fpdf_formfill.h)
// ---------------------------------------------------------------------------

/// Document contains no forms.
pub const FORMTYPE_NONE: c_int = 0;
/// Forms are specified using AcroForm spec.
pub const FORMTYPE_ACRO_FORM: c_int = 1;
/// Forms are specified using the entire XFA spec.
pub const FORMTYPE_XFA_FULL: c_int = 2;
/// Forms are specified using the XFAF subset of XFA spec.
pub const FORMTYPE_XFA_FOREGROUND: c_int = 3;

// ---------------------------------------------------------------------------
// Form-fill environment (fpdf_formfill.h)
// ---------------------------------------------------------------------------

/// `typedef void (*TimerCallback)(int idEvent);`
pub type TimerCallback = Option<unsafe extern "C" fn(idEvent: c_int)>;

/// `FPDF_SYSTEMTIME` — returned by value from `FFI_GetLocalTime`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FPDF_SYSTEMTIME {
    pub wYear: c_ushort,
    pub wMonth: c_ushort,
    pub wDayOfWeek: c_ushort,
    pub wDay: c_ushort,
    pub wHour: c_ushort,
    pub wMinute: c_ushort,
    pub wSecond: c_ushort,
    pub wMilliseconds: c_ushort,
}

/// The complete `FPDF_FORMFILLINFO` interface struct.
///
/// Transcribed field-for-field from `fpdf_formfill.h`. In current PDFium the
/// version-2/XFA members are unconditional struct members (their *use* is
/// gated on `version` and build flags, but the layout is fixed), so a single
/// Rust definition is ABI-correct for XFA and non-XFA builds alike.
///
/// We always submit `version = 1` and implement no-op stubs for the callbacks
/// the header marks "Implementation Required: yes" for version 1; everything
/// else is `None`/null. Stub callbacks MUST NOT call back into PDFium: the
/// caller holds the global FFI lock while PDFium runs.
#[repr(C)]
pub struct FPDF_FORMFILLINFO {
    /// Interface version: 1 (stable) or 2 (adds experimental/XFA interfaces).
    pub version: c_int,

    // --- Version 1 ---
    /// Optional. Final-cleanup hook.
    pub Release: Option<unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO)>,
    /// Required. Invalidate the client area within the specified rectangle.
    pub FFI_Invalidate: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            page: FPDF_PAGE,
            left: f64,
            top: f64,
            right: f64,
            bottom: f64,
        ),
    >,
    /// Optional. Selected-text rectangles callback.
    pub FFI_OutputSelectedRect: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            page: FPDF_PAGE,
            left: f64,
            top: f64,
            right: f64,
            bottom: f64,
        ),
    >,
    /// Required. Set the cursor shape.
    pub FFI_SetCursor:
        Option<unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO, nCursorType: c_int)>,
    /// Required. Install a system timer; return nonzero timer id on success.
    pub FFI_SetTimer: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            uElapse: c_int,
            lpTimerFunc: TimerCallback,
        ) -> c_int,
    >,
    /// Required. Uninstall a timer set by `FFI_SetTimer`.
    pub FFI_KillTimer:
        Option<unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO, nTimerID: c_int)>,
    /// Required (header notes it is currently unused). Local time query.
    pub FFI_GetLocalTime:
        Option<unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO) -> FPDF_SYSTEMTIME>,
    /// Optional. Form field value change notification.
    pub FFI_OnChange: Option<unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO)>,
    /// Required. Map page index -> already-loaded page handle (may be null).
    pub FFI_GetPage: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            document: FPDF_DOCUMENT,
            nPageIndex: c_int,
        ) -> FPDF_PAGE,
    >,
    /// Required only with V8 support, otherwise unused.
    pub FFI_GetCurrentPage: Option<
        unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO, document: FPDF_DOCUMENT) -> FPDF_PAGE,
    >,
    /// Required (header notes it is currently unused). Page-view rotation.
    pub FFI_GetRotation:
        Option<unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO, page: FPDF_PAGE) -> c_int>,
    /// Required. Execute a named action.
    pub FFI_ExecuteNamedAction: Option<
        unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO, namedAction: FPDF_BYTESTRING),
    >,
    /// Optional. Text field focus change.
    pub FFI_SetTextFieldFocus: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            value: FPDF_WIDESTRING,
            valueLen: FPDF_DWORD,
            is_focus: FPDF_BOOL,
        ),
    >,
    /// Optional. Navigate to URI.
    pub FFI_DoURIAction:
        Option<unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO, bsURI: FPDF_BYTESTRING)>,
    /// Optional. GoTo action.
    pub FFI_DoGoToAction: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            nPageIndex: c_int,
            zoomMode: c_int,
            fPosArray: *mut f32,
            sizeofArray: c_int,
        ),
    >,
    /// `IPDF_JSPLATFORM*`. Unused without V8; null disables JavaScript.
    pub m_pJsPlatform: *mut c_void,

    // --- Version 2 (experimental; ignored when version == 1) ---
    /// Whether the XFA module is disabled when built with the XFA module.
    pub xfa_disabled: FPDF_BOOL,
    pub FFI_DisplayCaret: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            page: FPDF_PAGE,
            bVisible: FPDF_BOOL,
            left: f64,
            top: f64,
            right: f64,
            bottom: f64,
        ),
    >,
    pub FFI_GetCurrentPageIndex: Option<
        unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO, document: FPDF_DOCUMENT) -> c_int,
    >,
    pub FFI_SetCurrentPage: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            document: FPDF_DOCUMENT,
            iCurPage: c_int,
        ),
    >,
    pub FFI_GotoURL: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            document: FPDF_DOCUMENT,
            wsURL: FPDF_WIDESTRING,
        ),
    >,
    pub FFI_GetPageViewRect: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            page: FPDF_PAGE,
            left: *mut f64,
            top: *mut f64,
            right: *mut f64,
            bottom: *mut f64,
        ),
    >,
    pub FFI_PageEvent: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            page_count: c_int,
            event_type: FPDF_DWORD,
        ),
    >,
    pub FFI_PopupMenu: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            page: FPDF_PAGE,
            hWidget: *mut c_void, // FPDF_WIDGET; always null, compatibility only
            menuFlag: c_int,
            x: f32,
            y: f32,
        ) -> FPDF_BOOL,
    >,
    pub FFI_OpenFile: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            fileFlag: c_int,
            wsURL: FPDF_WIDESTRING,
            mode: *const c_char,
        ) -> *mut c_void, // FPDF_FILEHANDLER*
    >,
    pub FFI_EmailTo: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            fileHandler: *mut c_void, // FPDF_FILEHANDLER*
            pTo: FPDF_WIDESTRING,
            pSubject: FPDF_WIDESTRING,
            pCC: FPDF_WIDESTRING,
            pBcc: FPDF_WIDESTRING,
            pMsg: FPDF_WIDESTRING,
        ),
    >,
    pub FFI_UploadTo: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            fileHandler: *mut c_void, // FPDF_FILEHANDLER*
            fileFlag: c_int,
            uploadTo: FPDF_WIDESTRING,
        ),
    >,
    pub FFI_GetPlatform: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            platform: *mut c_void,
            length: c_int,
        ) -> c_int,
    >,
    pub FFI_GetLanguage: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            language: *mut c_void,
            length: c_int,
        ) -> c_int,
    >,
    pub FFI_DownloadFromURL: Option<
        unsafe extern "C" fn(pThis: *mut FPDF_FORMFILLINFO, URL: FPDF_WIDESTRING) -> *mut c_void, // FPDF_FILEHANDLER*
    >,
    pub FFI_PostRequestURL: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            wsURL: FPDF_WIDESTRING,
            wsData: FPDF_WIDESTRING,
            wsContentType: FPDF_WIDESTRING,
            wsEncode: FPDF_WIDESTRING,
            wsHeader: FPDF_WIDESTRING,
            response: *mut c_void, // FPDF_BSTR*
        ) -> FPDF_BOOL,
    >,
    pub FFI_PutRequestURL: Option<
        unsafe extern "C" fn(
            pThis: *mut FPDF_FORMFILLINFO,
            wsURL: FPDF_WIDESTRING,
            wsData: FPDF_WIDESTRING,
            wsEncode: FPDF_WIDESTRING,
        ) -> FPDF_BOOL,
    >,
    pub FFI_OnFocusChange: Option<
        unsafe extern "C" fn(
            param: *mut FPDF_FORMFILLINFO,
            annot: *mut c_void, // FPDF_ANNOTATION
            page_index: c_int,
        ),
    >,
    pub FFI_DoURIActionWithKeyboardModifier: Option<
        unsafe extern "C" fn(
            param: *mut FPDF_FORMFILLINFO,
            uri: FPDF_BYTESTRING,
            modifiers: c_int,
        ),
    >,
}

/// Form field type selector for `FPDF_SetFormFieldHighlightColor`; the
/// `UNKNOWN` value applies the highlight color to all field types.
pub const FPDF_FORMFIELD_UNKNOWN: c_int = 0;
