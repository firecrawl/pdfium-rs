# firecrawl-pdfium

Safe, self-contained Rust bindings for [PDFium], Google's PDF engine: open
documents from memory, render pages to **owned** pixel buffers, map
coordinates between rendered pixels and PDF page space, extract positioned
text, and draw form fields — safely usable from concurrent code.

[![CI](https://github.com/firecrawl/pdfium-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/firecrawl/pdfium-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/firecrawl-pdfium.svg)](https://crates.io/crates/firecrawl-pdfium)
[![docs.rs](https://img.shields.io/docsrs/firecrawl-pdfium)](https://docs.rs/firecrawl-pdfium)
[![MSRV](https://img.shields.io/badge/MSRV-1.77-blue.svg)](docs/VERSIONING.md#msrv-policy)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

[PDFium]: https://pdfium.googlesource.com/pdfium/

## Why this crate

- **A small, curated, rendering-focused API.** The surface is the set of
  operations document pipelines actually need — load, inspect, render,
  extract, fail cleanly on hostile input — wrapped completely and tested
  hard, rather than a binding for every PDFium function. The raw function
  table stays reachable through the public `sys` module for anything the
  safe API does not cover yet.
- **Thread safety by serialization, not by assertion.** PDFium is not
  thread-safe, so every FFI call goes through one process-wide mutex; there
  is no unlocked path. All handle types are `Send + Sync` because of that
  discipline, with the invariants written down next to each `unsafe impl`
  (see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)).
- **Owned results and first-class coordinate transforms.** A rendered page
  is plain data — pixels, stride, format, and the pixel-to-page transform —
  holding no PDFium resources. OCR-style consumers can map pixel boxes back
  to PDF page space long after the page and document are gone.
- **Batteries-included binary acquisition.** The repository pins a specific
  upstream PDFium release with sha256 checksums, fetches and verifies it
  with one command, and republishes verified native archives with
  checksums, license texts, provenance metadata, and GitHub build
  attestations.

**When to use [pdfium-render] instead:** pdfium-render is an actively
maintained, much larger binding (MIT/Apache-2.0) that also covers PDF
*editing* — creating and modifying pages, objects, annotations, form field
values, signatures, and more. If you need to change PDFs rather than read
and rasterize them, use pdfium-render; it is good prior art and we link it
with respect. This crate is for teams that want a smaller audited surface,
strictly serialized FFI, and a verified binary supply chain for
read/render/extract workloads.

[pdfium-render]: https://crates.io/crates/pdfium-render

## Install

```sh
cargo add firecrawl-pdfium
```

The crate builds everywhere with plain `cargo build` — it contains no build
script and never links PDFium at build time. PDFium is loaded **at
runtime** from a shared library (`libpdfium.dylib` / `libpdfium.so` /
`pdfium.dll`).

### Getting a PDFium binary

| Route | Best for | Notes |
|---|---|---|
| `cargo xtask fetch-pdfium` | working in this repository | Downloads the release pinned in `pdfium.lock.json`, verifies its sha256, and extracts to `target/pdfium/<platform>/` where `Pdfium::load()` finds it automatically. |
| `native-v*` releases on [firecrawl/pdfium-rs](https://github.com/firecrawl/pdfium-rs/releases) | production deployments | The pinned upstream binaries, repackaged with upstream `LICENSE` and `licenses/` kept verbatim, plus `PROVENANCE.json`, `SHA256SUMS`, an SBOM, and GitHub build-provenance attestations. Verify with the commands below. |
| [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) directly | pinning your own PDFium version | The upstream source of our binaries (weekly builds). Upstream publishes no checksum files, so record your own at download time. |
| System / distro package | base images that already ship PDFium | Install `libpdfium` and rely on the system loader, or point `PDFIUM_LIB_PATH` at it. |

Verifying a `native-v*` release asset:

```sh
shasum -a 256 --check SHA256SUMS --ignore-missing
gh attestation verify firecrawl-pdfium-linux-x64.tgz --repo firecrawl/pdfium-rs
```

### How the library is found

`Pdfium::load()` tries, in order — first hit wins:

1. The `PDFIUM_LIB_PATH` environment variable: a path to the library file,
   or to a directory containing the platform library name. If set but
   unloadable this is a hard error; it never silently falls through.
2. The directory containing the current executable.
3. `./target/pdfium/<platform>/lib` then `.../bin` (the archives use `lib`
   everywhere except Windows, which uses `bin`; both are probed on every
   platform) — the layout produced by `cargo xtask fetch-pdfium`.
4. The system loader's default search path, by bare library name.

For production, skip discovery entirely with `Pdfium::load_from_path(...)`
or `Pdfium::load_from_directory(...)` and an absolute path.

## Quickstart

```rust
use firecrawl_pdfium::{Pdfium, RenderConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Discovery chain: $PDFIUM_LIB_PATH -> exe dir -> ./target/pdfium -> system.
    let pdfium = Pdfium::load()?;

    let bytes = std::fs::read("document.pdf")?;
    let doc = pdfium.load_document(bytes, None)?; // None = no password
    println!("{} pages", doc.page_count());

    let page = doc.page(0)?;
    let rendered = page.render(&RenderConfig::new().dpi(144.0))?;
    println!(
        "{}x{} pixels, {} bytes/row, {:?}",
        rendered.width(),
        rendered.height(),
        rendered.stride(),
        rendered.format(),
    );

    // Map a pixel back into PDF page space (points, origin bottom-left):
    let pt = rendered.transform().pixel_to_page((10.0, 10.0).into());
    println!("pixel (10,10) is at ({:.2}, {:.2})pt", pt.x, pt.y);
    Ok(())
}
```

Encrypted documents fail with typed errors (`Error::PasswordRequired`,
`Error::IncorrectPassword`, `Error::UnsupportedSecurity`); garbage and
truncated input with `Error::InvalidPdf`; oversized render requests with
`Error::RenderTooLarge`, bounded by `RenderConfig::max_output_bytes`
(default 1 GiB) *before* any allocation happens. Text extraction is
bounded the same way: pages claiming more than one million characters
fail with `Error::TextTooLarge` before allocating (tunable via
`PdfPage::text_with_limit`). One inherent caveat: opening pages and text
runs PDFium's parser over attacker-controlled content, and PDFium's own
CPU/memory use during parsing cannot be capped from the embedding API —
for hard isolation of hostile input, run extraction in a separate process
(the same recommendation PDFium's other embedders follow).

## Coordinate mapping

Two spaces appear throughout: **page space** (PDF points, origin
bottom-left, y-up) and **pixel space** (rendered bitmap, origin top-left,
y-down). Every render carries the exact affine transform between them,
derived from PDFium's own device-to-page mapping at render time, so
`/Rotate` pages and extra render rotations behave exactly as PDFium
rendered them.

The OCR round trip — render a page, run OCR on the bitmap, map the
resulting pixel boxes back into PDF page space — is one call:

```rust
use firecrawl_pdfium::{PageRect, PixelRect, RenderedPage};

/// Maps an OCR word box (bitmap pixels, origin top-left, y-down) into
/// PDF page space (points, origin bottom-left, y-up).
fn word_box_to_page(rendered: &RenderedPage, word: PixelRect) -> PageRect {
    rendered.transform().pixel_rect_to_page(word)
}
```

The transform is plain data on the owned render result: it stays valid
after the page, the document, and even the library handle are gone.
`PdfPage::transform_for(&config)` computes the same transform without
rendering, and `PageTransform::page_rect_to_pixel` maps the other way
(for example, to highlight extracted text on the bitmap).

## Concurrency

PDFium is single-threaded; this crate makes it **safe**, not parallel.
Every FFI call is serialized through one process-wide mutex, which is what
makes `Pdfium`, `PdfDocument`, and `PdfPage` sound as `Send + Sync` — use
them from any thread, but expect calls to queue. Owned results
(`RenderedPage`, `PageText`, `PageTransform`) involve no lock at all.

For CPU-bound throughput, shard documents across **processes** — PDFium
upstream's own recommendation. One process per core with a work queue
saturates hardware without any unsound shortcuts.

## Platform support

| Platform | Status |
|---|---|
| macOS arm64 (`mac-arm64`) | Tier 1 — CI-tested |
| macOS x64 (`mac-x64`) | Tier 1 — CI-tested |
| Linux x64, glibc (`linux-x64`) | Tier 1 — CI-tested |
| Linux arm64, glibc (`linux-arm64`) | Tier 1 — CI-tested |
| Windows x64 (`win-x64`) | Tier 1 — CI-tested |
| Windows arm64, Linux musl, other upstream builds | Expected to work, not CI-tested |
| WebAssembly, Android, iOS | Not yet supported |

Static linking is not yet available (see below); the crate always loads a
shared library at runtime.

## Feature status and limitations

Implemented and tested: rendering (scale / DPI / fixed-dimension / fit
sizing; BGRA, RGBA, BGR, grayscale outputs; background colors; extra
rotation; anti-aliasing toggles; output-size limits), text extraction with
per-character geometry, AcroForm field *display*, pixel/page coordinate
transforms, document metadata, permissions, page labels, and typed errors
for encrypted/malformed input.

Deliberately deferred (see the [design document](docs/DESIGN.md) for the
list and rationale): PDF editing or saving, XFA forms, JavaScript, form
field *values* and programmatic filling, text search, annotations API,
outlines/attachments, progressive rendering, and static linking. PDFium's
default builds disable V8 and XFA, which matches this crate's scope.

## Versioning and PDFium compatibility

See [docs/VERSIONING.md](docs/VERSIONING.md) for the full policy and the
per-release compatibility table. The short version: this crate follows
SemVer (pre-1.0: breaking changes bump the minor version) and currently
pins upstream release `chromium/7988` (PDFium 153.0.7988.0) for its tested
binaries. At runtime, **any** PDFium build that exports the symbols this
crate binds will work; a library missing one fails fast at
`Pdfium::load()` with `LoadError::MissingSymbol` naming the symbol — never
with undefined behavior at call time.

## Minimum supported Rust version

Rust **1.77**. MSRV bumps are minor version changes and the MSRV stays at
least six months behind current stable; see
[docs/VERSIONING.md](docs/VERSIONING.md).

## License

The crate is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. The crates.io package is pure Rust and ships no binaries.

The PDFium binaries distributed through this repository's `native-v*`
releases bundle PDFium and its statically linked third-party components,
all under permissive licenses — see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) for the complete list.
Those binaries include FreeType, whose license requires this credit:

> Portions of this software are copyright © The FreeType Project
> (www.freetype.org). All rights reserved.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

## Acknowledgements

- The [PDFium](https://pdfium.googlesource.com/pdfium/) project and the
  Chromium team, for the engine itself.
- [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries),
  whose weekly builds this crate pins and redistributes.
- [pdfium-render](https://crates.io/crates/pdfium-render), whose years of
  prior art on binding PDFium from Rust informed this design.
