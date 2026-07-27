//! RSA PKCS#1 v1.5 SHA-256 sign/verify tests.
//!
//! NOTE ON DETERMINISTIC KATs: RSA PKCS#1 v1.5 signatures are NOT
//! deterministic — the padding bytes are randomized by `sign_with_rng`
//! (the `RandomizedSigner` trait). Therefore there is no published
//! "deterministic signature" KAT for this signature scheme (the way there
//! is for RFC 6979 deterministic ECDSA). The test suite below uses:
//!
//! 1. **Round-trip tests** (sign → verify → true): the standard property
//!    test for randomized signature schemes.
//! 2. **Negative tests** (verify with wrong key / tampered signature → false):
//!    the AEAD-equivalent "authentication failure" check.
//! 3. **Structural tests** (PEM format, signature length = modulus length):
//!    verify the wire-format contract documented in `src/rsa.rs`.
//!
//! Reference for PKCS#1 v1.5 signature format: RFC 8017 (PKCS#1 v2.2)
//! Section 8.2 "RSASSA-PKCS1-v1_5". The 2048-bit floor is NIST SP 800-57
//! Part 1 Revision 5 Section 6.4 ("Algorithm Security Strengths: RSA").

use buff_crypto_extras::rsa_api;
use buff_crypto_extras::CryptoError;

/// Minimum acceptable RSA modulus size, copied from `rsa::MIN_BITS` so the
/// tests do not depend on the internal constant's exact export path.
const RSA_MIN_BITS: usize = 2048;

// ============================================================================
// Keypair-generation structural tests.
// ============================================================================

/// `generate_keypair(2048)` must succeed and return well-formed PEM strings.
/// Both PEMs must have the correct armor headers (`-----BEGIN PUBLIC KEY-----`
/// for Spki, `-----BEGIN PRIVATE KEY-----` for PKCS#8).
#[test]
fn rsa_generate_keypair_2048_returns_well_formed_pem_pair() {
    let kp =
        rsa_api::generate_keypair(RSA_MIN_BITS).expect("2048-bit keypair generation must succeed");

    assert!(
        !kp.public_pem.is_empty(),
        "public_pem must not be empty after successful generation"
    );
    assert!(
        !kp.private_pem.is_empty(),
        "private_pem must not be empty after successful generation"
    );
    assert!(
        kp.public_pem.contains("-----BEGIN PUBLIC KEY-----"),
        "public_pem must be a Spki-armored PEM (RFC 5280 / RFC 7468)"
    );
    assert!(
        kp.public_pem.contains("-----END PUBLIC KEY-----"),
        "public_pem must include the Spki PEM trailer"
    );
    assert!(
        kp.private_pem.contains("-----BEGIN PRIVATE KEY-----"),
        "private_pem must be a PKCS#8-armored PEM (RFC 5208 / RFC 5958)"
    );
    assert!(
        kp.private_pem.contains("-----END PRIVATE KEY-----"),
        "private_pem must include the PKCS#8 PEM trailer"
    );
}

/// A sub-2048-bit modulus must be rejected with `CryptoError::InvalidLength`
/// — never silently accepted, never panicking. This enforces the NIST SP
/// 800-57 floor documented in `src/rsa.rs::MIN_BITS`.
#[test]
fn rsa_generate_keypair_below_min_bits_returns_invalid_length() {
    let result = rsa_api::generate_keypair(1024);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: RSA_MIN_BITS,
                got: 1024,
                ..
            })
        ),
        "1024-bit modulus must be rejected with InvalidLength {{ expected: 2048 }}, got {:?}",
        result
    );

    // 2047 (one bit under the floor) must also be rejected.
    let result = rsa_api::generate_keypair(2047);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: RSA_MIN_BITS,
                got: 2047,
                ..
            })
        ),
        "2047-bit modulus must be rejected (one bit below floor), got {:?}",
        result
    );
}

// ============================================================================
// Sign → Verify round-trip tests.
// ============================================================================

/// Sign a known message with a fresh 2048-bit key, then verify the
/// signature with the corresponding public key. MUST return `true`.
///
/// This is the canonical positive test for RSA PKCS#1 v1.5 SHA-256
/// (RFC 8017 Section 8.2). The signature is non-deterministic (randomized
/// padding), so we do not assert on signature bytes — only on the
/// verify-true outcome.
#[test]
fn rsa_sign_verify_round_trip_returns_true() {
    let kp = rsa_api::generate_keypair(RSA_MIN_BITS)
        .expect("keypair generation for sign-verify test must succeed");
    let message = b"The quick brown fox jumps over the lazy dog";

    let signature =
        rsa_api::sign(&kp.private_pem, message).expect("RSA.sign on a valid PEM must succeed");

    // Signature length = modulus length in bytes (256 bytes for 2048-bit).
    assert_eq!(
        signature.len(),
        RSA_MIN_BITS / 8,
        "PKCS#1 v1.5 signature length must equal modulus byte length (256 for 2048-bit)"
    );

    let verified = rsa_api::verify(&kp.public_pem, message, &signature);
    assert!(
        verified,
        "verify must return true for a signature produced by the matching private key"
    );
}

/// Sign + verify with an empty message. PKCS#1 v1.5 + SHA-256 must handle
/// empty input cleanly (SHA-256 is well-defined on the empty string).
#[test]
fn rsa_sign_verify_empty_message_round_trip() {
    let kp = rsa_api::generate_keypair(RSA_MIN_BITS)
        .expect("keypair generation for empty-message test must succeed");
    let empty_message: &[u8] = b"";

    let signature = rsa_api::sign(&kp.private_pem, empty_message)
        .expect("RSA.sign on empty message must succeed");

    let verified = rsa_api::verify(&kp.public_pem, empty_message, &signature);
    assert!(
        verified,
        "verify must return true for an empty-message signature"
    );
}

// ============================================================================
// Negative tests — verification must return false (NEVER panic, NEVER error).
// ============================================================================

/// Verify a signature against a DIFFERENT public key (one generated
/// independently). MUST return `false` — the signature was produced under
/// a different modulus, so verification cannot succeed.
#[test]
fn rsa_verify_with_wrong_public_key_returns_false() {
    let kp_a = rsa_api::generate_keypair(RSA_MIN_BITS)
        .expect("keypair A for wrong-key test must generate");
    let kp_b = rsa_api::generate_keypair(RSA_MIN_BITS)
        .expect("keypair B for wrong-key test must generate");

    let message = b"signed under keypair A, verified against keypair B";
    let signature =
        rsa_api::sign(&kp_a.private_pem, message).expect("sign under keypair A must succeed");

    // Verify against kp_b's public key (NOT kp_a's). Must be false.
    let verified = rsa_api::verify(&kp_b.public_pem, message, &signature);
    assert!(
        !verified,
        "verify with a mismatched public key must return false (never panic)"
    );
}

/// Verify a signature against a DIFFERENT message than was signed. MUST
/// return `false` — the SHA-256 digest embedded in the PKCS#1 v1.5 padding
/// will not match.
#[test]
fn rsa_verify_tampered_message_returns_false() {
    let kp = rsa_api::generate_keypair(RSA_MIN_BITS)
        .expect("keypair for message-tamper test must generate");
    let signed_message = b"original message that was signed";
    let tampered_message = b"DIFFERENT message under the same signature";

    let signature = rsa_api::sign(&kp.private_pem, signed_message)
        .expect("sign must succeed for tampered-message test setup");

    let verified = rsa_api::verify(&kp.public_pem, tampered_message, &signature);
    assert!(
        !verified,
        "verify with a tampered message must return false (never panic)"
    );
}

/// Verify a signature whose bytes were flipped post-signing. MUST return
/// `false` — the corrupted signature cannot decode to a valid PKCS#1 v1.5
/// padded digest.
#[test]
fn rsa_verify_tampered_signature_bytes_returns_false() {
    let kp =
        rsa_api::generate_keypair(RSA_MIN_BITS).expect("keypair for sig-tamper test must generate");
    let message = b"payload protected against signature-bit flips";

    let mut signature = rsa_api::sign(&kp.private_pem, message)
        .expect("sign for sig-tamper test setup must succeed");

    // Flip a single bit in the signature body. Pick a byte index that is
    // safely inside the signature (not the trailing byte, in case any
    // implementation trims trailing zero bytes).
    signature[0] ^= 0x01;

    let verified = rsa_api::verify(&kp.public_pem, message, &signature);
    assert!(
        !verified,
        "verify with a bit-flipped signature must return false (never panic)"
    );
}

/// Verify with a malformed (garbage) public PEM string. MUST return `false`
/// — never panic, never error. The `verify` API contract (see `src/rsa.rs`
/// doc) is "false on ANY failure" per the T49 / T26 / T34 stance.
#[test]
fn rsa_verify_with_malformed_public_pem_returns_false() {
    let kp = rsa_api::generate_keypair(RSA_MIN_BITS)
        .expect("keypair for malformed-pem test must generate");
    let message = b"payload whose signature will be verified against garbage";

    let signature = rsa_api::sign(&kp.private_pem, message)
        .expect("sign for malformed-pem test setup must succeed");

    let garbage_pem = "not a PEM string at all";
    let verified = rsa_api::verify(garbage_pem, message, &signature);
    assert!(
        !verified,
        "verify with a malformed public PEM must return false (never panic)"
    );
}

/// Sign with a malformed private PEM string. MUST return `Err(CryptoError::Rsa(_))`
/// rather than producing a signature or panicking.
#[test]
fn rsa_sign_with_malformed_private_pem_returns_rsa_error() {
    let garbage_pem = "-----BEGIN PRIVATE KEY-----\nnot a real key\n-----END PRIVATE KEY-----";
    let result = rsa_api::sign(garbage_pem, b"data");

    assert!(
        matches!(result, Err(CryptoError::Rsa(_))),
        "sign with a malformed private PEM must return CryptoError::Rsa, got {:?}",
        result
    );
}
