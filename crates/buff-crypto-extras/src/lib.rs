//! `buff-crypto-extras` — AES-GCM / RSA / ECDH / Argon2 KDF for Buff.
//!
//! Extends T124k Hash/HMAC with symmetric + asymmetric encryption
//! plus a raw Argon2 KDF (distinct from T34's PHC-string Password
//! hashing). All backends are pure-Rust RustCrypto crates via a safe
//! FFI boundary per `crates/buff-lang-ffi-guide/GUIDE.md`.
//!
//! # Pipeline
//!
//! ```text
//!   AES.encrypt(key, nonce, pt)   ─▶ ciphertext || tag (16B appended)
//!   AES.decrypt(key, nonce, ct)   ─▶ plaintext (or Aes error on tag mismatch)
//!
//!   RSA.generate_keypair(bits)    ─▶ RsaKeypair { public_pem, private_pem }
//!   RSA.sign(private_pem, data)   ─▶ PKCS#1 v1.5 SHA-256 signature bytes
//!   RSA.verify(public_pem, d, sig)─▶ Bool (false on any failure)
//!
//!   ECDH.generate_private()       ─▶ 32-byte P-256 scalar
//!   ECDH.public_from_private(sk)  ─▶ 65-byte SEC1 uncompressed point
//!   ECDH.derive_shared(sk, pk)    ─▶ 32-byte shared secret (x-coord)
//!
//!   Argon2.generate_salt()        ─▶ 16-byte CSPRNG salt
//!   Argon2.derive_key(pw, salt)   ─▶ 32-byte AES-256 key
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only owned `Vec<u8>` / `String` / `RsaKeypair` / `CryptoError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | Every fn returns owned `Vec<u8>` / `String` / `RsaKeypair`. No borrowed slice crosses the boundary. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, CryptoError>`. `verify` collapses failures to `false` (mirrors T26 / T34). |
//! | R4 — Thread safety | All public types are `Send + Sync` (own only `String` / `Vec<u8>`). |
//! | R5 — Lifetime hiding | No public lifetime parameters anywhere. |
//! | R6 — Panic boundary | Every fn wraps its body in `catch_unwind` (panic → `CryptoError::Panic` or `false` for verify). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Length-checked input returns `Result`.

pub mod aes;
pub mod argon2;
pub mod ecc;
pub mod error;

pub use error::CryptoError;

/// Convenience re-export of the AES-256-GCM surface (4 fns).
pub mod aes_gcm_api {
    pub use crate::aes::{decrypt, encrypt, generate_key, generate_nonce};
}

/// Convenience re-export of the RSA sign/verify surface (3 fns + 1 type).
pub mod rsa_api {
    pub use crate::rsa::{generate_keypair, sign, verify, RsaKeypair};
}

/// Convenience re-export of the ECDH P-256 surface (4 fns).
pub mod ecdh_api {
    pub use crate::ecc::{
        p256_derive_shared, p256_generate_private, p256_public_from_private,
        p384_generate_private,
    };
}

/// Convenience re-export of the Argon2 KDF surface (2 fns).
pub mod argon2_api {
    pub use crate::argon2::{derive_key, generate_salt};
}
