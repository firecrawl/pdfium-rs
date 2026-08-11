# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(pre-1.0: breaking changes bump the minor version).

## [Unreleased]

## [0.1.0] - 2026-08-10

Initial release.

### Added

- Runtime loading of PDFium via `libloading` with a documented discovery
  chain (`PDFIUM_LIB_PATH`, executable directory, `target/pdfium`, system
  loader), explicit-path loading, and eager symbol resolution — an
  incompatible library fails at load time with
  `LoadError::MissingSymbol`.
- Process-wide singleton with every FFI call serialized through one
  global mutex; all handle types are `Send + Sync`.
- Document loading from memory or file with typed errors for encrypted
  (`PasswordRequired` / `IncorrectPassword` / `UnsupportedSecurity`) and
  malformed (`InvalidPdf`) input; metadata, permissions, PDF version,
  security handler revision, page labels, and cheap per-page size
  queries.
- Page rendering to owned pixel buffers: scale / DPI / width / height /
  fit / exact sizing, `Bgra8` / `Rgba8` / `Bgr8` / `Gray8` formats,
  background colors, extra rotation, annotation and form-field layers,
  anti-aliasing toggles, and a pre-allocation output-size ceiling
  (`RenderConfig::max_output_bytes`, default 1 GiB, enforced via
  `Error::RenderTooLarge`).
- Pixel/page coordinate mapping: every render carries a `PageTransform`
  (points and rects, both directions) derived from PDFium's own
  device-to-page mapping; `PdfPage::transform_for` computes it without
  rendering.
- Text extraction with per-character geometry (unicode, tight and loose
  bounds, baseline origin).
- AcroForm field appearance rendering via an owned form-fill environment.
- Public `sys` module exposing the raw function table, gated by the
  documented `ffi_lock` serialization contract.
- Binary acquisition tooling: `pdfium.lock.json` pin
  (`chromium/7988`, PDFium 153.0.7988.0) with sha256 verification,
  `cargo xtask fetch-pdfium`, and `cargo xtask repackage` for the
  `native-v*` release archives.

[Unreleased]: https://github.com/firecrawl/pdfium-rs/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/firecrawl/pdfium-rs/releases/tag/v0.1.0
