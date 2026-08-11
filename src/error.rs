//! Error types for the crate.

use std::fmt;
use std::path::PathBuf;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Failure to locate, open, or validate the PDFium shared library.
#[derive(Debug)]
#[non_exhaustive]
pub enum LoadError {
    /// No candidate library was found. Contains every location that was
    /// tried, in discovery order.
    LibraryNotFound {
        /// Every location tried, in discovery order.
        searched: Vec<String>,
    },
    /// A concrete library file was found (or explicitly given) but the
    /// dynamic loader failed to open it.
    OpenFailed {
        /// The library file that failed to open.
        path: PathBuf,
        /// The dynamic loader's error.
        source: libloading::Error,
    },
    /// The library opened, but a required PDFium symbol is missing — the
    /// build is older than the oldest version this crate supports, or the
    /// file is not PDFium at all.
    MissingSymbol(crate::sys::MissingSymbolError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::LibraryNotFound { searched } => {
                write!(
                    f,
                    "PDFium shared library not found; searched: {}. \
                     Run `cargo xtask fetch-pdfium`, set PDFIUM_LIB_PATH, or use \
                     Pdfium::load_from_path()",
                    searched.join(", ")
                )
            }
            LoadError::OpenFailed { path, source } => {
                write!(
                    f,
                    "failed to open PDFium library at {}: {source}",
                    path.display()
                )
            }
            LoadError::MissingSymbol(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::LibraryNotFound { .. } => None,
            LoadError::OpenFailed { source, .. } => Some(source),
            LoadError::MissingSymbol(e) => Some(e),
        }
    }
}

/// All errors returned by the safe API.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The PDFium library could not be loaded.
    Load(LoadError),
    /// PDFium is already loaded from a different library path; the process
    /// keeps the first successfully loaded library for its lifetime.
    AlreadyLoaded {
        /// Where the active instance was loaded from (`None` when it was
        /// resolved by bare name through the system loader).
        loaded_from: Option<PathBuf>,
        /// The conflicting path passed to this call.
        requested: PathBuf,
    },
    /// I/O failure while reading a PDF file from disk.
    Io(std::io::Error),
    /// The document is encrypted and requires a password, but none was
    /// supplied.
    PasswordRequired,
    /// A password was supplied but does not unlock the document.
    IncorrectPassword,
    /// The document uses a security/encryption scheme PDFium does not
    /// support.
    UnsupportedSecurity,
    /// The data is not a PDF, or is too corrupt to open.
    InvalidPdf,
    /// PDFium reported an error code this crate does not recognize.
    Pdfium {
        /// Raw `FPDF_GetLastError` value.
        code: u64,
    },
    /// Requested page index does not exist.
    PageIndexOutOfBounds {
        /// The requested 0-based page index.
        index: usize,
        /// The document's page count.
        count: usize,
    },
    /// PDFium failed to load a page that should exist (severely corrupt
    /// page tree or content).
    PageLoadFailed {
        /// The 0-based page index that failed to load.
        index: usize,
    },
    /// PDFium failed to prepare the page for text extraction.
    TextLoadFailed {
        /// The 0-based page index whose text failed to load.
        index: usize,
    },
    /// The page reports more text characters than the configured
    /// extraction limit (see [`PdfPage::text_with_limit`]); nothing was
    /// allocated.
    ///
    /// [`PdfPage::text_with_limit`]: crate::PdfPage::text_with_limit
    TextTooLarge {
        /// Characters PDFium reports on the page.
        chars: usize,
        /// The configured ceiling.
        limit: usize,
    },
    /// PDFium failed to initialize the form-fill environment.
    FormInitFailed,
    /// The rendered output would exceed [`RenderConfig::max_output_bytes`]
    /// (or the hard `i32` pixel-dimension limits of PDFium's bitmap API).
    ///
    /// [`RenderConfig::max_output_bytes`]: crate::RenderConfig::max_output_bytes
    RenderTooLarge {
        /// Bytes the requested output would need.
        required_bytes: u64,
        /// The configured ceiling.
        limit: u64,
    },
    /// Rendering failed inside PDFium (bitmap creation or coordinate
    /// transform rejected).
    RenderFailed {
        /// Which PDFium operation rejected the render.
        reason: &'static str,
    },
    /// An argument or configuration value is invalid (zero/non-finite
    /// scale, zero target dimensions, interior NUL byte in a password, ...).
    InvalidConfig(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Load(e) => write!(f, "{e}"),
            Error::AlreadyLoaded {
                loaded_from,
                requested,
            } => match loaded_from {
                Some(p) => write!(
                    f,
                    "PDFium is already loaded from {} (requested {}); a process keeps \
                     its first PDFium library for its lifetime",
                    p.display(),
                    requested.display()
                ),
                None => write!(
                    f,
                    "PDFium is already loaded via the system loader (requested {}); a \
                     process keeps its first PDFium library for its lifetime",
                    requested.display()
                ),
            },
            Error::Io(e) => write!(f, "I/O error reading PDF: {e}"),
            Error::PasswordRequired => {
                write!(f, "document is encrypted and requires a password")
            }
            Error::IncorrectPassword => write!(f, "incorrect password for encrypted document"),
            Error::UnsupportedSecurity => {
                write!(f, "document uses an unsupported security scheme")
            }
            Error::InvalidPdf => write!(f, "data is not a valid PDF document"),
            Error::Pdfium { code } => write!(f, "PDFium error code {code}"),
            Error::PageIndexOutOfBounds { index, count } => {
                write!(
                    f,
                    "page index {index} out of bounds (document has {count} pages)"
                )
            }
            Error::PageLoadFailed { index } => write!(f, "PDFium failed to load page {index}"),
            Error::TextLoadFailed { index } => {
                write!(f, "PDFium failed to load text for page {index}")
            }
            Error::TextTooLarge { chars, limit } => write!(
                f,
                "page reports {chars} text characters, exceeding the extraction \
                 limit of {limit}"
            ),
            Error::FormInitFailed => {
                write!(f, "PDFium failed to initialize the form-fill environment")
            }
            Error::RenderTooLarge {
                required_bytes,
                limit,
            } => write!(
                f,
                "rendered output would require {required_bytes} bytes, exceeding the \
                 configured limit of {limit} bytes"
            ),
            Error::RenderFailed { reason } => write!(f, "rendering failed: {reason}"),
            Error::InvalidConfig(msg) => write!(f, "invalid configuration: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Load(e) => Some(e),
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<LoadError> for Error {
    fn from(e: LoadError) -> Self {
        Error::Load(e)
    }
}

impl Error {
    /// True when the failure is specifically about encryption/passwords:
    /// [`Error::PasswordRequired`], [`Error::IncorrectPassword`], or
    /// [`Error::UnsupportedSecurity`].
    pub fn is_encryption_error(&self) -> bool {
        matches!(
            self,
            Error::PasswordRequired | Error::IncorrectPassword | Error::UnsupportedSecurity
        )
    }
}
