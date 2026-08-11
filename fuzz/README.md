# Fuzzing

`cargo-fuzz` (libFuzzer) targets that hammer the safe API with arbitrary
bytes. Two targets:

| Target | Exercises |
|---|---|
| `load_document` | `load_document` on raw bytes; on success: page count, every page's size, form type, Title metadata. Encryption errors are retried once with a throwaway password. |
| `render_page` | Everything above the render path: open page 0, extract text, render under tiny caps (fit 64×64, 1 MiB output ceiling), derive the transform, one pixel→page→pixel roundtrip. |

Neither target asserts on outcomes — hostile input is *supposed* to fail
with typed errors. Findings are crashes, sanitizer reports, hangs, and
out-of-memory aborts.

## Prerequisites

- A nightly toolchain (`rustup toolchain install nightly`) — libFuzzer
  instrumentation needs unstable `-Z` flags.
- `cargo install cargo-fuzz --locked`
- A PDFium binary: run `cargo xtask fetch-pdfium` once from the repository
  root. The targets **panic at startup** if PDFium cannot be loaded —
  fuzzing without the library would only exercise the load-error path.

## Seeding the corpus

Seed from the test fixtures (copy, never move — fixtures are the source of
truth for the test suite). From the repository root:

```sh
for t in load_document render_page; do
  mkdir -p fuzz/corpus/$t
  cp tests/fixtures/*.pdf tests/fixtures/pdfium/*.pdf tests/fixtures/garbage.bin fuzz/corpus/$t/
done
```

The corpus is deliberately **not** committed (`corpus/` is gitignored here
and at the repository root); re-seed after a fresh clone. libFuzzer grows
the corpus in place as it discovers new coverage.

## Running

From the repository root:

```sh
# Timed sessions (recommended; seconds):
cargo +nightly fuzz run load_document -- -max_total_time=300
cargo +nightly fuzz run render_page   -- -max_total_time=300

# Fixed iteration count (quick smoke, e.g. in CI on PRs):
cargo +nightly fuzz run load_document -- -runs=2000
cargo +nightly fuzz run render_page   -- -runs=2000
```

Useful extra flags after the `--`: `-jobs=N -workers=N` to parallelize
(each worker is its own process, so PDFium's single-threadedness is not a
bottleneck), `-rss_limit_mb=2048` / `-malloc_limit_mb=512` to tighten the
memory tripwires, `-timeout=10` to catch slow-input hangs sooner.

Reproduce a finding by passing the saved input back to the target:

```sh
cargo +nightly fuzz run load_document fuzz/artifacts/load_document/crash-<hash>
cargo +nightly fuzz tmin load_document fuzz/artifacts/load_document/crash-<hash>  # minimize
```

## What a finding means (crate wrapping a C library)

Any crash is a real bug somewhere; triage decides *whose*:

1. **Ours (the Rust wrapper).** The backtrace's faulting frames are in
   `firecrawl_pdfium::…` code: a Rust panic (index out of bounds, integer
   overflow in debug assertions, explicit `panic!`) or a sanitizer report
   pointing at buffers the wrapper owns. These are bugs in this crate —
   file them on `firecrawl/pdfium-rs` with the minimized input attached.
2. **Upstream (PDFium itself).** The faulting frames are inside
   `libpdfium.dylib` / `libpdfium.so` (often unsymbolized addresses below
   an `FPDF_*` entry point in the trace). The wrapper cannot fix these;
   verify the crash reproduces with the current
   [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) release
   (bump `pdfium.lock.json` locally and re-fetch), then report it to the
   PDFium project via the Chromium issue tracker
   (<https://issues.chromium.org>, component `Chromium > Internals >
   Plugins > PDF` / PDFium) with the minimized PDF. Treat memory-safety
   crashes as security-sensitive and use Chromium's security reporting
   flow rather than a public issue.
3. **OOMs and timeouts** deserve a look before blaming either side: an
   input that legitimately declares enormous structures may just need
   tighter limits in the harness, but unbounded allocation for a tiny
   input is a real (usually upstream) bug.

Two upstream-related caveats when reading sanitizer output:

- The prebuilt PDFium binary is **not** sanitizer-instrumented, so ASan
  only sees violations that cross the boundary into memory it tracks
  (allocations made on the Rust side, stack, shadow-visible regions).
  Heap corruption entirely inside PDFium's own allocations can go
  undetected here yet still be a real, reportable bug.
- The crate intentionally never unloads PDFium (see `docs/DESIGN.md`);
  the one-time library/init allocations stay reachable through a static,
  so LeakSanitizer does not flag them.

## Coverage feedback is Rust-side only

libFuzzer's coverage instrumentation applies to the code cargo compiles —
this crate and its Rust dependencies. The prebuilt `libpdfium` dylib
carries no SanitizerCoverage, so mutations are guided by wrapper-side
branches only and the fuzzer is effectively black-box with respect to
PDFium's parser internals. That makes the seed corpus matter more than
usual: seeds that already reach deep parser states (encrypted files,
forms, weird xref tables) are how deep paths get explored. A fully
coverage-guided setup would require a from-source, instrumented PDFium
build, which is out of scope here.
