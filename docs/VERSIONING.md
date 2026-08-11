# Versioning and compatibility

This document defines how `firecrawl-pdfium` versions itself, what Rust
toolchains it supports, and what "compatible PDFium binary" means
precisely. The README carries the summary; this is the contract.

## Crate versioning (SemVer, pre-1.0)

The crate follows [SemVer](https://semver.org/) with Cargo's pre-1.0
conventions:

- While the version is `0.x`, **breaking changes bump the minor version**
  (`0.1.z` -> `0.2.0`) and are called out in the changelog. Patch releases
  (`0.1.0` -> `0.1.1`) are strictly backward compatible.
- The public API includes everything re-exported from the crate root and
  the public `sys` module. `Error` and other enums marked
  `#[non_exhaustive]` may gain variants in minor releases; exhaustive
  matches on them are not part of the compatibility contract.
- The `Pdfium::load()` discovery chain (order and semantics) is a
  documented public contract; changing it is a breaking change.

## MSRV policy

- The minimum supported Rust version is declared as `rust-version` in
  `Cargo.toml` (currently **1.77**) and checked in CI.
- An MSRV bump is a **minor** version change, not a major one (following
  Cargo project guidance).
- The MSRV stays conservative: at least **six months** behind the current
  stable release at the time of a bump.

## PDFium binary compatibility

The crate binds PDFium at runtime and resolves every bound symbol
**eagerly at load time**. That makes compatibility a simple, mechanical
contract:

- **Any PDFium build that exports the bound symbols works** — regardless
  of who built it or which exact revision it is.
- A library missing any required symbol is rejected at `Pdfium::load()`
  with `LoadError::MissingSymbol` naming the absent symbol. There is no
  call-time failure mode: incompatibility is detected before the first
  PDFium call, never as undefined behavior later.
- Each crate release additionally documents the exact upstream release its
  binaries are **pinned and tested** against (below). Newer PDFium builds
  are expected to keep working (PDFium's public C API is stable in
  practice); older builds work as long as they predate none of the bound
  symbols.

PDFium's C API offers no version-query function, so the crate cannot
report the loaded engine's version at runtime. Track the binary version
through the artifact metadata shipped with our native archives
(`VERSION` file and `PROVENANCE.json`).

### Compatibility table

| Crate version | Pinned/tested PDFium | Oldest expected-working build | Tier 1 (CI-tested) platforms |
|---|---|---|---|
| 0.1.0 | `chromium/7988` (PDFium 153.0.7988.0) | ~`chromium/5845` era (newest bound symbol, `FPDFText_GetLooseCharBox`, stabilized well before it) | mac-arm64, mac-x64, linux-x64, linux-arm64 (glibc), win-x64 |

"Oldest expected-working" is an estimate based on when the newest symbol
this crate binds entered upstream; the load-time check is authoritative
for any particular library file.

## How the PDFium pin is updated

The pinned upstream release lives in **one place**:
[`pdfium.lock.json`](../pdfium.lock.json) — upstream tag, per-platform
asset URL, and sha256 (recorded at pin time because upstream publishes no
checksum files; `cargo xtask fetch-pdfium` verifies every download against
them).

A pin bump is:

1. Update `pdfium.lock.json` to the new `chromium/NNNN` release with fresh
   sha256s (via the xtask tooling).
2. Re-verify: full test matrix against the new binaries, and diff the
   pinned headers against the transcriptions in `src/sys/` (see the FFI
   change protocol in [CONTRIBUTING.md](../CONTRIBUTING.md)).
3. Tag a `native-vN` release; the release workflow repackages the newly
   pinned archives into verified `firecrawl-pdfium-<platform>.tgz` assets
   with checksums, licenses, provenance, and attestations.
4. Record the new pin in the table above and in the changelog.

### What a pin bump means for consumers

**Nothing at compile time.** The crate has no build-time PDFium
dependency, so a pin bump changes no Rust code and no crate API — it is a
runtime library swap. Deployments that ship a PDFium binary keep working
with the binary they have; pick up the new binaries (for upstream fixes,
including security fixes) by swapping the shared library your deployment
loads. Because of the symbol-presence contract, older crate versions load
newer binaries and vice versa, except when a crate release explicitly
raises the required symbol set — which the table above and the changelog
will say outright.
