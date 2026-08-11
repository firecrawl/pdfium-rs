# Third-party notices

This project distributes third-party software in two distinct channels,
with different contents:

- **The `firecrawl-pdfium` crate on crates.io** is pure Rust, licensed
  MIT OR Apache-2.0, and ships **no binaries** and none of the components
  below. Its only runtime dependency is [libloading].
- **The native binary archives** attached to `native-v*` releases on
  [firecrawl/pdfium-rs](https://github.com/firecrawl/pdfium-rs/releases)
  contain compiled PDFium shared libraries produced by
  [bblanchon/pdfium-binaries] from the upstream release pinned in
  `pdfium.lock.json`. Those libraries statically link the third-party
  components listed below. Every archive keeps the upstream `LICENSE` file
  and the per-component `licenses/` directory **verbatim**, so the full
  license texts always travel with the binaries; the archive additionally
  carries a `PROVENANCE.json` recording the upstream release, asset name,
  and sha256.

[libloading]: https://crates.io/crates/libloading
[bblanchon/pdfium-binaries]: https://github.com/bblanchon/pdfium-binaries

## Components bundled in the native binaries

All components are under permissive licenses; none are copyleft. License
identities were verified against the `licenses/` directory shipped inside
the pinned `pdfium-binaries` archives (release `chromium/7988`).

| Component | License |
|---|---|
| PDFium | BSD-3-Clause AND Apache-2.0 (dual; upstream's LICENSE carries both texts since the 2022 relicensing) |
| Abseil (abseil-cpp) | Apache-2.0 |
| Anti-Grain Geometry (agg23) | AGG permissive license |
| fast_float | MIT |
| FreeType | FreeType License (FTL) — carries a documentation credit requirement, satisfied below |
| ICU | Unicode License v3 |
| Little CMS (lcms) | MIT |
| libjpeg-turbo | IJG, BSD-3-Clause, and zlib (three-part license) |
| OpenJPEG (libopenjpeg) | BSD-2-Clause |
| libpng | PNG Reference Library License v2 |
| libtiff | libtiff/HylaFAX permissive license |
| llvm-libc | Apache-2.0 WITH LLVM-exception |
| simdutf | MIT |
| zlib | zlib |

## FreeType credit

The FreeType License requires the following statement in documentation
accompanying distributions that include FreeType:

> Portions of this software are copyright © The FreeType Project
> (www.freetype.org). All rights reserved.

## BSD non-endorsement notice

PDFium's BSD-3-Clause license terms prohibit using the name of Google Inc.
or the names of PDFium's contributors to endorse or promote products
derived from the software without specific prior written permission. This
project is not affiliated with, nor endorsed by, Google or the PDFium
authors.

## Build and packaging scripts

The binaries are built by the [bblanchon/pdfium-binaries] project, whose
build scripts are MIT-licensed, Copyright 2014-2025 Benoit Blanchon (the
copyright line as it appears in the LICENSE file shipped inside the
archives). This
repository's packaging step only repackages the verified upstream archives;
it does not modify the binaries.

## Vendored test fixtures

A small set of PDF test fixtures under `tests/fixtures/pdfium/` is copied
unmodified from the PDFium project's test corpus (`testing/resources/`),
Copyright 2014 The PDFium Authors, redistributed under PDFium's
BSD-3-Clause license. See `tests/fixtures/pdfium/LICENSE` for the license
text and per-file provenance. These fixtures are test inputs only and are
excluded from the package published to crates.io.
