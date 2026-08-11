//! Raw FFI layer: PDFium types, constants, and the dynamically loaded
//! function table.
//!
//! This module is public so applications with needs beyond the safe API can
//! drop down to raw PDFium calls, but it is a sharp tool:
//!
//! - Everything here is `unsafe` and carries PDFium's own preconditions.
//! - **PDFium is not thread-safe.** All calls through [`Bindings`] —
//!   including calls made by the safe layer — must be serialized
//!   process-wide. When mixing raw calls with the safe API, hold the lock
//!   returned by [`crate::Pdfium::ffi_lock`] for the duration of every raw
//!   call sequence.
//! - The struct and constant definitions are transcribed from the PDFium
//!   headers shipped with `bblanchon/pdfium-binaries` (currently pinned to
//!   `chromium/7988`, PDFium 153.0.7988.0) and are ABI contracts; see
//!   `docs/DESIGN.md` for the verification methodology.
//!
//! Naming intentionally mirrors the C API (`FPDF_*`) rather than Rust
//! conventions, so PDFium's documentation applies verbatim.

// The types module intentionally mirrors the C API: struct members carry
// the header comments where they matter, not one-liner docs per callback.
#[allow(missing_docs)]
mod bindings;
#[allow(missing_docs)]
mod types;

pub use bindings::{Bindings, MissingSymbolError};
pub use types::*;
