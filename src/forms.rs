//! Form-fill environment: form type detection and the no-op callback stubs
//! required to render AcroForm field appearances.

use std::ffi::c_int;

use crate::error::{Error, Result};
use crate::library::Pdfium;
use crate::sys;

/// The kind of interactive form a document contains, per
/// `FPDF_GetFormType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormType {
    /// No interactive form.
    None,
    /// AcroForm (the widely supported kind; renderable by this crate).
    AcroForm,
    /// Full XFA form. PDFium default builds cannot render XFA content;
    /// AcroForm fallback pages, if present, may still render.
    XfaFull,
    /// XFA foreground (XFAF) subset.
    XfaForeground,
    /// A value this crate does not recognize (newer PDFium).
    Unknown(i32),
}

impl FormType {
    pub(crate) fn from_raw(raw: c_int) -> FormType {
        match raw {
            sys::FORMTYPE_NONE => FormType::None,
            sys::FORMTYPE_ACRO_FORM => FormType::AcroForm,
            sys::FORMTYPE_XFA_FULL => FormType::XfaFull,
            sys::FORMTYPE_XFA_FOREGROUND => FormType::XfaForeground,
            other => FormType::Unknown(other),
        }
    }

    /// True if this crate can render the form's field appearances
    /// (AcroForm only).
    pub fn is_renderable(&self) -> bool {
        matches!(self, FormType::AcroForm)
    }
}

/// An initialized PDFium form-fill environment, owned by a `PdfDocument`.
///
/// Holds the `FPDF_FORMFILLINFO` at a stable heap address for the whole
/// life of the form handle — PDFium requires: "The FPDF_FORMFILLINFO passed
/// in via |formInfo| must remain valid until the returned FPDF_FORMHANDLE
/// is closed."
pub(crate) struct FormEnv {
    /// Boxed so the address PDFium captured stays stable.
    info: Box<sys::FPDF_FORMFILLINFO>,
    handle: sys::FPDF_FORMHANDLE,
}

// SAFETY: the raw handle and the boxed info struct are only dereferenced by
// PDFium during FFI calls, and every FFI call in this crate is serialized
// behind the process-wide lock. Moving/sharing the owning struct across
// threads does not touch them.
unsafe impl Send for FormEnv {}
unsafe impl Sync for FormEnv {}

impl FormEnv {
    /// Initializes the form-fill environment for `document`.
    ///
    /// Caller must guarantee `document` is a live handle owned by the same
    /// `PdfDocument` that will own the returned env (enforced by the only
    /// call site).
    pub(crate) fn new(pdfium: Pdfium, document: sys::FPDF_DOCUMENT) -> Result<FormEnv> {
        let mut info = Box::new(new_formfillinfo());
        let handle = pdfium.ffi(|b|
            // SAFETY: `document` is live; `info` is a fully initialized
            // version-1 FPDF_FORMFILLINFO whose heap address stays stable
            // for the life of the returned handle (boxed, dropped only in
            // FormEnv::drop after ExitFormFillEnvironment).
            unsafe { b.FPDFDOC_InitFormFillEnvironment(document, info.as_mut()) });
        if handle.is_null() {
            return Err(Error::FormInitFailed);
        }
        Ok(FormEnv { info, handle })
    }

    pub(crate) fn handle(&self) -> sys::FPDF_FORMHANDLE {
        self.handle
    }

    /// Tears down the environment. Must be called (under the FFI lock,
    /// with the owning document still open) before the document closes;
    /// `PdfDocument::drop` does this.
    pub(crate) fn destroy(&mut self, bindings: &sys::Bindings) {
        if !self.handle.is_null() {
            // SAFETY: handle is live and owned by us; PDFium docs:
            // "This function is a no-op when |hHandle| is null", and takes
            // ownership of the handle. `info` stays alive (boxed in self)
            // until after this call, satisfying the lifetime rule.
            unsafe { bindings.FPDFDOC_ExitFormFillEnvironment(self.handle) };
            self.handle = std::ptr::null_mut();
        }
        let _ = &self.info; // keep the box alive through teardown
    }
}

/// Builds the version-1 `FPDF_FORMFILLINFO` this crate submits.
///
/// The header marks these version-1 members "Implementation Required: yes":
/// `FFI_Invalidate`, `FFI_SetCursor`, `FFI_SetTimer`, `FFI_KillTimer`,
/// `FFI_GetLocalTime`, `FFI_GetPage`, `FFI_GetRotation`,
/// `FFI_ExecuteNamedAction` (plus `FFI_GetCurrentPage` "when V8 support is
/// present"). All of them get no-op stubs below; optional members are
/// `None`. Version-2/XFA members are ignored for a version-1 client but the
/// struct layout includes them (see `sys::FPDF_FORMFILLINFO`).
///
/// **Stub contract: no stub may call back into PDFium** — stubs run while
/// the process-wide FFI lock is held by the frame that entered PDFium.
fn new_formfillinfo() -> sys::FPDF_FORMFILLINFO {
    sys::FPDF_FORMFILLINFO {
        version: 1,
        Release: None,
        FFI_Invalidate: Some(ffi_invalidate),
        FFI_OutputSelectedRect: None,
        FFI_SetCursor: Some(ffi_set_cursor),
        FFI_SetTimer: Some(ffi_set_timer),
        FFI_KillTimer: Some(ffi_kill_timer),
        FFI_GetLocalTime: Some(ffi_get_local_time),
        FFI_OnChange: None,
        FFI_GetPage: Some(ffi_get_page),
        FFI_GetCurrentPage: Some(ffi_get_current_page),
        FFI_GetRotation: Some(ffi_get_rotation),
        FFI_ExecuteNamedAction: Some(ffi_execute_named_action),
        FFI_SetTextFieldFocus: None,
        FFI_DoURIAction: None,
        FFI_DoGoToAction: None,
        m_pJsPlatform: std::ptr::null_mut(),
        xfa_disabled: 0,
        FFI_DisplayCaret: None,
        FFI_GetCurrentPageIndex: None,
        FFI_SetCurrentPage: None,
        FFI_GotoURL: None,
        FFI_GetPageViewRect: None,
        FFI_PageEvent: None,
        FFI_PopupMenu: None,
        FFI_OpenFile: None,
        FFI_EmailTo: None,
        FFI_UploadTo: None,
        FFI_GetPlatform: None,
        FFI_GetLanguage: None,
        FFI_DownloadFromURL: None,
        FFI_PostRequestURL: None,
        FFI_PutRequestURL: None,
        FFI_OnFocusChange: None,
        FFI_DoURIActionWithKeyboardModifier: None,
    }
}

unsafe extern "C" fn ffi_invalidate(
    _this: *mut sys::FPDF_FORMFILLINFO,
    _page: sys::FPDF_PAGE,
    _left: f64,
    _top: f64,
    _right: f64,
    _bottom: f64,
) {
    // We render on demand; there is no incremental screen to invalidate.
}

unsafe extern "C" fn ffi_set_cursor(_this: *mut sys::FPDF_FORMFILLINFO, _cursor: c_int) {}

unsafe extern "C" fn ffi_set_timer(
    _this: *mut sys::FPDF_FORMFILLINFO,
    _elapse: c_int,
    _timer_func: sys::TimerCallback,
) -> c_int {
    // 0 = "could not install a timer". Without JavaScript there is nothing
    // that needs one.
    0
}

unsafe extern "C" fn ffi_kill_timer(_this: *mut sys::FPDF_FORMFILLINFO, _timer_id: c_int) {}

unsafe extern "C" fn ffi_get_local_time(
    _this: *mut sys::FPDF_FORMFILLINFO,
) -> sys::FPDF_SYSTEMTIME {
    sys::FPDF_SYSTEMTIME::default()
}

unsafe extern "C" fn ffi_get_page(
    _this: *mut sys::FPDF_FORMFILLINFO,
    _document: sys::FPDF_DOCUMENT,
    _page_index: c_int,
) -> sys::FPDF_PAGE {
    // Null is a documented valid answer ("page not yet loaded").
    std::ptr::null_mut()
}

unsafe extern "C" fn ffi_get_current_page(
    _this: *mut sys::FPDF_FORMFILLINFO,
    _document: sys::FPDF_DOCUMENT,
) -> sys::FPDF_PAGE {
    std::ptr::null_mut()
}

unsafe extern "C" fn ffi_get_rotation(
    _this: *mut sys::FPDF_FORMFILLINFO,
    _page: sys::FPDF_PAGE,
) -> c_int {
    0
}

unsafe extern "C" fn ffi_execute_named_action(
    _this: *mut sys::FPDF_FORMFILLINFO,
    _named_action: sys::FPDF_BYTESTRING,
) {
}
