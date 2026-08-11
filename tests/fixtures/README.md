# Test fixtures

Three provenance classes:

## 1. Generated, byte-deterministic (`tools/gen_fixtures.py`)

Regenerate with `python3 tools/gen_fixtures.py`; the output is byte-identical
on every run, so `git diff` proves fixtures match the generator.

| File | Contents |
|---|---|
| `valid.pdf` | one blank US-letter page (612×792 pt) |
| `mixed_pages.pdf` | A4 portrait; US-letter with `/Rotate 90`; 200×100 pt page |
| `red_rect.pdf` | 200×100 pt page, opaque red rectangle spanning (50,25)–(150,75) in page space — rendering and coordinate tests key off these exact numbers |
| `hello_text.pdf` | "Hello, PDFium!" in Helvetica 24pt at origin (50, 700) |
| `form_text_field.pdf` | minimal AcroForm: text field `name`, value `initial`, widget rect (50,80)–(250,120) on a 300×200 page |
| `large_page.pdf` | 14400×14400 pt page (PDF's maximum) — resource-limit tests |
| `empty.pdf` | zero bytes |
| `header_only.pdf` | just `%PDF-1.7\n` |
| `truncated.pdf` | first third of `valid.pdf` |
| `garbage.bin` | 4 KiB of deterministic non-PDF bytes |
| `bad_xref.pdf` | `valid.pdf` with the `startxref` keyword corrupted |

## 2. Generated once with mutool (`tools/gen_encrypted_fixtures.sh`)

Encryption embeds random salts, so these are committed rather than
regenerated (regenerating produces functionally identical files with
different bytes). Derived from `hello_text.pdf`.

| File | Encryption | User password | Owner password |
|---|---|---|---|
| `encrypted_aes256.pdf` | AES-256 (R6) | `userpw` | `ownerpw` |
| `encrypted_rc4.pdf` | RC4-40 (R2) | `userpw` | `ownerpw` |
| `owner_only.pdf` | AES-256 (R6), opens with no password, permissions restricted to print-only (`-P 4`) | *(none)* | `ownerpw` |

## 3. Vendored from the PDFium test corpus (`pdfium/`)

Copied unmodified from `testing/resources/` in the PDFium repository;
BSD-3-Clause, see `pdfium/LICENSE` for the license text, provenance note,
and documented passwords (verified against PDFium's own embedder tests).
