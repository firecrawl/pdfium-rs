# Architecture

This document describes how `firecrawl-pdfium` is layered, who owns what
memory, and the rules that keep the unsafe code contained. The design
rationale behind these choices lives in [DESIGN.md](DESIGN.md); this file
is the map for reading and changing the code.

## Layers

```
+---------------------------------------------------------------+
|                        consumer code                          |
|      (any thread; owned results are freely shareable)         |
+---------------------------------------------------------------+
        |  safe types: Pdfium, PdfDocument, PdfPage,
        |  RenderConfig -> RenderedPage, PageText, PageTransform
        v
+---------------------------------------------------------------+
|                     safe API (src/*.rs)                       |
|   validates arguments, owns buffers, maps errors; the only    |
|   place safe code touches FFI, always via Pdfium::ffi(...)    |
+---------------------------------------------------------------+
        |  every call wrapped in...
        v
+---------------------------------------------------------------+
|              global FFI mutex (library.rs ffi_lock)           |
|   one process-wide Mutex; no PDFium call happens outside it   |
+---------------------------------------------------------------+
        |  serialized calls into...
        v
+---------------------------------------------------------------+
|              sys::Bindings function table (src/sys/)          |
|   ~47 FPDF_* function pointers, resolved eagerly at load;     |
|   signatures transcribed verbatim from the pinned headers     |
+---------------------------------------------------------------+
        |  resolved via...
        v
+---------------------------------------------------------------+
|                          libloading                           |
+---------------------------------------------------------------+
        |  dlopen/LoadLibrary of...
        v
+---------------------------------------------------------------+
|        libpdfium.dylib / libpdfium.so / pdfium.dll            |
|     (process-global state; loaded once, never unloaded)       |
+---------------------------------------------------------------+
```

There is exactly one instance of the bottom three layers per process: the
first successful `Pdfium::load*()` call initializes PDFium and installs a
`'static` singleton; `Pdfium` values are `Copy` handles to it. The library
is deliberately never unloaded (PDFium re-initialization is not a
well-tested upstream path, and unload-while-in-use is unsound by
construction) — a one-time, bounded leak.

## Ownership map

```
Pdfium (Copy handle -> 'static singleton)
 |         owns: Bindings table, loaded Library, ffi_lock
 |
 +-- PdfDocument                       (created by pdfium.load_document)
      |    owns: FPDF_DOCUMENT handle
      |          the PDF bytes (Box<[u8]>; PDFium requires the buffer
      |            to outlive the document handle)
      |          FormEnv (OnceLock; boxed FPDF_FORMFILLINFO at a stable
      |            address, torn down before FPDF_CloseDocument in Drop)
      |
      +-- PdfPage<'doc>                (borrows the document)
           |    owns: FPDF_PAGE handle (closed on drop, after
           |          FORM_OnBeforeClosePage when forms are active)
           |    the borrow checker statically prevents pages from
           |    outliving their document
           |
           +-- RenderedPage            (owned pixels + PageTransform)
           +-- PageText                (owned string + PageChar geometry)
                    ZERO FFI attachments: plain data, Send + Sync,
                    valid after page, document, and library handles
                    are gone; no lock involved in reading them
```

Rendering uses a caller-owned buffer end to end: the safe layer allocates
the `Vec<u8>`, hands it to `FPDFBitmap_CreateEx` in external-buffer mode
(PDFium documents that `FPDFBitmap_Destroy` then does not free it),
destroys the FPDF bitmap after rendering, and moves the `Vec` into
`RenderedPage`. No pixel data is ever owned by PDFium.

## Unsafe-code containment rules

All four rules are load-bearing; a PR that violates any of them is
unsound, not merely unidiomatic.

1. **All `unsafe` lives in `src/sys/` plus narrow call sites in the safe
   layer, each with a `SAFETY:` comment.** The `sys` module holds the
   transcribed types and the function table; the safe layer's `unsafe`
   blocks are single FFI calls whose preconditions the surrounding code
   just established, and the comment states which ones.
2. **Every FFI call is serialized.** No call into PDFium — including via
   the public `sys` escape hatch — may execute outside the process-wide
   `ffi_lock`. The safe layer has no unlocked path (`Pdfium::ffi` is the
   only door); raw `sys` users must hold `Pdfium::ffi_lock()` for the
   duration of every call sequence.
3. **Handles never escape lock discipline.** `FPDF_*` handles are private
   fields of the safe types that own their lifecycle; they are passed to
   PDFium only inside `ffi(...)` closures. `Send`/`Sync` on the owning
   types is justified exactly by rules 2 and 3, and the argument is
   written next to each `unsafe impl`.
4. **Callbacks never re-enter PDFium.** The form-fill stubs handed to
   PDFium run while the FFI lock is held by the frame that entered PDFium;
   a stub that called back into PDFium (or into the safe API) would
   deadlock or violate serialization. Every stub is a no-op returning the
   documented "not supported" value.

## How to add a new PDFium function

1. **Verify the signature against the pinned headers.** Fetch the pinned
   archive (`cargo xtask fetch-pdfium`) and read the function's
   declaration in `target/pdfium/<platform>/include/*.h`, including the
   documentation comment — preconditions, buffer protocols, and lifetime
   requirements are part of the contract. The full header-diff procedure
   is in [CONTRIBUTING.md](../CONTRIBUTING.md#ffi-change-protocol).
2. **Add it to the `pdfium_bindings!` table** in `src/sys/bindings.rs`,
   transcribing the C signature verbatim (`c_int`, `c_ulong`, raw
   pointers — note `c_ulong` differs between Windows and 64-bit Unix, so
   never substitute `u32`/`u64`). Any new structs or constants go in
   `src/sys/types.rs` with the same fidelity. Keep the table grouped by
   header. Do not add Windows-only or Skia-only symbols: the table is
   resolved eagerly on every platform, so a symbol missing anywhere makes
   loading fail everywhere.
3. **Wrap it safely.** Call it from the appropriate safe-layer module via
   `self.ffi(|b| ...)`, validate arguments before the call, map failure
   returns onto an existing `Error` variant (or add one), and write the
   `SAFETY:` comment stating which preconditions hold and why. If the
   function introduces a new lifetime or callback pattern, update the
   soundness argument in [DESIGN.md](DESIGN.md).
4. **Test it**, including failure paths, against the fixtures in
   `tests/fixtures/` (add a generated fixture if none exercises the new
   code path). Remember that raising the required symbol set raises the
   minimum PDFium build the crate loads against — call that out in the PR
   so `docs/VERSIONING.md` can be updated.
