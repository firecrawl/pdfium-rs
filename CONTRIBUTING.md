# Contributing to firecrawl-pdfium

Thanks for contributing. This crate wraps a large C library behind a small
amount of carefully audited unsafe code, so the bar for changes — and
especially FFI changes — is deliberately explicit. Read the parts of this
document relevant to your change before opening a PR.

## Development setup

```sh
git clone https://github.com/firecrawl/pdfium-rs
cd pdfium-rs
cargo xtask fetch-pdfium   # downloads + sha256-verifies the pinned PDFium
cargo test                 # tests find the library under ./target/pdfium
```

`cargo xtask fetch-pdfium` downloads the upstream release pinned in
`pdfium.lock.json` for your host platform, verifies its sha256, and
extracts it to `target/pdfium/<platform>/` — which is on
`Pdfium::load()`'s discovery chain, so tests, examples, and benches work
with no further setup. Use `--platform <slug>` or `--all` for
cross-platform work, and `--print-path` for shell interpolation.

Plain `cargo build` and `cargo doc` need no PDFium binary at all: the
crate has no build script and links nothing at build time.

## Repository map

| Path | Contents |
|---|---|
| `src/` | The safe API: library loading (`library.rs`), documents, pages, rendering, text, forms, coordinates, errors. |
| `src/sys/` | The unsafe FFI layer: transcribed types/constants (`types.rs`) and the eagerly resolved function table (`bindings.rs`). |
| `docs/` | [DESIGN.md](docs/DESIGN.md) (decisions + soundness argument), [ARCHITECTURE.md](docs/ARCHITECTURE.md) (layering + unsafe rules), [VERSIONING.md](docs/VERSIONING.md), [RELEASING.md](docs/RELEASING.md). |
| `tests/` | Integration test suites and `tests/fixtures/` (see below). |
| `tools/` | Fixture generators: `gen_fixtures.py`, `gen_encrypted_fixtures.sh`. |
| `xtask/` | Repo automation (`cargo xtask fetch-pdfium`, `cargo xtask repackage`). |
| `benches/` | Criterion render benchmarks. |
| `pdfium.lock.json` | The single source of truth for the pinned PDFium release and checksums. |

## Test fixtures

Fixtures under `tests/fixtures/` are **frozen artifacts with documented
provenance** (`tests/fixtures/README.md`). Never hand-edit a fixture:
tests key off exact geometry, exact pixels, and documented passwords.

- **Deterministic fixtures** are regenerated with
  `python3 tools/gen_fixtures.py`. The generator is byte-deterministic (no
  timestamps, no randomness), so after regenerating, `git diff` proves the
  fixtures match the generator. Change fixtures by changing the generator,
  regenerating, and committing both together.
- **Encrypted fixtures** were generated once by
  `tools/gen_encrypted_fixtures.sh` (mutool; encryption embeds random
  salts, so regeneration changes bytes without changing behavior) and are
  committed. The passwords in `tests/fixtures/README.md` are part of the
  test contract.
- **Vendored PDFium-corpus fixtures** (`tests/fixtures/pdfium/`) are
  copied unmodified from upstream; see the LICENSE file there for
  provenance. Do not modify them; add new ones only with provenance and
  documented passwords verified against PDFium's own test sources.

## FFI change protocol

The `src/sys/` layer is a transcription of PDFium's headers, and the
headers are the **only** ground truth. To add or change a binding:

1. **Fetch the pinned headers**: `cargo xtask fetch-pdfium` — the archive
   ships them at `target/pdfium/<platform>/include/`. When bumping the
   pin, diff the whole `include/` directory between old and new releases
   and audit every function and struct this crate binds for signature or
   semantic changes.
2. **Transcribe verbatim** into `src/sys/bindings.rs` (functions) and
   `src/sys/types.rs` (types/constants): exact C types (`c_int`,
   `c_ulong` — never `u32`/`u64` for `unsigned long`, whose width differs
   between Windows and 64-bit Unix), exact field order, and the header's
   documented preconditions carried over into comments. No Windows-only or
   Skia-only symbols: the table resolves eagerly on every platform.
3. **Update the `sys_layout` struct-layout tests** whenever a struct in
   `src/sys/types.rs` is added or changed, so size/offset regressions are
   caught by CI rather than by UB.
4. **Wrap it safely and test it** — see the checklist in
   [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#how-to-add-a-new-pdfium-function).
   Note in the PR that the required symbol set grew (it raises the oldest
   PDFium build the crate loads against; `docs/VERSIONING.md` tracks
   this).

## Unsafe code review bar

- **Every `unsafe` block and `unsafe impl` carries a `SAFETY:` comment**
  stating the preconditions and why they hold at this exact call site. No
  exceptions, including "obviously fine" one-liners.
- **No `unsafe` outside `src/sys/` and the documented safe-layer call
  sites.** New unsafe call sites are a reviewed, deliberate event — if a
  change can be written without new unsafe code, it must be.
- The four containment rules in
  [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#unsafe-code-containment-rules)
  are hard requirements: serialization of every FFI call, no handle
  escaping lock discipline, no callback re-entering PDFium.
- Changes that alter ownership, lifetimes, locking, or callback behavior
  must update the soundness argument in [docs/DESIGN.md](docs/DESIGN.md)
  in the same PR.

## Lint gates

CI enforces, and PRs are expected to pass locally:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
cargo test
```

The crate denies `unsafe_op_in_unsafe_fn` and warns on `missing_docs`;
new public items need complete-sentence doc comments.

## Pull request expectations

- Small, focused PRs with a clear statement of what changed and why; link
  the design/architecture section your change touches.
- Tests for new behavior, including failure paths (this crate treats
  hostile input as a first-class code path).
- No new dependencies without prior discussion in an issue — the
  dependency surface (`libloading` only) is deliberate.
- Public API changes: call out SemVer impact in the PR description and
  add a changelog entry under `[Unreleased]`.
- Fixture changes: generator change + regenerated output committed
  together, never edited artifacts.
- Releases are maintainer territory: see [docs/RELEASING.md](docs/RELEASING.md).

## Security issues

Do not open public issues for suspected vulnerabilities — see
[SECURITY.md](SECURITY.md) for private reporting.
