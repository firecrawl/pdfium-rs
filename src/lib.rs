//! Safe, self-contained Rust bindings for [PDFium], Google's PDF engine:
//! open documents from memory, inspect pages, render to **owned** pixel
//! buffers, map coordinates between rendered pixels and PDF page space,
//! extract positioned text, and draw form fields — safely usable from
//! concurrent code.
//!
//! [PDFium]: https://pdfium.googlesource.com/pdfium/
//!
//! # Quickstart
//!
//! PDFium is loaded **at runtime** (no build-time linking): fetch a binary
//! once with `cargo xtask fetch-pdfium` (repo checkouts), ship one next to
//! your executable, or point `PDFIUM_LIB_PATH` at one. Then:
//!
//! ```no_run
//! use firecrawl_pdfium::{Pdfium, RenderConfig};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Discovery chain: $PDFIUM_LIB_PATH → exe dir → ./target/pdfium → system.
//! let pdfium = Pdfium::load()?;
//!
//! let bytes = std::fs::read("document.pdf")?;
//! let doc = pdfium.load_document(bytes, None)?; // None = no password
//! println!("{} pages", doc.page_count());
//!
//! let page = doc.page(0)?;
//! let rendered = page.render(&RenderConfig::new().dpi(144.0))?;
//! println!(
//!     "{}x{} pixels, {} bytes/row, {:?}",
//!     rendered.width(),
//!     rendered.height(),
//!     rendered.stride(),
//!     rendered.format(),
//! );
//!
//! // Map a pixel back into PDF page space (points, origin bottom-left):
//! let pt = rendered.transform().pixel_to_page((10.0, 10.0).into());
//! println!("pixel (10,10) is at ({:.2}, {:.2})pt", pt.x, pt.y);
//! # Ok(())
//! # }
//! ```
//!
//! # Errors on hostile input
//!
//! Encrypted documents surface as [`Error::PasswordRequired`] /
//! [`Error::IncorrectPassword`] / [`Error::UnsupportedSecurity`]; garbage
//! and truncated files as [`Error::InvalidPdf`]; oversized render requests
//! as [`Error::RenderTooLarge`] (bounded by
//! [`RenderConfig::max_output_bytes`]).
//!
//! # Concurrency model
//!
//! PDFium itself is single-threaded. Every call is serialized through one
//! process-wide mutex, making all handle types `Send + Sync` — safe from
//! any thread, but not parallel. See [`Pdfium`] for details and
//! `docs/DESIGN.md` for the soundness argument. For parallel throughput,
//! shard across processes.
//!
//! # Raw FFI escape hatch
//!
//! The [`sys`] module exposes the loaded function table for calls this
//! crate does not wrap yet; combine with [`Pdfium::ffi_lock`] to stay
//! within the serialization contract.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod coords;
mod document;
mod error;
mod forms;
mod library;
mod page;
mod render;
mod text;

pub mod sys;

pub use coords::{PagePoint, PageRect, PageTransform, PixelPoint, PixelRect};
pub use document::{MetadataTag, PdfDocument, Permissions};
pub use error::{Error, LoadError, Result};
pub use forms::FormType;
pub use library::{platform_library_name, platform_slug, Pdfium, PDFIUM_LIB_PATH_ENV};
pub use page::{PageSize, PdfPage, Rotation};
pub use render::{Color, PixelFormat, RenderConfig, RenderedPage};
pub use text::{PageChar, PageText};

impl From<(f64, f64)> for PixelPoint {
    fn from((x, y): (f64, f64)) -> Self {
        PixelPoint::new(x, y)
    }
}

impl From<(f64, f64)> for PagePoint {
    fn from((x, y): (f64, f64)) -> Self {
        PagePoint::new(x, y)
    }
}
