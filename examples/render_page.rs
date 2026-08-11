//! Renders a selected page at a configurable resolution and shows how to
//! access the owned pixel data: dimensions, stride, pixel format, and the
//! raw bytes.
//!
//! The render result (`RenderedPage`) is plain data — it holds no PDFium
//! resources, so it can be encoded, sent across threads, or kept after the
//! document is closed. This example encodes it to a PNG with the `png`
//! crate: `Gray8` renders are written as grayscale directly, everything
//! else goes through `RenderedPage::to_rgba8`.
//!
//! Run with:
//!
//! ```text
//! cargo run --example render_page -- [pdf-path] [--page N] \
//!     [--dpi D | --width W] [--format bgra8|rgba8|bgr8|gray8] [--out file.png]
//! ```
//!
//! Without arguments it renders page 0 of the bundled `hello_text.pdf`
//! fixture at 144 DPI into `target/render_page.png`. When both `--dpi`
//! and `--width` are given, the one appearing last wins.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use firecrawl_pdfium::{Pdfium, PixelFormat, RenderConfig, RenderedPage};

/// How the output resolution was requested on the command line.
enum SizeArg {
    /// Dots per inch (72 DPI is 1:1 with page points).
    Dpi(f32),
    /// Exact output width in pixels; height keeps the aspect ratio.
    Width(u32),
}

struct Args {
    pdf: PathBuf,
    page: usize,
    size: SizeArg,
    format: PixelFormat,
    out: PathBuf,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let pdfium = Pdfium::load()?;

    let bytes =
        std::fs::read(&args.pdf).map_err(|e| format!("cannot read {}: {e}", args.pdf.display()))?;
    let doc = pdfium.load_document(bytes, None)?;
    println!("{}: {} page(s)", args.pdf.display(), doc.page_count());

    let page = doc.page(args.page)?;
    println!(
        "page {}: {} x {} pt, rotation {:?}",
        args.page,
        page.width(),
        page.height(),
        page.rotation(),
    );

    let config = match args.size {
        SizeArg::Dpi(dpi) => RenderConfig::new().dpi(dpi),
        SizeArg::Width(px) => RenderConfig::new().width(px),
    }
    .pixel_format(args.format);

    let rendered = page.render(&config)?;
    println!("rendered: {} x {} px", rendered.width(), rendered.height());
    println!("stride:   {} bytes/row", rendered.stride());
    println!(
        "format:   {:?} ({} bytes/pixel, alpha: {})",
        rendered.format(),
        rendered.format().bytes_per_pixel(),
        rendered.format().has_alpha(),
    );
    println!(
        "buffer:   {} bytes, owned by the result",
        rendered.pixels().len()
    );
    println!("first pixel bytes: {:?}", rendered.pixel(0, 0));

    write_png(&args.out, &rendered)?;
    println!("wrote {}", args.out.display());
    Ok(())
}

fn parse_args() -> Result<Args, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut args = Args {
        pdf: manifest.join("tests/fixtures/hello_text.pdf"),
        page: 0,
        size: SizeArg::Dpi(144.0),
        format: PixelFormat::Bgra8,
        out: manifest.join("target/render_page.png"),
    };

    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--page" => {
                args.page = value(&mut argv, "--page")?
                    .parse()
                    .map_err(|e| format!("--page: {e}"))?;
            }
            "--dpi" => {
                args.size = SizeArg::Dpi(
                    value(&mut argv, "--dpi")?
                        .parse()
                        .map_err(|e| format!("--dpi: {e}"))?,
                );
            }
            "--width" => {
                args.size = SizeArg::Width(
                    value(&mut argv, "--width")?
                        .parse()
                        .map_err(|e| format!("--width: {e}"))?,
                );
            }
            "--format" => {
                args.format = match value(&mut argv, "--format")?.as_str() {
                    "bgra8" => PixelFormat::Bgra8,
                    "rgba8" => PixelFormat::Rgba8,
                    "bgr8" => PixelFormat::Bgr8,
                    "gray8" => PixelFormat::Gray8,
                    other => {
                        return Err(format!(
                            "--format: unknown format {other:?} (expected bgra8|rgba8|bgr8|gray8)"
                        ))
                    }
                };
            }
            "--out" => args.out = PathBuf::from(value(&mut argv, "--out")?),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            path => args.pdf = PathBuf::from(path),
        }
    }
    Ok(args)
}

fn value(argv: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    argv.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

/// Encodes a render to a PNG file: `Gray8` directly as grayscale,
/// everything else converted to RGBA via `to_rgba8`.
fn write_png(path: &Path, rendered: &RenderedPage) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    let file = File::create(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), rendered.width(), rendered.height());
    encoder.set_depth(png::BitDepth::Eight);

    let data = match rendered.format() {
        PixelFormat::Gray8 => {
            encoder.set_color(png::ColorType::Grayscale);
            // Copy row by row: consumers should honor stride() rather than
            // assume the buffer is dense.
            let mut data =
                Vec::with_capacity(rendered.width() as usize * rendered.height() as usize);
            for y in 0..rendered.height() {
                data.extend_from_slice(rendered.row(y));
            }
            data
        }
        _ => {
            encoder.set_color(png::ColorType::Rgba);
            rendered.to_rgba8()
        }
    };

    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(())
}
