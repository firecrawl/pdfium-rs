//! Renders an AcroForm document with and without the form-field layer.
//!
//! Form field *appearances* (the visible widgets: text boxes, checkboxes,
//! and their current values) are drawn by a separate PDFium subsystem
//! that must be switched on per document with
//! `PdfDocument::enable_form_rendering`. Whether a document has a form
//! worth enabling is reported by `PdfDocument::form_type`, and
//! `FormType::is_renderable()` is the gate: only AcroForm appearances can
//! be drawn (default PDFium builds cannot render XFA content).
//!
//! The example renders the same page twice — once with
//! `RenderConfig::form_fields(false)` and once with `(true)` — writes
//! both PNGs to `target/`, and reports how many pixels the form layer
//! changed.
//!
//! Run with:
//!
//! ```text
//! cargo run --example render_forms [-- /path/to/form.pdf]
//! ```
//!
//! Without an argument it renders the bundled `form_text_field.pdf`
//! fixture (a text field named `name` with value `initial`).

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use firecrawl_pdfium::{Pdfium, RenderConfig, RenderedPage};

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
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/form_text_field.pdf")
        });
    let pdfium = Pdfium::load()?;
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let doc = pdfium.load_document(bytes, None)?;

    // Form type detection is cheap and does not initialize anything.
    let form_type = doc.form_type();
    println!("{}", path.display());
    println!("form type: {form_type:?}");
    println!(
        "renderable by this crate (AcroForm only): {}",
        form_type.is_renderable()
    );

    // The is_renderable() gate: enabling form rendering only pays off for
    // AcroForm documents. Enable *before* opening pages — pages opened
    // after the call participate in form rendering.
    if form_type.is_renderable() {
        doc.enable_form_rendering()?;
        println!("form rendering enabled: {}", doc.forms_enabled());
    } else {
        println!("no renderable form layer; the two renders will be identical");
    }

    let page = doc.page(0)?;
    println!("page 0: {} x {} pt", page.width(), page.height());

    // Same geometry twice; only the form-field layer differs.
    let base = page.render(&RenderConfig::new().scale(2.0).form_fields(false))?;
    let with_forms = page.render(&RenderConfig::new().scale(2.0).form_fields(true))?;

    // Both renders are Bgra8 (4 bytes/pixel), so compare pixel by pixel.
    let differing = base
        .pixels()
        .chunks_exact(4)
        .zip(with_forms.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    let total = base.width() as usize * base.height() as usize;
    println!("pixels changed by the form layer: {differing} of {total}");

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let base_path = out_dir.join("form_base.png");
    let forms_path = out_dir.join("form_fields.png");
    write_png(&base_path, &base)?;
    write_png(&forms_path, &with_forms)?;
    println!("wrote {}", base_path.display());
    println!("wrote {}", forms_path.display());
    Ok(())
}

/// Encodes a render to an RGBA PNG file.
fn write_png(path: &Path, rendered: &RenderedPage) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let file = File::create(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), rendered.width(), rendered.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&rendered.to_rgba8())?;
    writer.finish()?;
    Ok(())
}
