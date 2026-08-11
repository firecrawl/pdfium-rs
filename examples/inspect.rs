//! Opens an in-memory PDF and inspects its pages and dimensions.
//!
//! The whole file is read into a `Vec<u8>` first and ownership of the
//! bytes is handed to `Pdfium::load_document` — the crate's primary
//! loading path (the document keeps the buffer alive for its whole
//! lifetime). The example then prints document-level facts (page count,
//! PDF version, form type, permissions, security handler revision, and
//! whichever metadata fields exist) followed by a per-page table. Page
//! dimensions come from `PdfDocument::page_size`, which does *not* load
//! pages — cheap even for huge documents — while the `/Rotate` entry
//! requires actually opening each page with `PdfDocument::page`.
//!
//! Run with:
//!
//! ```text
//! cargo run --example inspect [-- /path/to/document.pdf]
//! ```
//!
//! Without an argument it inspects the bundled `mixed_pages.pdf` fixture
//! (A4 portrait, rotated US letter, and a small 200x100 pt page).

use std::path::{Path, PathBuf};

use firecrawl_pdfium::{MetadataTag, Pdfium};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed_pages.pdf")
        });

    let pdfium = Pdfium::load()?;

    // Read the file ourselves and load from the in-memory bytes.
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    println!("{} ({} bytes)", path.display(), bytes.len());
    let doc = pdfium.load_document(bytes, None)?;

    println!("pages:            {}", doc.page_count());
    match doc.pdf_version() {
        Some(v) => println!("pdf version:      {}.{}", v / 10, v % 10),
        None => println!("pdf version:      (unavailable)"),
    }
    println!("form type:        {:?}", doc.form_type());

    let perms = doc.permissions();
    println!(
        "permissions:      print={} modify={} copy={} annotate={}",
        perms.can_print(),
        perms.can_modify(),
        perms.can_copy(),
        perms.can_annotate(),
    );
    match doc.security_handler_revision() {
        Some(rev) => println!("security handler: revision {rev}"),
        None => println!("security handler: none (document is not encrypted)"),
    }

    // Print only the metadata fields the document actually defines.
    let tags = [
        MetadataTag::Title,
        MetadataTag::Author,
        MetadataTag::Subject,
        MetadataTag::Keywords,
        MetadataTag::Creator,
        MetadataTag::Producer,
        MetadataTag::CreationDate,
        MetadataTag::ModDate,
    ];
    let mut any_metadata = false;
    for tag in tags {
        if let Some(value) = doc.metadata(tag) {
            println!("metadata:         {tag:?} = {value}");
            any_metadata = true;
        }
    }
    if !any_metadata {
        println!("metadata:         (none present)");
    }

    // Per-page survey: sizes without page loads, then a real page load for
    // the rotation, plus the page label when the document defines one.
    println!();
    println!(
        "{:>4}  {:>18}  {:<12}  label",
        "page", "size (pt)", "rotation"
    );
    for index in 0..doc.page_count() {
        let size = doc.page_size(index)?;
        let page = doc.page(index)?;
        let label = doc.page_label(index).unwrap_or_else(|| "-".into());
        let rotation = format!("{:?}", page.rotation());
        println!(
            "{index:>4}  {:>8.2} x {:>7.2}  {rotation:<12}  {label}",
            size.width, size.height,
        );
    }
    Ok(())
}
