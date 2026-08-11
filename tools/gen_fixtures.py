#!/usr/bin/env python3
"""Deterministic generator for the PDF test fixtures under tests/fixtures/.

Every fixture that can be produced deterministically is emitted byte-for-byte
identical on every run (no timestamps, no randomness), so `git diff` after
regeneration proves fixtures match this script.

Encrypted fixtures are derived from valid.pdf with `mutool clean -E` (see
tools/gen_encrypted_fixtures.sh); encryption embeds salts, so those are
generated once and committed rather than regenerated on every run.

Usage: python3 tools/gen_fixtures.py [output_dir]   (default: tests/fixtures)
"""

import sys
from pathlib import Path

HEADER = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"


def build_pdf(objects: list[bytes]) -> bytes:
    """Assemble numbered objects 1..n into a PDF with a correct xref table."""
    out = bytearray(HEADER)
    offsets = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
    xref_pos = len(out)
    out += f"xref\n0 {len(objects) + 1}\n".encode()
    out += b"0000000000 65535 f \n"
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n".encode()
    out += f"startxref\n{xref_pos}\n%%EOF\n".encode()
    return bytes(out)


def stream_obj(dict_extra: bytes, data: bytes) -> bytes:
    d = b"<< /Length " + str(len(data)).encode() + b" " + dict_extra + b">>"
    return d + b"\nstream\n" + data + b"\nendstream"


def valid_pdf() -> bytes:
    """One blank US-letter page (612x792 pt)."""
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
    ])


def mixed_pages_pdf() -> bytes:
    """Three pages: A4 portrait; US-letter with /Rotate 90; small 200x100."""
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate 90 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>",
    ])


def red_rect_pdf() -> bytes:
    """200x100 pt page, opaque red rectangle covering (50,25)..(150,75) in
    page space. Used by rendering pixel tests and coordinate-transform tests:
    the rectangle center is (100, 50) in page space."""
    content = b"1 0 0 rg 50 25 100 50 re f"
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>",
        stream_obj(b"", content),
    ])


def hello_text_pdf() -> bytes:
    """612x792 page with 'Hello, PDFium!' in Helvetica 24pt at (50, 700)."""
    content = b"BT /F1 24 Tf 50 700 Td (Hello, PDFium!) Tj ET"
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        stream_obj(b"", content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ])


def form_pdf() -> bytes:
    """Minimal AcroForm: one text field 'name' with value 'initial', plus a
    visible widget annotation with an appearance-free field (PDFium generates
    the appearance when forms are enabled)."""
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] "
        b"/DA (/Helv 12 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>",
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V (initial) "
        b"/DA (/Helv 12 Tf 0 g) /Rect [50 80 250 120] /F 4 /P 3 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ])


def large_page_pdf() -> bytes:
    """A 14400x14400 pt page (PDF spec maximum is 14400 in user units).
    Rendering this at any real scale explodes: used by resource-limit tests."""
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 14400 14400] >>",
    ])


def main() -> None:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("tests/fixtures")
    out_dir.mkdir(parents=True, exist_ok=True)

    valid = valid_pdf()
    fixtures: dict[str, bytes] = {
        "valid.pdf": valid,
        "mixed_pages.pdf": mixed_pages_pdf(),
        "red_rect.pdf": red_rect_pdf(),
        "hello_text.pdf": hello_text_pdf(),
        "form_text_field.pdf": form_pdf(),
        "large_page.pdf": large_page_pdf(),
        # Malformed inputs.
        "empty.pdf": b"",
        "header_only.pdf": b"%PDF-1.7\n",
        "truncated.pdf": valid[: len(valid) // 3],
        "garbage.bin": bytes((i * 197 + 91) % 256 for i in range(4096)),
        # Valid header, corrupt xref offset.
        "bad_xref.pdf": valid.replace(b"startxref", b"startxrEF", 1),
    }

    for name, data in fixtures.items():
        path = out_dir / name
        path.write_bytes(data)
        print(f"wrote {path} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
