//! Handles encrypted, invalid, and unsupported documents the way a real
//! application would.
//!
//! Walks the bundled malformed and encrypted fixtures, matching every
//! `Error` variant explicitly: `PasswordRequired` triggers a retry with
//! the correct password (standing in for prompting the user),
//! `IncorrectPassword`, `UnsupportedSecurity`, and `InvalidPdf` are
//! reported for what they are, and `Error::is_encryption_error()` shows
//! how to branch on the whole encryption family at once. The example
//! finishes by opening `encrypted_aes256.pdf` with its documented
//! password and rendering it.
//!
//! Run with:
//!
//! ```text
//! cargo run --example error_handling
//! ```

use std::path::{Path, PathBuf};

use firecrawl_pdfium::{Error, Pdfium, RenderConfig};

/// The fixtures to probe, with the user password an application could
/// supply after prompting (documented in tests/fixtures/README.md).
const FIXTURES: &[(&str, Option<&str>)] = &[
    ("empty.pdf", None),
    ("header_only.pdf", None),
    ("truncated.pdf", None),
    ("garbage.bin", None),
    ("bad_xref.pdf", None),
    ("encrypted_rc4.pdf", Some("userpw")),
    ("encrypted_aes256.pdf", Some("userpw")),
    ("owner_only.pdf", None),
];

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let pdfium = Pdfium::load()?;

    for (name, password) in FIXTURES {
        println!("{name}:");
        let bytes =
            std::fs::read(fixture(name)).map_err(|e| format!("cannot read fixture {name}: {e}"))?;
        try_open(&pdfium, name, &bytes, *password);
        println!();
    }

    // A wrong password is a distinct condition from a missing one.
    let aes_bytes = std::fs::read(fixture("encrypted_aes256.pdf"))
        .map_err(|e| format!("cannot read fixture encrypted_aes256.pdf: {e}"))?;
    println!("encrypted_aes256.pdf with a deliberately wrong password:");
    match pdfium.load_document(aes_bytes.clone(), Some("definitely-wrong")) {
        Err(err) => {
            if matches!(err, Error::IncorrectPassword) {
                println!(
                    "  -> {err} (is_encryption_error: {})\n",
                    err.is_encryption_error()
                );
            } else {
                return Err(format!("expected IncorrectPassword, got: {err}").into());
            }
        }
        Ok(_) => return Err("expected IncorrectPassword, but the document opened".into()),
    }

    // Success path: the right password opens the document, and from there
    // it behaves like any other — inspect it and render a page.
    let doc = pdfium.load_document(aes_bytes, Some("userpw"))?;
    println!("encrypted_aes256.pdf opened with the documented password:");
    println!("  pages: {}", doc.page_count());
    match doc.security_handler_revision() {
        Some(rev) => println!("  security handler revision: {rev}"),
        None => println!("  security handler revision: none"),
    }
    let page = doc.page(0)?;
    let rendered = page.render(&RenderConfig::new().dpi(144.0))?;
    println!(
        "  rendered page 0: {} x {} px, {:?}, {} bytes",
        rendered.width(),
        rendered.height(),
        rendered.format(),
        rendered.pixels().len(),
    );
    Ok(())
}

/// Attempts to open `bytes` without a password and reacts to the outcome
/// the way an application would, matching every error variant explicitly.
fn try_open(pdfium: &Pdfium, name: &str, bytes: &[u8], known_password: Option<&str>) {
    match pdfium.load_document(bytes.to_vec(), None) {
        Ok(doc) => match doc.security_handler_revision() {
            // owner_only.pdf lands here: encrypted, but the user password
            // is empty, so it opens — with restricted permissions.
            Some(rev) => {
                let perms = doc.permissions();
                println!(
                    "  opened without a password despite encryption (revision {rev}); \
                     permissions: print={} modify={} copy={}",
                    perms.can_print(),
                    perms.can_modify(),
                    perms.can_copy(),
                );
            }
            None => println!(
                "  opened: {} page(s), not encrypted (PDFium repaired the file if needed)",
                doc.page_count()
            ),
        },
        Err(err) => {
            println!("  load failed: {err}");
            println!("  is_encryption_error: {}", err.is_encryption_error());
            match err {
                Error::PasswordRequired => match known_password {
                    // An application would prompt the user here; we use the
                    // fixture's documented password instead.
                    Some(pw) => match pdfium.load_document(bytes.to_vec(), Some(pw)) {
                        Ok(doc) => println!(
                            "  retried with the correct password: opened, {} page(s)",
                            doc.page_count()
                        ),
                        Err(retry) => println!("  retry with password failed: {retry}"),
                    },
                    None => println!("  no password known for {name}; giving up"),
                },
                Error::IncorrectPassword => {
                    println!("  the supplied password does not unlock this document");
                }
                Error::UnsupportedSecurity => {
                    println!("  encryption scheme not supported by this PDFium build");
                }
                Error::InvalidPdf => {
                    println!("  not a PDF, or too corrupt to open — nothing to retry");
                }
                Error::Pdfium { code } => {
                    println!("  PDFium reported an unclassified error code: {code}");
                }
                Error::Load(_) | Error::AlreadyLoaded { .. } => {
                    println!("  library-level failure (unexpected after a successful load)");
                }
                Error::Io(_) => {
                    println!("  I/O failure (unexpected: the bytes are already in memory)");
                }
                Error::PageIndexOutOfBounds { .. }
                | Error::PageLoadFailed { .. }
                | Error::TextLoadFailed { .. }
                | Error::FormInitFailed
                | Error::RenderTooLarge { .. }
                | Error::RenderFailed { .. }
                | Error::InvalidConfig(_) => {
                    println!("  not a document-load error (unexpected from load_document)");
                }
                // Error is #[non_exhaustive]: future crate versions may add
                // variants, so downstream matches always need this arm.
                other => println!("  unrecognized error variant: {other}"),
            }
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
