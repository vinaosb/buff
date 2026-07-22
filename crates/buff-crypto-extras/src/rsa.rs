//! RSA PKCS#1 v1.5 sign / verify with SHA-256.
//!
//! RSA is exposed ONLY for digital signatures (sign + verify + keypair
//! generation). Encryption (RSAES-PKCS1-v1.5 / RSAES-OAEP) is
//! deliberately NOT exposed because the T49 spec scopes RSA to
//! signatures; for public-key encryption use ECIES (future task) or
//! hybrid AES-GCM + ECDH (see [`crate::ecc`]).
//!
//! PKCS#1 v1.5 with SHA-256 is the deterministic, baseline signature
//! scheme with the widest cross-language support (OpenSSL /
//! pycryptodome / BouncyCastle / System.Security.Cryptography all
//! ship it in stdlib). RSA-PSS would also be acceptable but is left
//! for a follow-up.
//!
//! # PEM wire format
//!
//! [`generate_keypair`] returns Spki (public) + PKCS#8 (private) PEM
//! strings, the same shape `openssl genrsa` / `openssl rsa -pubout`
//! produce. The Buff surface exchanges keys as PEM strings (not raw
//! DER bytes) because the PEM armor (`-----BEGIN PUBLIC KEY-----`)
//! is the universal RSA key serialization across all 6 of the T49
//! cross-language targets.

use crate::error::CryptoError;
use rand::rngs::OsRng;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::{DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use rsa::pkcs1v15::{SigningKey, VerifyingKey};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;
use signature::{RandomizedSigner, Verifier};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Minimum acceptable RSA modulus size (bits). 2048 is the current
/// NIST SP 800-57 floor for "use beyond 2030"; we reject anything
/// below it.
pub const MIN_BITS: usize = 2048;

/// An RSA keypair — `(public_pem, private_pem)`.
///
/// Derives `Default` (both PEM strings default to empty) so the
/// codegen-lowered `RSA.generate_keypair(bits)` call can collapse
/// the `Result<RsaKeypair, CryptoError>` to a default value on
/// failure via `.unwrap_or_default()` — matching Buff's "no
/// panicking generated code" rule (mirrors T48 buff_web3::Wallet +
/// T48 buff_web3::Provider's Default-impl precedent). NEVER use the
/// Default value in production — an empty PEM string is a malformed
/// key; the default exists solely as a panic-free failure fallback.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RsaKeypair {
    pub public_pem: String,
    pub private_pem: String,
}

/// Generate a fresh RSA keypair of `bits` modulus size.
///
/// `bits` MUST be ≥ [`MIN_BITS`] (2048) and a multiple of 8. Returns
/// a [`RsaKeypair`] with both PEM strings populated. Computationally
/// expensive (~100ms for 2048-bit, ~1s for 4096-bit on a modern CPU).
pub fn generate_keypair(bits: usize) -> Result<RsaKeypair, CryptoError> {
    if bits < MIN_BITS {
        return Err(CryptoError::InvalidLength {
            what: "rsa modulus bits",
            expected: MIN_BITS,
            got: bits,
        });
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut rng = OsRng;
        let private = RsaPrivateKey::new(&mut rng, bits)?;
        let public = RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)?
            .as_bytes()
            .to_vec();
        let public_pem = public.to_public_key_pem(rsa::pkcs8::LineEnding::LF)?;
        Ok::<RsaKeypair, CryptoError>(RsaKeypair {
            public_pem: String::from_utf8(public_pem).unwrap_or_default(),
            private_pem: String::from_utf8(private_pem).unwrap_or_default(),
        })
    }));
    match result {
        Ok(Ok(kp)) => Ok(kp),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(CryptoError::Panic),
    }
}

/// Sign `data` with `private_pem` using RSASSA-PKCS1-v1.5 + SHA-256.
///
/// `private_pem` MUST be a valid PKCS#8 PEM string. Returns the raw
/// signature bytes (length = modulus length: 256 bytes for 2048-bit,
/// 512 bytes for 4096-bit). Empty Vec on failure (NEVER panics).
pub fn sign(private_pem: &str, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let pem_owned = private_pem.to_string();
    let data_owned = data.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let private_key =
            RsaPrivateKey::from_pkcs8_pem(&pem_owned).or_else(|_| {
                RsaPrivateKey::from_pkcs1_pem(&pem_owned).map_err(CryptoError::from)
            })?;
        let signing_key = SigningKey::<Sha256>::new(private_key);
        let mut rng = OsRng;
        let signature = signing_key.sign_with_rng(&mut rng, &data_owned);
        Ok::<Vec<u8>, CryptoError>(signature.to_bytes().into_vec())
    }));
    match result {
        Ok(Ok(sig)) => Ok(sig),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(CryptoError::Panic),
    }
}

/// Verify a PKCS#1 v1.5 SHA-256 `signature` against `data` under
/// `public_pem`.
///
/// Returns `true` iff the signature is valid. Returns `false` (NEVER
/// an error) on signature mismatch, malformed PEM, or panic. The
/// `false`-on-malformed-input stance mirrors the T26 Signature.verify
/// + T34 Password.verify stance so a future `verify_allow` policy
/// can layer cleanly.
pub fn verify(public_pem: &str, data: &[u8], signature: &[u8]) -> bool {
    let pem_owned = public_pem.to_string();
    let data_owned = data.to_vec();
    let sig_owned = signature.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<bool, CryptoError> {
        let public_key = RsaPublicKey::from_public_key_pem(&pem_owned)?;
        let verifying_key = VerifyingKey::<Sha256>::new(public_key);
        let sig = rsa::pkcs1v15::Signature::from_bytes(&sig_owned)?;
        Ok(verifying_key.verify(&data_owned, &sig).is_ok())
    }));
    matches!(result, Ok(Ok(true)))
}
