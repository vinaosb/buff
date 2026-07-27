//! AES-256-GCM known-answer tests (KATs).
//!
//! All test vectors are sourced from the McGrew & Viega "The Galois/Counter
//! Mode of Operation (GCM)" paper Appendix B (also reproduced verbatim in
//! NIST SP 800-38D, and in the widely-used `ring` test vector file
//! `tests/aead_aes_256_gcm_tests.txt`). The SAS broadcast version of these
//! vectors is mirrored across many independent implementations
//! (RustCrypto/aes-gcm, BoringSSL, OpenSSL, pycryptodome, BouncyCastle).
//!
//! Wire-format note: `buff_crypto_extras::aes_gcm_api::encrypt` returns
//! `ciphertext || 16-byte GCM tag`. For empty plaintexts the returned Vec
//! is therefore exactly 16 bytes (the standalone tag). This matches the
//! underlying `aes_gcm::aead::Aead::encrypt` contract.

use buff_crypto_extras::aes_gcm_api;
use buff_crypto_extras::CryptoError;

/// Hex-decode helper for the test vectors below. Panics on malformed hex
/// (test-only — never reaches production code paths).
fn hex_decode(s: &str) -> Vec<u8> {
    hex::decode(s).expect("test vector hex must be valid")
}

// ============================================================================
// Known-answer tests (KATs) — published NIST / McGrew-Viega / ring vectors.
// ============================================================================

/// **NIST SP 800-38D / McGrew-Viega GCM paper, Test Case 13.**
///
/// AES-256-GCM with the all-zeros key, all-zeros IV, empty plaintext, and
/// no associated data. The output is the standalone 16-byte GCM tag.
///
/// Source: D. McGrew, J. Viega, "The Galois/Counter Mode of Operation
/// (GCM)", Appendix B, Test Case 13. Also in NIST SP 800-38D and
/// independently mirrored in the `ring` test corpus.
#[test]
fn aes256_gcm_kat_test_case_13_empty_plaintext_zero_key() {
    let key = hex_decode("0000000000000000000000000000000000000000000000000000000000000000");
    let nonce = hex_decode("000000000000000000000000");
    let plaintext: &[u8] = b"";

    // Expected tag for the all-zero (key, IV, empty-PT, no-AAD) input.
    // Cross-verified by: RustCrypto aes-gcm, ring, BoringSSL, OpenSSL,
    // pycryptodome, BouncyCastle. 16 bytes total (the bare GCM tag).
    let expected_ct_plus_tag = hex_decode("530f8afbc74536b9a963b4f1c4cb738b");

    let actual = aes_gcm_api::encrypt(&key, &nonce, plaintext)
        .expect("AES-256-GCM Test Case 13 must succeed");
    assert_eq!(
        actual, expected_ct_plus_tag,
        "Test Case 13: ciphertext||tag must match published NIST KAT"
    );

    // Round-trip the same input: decrypt the published tag → recover the
    // (empty) plaintext.
    let recovered = aes_gcm_api::decrypt(&key, &nonce, &expected_ct_plus_tag)
        .expect("decrypt of Test Case 13 ciphertext must succeed");
    assert!(recovered.is_empty(), "recovered plaintext must be empty");
}

/// **`ring` AES-256-GCM test vector (no AAD, empty PT, random key/IV).**
///
/// Source: briansmith/ring `tests/aead_aes_256_gcm_tests.txt`, first entry.
/// Vector parameters:
///   KEY   = e5ac4a32c67e425ac4b143c83c6f161312a97d88d634afdf9f4da5bd35223f01
///   NONCE = 5bf11a0951f0bfc7ea5c9e58
///   IN    = "" (empty)
///   AD    = "" (empty)
///   CT    = "" (empty)
///   TAG   = d7cba289d6d19a5af45dc13857016bac
///
/// This vector complements Test Case 13 by using non-zero key/IV material
/// while still exercising the empty-PT path — so a regression that only
/// manifests for non-trivial keys would be caught here.
#[test]
fn aes256_gcm_kat_ring_first_vector_no_aad_empty_pt() {
    let key = hex_decode("e5ac4a32c67e425ac4b143c83c6f161312a97d88d634afdf9f4da5bd35223f01");
    let nonce = hex_decode("5bf11a0951f0bfc7ea5c9e58");
    let plaintext: &[u8] = b"";

    let expected_ct_plus_tag = hex_decode("d7cba289d6d19a5af45dc13857016bac");

    let actual = aes_gcm_api::encrypt(&key, &nonce, plaintext)
        .expect("ring AES-256-GCM vector 1 must succeed");
    assert_eq!(
        actual, expected_ct_plus_tag,
        "ring vector 1: ciphertext||tag must match published bytes"
    );

    // Decrypt-KAT: feeding the published (ct||tag) through decrypt must
    // recover the empty plaintext.
    let recovered = aes_gcm_api::decrypt(&key, &nonce, &expected_ct_plus_tag)
        .expect("decrypt of ring vector 1 must succeed");
    assert!(recovered.is_empty());
}

// ============================================================================
// Round-trip + structural tests (no published KAT possible because the
// underlying AES-GCM API in this crate does NOT accept associated data,
// so McGrew-Viega Test Cases 14–16 cannot be used as direct KATs).
// ============================================================================

/// Round-trip: encrypt a 60-byte plaintext under a fixed 32-byte key +
/// 12-byte nonce, then decrypt the result. The recovered plaintext must
/// byte-match the input. The plaintext is taken verbatim from McGrew-Viega
/// Test Case 14 (just the PT bytes — AAD is not part of this crate's
/// surface, so the resulting CT/Tag will differ from the published vector).
#[test]
fn aes256_gcm_round_trip_recovers_plaintext() {
    let key = hex_decode("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308");
    let nonce = hex_decode("cafebabefacedbaddecaf888");
    let plaintext = hex_decode(
        "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72\
         1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
    );

    let ct_plus_tag =
        aes_gcm_api::encrypt(&key, &nonce, &plaintext).expect("round-trip encrypt must succeed");

    // Wire-format invariant: ciphertext length = plaintext.len() + 16-byte tag.
    assert_eq!(
        ct_plus_tag.len(),
        plaintext.len() + 16,
        "wire format must be ciphertext || 16-byte tag"
    );

    let recovered =
        aes_gcm_api::decrypt(&key, &nonce, &ct_plus_tag).expect("round-trip decrypt must succeed");
    assert_eq!(
        recovered, plaintext,
        "recovered plaintext must byte-match input"
    );
}

/// Round-trip with random key/nonce + non-aligned plaintext (33 bytes —
/// forces partial final AES block in GCM's counter mode).
#[test]
fn aes256_gcm_round_trip_random_key_nonce_non_block_aligned() {
    let key = aes_gcm_api::generate_key();
    assert_eq!(key.len(), 32, "generate_key must return 32 bytes");

    let nonce = aes_gcm_api::generate_nonce();
    assert_eq!(nonce.len(), 12, "generate_nonce must return 12 bytes");

    let plaintext = b"The quick brown fox jumps over."; // 31 bytes, non-block-aligned
    let ct_plus_tag =
        aes_gcm_api::encrypt(&key, &nonce, plaintext).expect("random-key encrypt must succeed");
    assert_eq!(ct_plus_tag.len(), plaintext.len() + 16);

    let recovered =
        aes_gcm_api::decrypt(&key, &nonce, &ct_plus_tag).expect("random-key decrypt must succeed");
    assert_eq!(recovered.as_slice(), plaintext.as_slice());
}

// ============================================================================
// Authentication-failure tests (GCM is an AEAD — tag mismatch MUST fail).
// ============================================================================

/// Tampered ciphertext bit → GCM tag verification MUST fail (returns
/// `CryptoError::Aes`, never panics). This is the AEAD integrity property.
#[test]
fn aes256_gcm_tampered_ciphertext_fails_authentication() {
    let key = aes_gcm_api::generate_key();
    let nonce = aes_gcm_api::generate_nonce();
    let plaintext = b"sensitive payload requiring integrity";

    let mut ct_plus_tag = aes_gcm_api::encrypt(&key, &nonce, plaintext)
        .expect("encrypt for tamper test must succeed");

    // Flip a single bit in the ciphertext body (NOT the tag).
    ct_plus_tag[0] ^= 0x01;

    let result = aes_gcm_api::decrypt(&key, &nonce, &ct_plus_tag);
    assert!(
        matches!(result, Err(CryptoError::Aes(_))),
        "tampered ciphertext must fail authentication with CryptoError::Aes, got {:?}",
        result
    );
}

/// Tampered tag bit → same authentication failure. The 16-byte trailing
/// tag is what GCM verifies; corrupting it MUST be detected.
#[test]
fn aes256_gcm_tampered_tag_fails_authentication() {
    let key = aes_gcm_api::generate_key();
    let nonce = aes_gcm_api::generate_nonce();
    let plaintext = b"another payload for tag-tamper test";

    let mut ct_plus_tag = aes_gcm_api::encrypt(&key, &nonce, plaintext)
        .expect("encrypt for tag-tamper test must succeed");

    // Flip a bit in the trailing 16-byte tag region.
    let last = ct_plus_tag.len() - 1;
    ct_plus_tag[last] ^= 0x80;

    let result = aes_gcm_api::decrypt(&key, &nonce, &ct_plus_tag);
    assert!(
        matches!(result, Err(CryptoError::Aes(_))),
        "tampered tag must fail authentication with CryptoError::Aes, got {:?}",
        result
    );
}

/// Wrong key → authentication failure (the GCM tag is keyed, so decrypting
/// with a different key cannot verify).
#[test]
fn aes256_gcm_wrong_key_fails_authentication() {
    let key_a = aes_gcm_api::generate_key();
    let key_b = aes_gcm_api::generate_key();
    assert_ne!(key_a, key_b, "test setup: two random keys must differ");

    let nonce = aes_gcm_api::generate_nonce();
    let plaintext = b"payload encrypted under key A";

    let ct_plus_tag =
        aes_gcm_api::encrypt(&key_a, &nonce, plaintext).expect("encrypt under key A must succeed");

    let result = aes_gcm_api::decrypt(&key_b, &nonce, &ct_plus_tag);
    assert!(
        matches!(result, Err(CryptoError::Aes(_))),
        "decrypt with wrong key must fail authentication, got {:?}",
        result
    );
}

// ============================================================================
// Input-validation tests (wrong-length inputs must return InvalidLength).
// ============================================================================

/// A 31-byte key (one byte short of AES-256's 32-byte requirement) must
/// produce `CryptoError::InvalidLength`, never a panic.
#[test]
fn aes256_gcm_short_key_returns_invalid_length() {
    let short_key = vec![0u8; 31]; // 31 bytes, one short
    let nonce = vec![0u8; 12];
    let plaintext = b"data";

    let result = aes_gcm_api::encrypt(&short_key, &nonce, plaintext);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: 32,
                got: 31,
                ..
            })
        ),
        "31-byte key must return InvalidLength {{ expected: 32, got: 31 }}, got {:?}",
        result
    );

    // Same check for decrypt.
    let result = aes_gcm_api::decrypt(&short_key, &nonce, &[0u8; 32]);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: 32,
                got: 31,
                ..
            })
        ),
        "decrypt with 31-byte key must return InvalidLength, got {:?}",
        result
    );
}

/// An 11-byte nonce (GCM mandates the 12-byte standard nonce) must produce
/// `CryptoError::InvalidLength`.
#[test]
fn aes256_gcm_short_nonce_returns_invalid_length() {
    let key = vec![0u8; 32];
    let short_nonce = vec![0u8; 11];
    let plaintext = b"data";

    let result = aes_gcm_api::encrypt(&key, &short_nonce, plaintext);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: 12,
                got: 11,
                ..
            })
        ),
        "11-byte nonce must return InvalidLength {{ expected: 12, got: 11 }}, got {:?}",
        result
    );
}

/// Decrypt with a ciphertext shorter than the 16-byte tag (so the implied
/// plaintext would be zero-or-negative length) must return InvalidLength.
#[test]
fn aes256_gcm_too_short_ciphertext_returns_invalid_length() {
    let key = vec![0u8; 32];
    let nonce = vec![0u8; 12];
    let short_ct = vec![0u8; 15]; // 15 bytes, less than the 16-byte tag

    let result = aes_gcm_api::decrypt(&key, &nonce, &short_ct);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: 16,
                got: 15,
                ..
            })
        ),
        "15-byte ciphertext must return InvalidLength {{ expected: 16, got: 15 }}, got {:?}",
        result
    );
}
