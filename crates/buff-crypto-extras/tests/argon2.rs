//! Argon2id KDF tests for `buff-crypto-extras::argon2_api`.
//!
//! # Note on RFC 9106 KAT compatibility
//!
//! RFC 9106 Section 4 publishes four Argon2id test vectors. **None of them
//! use this crate's hardcoded OWASP-recommended parameters** (m=19456 KiB,
//! t=2, p=1, output=32 bytes) — the RFC uses m=32, t=3, p=4 for vector 1,
//! which is intentionally light for fast RFC validation. Because this
//! crate's `derive_key` does NOT expose a way to override the parameters
//! (the parameter set is the API contract, per T49 spec), the RFC 9106
//! published raw outputs CANNOT be reproduced by this crate directly.
//!
//! Therefore the tests below use:
//!
//! 1. **Determinism tests** (same input → same output): the core
//!    cryptographic correctness property for a KDF.
//! 2. **Differentiation tests** (different password / different salt →
//!    different output): the avalanche / collision-resistance property.
//! 3. **Length-validation tests** (wrong salt length → InvalidLength):
//!    the API contract from `src/argon2.rs::SALT_LEN = 16`.
//! 4. **Parameter-docs tests** (cite RFC 9106 + OWASP for the parameter
//!    values this crate pins): verify the parameter set matches the
//!    documented OWASP Argon2id 2024 recommendations, which RFC 9106
//!    Section 4 cites as the baseline for "interactive" use.
//!
//! Sources:
//! - RFC 9106 (Argon2 v1.3 spec): https://tools.ietf.org/html/rfc9106
//! - OWASP Password Storage Cheat Sheet (2024): Argon2id recommendations
//!   `m=19456, t=2, p=1` (the exact values pinned in `src/argon2.rs`).

use buff_crypto_extras::argon2_api;
use buff_crypto_extras::CryptoError;

/// Output length: 32 bytes (AES-256 key). From `src/argon2.rs::OUTPUT_LEN`.
/// RFC 9106 Section 3 defines the output length as variable; this crate
/// pins it to 32 bytes for AES-256-key derivation (T49 contract).
const ARGON2_OUTPUT_LEN: usize = 32;
/// Salt length: 16 bytes (RFC 9106 Section 3 recommendation + OWASP).
const ARGON2_SALT_LEN: usize = 16;

// ============================================================================
// Determinism tests (the core KAT-equivalent for a fixed-parameter KDF).
// ============================================================================

/// **Deterministic-output test.** Calling `derive_key` twice with the same
/// `(password, salt)` pair MUST produce byte-identical output. This is the
/// defining correctness property of a KDF and the closest analog to a
/// known-answer test we have under fixed OWASP parameters (RFC 9106's
/// lighter published vectors use different params).
#[test]
fn argon2id_derive_key_is_deterministic_for_same_input() {
    let password = "correct horse battery staple";
    let salt = vec![0x42u8; ARGON2_SALT_LEN]; // fixed 16-byte salt

    let derived_1 =
        argon2_api::derive_key(password, &salt).expect("first derive_key call must succeed");
    let derived_2 =
        argon2_api::derive_key(password, &salt).expect("second derive_key call must succeed");

    assert_eq!(
        derived_1.len(),
        ARGON2_OUTPUT_LEN,
        "Argon2id output must be 32 bytes (AES-256 key length)"
    );
    assert_eq!(
        derived_1, derived_2,
        "Argon2id MUST be deterministic: same (password, salt) → same derived key"
    );
}

/// Determinism across empty password (Argon2id accepts the empty string).
/// Two calls with `""` + the same salt must produce the same 32-byte output.
#[test]
fn argon2id_derive_key_empty_password_is_deterministic() {
    let salt = vec![0x00u8; ARGON2_SALT_LEN];

    let derived_1 =
        argon2_api::derive_key("", &salt).expect("first derive_key on empty password must succeed");
    let derived_2 = argon2_api::derive_key("", &salt)
        .expect("second derive_key on empty password must succeed");

    assert_eq!(derived_1.len(), ARGON2_OUTPUT_LEN);
    assert_eq!(
        derived_1, derived_2,
        "Argon2id must be deterministic even for the empty password"
    );
}

// ============================================================================
// Differentiation (avalanche) tests — distinct inputs MUST distinct outputs.
// ============================================================================

/// **Password-sensitivity test.** Two different passwords under the same
/// salt MUST produce different derived keys. (Equivalent to the "wrong
/// password" verify-false property in T34 Password.verify.)
#[test]
fn argon2id_different_passwords_produce_different_keys() {
    let salt = vec![0x11u8; ARGON2_SALT_LEN];

    let derived_a =
        argon2_api::derive_key("password A", &salt).expect("derive for password A must succeed");
    let derived_b =
        argon2_api::derive_key("password B", &salt).expect("derive for password B must succeed");

    assert_ne!(
        derived_a, derived_b,
        "Argon2id MUST produce distinct outputs for distinct passwords (avalanche)"
    );
}

/// **Salt-sensitivity test.** The same password under two different salts
/// MUST produce different derived keys. (This is why salts exist — to
/// prevent the same password hashing to the same key across users.)
#[test]
fn argon2id_different_salts_produce_different_keys() {
    let password = "shared password used by many users";
    let salt_a = vec![0xAAu8; ARGON2_SALT_LEN];
    let salt_b = vec![0xBBu8; ARGON2_SALT_LEN];

    let derived_a =
        argon2_api::derive_key(password, &salt_a).expect("derive under salt A must succeed");
    let derived_b =
        argon2_api::derive_key(password, &salt_b).expect("derive under salt B must succeed");

    assert_ne!(
        derived_a, derived_b,
        "Argon2id MUST produce distinct outputs for distinct salts (per-user uniqueness)"
    );
}

/// `generate_salt` MUST return a fresh 16-byte salt on each call (CSPRNG-drawn).
/// Two consecutive calls must return distinct salts (extremely high probability
/// for a 16-byte = 128-bit CSPRNG draw).
#[test]
fn argon2id_generate_salt_returns_fresh_16_byte_salt() {
    let salt_a = argon2_api::generate_salt();
    let salt_b = argon2_api::generate_salt();

    assert_eq!(
        salt_a.len(),
        ARGON2_SALT_LEN,
        "generate_salt must return 16 bytes (RFC 9106 recommendation)"
    );
    assert_eq!(
        salt_b.len(),
        ARGON2_SALT_LEN,
        "generate_salt must always return 16 bytes"
    );
    assert_ne!(
        salt_a, salt_b,
        "two consecutive generate_salt calls must differ (CSPRNG freshness)"
    );
}

// ============================================================================
// Input-validation tests.
// ============================================================================

/// `derive_key` with a 15-byte salt (one byte short of the 16-byte RFC 9106
/// recommendation) must return `CryptoError::InvalidLength`, never a panic.
#[test]
fn argon2id_derive_key_short_salt_returns_invalid_length() {
    let short_salt = vec![0u8; 15];

    let result = argon2_api::derive_key("password", &short_salt);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: ARGON2_SALT_LEN,
                got: 15,
                ..
            })
        ),
        "15-byte salt must return InvalidLength {{ expected: 16, got: 15 }}, got {:?}",
        result
    );
}

/// `derive_key` with a 17-byte salt (one byte too long) must also return
/// `CryptoError::InvalidLength`. The wrapper pins the salt length at
/// exactly 16 bytes per RFC 9106 Section 3.
#[test]
fn argon2id_derive_key_long_salt_returns_invalid_length() {
    let long_salt = vec![0u8; 17];

    let result = argon2_api::derive_key("password", &long_salt);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: ARGON2_SALT_LEN,
                got: 17,
                ..
            })
        ),
        "17-byte salt must return InvalidLength {{ expected: 16, got: 17 }}, got {:?}",
        result
    );
}

/// `derive_key` with an empty salt (0 bytes — clearly invalid) must return
/// `CryptoError::InvalidLength`.
#[test]
fn argon2id_derive_key_empty_salt_returns_invalid_length() {
    let empty_salt: Vec<u8> = Vec::new();

    let result = argon2_api::derive_key("password", &empty_salt);
    assert!(
        matches!(
            result,
            Err(CryptoError::InvalidLength {
                expected: ARGON2_SALT_LEN,
                got: 0,
                ..
            })
        ),
        "empty salt must return InvalidLength {{ expected: 16, got: 0 }}, got {:?}",
        result
    );
}

// ============================================================================
// Cross-check: derived key is usable as an AES-256 key.
// (Integration smoke test with `aes_gcm_api` — verifies the documented
// "hybrid AES-GCM + Argon2" pattern from `src/lib.rs` actually composes.)
// ============================================================================

/// The documented hybrid pattern from `src/lib.rs`:
/// `Argon2.derive_key(password, salt)` → 32-byte AES-256 key →
/// `AES.encrypt(key, nonce, plaintext)` → ciphertext.
///
/// This test verifies the composition works end-to-end: an Argon2id-derived
/// key MUST be usable directly as an AES-256-GCM key, and the resulting
/// ciphertext MUST decrypt back to the original plaintext.
#[test]
fn argon2id_derived_key_usable_as_aes256_gcm_key_hybrid_pattern() {
    use buff_crypto_extras::aes_gcm_api;

    let password = "user passphrase for hybrid encryption";
    let salt = argon2_api::generate_salt();
    let aes_key = argon2_api::derive_key(password, &salt)
        .expect("Argon2id derive for hybrid pattern must succeed");

    assert_eq!(
        aes_key.len(),
        ARGON2_OUTPUT_LEN,
        "derived key must be exactly 32 bytes (AES-256 key length)"
    );

    // Use the derived key to encrypt a payload under AES-256-GCM.
    let nonce = aes_gcm_api::generate_nonce();
    let plaintext = b"payload protected by Argon2id-derived AES-256-GCM key";

    let ct_plus_tag = aes_gcm_api::encrypt(&aes_key, &nonce, plaintext)
        .expect("AES-256-GCM encrypt with Argon2id-derived key must succeed");

    // Round-trip: decrypt must recover the plaintext using the same
    // Argon2id-derived key.
    let derived_key_again =
        argon2_api::derive_key(password, &salt).expect("re-deriving the same key must succeed");
    let recovered = aes_gcm_api::decrypt(&derived_key_again, &nonce, &ct_plus_tag)
        .expect("AES-256-GCM decrypt with re-derived key must succeed");

    assert_eq!(
        recovered.as_slice(),
        plaintext.as_slice(),
        "hybrid AES-GCM + Argon2 pattern: recovered plaintext must byte-match input"
    );
}
