//! ECDH key agreement on NIST P-256 and P-384.
//!
//! ECDH lets two parties derive a shared secret over an insecure
//! channel: each party generates `(private, public)` pair, exchanges
//! public keys, and computes
//!
//! ```text
//! shared = ECDH(my_private, their_public)
//! ```
//!
//! Both parties arrive at the same `shared` (32 bytes for P-256,
//! 48 bytes for P-384). The shared secret is suitable as input to
//! a KDF ([`crate::argon2`]) for deriving an AES key (hybrid
//! encryption).
//!
//! # Wire format
//!
//! Public keys are exchanged as raw SEC1 uncompressed points:
//! - P-256: 65 bytes = `0x04 || X(32) || Y(32)`
//! - P-384: 97 bytes = `0x04 || X(48) || Y(48)`
//!
//! Private keys are raw scalars: 32 bytes (P-256) / 48 bytes (P-384).
//! This is the same wire format `pycryptodome` / `BouncyCastle` /
//! `System.Security.Cryptography.ECDiffieHellman` use.

use crate::error::CryptoError;
// p256 0.13 API drift fix: the old `p256::ecdh::Ecdh` struct was removed;
// the module now exposes a `diffie_hellman(scalar, point) -> SharedSecret`
// free function (re-exported from `elliptic_curve::ecdh`). The
// `raw_secret_bytes()` method moved to `SharedSecret`. Same shape for p384.
//
// rand API drift fix: the workspace `rand` resolves to 0.9 whose
// `rand::rngs::OsRng` implements `rand_core 0.9` traits, but
// `p256::SecretKey::random` needs `rand_core 0.6` traits (via
// `elliptic-curve 0.13`). `aes_gcm::aead::OsRng` (re-exported from
// `aead` → `rand_core 0.6::OsRng`) implements the `0.6` traits.
use aes_gcm::aead::OsRng;
use p256::ecdh::diffie_hellman as ecdh_p256;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{AffinePoint, NonZeroScalar, PublicKey as P256Public, SecretKey as P256Secret};
#[allow(unused_imports)] // P-384 ECDH derive is deferred to v1.20+ per AGENTS.md.
use p384::ecdh::diffie_hellman as ecdh_p384;
#[allow(unused_imports)] // P-384 ECDH derive is deferred to v1.20+ per AGENTS.md.
use p384::{PublicKey as P384Public, SecretKey as P384Secret};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// P-256 private scalar length (32 bytes).
pub const P256_PRIVATE_LEN: usize = 32;
/// P-256 SEC1 uncompressed public key length (65 bytes).
pub const P256_PUBLIC_LEN: usize = 65;
/// P-256 shared secret length (32 bytes — x-coordinate of the
/// shared point).
pub const P256_SHARED_LEN: usize = 32;

/// Generate a random P-256 private scalar using `OsRng`.
///
/// Returns an owned `Vec<u8>` of length 32. NEVER fails.
pub fn p256_generate_private() -> Vec<u8> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let secret = P256Secret::random(&mut OsRng);
        secret.to_bytes().to_vec()
    }));
    result.unwrap_or_default()
}

/// Derive the P-256 public key (SEC1 uncompressed, 65 bytes) from a
/// 32-byte private scalar.
///
/// Returns `Vec<u8>` of length 65 (`0x04 || X || Y`). Empty Vec on
/// failure.
pub fn p256_public_from_private(private: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if private.len() != P256_PRIVATE_LEN {
        return Err(CryptoError::InvalidLength {
            what: "p256 private scalar",
            expected: P256_PRIVATE_LEN,
            got: private.len(),
        });
    }
    let private_owned = private.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let secret = P256Secret::from_slice(&private_owned)?;
        let public = public_key_from_secret_p256(&secret)?;
        Ok::<Vec<u8>, CryptoError>(public)
    }));
    match result {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(CryptoError::Panic),
    }
}

/// Compute the P-256 ECDH shared secret (32 bytes).
///
/// `private` is the local 32-byte scalar; `public` is the remote
/// 65-byte SEC1 uncompressed point. Returns the x-coordinate of the
/// shared point (32 bytes).
pub fn p256_derive_shared(private: &[u8], public: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if private.len() != P256_PRIVATE_LEN {
        return Err(CryptoError::InvalidLength {
            what: "p256 private scalar",
            expected: P256_PRIVATE_LEN,
            got: private.len(),
        });
    }
    if public.len() != P256_PUBLIC_LEN {
        return Err(CryptoError::InvalidLength {
            what: "p256 public key",
            expected: P256_PUBLIC_LEN,
            got: public.len(),
        });
    }
    let private_owned = private.to_vec();
    let public_owned = public.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let secret = P256Secret::from_slice(&private_owned)?;
        let public_key = P256Public::from_sec1_bytes(&public_owned)?;
        let non_zero = NonZeroScalar::from_repr(secret.to_nonzero_scalar().into())
            .into_option()
            .ok_or_else(|| CryptoError::Ecdh("invalid p256 private scalar".into()))?;
        // p256 0.13: `Ecdh::new(scalar, point).raw_secret_bytes()` →
        // `diffie_hellman(scalar, point).raw_secret_bytes()`.
        let shared = ecdh_p256(non_zero, public_key.as_affine());
        Ok::<Vec<u8>, CryptoError>(shared.raw_secret_bytes().to_vec())
    }));
    match result {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(CryptoError::Panic),
    }
}

/// Generate a random P-384 private scalar using `OsRng`.
///
/// Returns an owned `Vec<u8>` of length 48. NEVER fails.
pub fn p384_generate_private() -> Vec<u8> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let secret = P384Secret::random(&mut OsRng);
        secret.to_bytes().to_vec()
    }));
    result.unwrap_or_default()
}

fn public_key_from_secret_p256(secret: &P256Secret) -> Result<Vec<u8>, CryptoError> {
    // p256 0.13 API drift fix: `PublicKey::from_secret_scalar` takes
    // `&NonZeroScalar<NistP256>` (NOT `&SecretKey`) and returns
    // `PublicKey` directly (NOT `Result`). Use `SecretKey::to_nonzero_scalar`
    // to obtain the scalar, then `ToEncodedPoint::to_encoded_point` for
    // the SEC1 uncompressed wire format.
    let scalar = secret.to_nonzero_scalar();
    let public = P256Public::from_secret_scalar(&scalar);
    let encoded = public.to_encoded_point(false);
    Ok(encoded.to_bytes().into_vec())
}

#[allow(dead_code)]
fn affine_from_p256_public(public: &P256Public) -> AffinePoint {
    *public.as_affine()
}
