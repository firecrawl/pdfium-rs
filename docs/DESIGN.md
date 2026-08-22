# Design

This document records the load-bearing design decisions for `firecrawl-pdfium`
and the reasoning behind them. It is the source of truth the rest of the
repository is built against. Facts cited below (header semantics, distribution
layout, publishing constraints) were verified against primary sources on
2026-08-10: the PDFium headers shipped in `bblanchon/pdfium-binaries` release
`chromium/7988` (PDFium 153.0.7988.0), the crates.io trusted-publishing
documentation, and the crates.io registry itself.

## Goals

1. A **safe, small, production-focused** Rust API over PDFium for the
   workflows document pipelines actually need: open documents from memory,
   inspect pages, render at configurable resolution to **owned** pixel
   buffers, map coordinates between rendered pixels and PDF page space,
   extract text with positions, render form fields, and fail cleanly on
   encrypted/invalid/unsupported input — all safely usable from concurrent
   code.
2. A **complete distribution story**: the crate knows how to find PDFium, the
   repository knows how to fetch, verify, and re-release the native binaries,
   and both are automated in CI.
3. **Contained, justified, and tested unsafe code**: `unsafe` lives in one
   auditable layer with an explicit soundness argument.

## Non-goals (v0.x)

- PDF editing, saving, or generation.
- JavaScript execution or XFA forms (PDFium's default builds disable both).
- Interactive form *filling* (we render form field appearances; we do not
  simulate input events).
- Bindings for all ~470 `FPDF_*` functions. We bind a curated surface and
  grow it deliberately.

## Crate name and organization

**Crate: `firecrawl-pdfium`. Repository: `firecrawl/pdfium-rs`. Single
publishable crate with a public `sys` module, plus a `publish = false` `xtask`
tool crate.**

Naming: checked against crates.io on 2026-08-10 — `pdfium` (active,
unrelated project), `pdfium-rs` (squatted, single 0.1.0), `pdfium-sys`
(abandoned 2021), and `pdfium-bind` are all taken. `firecrawl-pdfium` is
available, unambiguous about provenance, and avoids implying a relationship
with the existing `pdfium` crate.

Single crate rather than a `-sys` + wrapper pair because:

- There is no build script and no link-time dependency (see *Dynamic loading*
  below), so the traditional reason for a `-sys` crate — centralizing native
  build/link logic per the `links` convention — does not apply.
- `cargo publish --dry-run -p firecrawl-pdfium` passes **before any first
  publish** in a single-crate workspace (verified empirically with cargo
  1.95). In a two-crate workspace, the dependent crate's dry-run fails until
  the dependency is actually on crates.io, which would leave the release
  pipeline unverifiable until after a production publish.
- Publication ordering becomes trivially correct: there is exactly one
  package to publish.
- Consumers who want raw FFI still get it: `firecrawl_pdfium::sys` is public.

## Dynamic loading, not linking

The crate binds PDFium **at runtime** with `libloading` (function pointers
resolved into a table at load time). No `build.rs`, no bindgen, no libclang,
no link-time artifacts.

Why:

- **docs.rs and plain `cargo build` always work** — nothing links against
  libpdfium, so building the crate (and its docs) requires no native library.
- **Deployment matches reality**: PDFium is a ~10 MB platform-specific shared
  library that teams ship next to their binary or in a base image. An
  explicit load step with a documented discovery chain is more predictable
  than linker magic.
- **One failure mode, surfaced early**: a missing/incompatible library is a
  typed `Error::Load` at `Pdfium::load()` time, not an opaque linker or
  loader error at process start.
- Static linking can be added later as an additive feature without breaking
  the API (documented as deferred).

Symbols are resolved eagerly at load; a missing symbol yields
`LoadError::MissingSymbol { name }`, which doubles as the binary
compatibility check (older PDFium builds lacking a newer API fail loudly and
specifically at load time, never at call time).

### Library discovery

`Pdfium::load()` tries, in order (first hit wins; the chain is part of the
public contract and documented on the method):

1. `PDFIUM_LIB_PATH` environment variable — a path to the library file or to
   a directory containing the platform library name.
2. The directory of the current executable.
3. `./target/pdfium/<platform>/lib/` — the layout produced by
   `cargo xtask fetch-pdfium`, so tests/examples work with zero setup.
4. The platform loader's default search path, by bare library name
   (`libpdfium.dylib` / `libpdfium.so` / `pdfium.dll`).

`Pdfium::load_from_path(...)` and `Pdfium::load_from_directory(...)` skip
discovery entirely (the recommended production configuration, per libloading
guidance to prefer absolute paths).

### Process-wide singleton

PDFium has process-global state (`FPDF_InitLibrary*`/`FPDF_DestroyLibrary`).
The first successful load initializes the library and installs a process-wide
instance; `Pdfium` handles are cheap clones of it. Subsequent `load()` calls
return the existing instance; an explicit `load_from_path()` naming a
*different* path returns `Error::AlreadyLoaded` rather than silently using
the wrong binary. The library is never unloaded (`FPDF_DestroyLibrary` is
deliberately never called): PDFium re-initialization is not a well-tested
upstream path, and unload-while-in-use is unsound by construction. This is a
one-time, bounded leak, documented in the API.

## Thread safety

**PDFium is not thread-safe** (upstream's own guidance: serialize all calls).
Ecosystem history shows how easy this is to get wrong: wrappers have shipped
`unsafe impl Send + Sync` without actually serializing every call path,
which is unsound. We treat that as the cautionary tale:

- Every FFI call in the safe layer goes through **one global
  `Mutex`** (`ffi_lock`). There is no unlocked path; the raw `sys` table is
  reachable only via `unsafe` and documents the serialization requirement as
  a safety precondition.
- Handle-owning types (`Pdfium`, `PdfDocument`, `PdfPage`) are `Send + Sync`
  *because* every method serializes through that mutex; the invariants are
  written next to each `unsafe impl`.
- Callbacks we hand to PDFium (form-fill stubs) never call back into PDFium,
  so lock re-entrancy cannot occur.
- `RenderedPage`, `PageText`, and friends are plain owned data with no FFI
  attachments — freely shareable, no lock involved.

This gives correctness, not parallelism (calls serialize). The documented
pattern for throughput is the same one PDFium upstream recommends:
process-level parallelism. The concurrency tests hammer the serialization
from many threads.

## Memory model

- `PdfDocument` **owns the PDF bytes**. `FPDF_LoadMemDocument64` requires the
  buffer to outlive the document handle (header: "The memory buffer must
  remain valid when the document is open"), so the safe API takes
  `Into<Vec<u8>>` and stores the bytes in the document. No borrowing
  footguns, no copies beyond the one the caller chose.
- `PdfPage<'doc>` borrows its document; the borrow checker statically
  prevents use-after-close. Pages call `FORM_OnBeforeClosePage` (when forms
  are active) and `FPDF_ClosePage` on drop.
- Rendering uses a **caller-owned buffer**: we allocate the `Vec<u8>`, hand
  it to `FPDFBitmap_CreateEx` (external-buffer mode; PDFium documents that
  `FPDFBitmap_Destroy` then does not free it), destroy the FPDF bitmap after
  rendering, and move the `Vec` into `RenderedPage`. The result carries **no
  FFI resources**: pixels, dimensions, stride, format, and the
  pixel↔page-space transform are all plain data.
- The form-fill environment (`FPDF_FORMFILLINFO`) must stay at a stable
  address until `FPDFDOC_ExitFormFillEnvironment` (header requirement); it is
  boxed and owned by the document, and torn down before `FPDF_CloseDocument`
  in `Drop`.

## Rendering and pixel formats

- `RenderConfig` selects size by `scale`, `dpi` (points are 1/72"), exact
  `width`/`height`, or fit-within box; plus pixel format (`Bgra8` default,
  `Rgba8` via PDFium's `FPDF_REVERSE_BYTE_ORDER`, `Bgr8`, `Gray8`),
  background color, extra 90° rotation, annotation and form-field layers,
  and anti-aliasing toggles.
- The output buffer is pre-filled with the encoded background color by us
  (PDFium's `FPDFBitmap_FillRect` *replaces* rather than composites, and its
  color interacts confusingly with reversed byte order — prefilling the
  buffer ourselves is exact for every format).
- BGRA output is **straight (non-premultiplied) alpha** per the headers;
  `BGRA_Premul` is Skia-renderer-only and not exposed (default binaries use
  the AGG renderer).
- **Resource limits**: `RenderConfig::max_output_bytes` (default 1 GiB)
  bounds the output allocation, and text extraction is bounded the same
  way by a character ceiling (`PdfPage::text_with_limit`, default one
  million characters) checked before any allocation; oversize requests fail with
  `Error::RenderTooLarge` before any allocation. PDF's 14,400 pt page-size
  ceiling times a large scale overflows quickly; the limit makes that a
  typed error instead of an OOM.

## Coordinates

Two spaces: **page space** (PDF user space: points, origin bottom-left,
y-up) and **pixel space** (rendered bitmap: pixels, origin top-left, y-down).

`RenderedPage::transform()` returns a `PageTransform` derived at render time
by querying `FPDF_DeviceToPage` at three exact integer device corners and
building the affine map (and its inverse) from the results. Deriving from
PDFium itself — rather than reimplementing the mapping — means `/Rotate`
pages and extra render rotations are handled with PDFium's exact semantics
(the header requires DeviceToPage to be called with the same
start/size/rotate values as the render; we do exactly that). The transform is
plain data on the owned render result, so OCR-style consumers can map boxes
back to page space long after the page and document are gone.

`PdfPage::device_to_page` / `page_to_device` also expose PDFium's own
(integer-quantized) conversions directly; tests assert the derived affine
agrees with them within one pixel of quantization.

## Error taxonomy

`FPDF_GetLastError` is only meaningful immediately after a failed
document-load call (header-verified), and that is the only place we consult
it. Mapping:

| Condition | Variant |
|---|---|
| `FPDF_ERR_PASSWORD`, no password given | `PasswordRequired` |
| `FPDF_ERR_PASSWORD`, password given | `IncorrectPassword` |
| `FPDF_ERR_SECURITY` | `UnsupportedSecurity` |
| `FPDF_ERR_FORMAT` / `FPDF_ERR_FILE` | `InvalidPdf` |
| other/unknown codes | `PdfiumError { code }` |

Library loading failures are structured (`LibraryNotFound { searched }`,
`OpenFailed`, `MissingSymbol`), page access is `PageIndexOutOfBounds` /
`PageLoadFailed`, and render-size violations are `RenderTooLarge`. The enum
is `#[non_exhaustive]`.

## Native binary acquisition and redistribution

Upstream: **`bblanchon/pdfium-binaries`** (weekly automated builds since
2017, widest platform matrix, consistent archive layout, sigstore
attestations; it is what PDFium bindings across languages already
target). Default builds have `pdf_enable_v8 = false`,
`pdf_enable_xfa = false`, which matches our non-goals.

- The pinned release is recorded in **one place**, `pdfium.lock.json`
  (tag, per-platform URL + sha256). Currently `chromium/7988`
  (PDFium 153.0.7988.0). Upstream publishes no checksum files, so we record
  sha256s ourselves at pin time; `cargo xtask fetch-pdfium` verifies them on
  every download.
- **Supported platform matrix** (tier 1, CI-tested): mac-arm64, mac-x64,
  linux-x64 (glibc), linux-arm64 (glibc), win-x64. Other upstream platforms
  (musl, android, ios, wasm) are documented as untested-but-probably-fine.
- The **release workflow repackages** the pinned archives into
  `firecrawl-pdfium` GitHub release assets, preserving the `licenses/`
  directory and `LICENSE` verbatim (that satisfies every notice obligation —
  all 14 bundled components are permissively licensed, see
  THIRD-PARTY-NOTICES.md), adding `VERSION`, build metadata, `SHA256SUMS`,
  an SPDX SBOM (syft), and GitHub build-provenance attestations
  (`actions/attest`), all verifiable with `gh attestation verify`.

## Versioning and compatibility

- **Crate**: SemVer, pre-1.0 (`0.x`: breaking changes bump minor). MSRV is
  declared via `rust-version` (1.77), checked in CI, and an MSRV bump is a
  minor, not major, change (per Cargo guidance).
- **PDFium binary**: the crate is compatible with any PDFium build that
  exports the bound symbols — in practice anything ≥ ~chromium/5845 (the
  newest symbol we require, `FPDFText_IsGenerated`, landed upstream in the
  ~chromium/5414 era, well before that). Each crate release documents the
  *pinned/tested* PDFium release; the load-time symbol check turns
  incompatibility into a specific error. `Pdfium::version()` is not
  provided by PDFium's C API; consumers should track the binary version
  via the artifact metadata we ship.
- The compatibility table lives in `docs/VERSIONING.md` and is updated per
  release.

## Publishing and release automation

- **crates.io**: trusted publishing (OIDC) via `rust-lang/crates-io-auth-action`.
  Constraint (verified from the official docs): the **first** publish of a
  new crate cannot use trusted publishing — it requires an API token. The
  bootstrap procedure (manual first publish by a maintainer, then configure
  trusted publishing, then revoke the token) is step-by-step in
  `docs/RELEASING.md`. No long-lived credentials in the repository or its
  secrets after bootstrap.
- **Gating**: tag `vX.Y.Z` → `publish.yml` verifies tag == Cargo.toml
  version, runs the full test suite on Linux (the five-platform matrix
  runs in `ci.yml` pre-merge), then the publish job waits on the
  `release` GitHub Environment (required reviewers = human approval gate)
  before `cargo publish`.
- **Idempotency**: every phase is independently re-runnable — the publish
  step consults the crates.io sparse index and skips if the version already
  exists; release asset upload uses `--clobber`; `gh release create` is
  guarded by `gh release view`.
- `cargo publish --dry-run` runs on every push in CI, so the packaging path
  is continuously validated without publishing anything.

## Testing strategy

- **Fixtures**: deterministic, byte-reproducible PDFs generated by
  `tools/gen_fixtures.py` (geometry, rendering-pixel, text, form, malformed
  classes), mutool-generated encrypted fixtures with documented passwords,
  plus a small set vendored unmodified from PDFium's own test corpus
  (`tests/fixtures/pdfium/`, BSD-3-Clause, with license + provenance file)
  whose passwords are verified from PDFium's own test sources
  (`encrypted.pdf`: user `1234`, owner `5678`).
- **Suites**: lifecycle, malformed input, encryption, rendering (including
  pixel-exact assertions on a known geometry fixture), forms, text,
  coordinate transforms (agreement with PDFium's own DeviceToPage), 
  concurrency stress, and resource limits. Loading-failure tests run in
  their own test binary (process isolation from the singleton).
- **Fuzzing**: `cargo-fuzz` targets for document load + first-page render on
  arbitrary bytes; scheduled weekly CI job plus on-demand `workflow_dispatch`
  (fuzz builds need nightly + cargo-fuzz, too heavy for every PR).
- **Benchmarks**: criterion render latency across scales/formats, plus a
  peak-RSS report example; native artifact sizes are recorded in each
  release's `ARTIFACT-SIZES.txt` asset and its notes table.

## Deferred (documented limitations)

- Static linking feature; musl/Android/iOS/WASM support.
- Progressive rendering, `FPDF_RenderPageBitmapWithMatrix` clipping (bound in
  `sys`, not yet exposed safely), page-range rasterization helpers.
- Form field *values* and programmatic form filling; annotations API;
  outline/bookmarks; attachments; digital signature inspection.
- Text search (`FPDFText_FindStart`) and layout-aware text segmentation.
- Custom font directories at init.
