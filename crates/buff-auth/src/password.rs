//! Password hashing (Argon2id PHC string format — pure-Rust, NO `ring`).

use std::panic::{catch_unwind, AssertUnwindSafe};

// `rand_core::OsRng` is re-exported by `password_hash` so we use the
// OS-provided CSPRNG without pulling in a separate `rand` workspace
// dep (the workspace `rand` 0.9 pin uses `rand_core` 0.10, which is
// NOT the `rand_core` 0.6 that `password-hash` 0.5 requires).
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

use crate::error::AuthError;

/// Hash a plaintext password using Argon2id with a random 22-char salt.
///
/// Returns the canonical PHC string (`$argon2id$v=19$m=...,t=...,p=...$<salt>$<hash>`)
/// ready for storage in a user database. Wraps `argon2::Argon2::hash_password`
/// in `catch_unwind` per FFI guide R6. Uses the default Argon2id params
/// (m=19456 KiB, t=2, p=1) — these match OWASP's 2024 minimum recommendation.
pub fn password_hash(plain: &str) -> Result<String, AuthError> {
    let plain_owned = plain.to_string();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2.hash_password(plain_owned.as_bytes(), &salt)?;
        Ok::<String, AuthError>(hash.to_string())
    }));
    match result {
        Ok(Ok(hash)) => Ok(hash),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuthError::Panic),
    }
}

/// Verify a plaintext password against a stored PHC hash.
///
/// Returns `Ok(true)` on match, `Ok(false)` on mismatch (NEVER panics,
/// NEVER errors on plain mismatch — mirrors the T26 Signature.verify
/// stance so a future `login_allow` policy can layer cleanly). Returns
/// `Err(AuthError::PasswordHash(_))` only when the stored hash is
/// malformed (wrong algorithm, bad PHC encoding). Wraps
/// `argon2::Argon2::verify_password` in `catch_unwind` per FFI guide R6.
pub fn password_verify(plain: &str, phc_hash: &str) -> Result<bool, AuthError> {
    let plain_owned = plain.to_string();
    let hash_owned = phc_hash.to_string();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let parsed = PasswordHash::new(&hash_owned)?;
        let argon2 = Argon2::default();
        Ok::<bool, AuthError>(
            argon2
                .verify_password(plain_owned.as_bytes(), &parsed)
                .map(|_| true)
                .unwrap_or(false),
        )
    }));
    match result {
        Ok(Ok(ok)) => Ok(ok),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuthError::Panic),
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn password_hash_produces_phc_string() {
        let hash = password_hash("hunter2").expect("hash");
        assert!(hash.starts_with("$argon2id$"), "phc prefix: {hash}");
    }

    #[test]
    fn password_verify_accepts_correct_password() {
        let hash = password_hash("correct-horse").expect("hash");
        let ok = password_verify("correct-horse", &hash).expect("verify shape");
        assert!(ok);
    }

    #[test]
    fn password_verify_rejects_wrong_password() {
        let hash = password_hash("correct-horse").expect("hash");
        let ok = password_verify("battery-staple", &hash).expect("verify shape");
        assert!(!ok, "mismatch must be Ok(false), not an error");
    }

    #[test]
    fn password_verify_rejects_garbage_hash() {
        let err = password_verify("x", "not-a-phc-hash").unwrap_err();
        assert!(matches!(err, AuthError::PasswordHash(_)));
    }

    #[test]
    fn password_hash_each_call_yields_different_salt() {
        let a = password_hash("same").expect("a");
        let b = password_hash("same").expect("b");
        assert_ne!(a, b, "salts must differ between calls");
    }
}
