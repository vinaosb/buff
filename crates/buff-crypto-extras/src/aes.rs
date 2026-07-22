//! AES-256-GCM authenticated encryption.
//!
//! AES-GCM is the only AES mode `buff-crypto-extras` exposes. GCM is
//! an AEAD (authenticated encryption with associated data) — ciphertexts
//! produced by [`Aes256::encrypt`] carry a 16-byte authentication tag
//! that [`Aes256::decrypt`] verifies before returning plaintext. Raw
//! AES-ECB / AES-CBC are explicitly NOT exposed because they are not
//! authenticated and the T49 spec mandates "no homebrew crypto".
//!
//! # Wire format
//!
//! [`Aes256::encrypt`] returns `ciphertext || tag` (the GCM tag is
//! appended to the ciphertext, total length = `plaintext.len() + 16`).
//! This matches the `aes-gcm` crate's default `encrypt()` output and
//! is the same shape OpenSSL / pycryptodome / BouncyCastle use.
//!
//! # Nonce handling
//!
//! The 12-byte nonce MUST be unique per (key, message) pair. Reusing
//! a (key, nonce) pair is CATASTROPHIC for GCM confidentiality AND
//! integrity. [`Aes256::generate_nonce`] draws from `rand::thread_rng()`
//! (CSPRNG). For deterministic test vectors use a fixed nonce.

use crate::error::CryptoError;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// AES-256-GCM key length (32 bytes).
pub const KEY_LEN: usize = 32;
/// AES-GCM nonce length (12 bytes — the standard GCM nonce size).
pub const NONCE_LEN: usize = 12;
/// AES-GCM authentication tag length (16 bytes).
pub const TAG_LEN: usize = 16;

/// Generate a random 32-byte AES-256 key using `OsRng` (CSPRNG).
///
/// Wraps `<Key<Aes256Gcm> as KeyInit>::generate(&mut OsRng)`. Returns
/// an owned `Vec<u8>` of length 32. NEVER fails (`OsRng::fill_bytes`
/// is infallible on all platforms Buff supports).
pub fn generate_key() -> Vec<u8> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let key = Aes256Gcm::generate_key(&mut OsRng);
        key.to_vec()
    }));
    result.unwrap_or_default()
}

/// Generate a random 12-byte GCM nonce using `OsRng` (CSPRNG).
///
/// Wraps `<Nonce<Aes256Gcm>> ::generate(&mut OsRng)`. Returns an
/// owned `Vec<u8>` of length 12. NEVER fails.
pub fn generate_nonce() -> Vec<u8> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        nonce.to_vec()
    }));
    result.unwrap_or_default()
}

/// Encrypt `plaintext` with AES-256-GCM under `(key, nonce)`.
///
/// `key` MUST be exactly 32 bytes; `nonce` MUST be exactly 12 bytes.
/// Returns `ciphertext || tag` (length = `plaintext.len() + 16`).
///
/// On any failure (wrong length, AES engine error, panic) returns
/// [`CryptoError`]. The body is wrapped in `catch_unwind` per T4 FFI
/// guide R6 so a panic never propagates across the boundary.
pub fn encrypt(key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if key.len() != KEY_LEN {
        return Err(CryptoError::InvalidLength {
            what: "aes-256 key",
            expected: KEY_LEN,
            got: key.len(),
        });
    }
    if nonce.len() != NONCE_LEN {
        return Err(CryptoError::InvalidLength {
            what: "aes-gcm nonce",
            expected: NONCE_LEN,
            got: nonce.len(),
        });
    }
    let key_owned = key.to_vec();
    let nonce_owned = nonce.to_vec();
    let pt_owned = plaintext.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_owned));
        let nonce = Nonce::<Aes256Gcm>::from_slice(&nonce_owned);
        cipher.encrypt(nonce, pt_owned.as_ref())
    }));
    match result {
        Ok(Ok(ct)) => Ok(ct),
        Ok(Err(e)) => Err(CryptoError::from(e)),
        Err(_) => Err(CryptoError::Panic),
    }
}

/// Decrypt `ciphertext || tag` with AES-256-GCM under `(key, nonce)`.
///
/// `ciphertext` MUST be at least 16 bytes (the trailing GCM tag).
/// Returns the recovered plaintext (length = `ciphertext.len() - 16`).
///
/// Authentication-tag mismatch → [`CryptoError::Aes`]. NEVER panics.
pub fn decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if key.len() != KEY_LEN {
        return Err(CryptoError::InvalidLength {
            what: "aes-256 key",
            expected: KEY_LEN,
            got: key.len(),
        });
    }
    if nonce.len() != NONCE_LEN {
        return Err(CryptoError::InvalidLength {
            what: "aes-gcm nonce",
            expected: NONCE_LEN,
            got: nonce.len(),
        });
    }
    if ciphertext.len() < TAG_LEN {
        return Err(CryptoError::InvalidLength {
            what: "aes-gcm ciphertext",
            expected: TAG_LEN,
            got: ciphertext.len(),
        });
    }
    let key_owned = key.to_vec();
    let nonce_owned = nonce.to_vec();
    let ct_owned = ciphertext.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_owned));
        let nonce = Nonce::<Aes256Gcm>::from_slice(&nonce_owned);
        cipher.decrypt(nonce, ct_owned.as_ref())
    }));
    match result {
        Ok(Ok(pt)) => Ok(pt),
        Ok(Err(e)) => Err(CryptoError::from(e)),
        Err(_) => Err(CryptoError::Panic),
    }
}
