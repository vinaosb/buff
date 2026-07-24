//! Raw Argon2id key derivation.
//!
//! Distinct from T34's [`Password.hash`] PHC-string API (which targets
//! human password storage), this module exposes the raw Argon2id KDF
//! for deriving fixed-length symmetric keys (typically 32-byte AES
//! keys) from a password + salt. The output is the raw derived bytes
//! (NOT a PHC string), suitable for direct use as an AES-256 key.
//!
//! # Parameters
//!
//! Defaults follow OWASP Argon2id recommendations (2024):
//! - `m_cost`: 19456 KiB (~19 MiB) memory
//! - `t_cost`: 2 iterations
//! - `p_cost`: 1 thread
//! - `output_len`: 32 bytes (AES-256 key)
//!
//! These are conservative-but-fast; security-sensitive applications
//! should bump `m_cost` to 65536+ and `t_cost` to 3+.
//!
//! # Salt
//!
//! Salt length MUST be 16 bytes (the RFC 9106 recommendation). Use
//! [`generate_salt`] to draw a fresh salt from `OsRng`.

use crate::error::CryptoError;
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Output length: 32 bytes (AES-256 key).
pub const OUTPUT_LEN: usize = 32;
/// Salt length: 16 bytes (RFC 9106 recommendation).
pub const SALT_LEN: usize = 16;
/// Memory cost (KiB). OWASP Argon2id 2024 recommendation: 19456.
pub const M_COST: u32 = 19456;
/// Time cost (iterations). OWASP: 2.
pub const T_COST: u32 = 2;
/// Parallelism (lanes). OWASP: 1.
pub const P_COST: u32 = 1;

/// Generate a random 16-byte salt using `OsRng` (CSPRNG).
pub fn generate_salt() -> Vec<u8> {
    let mut salt = vec![0u8; SALT_LEN];
    rand::rng().fill_bytes(&mut salt);
    salt
}

/// Derive a 32-byte key from `password` + `salt` using Argon2id.
///
/// `salt` MUST be exactly 16 bytes (use [`generate_salt`]). Returns
/// an owned `Vec<u8>` of length 32 suitable for direct use as an
/// AES-256 key. NEVER panics.
pub fn derive_key(password: &str, salt: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if salt.len() != SALT_LEN {
        return Err(CryptoError::InvalidLength {
            what: "argon2 salt",
            expected: SALT_LEN,
            got: salt.len(),
        });
    }
    let pw_owned = password.as_bytes().to_vec();
    let salt_owned = salt.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let params = Params::new(M_COST, T_COST, P_COST, Some(OUTPUT_LEN))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut out = [0u8; OUTPUT_LEN];
        argon2.hash_password_into(&pw_owned, &salt_owned, &mut out)?;
        Ok::<Vec<u8>, CryptoError>(out.to_vec())
    }));
    match result {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(CryptoError::Panic),
    }
}
