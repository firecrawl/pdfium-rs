//! Loads the PDFium shared library explicitly or through discovery.
//!
//! Demonstrates, in order:
//!
//! 1. `Pdfium::instance()` before any load — returns `None` because
//!    nothing has initialized the process-wide instance yet.
//! 2. Explicit loading with `Pdfium::load_from_path` /
//!    `Pdfium::load_from_directory` when a library path is supplied as the
//!    first argument or via the `PDFIUM_LIB_PATH` environment variable
//!    (an explicit path is the recommended production configuration).
//! 3. Otherwise `Pdfium::load()`, whose documented discovery chain tries
//!    `PDFIUM_LIB_PATH`, the executable's directory,
//!    `./target/pdfium/<platform>/lib`, and finally the system loader.
//!
//! On failure the error's `Display` output — which lists every searched
//! location — is printed to stderr and the process exits nonzero.
//!
//! Run with:
//!
//! ```text
//! cargo run --example load_pdfium [-- /path/to/libpdfium.dylib]
//! ```

use std::path::PathBuf;

use firecrawl_pdfium::{platform_library_name, platform_slug, Pdfium, PDFIUM_LIB_PATH_ENV};

fn main() {
    // Before anything loads PDFium, there is no process-wide instance.
    println!(
        "instance before load:  {}",
        if Pdfium::instance().is_some() {
            "Some (already loaded)"
        } else {
            "None"
        }
    );
    println!("platform library name: {}", platform_library_name());
    println!("platform slug:         {}", platform_slug());

    // An explicit path (argv[1], or $PDFIUM_LIB_PATH) skips discovery
    // entirely; with neither present, run the documented discovery chain.
    let explicit: Option<PathBuf> = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(PDFIUM_LIB_PATH_ENV).map(PathBuf::from));

    let result = match explicit {
        Some(path) => {
            println!("loading explicitly from {}", path.display());
            if path.is_dir() {
                // A directory: append the platform library name for us.
                Pdfium::load_from_directory(&path)
            } else {
                Pdfium::load_from_path(&path)
            }
        }
        None => {
            println!("no explicit path given; running discovery");
            Pdfium::load()
        }
    };

    let pdfium = match result {
        Ok(pdfium) => pdfium,
        Err(err) => {
            // For LibraryNotFound this Display output lists every location
            // the discovery chain searched, in order.
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    };

    match pdfium.loaded_from() {
        Some(path) => println!("loaded from:           {}", path.display()),
        None => println!("loaded from:           <system loader, by bare name>"),
    }
    println!(
        "instance after load:   {}",
        if Pdfium::instance().is_some() {
            "Some"
        } else {
            "None"
        }
    );
}
