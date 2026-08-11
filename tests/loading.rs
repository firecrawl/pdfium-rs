//! Library loading and singleton semantics.
//!
//! This suite MUST be its own test binary: PDFium is a process-wide
//! singleton, so the load-failure cases below only work while nothing has
//! successfully loaded PDFium in this process yet. Rust runs `#[test]`s of
//! one binary on parallel threads, so every ordering-sensitive step lives
//! in this single test function.

mod common;

use firecrawl_pdfium::{platform_library_name, platform_slug, Error, LoadError, Pdfium};
use std::path::PathBuf;

/// Well-known shared libraries that exist on disk and are definitely not
/// PDFium. On modern macOS the classic `/usr/lib` dylibs live only in the
/// dyld shared cache (not as files), so the list may come up empty there;
/// the sysroot scan below is the reliable fallback.
#[cfg(target_os = "macos")]
fn non_pdfium_library_candidates() -> Vec<PathBuf> {
    vec![PathBuf::from("/usr/lib/libSystem.B.dylib")]
}

#[cfg(target_os = "linux")]
fn non_pdfium_library_candidates() -> Vec<PathBuf> {
    [
        "/lib/x86_64-linux-gnu/libc.so.6",
        "/usr/lib/x86_64-linux-gnu/libc.so.6",
        "/lib/aarch64-linux-gnu/libc.so.6",
        "/usr/lib/aarch64-linux-gnu/libc.so.6",
        "/lib64/libc.so.6",
        "/usr/lib/libc.so.6",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(target_os = "windows")]
fn non_pdfium_library_candidates() -> Vec<PathBuf> {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    vec![root.join("System32").join("kernel32.dll")]
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn non_pdfium_library_candidates() -> Vec<PathBuf> {
    Vec::new()
}

/// Shared libraries shipped inside the Rust toolchain's sysroot. These are
/// always real files on disk on every platform Rust supports, which makes
/// them dependable "openable but not PDFium" inputs.
fn sysroot_library_candidates() -> Vec<PathBuf> {
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        "so"
    };
    let Ok(output) = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let sysroot = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let mut found = Vec::new();
    // Windows keeps its runtime DLLs in bin/, Unix in lib/.
    for dir in ["lib", "bin"] {
        if let Ok(entries) = std::fs::read_dir(sysroot.join(dir)) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == ext) && path.is_file() {
                    found.push(path);
                }
            }
        }
    }
    // Prefer small runtime shims over the ~100 MB rustc_driver library:
    // their dependency closures are tiny, so dlopen reliably succeeds.
    found.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
    found
}

/// Steps 1-2 must run before anything loads PDFium successfully; steps 3+
/// exercise the singleton once it is installed. One `#[test]`, so the order
/// is guaranteed.
#[test]
fn load_failure_then_singleton_lifecycle() {
    // Step 0: nothing has loaded PDFium in this process yet.
    assert!(
        Pdfium::instance().is_none(),
        "test-binary invariant violated: PDFium already loaded before the loading tests ran"
    );

    // Step 1: a nonexistent path is LibraryNotFound, and reports what it
    // searched.
    let missing = PathBuf::from("target/definitely/not/here").join(platform_library_name());
    match Pdfium::load_from_path(&missing) {
        Err(Error::Load(LoadError::LibraryNotFound { searched })) => {
            assert_eq!(searched, vec![missing.display().to_string()]);
        }
        other => panic!("expected Load(LibraryNotFound), got {other:?}"),
    }

    // Step 2: a real, openable shared library that is not PDFium fails the
    // eager symbol check with MissingSymbol (the documented binary
    // compatibility gate). Resilient: if no candidate exists on disk or
    // none can be dlopen'ed, skip with a note rather than fail.
    let mut candidates = non_pdfium_library_candidates();
    candidates.retain(|p| p.is_file());
    candidates.extend(sysroot_library_candidates());
    let mut verified_missing_symbol = false;
    for candidate in &candidates {
        match Pdfium::load_from_path(candidate) {
            Err(Error::Load(LoadError::MissingSymbol(e))) => {
                // The error message names the first symbol that was absent.
                assert!(!e.to_string().is_empty());
                verified_missing_symbol = true;
                break;
            }
            // Not dlopen'able here (missing deps, wrong arch, hardened
            // runtime, ...) — try the next candidate.
            Err(Error::Load(LoadError::OpenFailed { .. })) => continue,
            // Deleted between the is_file() check and the open.
            Err(Error::Load(LoadError::LibraryNotFound { .. })) => continue,
            other => panic!(
                "loading non-PDFium library {} should fail with MissingSymbol/OpenFailed, got {other:?}",
                candidate.display()
            ),
        }
    }
    if !verified_missing_symbol {
        eprintln!(
            "skipping MissingSymbol check: no openable non-PDFium shared library found \
             among {} candidates",
            candidates.len()
        );
    }

    // Step 3: the failures above must not have installed an instance.
    assert!(
        Pdfium::instance().is_none(),
        "failed loads must not install the singleton"
    );

    // Step 4: the discovery chain finds the fetched binary and loads it.
    let pdfium = common::pdfium();

    // Step 5: the instance is now visible process-wide.
    assert!(
        Pdfium::instance().is_some(),
        "instance() must be Some after a successful load"
    );

    // Step 6: loading a *different* path — even a byte-identical copy of
    // the same library — is refused with AlreadyLoaded, because the process
    // keeps its first PDFium for its lifetime.
    let copy_dir = PathBuf::from("target").join("test-loading-tmp");
    let copy_path = copy_dir.join(platform_library_name());
    let source = PathBuf::from("target")
        .join("pdfium")
        .join(platform_slug())
        .join("lib")
        .join(platform_library_name());
    let source = if source.is_file() {
        Some(source)
    } else {
        // Loaded from somewhere else (e.g. PDFIUM_LIB_PATH); copy that.
        pdfium.loaded_from().map(PathBuf::from)
    };
    let different_path = match source {
        Some(src) => {
            std::fs::create_dir_all(&copy_dir).expect("create tmp dir under target/");
            std::fs::copy(&src, &copy_path).expect("copy the PDFium library");
            copy_path.clone()
        }
        // System-loader load with no known file: any distinct path proves
        // the same contract (the path comparison happens before any I/O).
        None => PathBuf::from("target/test-loading-tmp/nonexistent-different-path"),
    };
    match Pdfium::load_from_path(&different_path) {
        Err(Error::AlreadyLoaded {
            loaded_from,
            requested,
        }) => {
            // `requested` is canonicalized (absolute) when the file exists,
            // so different spellings of the same file are recognized;
            // compare canonical forms.
            let expected = different_path
                .canonicalize()
                .unwrap_or_else(|_| different_path.clone());
            assert_eq!(requested, expected);
            assert_eq!(loaded_from.as_deref(), pdfium.loaded_from());
        }
        other => panic!("expected AlreadyLoaded for a different path, got {other:?}"),
    }

    // Step 7: re-loading the *same* path is idempotent and returns the
    // existing instance.
    if let Some(active) = pdfium.loaded_from() {
        let again = Pdfium::load_from_path(active).expect("same-path load_from_path must succeed");
        assert_eq!(again.loaded_from(), pdfium.loaded_from());
    }

    // Step 8: plain load() keeps returning Ok forever after.
    let again = Pdfium::load().expect("Pdfium::load() must succeed once loaded");
    assert_eq!(again.loaded_from(), pdfium.loaded_from());

    // The handle is actually usable: open a document through it.
    let doc = again
        .load_document(common::fixture("valid.pdf"), None)
        .expect("document loads through the re-obtained handle");
    assert_eq!(doc.page_count(), 1);

    // Cleanup (best-effort; the copy lives under target/ anyway).
    let _ = std::fs::remove_file(&copy_path);
    let _ = std::fs::remove_dir(&copy_dir);
}
