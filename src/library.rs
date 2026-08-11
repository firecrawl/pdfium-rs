//! Library loading, discovery, and the process-wide PDFium instance.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::error::{Error, LoadError, Result};
use crate::sys;

/// The name of the PDFium shared library on this platform
/// (`libpdfium.dylib`, `libpdfium.so`, or `pdfium.dll`).
pub fn platform_library_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else {
        "libpdfium.so"
    }
}

/// The platform slug used by `pdfium-binaries` release assets and by
/// `cargo xtask fetch-pdfium` (`mac-arm64`, `linux-x64`, `win-x64`, ...).
pub fn platform_slug() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("windows", "aarch64") => "win-arm64",
        ("windows", "x86_64") => "win-x64",
        _ => "unknown",
    }
}

/// Environment variable consulted first by [`Pdfium::load`]: a path to the
/// PDFium library file, or to a directory containing it.
pub const PDFIUM_LIB_PATH_ENV: &str = "PDFIUM_LIB_PATH";

struct LibraryInner {
    bindings: Box<sys::Bindings>,
    ffi_lock: Mutex<()>,
    loaded_from: Option<PathBuf>,
}

static INSTANCE: OnceLock<LibraryInner> = OnceLock::new();
/// Serializes load attempts so exactly one thread initializes PDFium.
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Handle to the process-wide PDFium library.
///
/// # Loading model
///
/// PDFium has process-global state, so this crate maintains **one instance
/// per process**: the first successful [`Pdfium::load`] /
/// [`Pdfium::load_from_path`] / [`Pdfium::load_from_directory`] call
/// initializes PDFium and every later call returns a handle to the same
/// instance ([`Pdfium::load_from_path`] with a *different* path returns
/// [`Error::AlreadyLoaded`] instead of silently using the wrong binary).
/// The library is never unloaded; see `docs/DESIGN.md` for why.
///
/// # Thread safety
///
/// `Pdfium` (and every handle derived from it) is `Send + Sync`. PDFium
/// itself is single-threaded, so all FFI calls are serialized through one
/// process-wide mutex — concurrent use is safe but not parallel. For
/// CPU-bound throughput, use multiple processes (PDFium upstream's own
/// recommendation).
#[derive(Clone, Copy)]
pub struct Pdfium {
    inner: &'static LibraryInner,
}

impl std::fmt::Debug for Pdfium {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pdfium")
            .field("loaded_from", &self.inner.loaded_from)
            .finish_non_exhaustive()
    }
}

impl Pdfium {
    /// Loads PDFium using the documented discovery chain.
    ///
    /// Candidates are tried in order; the first that exists wins:
    ///
    /// 1. The [`PDFIUM_LIB_PATH`](PDFIUM_LIB_PATH_ENV) environment variable
    ///    (library file, or directory containing
    ///    [`platform_library_name`]). If set but unloadable, this is a hard
    ///    error — no silent fallback.
    /// 2. The directory containing the current executable.
    /// 3. `./target/pdfium/<platform>/lib` then `.../bin` (the archives use
    ///    `lib` everywhere except Windows, which uses `bin`; both are
    ///    probed on every platform) — the layout produced by
    ///    `cargo xtask fetch-pdfium`.
    /// 4. The system loader's default search path, by bare library name.
    ///
    /// If PDFium is already loaded, returns the existing instance without
    /// consulting the chain.
    pub fn load() -> Result<Pdfium> {
        let _init = lock_ignore_poison(&INIT_LOCK);
        if let Some(inner) = INSTANCE.get() {
            return Ok(Pdfium { inner });
        }

        let mut searched: Vec<String> = Vec::new();

        // 1. Environment variable — explicit configuration never falls through.
        if let Some(raw) = std::env::var_os(PDFIUM_LIB_PATH_ENV) {
            let p = PathBuf::from(&raw);
            let file = if p.is_dir() {
                p.join(platform_library_name())
            } else {
                p
            };
            return init_from_file(&file);
        }

        // 2. Next to the current executable.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let candidate = dir.join(platform_library_name());
                searched.push(candidate.display().to_string());
                if candidate.is_file() {
                    return init_from_file(&candidate);
                }
            }
        }

        // 3. The `cargo xtask fetch-pdfium` layout under ./target.
        let slug = platform_slug();
        for sub in ["lib", "bin"] {
            let candidate = PathBuf::from("target")
                .join("pdfium")
                .join(slug)
                .join(sub)
                .join(platform_library_name());
            searched.push(candidate.display().to_string());
            if candidate.is_file() {
                return init_from_file(&candidate);
            }
        }

        // 4. System loader by bare name.
        searched.push(format!("<system loader: {}>", platform_library_name()));
        match unsafe { libloading::Library::new(platform_library_name()) } {
            Ok(lib) => init_from_library(lib, None),
            Err(_) => Err(Error::Load(LoadError::LibraryNotFound { searched })),
        }
    }

    /// Loads PDFium from an explicit library file path (the recommended
    /// production configuration).
    ///
    /// Returns [`Error::AlreadyLoaded`] if PDFium was already loaded from a
    /// different path in this process.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Pdfium> {
        let path = path.as_ref();
        let _init = lock_ignore_poison(&INIT_LOCK);
        if let Some(inner) = INSTANCE.get() {
            return if inner.loaded_from.as_deref() == Some(path) {
                Ok(Pdfium { inner })
            } else {
                Err(Error::AlreadyLoaded {
                    loaded_from: inner.loaded_from.clone(),
                    requested: path.to_path_buf(),
                })
            };
        }
        init_from_file(path)
    }

    /// Loads PDFium from `dir/`[`platform_library_name()`].
    pub fn load_from_directory(dir: impl AsRef<Path>) -> Result<Pdfium> {
        Self::load_from_path(dir.as_ref().join(platform_library_name()))
    }

    /// Returns the already-loaded instance, if any, without attempting a
    /// load.
    pub fn instance() -> Option<Pdfium> {
        INSTANCE.get().map(|inner| Pdfium { inner })
    }

    /// The path the active library was loaded from (`None` when it was
    /// resolved by bare name through the system loader).
    pub fn loaded_from(&self) -> Option<&Path> {
        self.inner.loaded_from.as_deref()
    }

    /// Acquires the process-wide FFI lock guarding all PDFium calls.
    ///
    /// Only needed when calling into [`crate::sys`] directly: hold the
    /// guard for the duration of every raw call sequence, and never call
    /// safe-API methods while holding it (they would deadlock re-acquiring
    /// the same lock).
    pub fn ffi_lock(&self) -> MutexGuard<'static, ()> {
        lock_ignore_poison(&self.inner.ffi_lock)
    }

    /// The raw bindings table, for use with [`Pdfium::ffi_lock`].
    ///
    /// # Safety
    ///
    /// See [`crate::sys::Bindings`]: all calls must be serialized via
    /// [`Pdfium::ffi_lock`], and PDFium's per-function preconditions apply.
    pub unsafe fn raw(&self) -> &sys::Bindings {
        &self.inner.bindings
    }

    /// Runs `f` with the bindings while holding the FFI lock.
    pub(crate) fn ffi<R>(&self, f: impl FnOnce(&sys::Bindings) -> R) -> R {
        let _guard = lock_ignore_poison(&self.inner.ffi_lock);
        f(&self.inner.bindings)
    }
}

/// A panic while holding the FFI lock leaves PDFium state consistent from
/// the C side (each call completed or never started), so we recover the
/// guard rather than propagate poisoning to unrelated threads.
fn lock_ignore_poison<'a, T>(m: &'a Mutex<T>) -> MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn init_from_file(path: &Path) -> Result<Pdfium> {
    if !path.is_file() {
        return Err(Error::Load(LoadError::LibraryNotFound {
            searched: vec![path.display().to_string()],
        }));
    }
    let lib = unsafe { libloading::Library::new(path) }.map_err(|source| {
        Error::Load(LoadError::OpenFailed {
            path: path.to_path_buf(),
            source,
        })
    })?;
    init_from_library(lib, Some(path.to_path_buf()))
}

/// Caller must hold `INIT_LOCK` and have verified `INSTANCE` is unset.
fn init_from_library(lib: libloading::Library, loaded_from: Option<PathBuf>) -> Result<Pdfium> {
    // SAFETY: the file was either explicitly configured or found via the
    // documented discovery chain; `load_from_library` validates it exports
    // the full PDFium symbol table before any call is made.
    let bindings = unsafe { sys::Bindings::load_from_library(lib) }
        .map_err(|e| Error::Load(LoadError::MissingSymbol(e)))?;

    let config = sys::FPDF_LIBRARY_CONFIG {
        version: 2,
        m_pUserFontPaths: std::ptr::null(),
        m_pIsolate: std::ptr::null_mut(),
        m_v8EmbedderSlot: 0,
        // Version >2 fields are zeroed and ignored (version = 2).
        m_pPlatform: std::ptr::null_mut(),
        m_RendererType: 0,
        m_FontLibraryType: 0,
        m_BrotliEnabled: 0,
    };
    // SAFETY: single-threaded here (INIT_LOCK held, instance unset), config
    // is a valid version-2 struct, and null font paths are documented as
    // "use the default paths".
    unsafe { bindings.FPDF_InitLibraryWithConfig(&config) };

    let inner = LibraryInner {
        bindings,
        ffi_lock: Mutex::new(()),
        loaded_from,
    };
    // Cannot race: INIT_LOCK is held. `set` only fails if already set,
    // which the callers ruled out.
    let _ = INSTANCE.set(inner);
    Ok(Pdfium {
        inner: INSTANCE.get().expect("instance just set"),
    })
}
