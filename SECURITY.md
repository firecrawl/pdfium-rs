# Security policy

## Reporting a vulnerability

Report suspected vulnerabilities **privately** via GitHub Security
Advisories:
[github.com/firecrawl/pdfium-rs/security/advisories/new](https://github.com/firecrawl/pdfium-rs/security/advisories/new).

Please do not open public issues or pull requests for suspected
vulnerabilities before a fix is available.

We handle reports on a best-effort basis — there is no formal SLA — but
memory-safety reports against this crate's code are treated as
top-priority work.

## Scope

In scope for this repository's process:

- **The crate's own code**: the safe API, the `sys` FFI layer, and the
  soundness of the `unsafe` code (serialization, ownership, lifetimes,
  callback discipline). Anything that lets safe Rust cause undefined
  behavior is a vulnerability, even without a demonstrated exploit.
- **The packaging and distribution pipeline**: `pdfium.lock.json` pins and
  checksums, `cargo xtask fetch-pdfium` verification, the repackaged
  `native-v*` release archives, their `SHA256SUMS`, provenance metadata,
  and attestations. A way to make users fetch or trust a tampered binary
  is in scope.

## Vulnerabilities in the PDFium engine itself

Parsing/rendering bugs inside PDFium (memory corruption on a crafted PDF,
etc.) are **upstream** issues — report them to the Chromium/PDFium
project, which runs its own well-staffed security process:

- Chromium security reporting guide:
  <https://www.chromium.org/Home/chromium-security/reporting-security-bugs/>
- PDFium issue tracker (component in the Chromium tracker):
  <https://issues.chromium.org/issues?q=componentid:1586257>

What we do on our side: upstream `pdfium-binaries` builds weekly, so fixes
reach binary releases quickly. After a relevant upstream fix lands, we
bump the pin in `pdfium.lock.json` and ship refreshed `native-v*` archives;
consuming the fix is a runtime library swap with no crate code change (see
[docs/VERSIONING.md](docs/VERSIONING.md)). Hardening on the crate side —
typed errors on hostile input, render output-size ceilings, no unlocked
FFI path — limits blast radius but cannot fix engine bugs.

## Supported versions

| Version | Supported |
|---|---|
| Latest 0.x release | Yes — fixes ship as a new 0.x release |
| Older releases | No — please upgrade |
