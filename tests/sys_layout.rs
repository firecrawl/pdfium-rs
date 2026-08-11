//! ABI guards for the `sys` module: struct sizes/alignments on 64-bit
//! targets and the documented PDFium constant values. A failure here means
//! a struct or constant drifted from the C headers — an ABI break, not a
//! logic bug.

use firecrawl_pdfium::sys;
use std::mem::{align_of, size_of};

/// Pointer-width independent structs first: these contain only f32/u16
/// fields, so their layout is identical on every supported target.
#[test]
fn geometry_struct_sizes() {
    // FS_RECTF: 4 x f32.
    assert_eq!(size_of::<sys::FS_RECTF>(), 16);
    assert_eq!(align_of::<sys::FS_RECTF>(), 4);
    // FS_MATRIX: 6 x f32.
    assert_eq!(size_of::<sys::FS_MATRIX>(), 24);
    assert_eq!(align_of::<sys::FS_MATRIX>(), 4);
    // FS_SIZEF: 2 x f32.
    assert_eq!(size_of::<sys::FS_SIZEF>(), 8);
    // FS_POINTF: 2 x f32.
    assert_eq!(size_of::<sys::FS_POINTF>(), 8);
    // FPDF_SYSTEMTIME: 8 x c_ushort.
    assert_eq!(size_of::<sys::FPDF_SYSTEMTIME>(), 16);
    assert_eq!(align_of::<sys::FPDF_SYSTEMTIME>(), 2);
}

/// FPDF_LIBRARY_CONFIG on 64-bit targets, field by field:
///
/// | field             | type    | offset | size |
/// |-------------------|---------|--------|------|
/// | version           | c_int   |      0 |    4 |
/// | (padding)         |         |      4 |    4 |
/// | m_pUserFontPaths  | ptr     |      8 |    8 |
/// | m_pIsolate        | ptr     |     16 |    8 |
/// | m_v8EmbedderSlot  | c_uint  |     24 |    4 |
/// | (padding)         |         |     28 |    4 |
/// | m_pPlatform       | ptr     |     32 |    8 |
/// | m_RendererType    | c_int   |     40 |    4 |
/// | m_FontLibraryType | c_int   |     44 |    4 |
/// | m_BrotliEnabled   | c_int   |     48 |    4 |
/// | (tail padding)    |         |     52 |    4 |
///
/// Total: 56 bytes, alignment 8.
#[cfg(target_pointer_width = "64")]
#[test]
fn library_config_size() {
    assert_eq!(size_of::<sys::FPDF_LIBRARY_CONFIG>(), 56);
    assert_eq!(align_of::<sys::FPDF_LIBRARY_CONFIG>(), 8);
}

/// FPDF_FORMFILLINFO on 64-bit targets, derived field by field:
///
/// - `version` (c_int): offset 0, size 4, then 4 bytes padding to align
///   the first pointer member at 8.
/// - 16 pointer-sized version-1 members (`Release`, `FFI_Invalidate`,
///   `FFI_OutputSelectedRect`, `FFI_SetCursor`, `FFI_SetTimer`,
///   `FFI_KillTimer`, `FFI_GetLocalTime`, `FFI_OnChange`, `FFI_GetPage`,
///   `FFI_GetCurrentPage`, `FFI_GetRotation`, `FFI_ExecuteNamedAction`,
///   `FFI_SetTextFieldFocus`, `FFI_DoURIAction`, `FFI_DoGoToAction`,
///   `m_pJsPlatform`): 16 * 8 = 128 bytes, offsets 8..136.
/// - `xfa_disabled` (FPDF_BOOL = c_int): offset 136, size 4, then 4 bytes
///   padding to re-align at 144.
/// - 17 pointer-sized version-2 callbacks (`FFI_DisplayCaret` through
///   `FFI_DoURIActionWithKeyboardModifier`): 17 * 8 = 136 bytes,
///   offsets 144..280.
///
/// Total: 8 + 128 + 8 + 136 = 280 bytes, alignment 8. Verified both by
/// this computation and empirically against the running struct.
#[cfg(target_pointer_width = "64")]
#[test]
fn formfillinfo_size() {
    assert_eq!(size_of::<sys::FPDF_FORMFILLINFO>(), 280);
    assert_eq!(align_of::<sys::FPDF_FORMFILLINFO>(), 8);
}

/// `FPDF_DWORD` is C's `unsigned long`: 8 bytes on 64-bit Unix, 4 bytes on
/// Windows (LLP64). Getting this wrong corrupts every function signature
/// that passes one, so pin it per family.
#[cfg(all(target_pointer_width = "64", not(target_os = "windows")))]
#[test]
fn dword_is_unsigned_long() {
    assert_eq!(size_of::<sys::FPDF_DWORD>(), 8);
}

#[cfg(target_os = "windows")]
#[test]
fn dword_is_unsigned_long() {
    assert_eq!(size_of::<sys::FPDF_DWORD>(), 4);
}

/// Document-load error codes from fpdfview.h.
#[test]
fn error_code_constants() {
    assert_eq!(sys::FPDF_ERR_SUCCESS, 0);
    assert_eq!(sys::FPDF_ERR_UNKNOWN, 1);
    assert_eq!(sys::FPDF_ERR_FILE, 2);
    assert_eq!(sys::FPDF_ERR_FORMAT, 3);
    assert_eq!(sys::FPDF_ERR_PASSWORD, 4);
    assert_eq!(sys::FPDF_ERR_SECURITY, 5);
    assert_eq!(sys::FPDF_ERR_PAGE, 6);
}

/// Bitmap format selectors from fpdfview.h.
#[test]
fn bitmap_format_constants() {
    assert_eq!(sys::FPDFBitmap_Unknown, 0);
    assert_eq!(sys::FPDFBitmap_Gray, 1);
    assert_eq!(sys::FPDFBitmap_BGR, 2);
    assert_eq!(sys::FPDFBitmap_BGRx, 3);
    assert_eq!(sys::FPDFBitmap_BGRA, 4);
    assert_eq!(sys::FPDFBitmap_BGRA_Premul, 5);
}

/// Render flag bits from fpdfview.h.
#[test]
fn render_flag_constants() {
    assert_eq!(sys::FPDF_ANNOT, 0x01);
    assert_eq!(sys::FPDF_LCD_TEXT, 0x02);
    assert_eq!(sys::FPDF_NO_NATIVETEXT, 0x04);
    assert_eq!(sys::FPDF_GRAYSCALE, 0x08);
    assert_eq!(sys::FPDF_REVERSE_BYTE_ORDER, 0x10);
    assert_eq!(sys::FPDF_CONVERT_FILL_TO_STROKE, 0x20);
    assert_eq!(sys::FPDF_DEBUG_INFO, 0x80);
    assert_eq!(sys::FPDF_NO_CATCH, 0x100);
    assert_eq!(sys::FPDF_RENDER_LIMITEDIMAGECACHE, 0x200);
    assert_eq!(sys::FPDF_RENDER_FORCEHALFTONE, 0x400);
    assert_eq!(sys::FPDF_PRINTING, 0x800);
    assert_eq!(sys::FPDF_RENDER_NO_SMOOTHTEXT, 0x1000);
    assert_eq!(sys::FPDF_RENDER_NO_SMOOTHIMAGE, 0x2000);
    assert_eq!(sys::FPDF_RENDER_NO_SMOOTHPATH, 0x4000);
}

/// Form type values from fpdf_formfill.h.
#[test]
fn form_type_constants() {
    assert_eq!(sys::FORMTYPE_NONE, 0);
    assert_eq!(sys::FORMTYPE_ACRO_FORM, 1);
    assert_eq!(sys::FORMTYPE_XFA_FULL, 2);
    assert_eq!(sys::FORMTYPE_XFA_FOREGROUND, 3);
    assert_eq!(sys::FPDF_FORMFIELD_UNKNOWN, 0);
}

/// Opaque handles are pointer-sized (they are `*mut` to opaque markers).
#[test]
fn handles_are_pointers() {
    assert_eq!(size_of::<sys::FPDF_DOCUMENT>(), size_of::<*mut ()>());
    assert_eq!(size_of::<sys::FPDF_PAGE>(), size_of::<*mut ()>());
    assert_eq!(size_of::<sys::FPDF_BITMAP>(), size_of::<*mut ()>());
    assert_eq!(size_of::<sys::FPDF_TEXTPAGE>(), size_of::<*mut ()>());
    assert_eq!(size_of::<sys::FPDF_FORMHANDLE>(), size_of::<*mut ()>());
}
