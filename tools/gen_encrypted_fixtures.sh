#!/usr/bin/env bash
# Generates the encrypted test fixtures from hello_text.pdf using mutool
# (mupdf-tools >= 1.20). Encryption embeds random salts and document IDs, so
# unlike gen_fixtures.py this is NOT byte-deterministic; the outputs are
# generated once and committed. Passwords are part of the test contract:
#
#   encrypted_aes256.pdf  user password: "userpw"   owner password: "ownerpw"
#   encrypted_rc4.pdf     user password: "userpw"   owner password: "ownerpw"
#   owner_only.pdf        no user password           owner password: "ownerpw"
#                         (opens without a password; permissions restricted
#                          to printing only, -P 4)
set -euo pipefail
cd "$(dirname "$0")/.."

SRC=tests/fixtures/hello_text.pdf
[ -f "$SRC" ] || { echo "run tools/gen_fixtures.py first" >&2; exit 1; }

mutool clean -E aes-256 -U userpw -O ownerpw "$SRC" tests/fixtures/encrypted_aes256.pdf
mutool clean -E rc4-40 -U userpw -O ownerpw "$SRC" tests/fixtures/encrypted_rc4.pdf
mutool clean -E aes-256 -U "" -O ownerpw -P 4 "$SRC" tests/fixtures/owner_only.pdf

echo "done:"
ls -la tests/fixtures/encrypted_aes256.pdf tests/fixtures/encrypted_rc4.pdf tests/fixtures/owner_only.pdf
