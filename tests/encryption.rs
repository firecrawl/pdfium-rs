//! Encrypted documents: password classification, owner vs. user passwords,
//! restricted permissions, security handler revisions, and rendering after
//! unlock.
//!
//! Fixture passwords (tests/fixtures/README.md and pdfium/LICENSE):
//! - encrypted_aes256.pdf  AES-256 (R6), user "userpw", owner "ownerpw"
//! - encrypted_rc4.pdf     RC4-40 (R2), user "userpw", owner "ownerpw"
//! - owner_only.pdf        AES-256 (R6), no user password, owner "ownerpw",
//!   permissions restricted to print-only (-P 4)
//! - pdfium/encrypted.pdf  RC4 (R4 handler), user "1234", owner "5678"

mod common;

use common::{fixture, pdfium};
use firecrawl_pdfium::{Error, PixelFormat, RenderConfig};

const MUTOOL_FIXTURES: [&str; 2] = ["encrypted_aes256.pdf", "encrypted_rc4.pdf"];

#[test]
fn no_password_is_password_required() {
    let p = pdfium();
    for name in MUTOOL_FIXTURES
        .iter()
        .chain(["pdfium/encrypted.pdf"].iter())
    {
        match p.load_document(fixture(name), None) {
            Err(e @ Error::PasswordRequired) => assert!(e.is_encryption_error()),
            other => panic!("{name}: expected PasswordRequired, got {other:?}"),
        }
    }
}

#[test]
fn wrong_password_is_incorrect_password() {
    let p = pdfium();
    for name in MUTOOL_FIXTURES
        .iter()
        .chain(["pdfium/encrypted.pdf"].iter())
    {
        match p.load_document(fixture(name), Some("wrong")) {
            Err(e @ Error::IncorrectPassword) => assert!(e.is_encryption_error()),
            other => panic!("{name}: expected IncorrectPassword, got {other:?}"),
        }
    }
}

#[test]
fn correct_user_password_opens() {
    let p = pdfium();
    for name in MUTOOL_FIXTURES {
        let doc = p
            .load_document(fixture(name), Some("userpw"))
            .expect("user password unlocks");
        assert_eq!(doc.page_count(), 1, "{name}");
    }
    let doc = p
        .load_document(fixture("pdfium/encrypted.pdf"), Some("1234"))
        .expect("user password unlocks the vendored fixture");
    assert_eq!(doc.page_count(), 1);
}

#[test]
fn correct_owner_password_also_opens() {
    let p = pdfium();
    for name in MUTOOL_FIXTURES {
        let doc = p
            .load_document(fixture(name), Some("ownerpw"))
            .expect("owner password unlocks");
        assert_eq!(doc.page_count(), 1, "{name}");
        // Owner access reports full permissions (mutool grants everything
        // it can; bits 1-2 stay reserved-zero => ...fffc).
        assert_eq!(doc.permissions().0, 0xFFFF_FFFC, "{name}");
    }
    let doc = p
        .load_document(fixture("pdfium/encrypted.pdf"), Some("5678"))
        .expect("owner password '5678' unlocks the vendored fixture");
    assert_eq!(doc.page_count(), 1);
    assert_eq!(
        doc.permissions().0,
        0xFFFF_FFFC,
        "owner access => full permissions"
    );
}

#[test]
fn owner_only_opens_without_password_and_with_owner_password() {
    let p = pdfium();

    // No user password: opens with no password at all...
    let doc = p
        .load_document(fixture("owner_only.pdf"), None)
        .expect("owner_only.pdf has an empty user password");
    assert_eq!(doc.page_count(), 1);

    // ...under the restricted user-level permissions (`mutool -P 4` grants
    // print only). Raw value pinned from the pinned PDFium build: the low
    // permission bits are 0xc4 = bit 3 (print) plus reserved bits 7-8.
    let perms = doc.permissions();
    assert_eq!(perms.0, 0xFFFF_F0C4);
    assert!(perms.can_print(), "-P 4 grants printing");
    assert!(!perms.can_copy(), "copy/extract is restricted");
    assert!(!perms.can_modify(), "modification is restricted");
    assert!(!perms.can_annotate(), "annotation/form-fill is restricted");

    // AND with the owner password, which lifts the restrictions.
    let doc = p
        .load_document(fixture("owner_only.pdf"), Some("ownerpw"))
        .expect("owner password unlocks owner_only.pdf");
    let perms = doc.permissions();
    assert_eq!(perms.0, 0xFFFF_FFFC);
    assert!(perms.can_print() && perms.can_copy() && perms.can_modify() && perms.can_annotate());
}

#[test]
fn security_handler_revisions() {
    let p = pdfium();
    let cases = [
        ("encrypted_aes256.pdf", Some("userpw"), 6), // AES-256 => R6
        ("encrypted_rc4.pdf", Some("userpw"), 2),    // RC4-40 => R2
        ("owner_only.pdf", None, 6),                 // AES-256 => R6
        ("pdfium/encrypted.pdf", Some("1234"), 4),   // vendored RC4 => R4
    ];
    for (name, password, revision) in cases {
        let doc = p.load_document(fixture(name), password).expect("open");
        assert_eq!(doc.security_handler_revision(), Some(revision), "{name}");
    }
}

#[test]
fn encryption_error_classification_is_exact() {
    let p = pdfium();
    let bytes = fixture("encrypted_aes256.pdf");

    let required = p.load_document(bytes.clone(), None).expect_err("locked");
    let incorrect = p
        .load_document(bytes, Some("nope"))
        .expect_err("wrong password");
    assert!(matches!(required, Error::PasswordRequired));
    assert!(matches!(incorrect, Error::IncorrectPassword));
    assert!(required.is_encryption_error());
    assert!(incorrect.is_encryption_error());

    // Non-encryption errors answer false.
    let invalid = p
        .load_document(fixture("garbage.bin"), None)
        .expect_err("garbage");
    assert!(!invalid.is_encryption_error());
}

#[test]
fn unlocked_encrypted_page_renders() {
    let p = pdfium();

    // The mutool fixtures derive from hello_text.pdf (612x792, dark text at
    // (50, 700)): after unlocking, rendering works and produces real
    // content, not a blank page.
    let doc = p
        .load_document(fixture("encrypted_aes256.pdf"), Some("userpw"))
        .expect("unlock aes256");
    let page = doc.page(0).expect("page");
    let rendered = page
        .render(
            &RenderConfig::new()
                .scale(1.0)
                .pixel_format(PixelFormat::Gray8),
        )
        .expect("render decrypted page");
    assert_eq!((rendered.width(), rendered.height()), (612, 792));
    let dark_pixels = rendered.pixels().iter().filter(|&&v| v < 128).count();
    assert!(
        dark_pixels > 0,
        "decrypted render must contain the text glyphs"
    );

    // Same for the vendored RC4 fixture.
    let doc = p
        .load_document(fixture("pdfium/encrypted.pdf"), Some("1234"))
        .expect("unlock vendored fixture");
    let rendered = doc
        .page(0)
        .expect("page")
        .render(&RenderConfig::new())
        .expect("render");
    assert_eq!((rendered.width(), rendered.height()), (612, 792));
}
