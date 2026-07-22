//! Error type for the `buff-crypto-extras` crate.
//!
//! Every fallible operation surfaces as [`CryptoError`]. The single
//! error enum covers all four sub-modules (AES / RSA / ECDH / Argon2)
//! so the crate's public surface depends only on its own types (Buff
//! code never sees a raw `aes_gcm::*` / `rsa::*` / `p256::*` /
//! `argon2::*` error).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. Per the T4 FFI guide R6
//! (Panic Boundary) the public entry points use `catch_unwind` so
//! panics never propagate across the FFI boundary into Buff code.

use thiserror::Error;

/// The single error type returned by every fallible `buff-crypto-extras`
/// operation.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// AES-GCM encryption or decryption failed. Covers: wrong key
    /// length (must be 32 bytes for AES-256), wrong nonce length
    /// (must be 12 bytes for GCM), authentication-tag mismatch on
    /// decrypt (ciphertext was tampered or wrong key), or AES engine
    /// internal failure. Carries the underlying `aes_gcm::aead::Error`
    /// message verbatim.
    #[error("aes-gcm error: {0}")]
    Aes(String),

    /// RSA key generation, signing, or key parsing failed. Covers:
    /// weak key size (< 2048 bits), PKCS#1 v1.5 sign engine failure,
    /// invalid PEM input to `RSA.sign` / `RSA.verify`, modulus-size
    /// overflow, etc.
    #[error("rsa error: {0}")]
    Rsa(String),

    /// ECDH key agreement or point encoding/decoding failed. Covers:
    /// invalid SEC1 uncompressed point (must be 65 bytes starting
    /// with 0x04 for P-256 / 97 bytes for P-384), scalar out of
    /// range, shared-secret derivation failure (e.g. cofactor edge
    /// case), wrong curve parameter on cross-curve mix.
    #[error("ecdh error: {0}")]
    Ecdh(String),

    /// Argon2 key derivation failed. Covers: invalid params (memory
    /// cost too low, time cost zero, parallelism zero), salt length
    /// violation (Argon2 accepts 8..=2^32-1 bytes; we gate at
    /// 16..=64 in the wrapper for sane defaults).
    #[error("argon2 error: {0}")]
    Argon2(String),

    /// The user supplied an input with the wrong byte length for the
    /// requested operation (e.g. 31-byte AES key, 11-byte nonce, 64-
    /// byte P-256 public key). The `expected` field documents the
    /// canonical length so the diagnostic is actionable.
    #[error("invalid length: {what} must be {expected} bytes, got {got}")]
    InvalidLength {
        what: &'static str,
        expected: usize,
        got: usize,
    },

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: crypto operation panicked")]
    Panic,
}

impl From<aes_gcm::aead::Error> for CryptoError {
    fn from(err: aes_gcm::aead::Error) -> Self {
        CryptoError::Aes(err.to_string())
    }
}

impl From<rsa::errors::Error> for CryptoError {
    fn from(err: rsa::errors::Error) -> Self {
        CryptoError::Rsa(err.to_string())
    }
}

impl From<rsa::pkcs8::Error> for CryptoError {
    fn from(err: rsa::pkcs8::Error) -> Self {
        CryptoError::Rsa(err.to_string())
    }
}

impl From<rsa::pkcs8::spki::Error> for CryptoError {
    fn from(err: rsa::pkcs8::spki::Error) -> Self {
        CryptoError::Rsa(err.to_string())
    }
}

impl From<argon2::Error> for CryptoError {
    fn from(err: argon2::Error) -> Self {
        CryptoError::Argon2(err.to_string())
    }
}
