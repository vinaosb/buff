//! Integration tests for the `buff-auth` Password module (Argon2id).
//!
//! Covers: hash shape, verify correct, verify wrong (Ok(false)),
//! malformed hash rejection, salt uniqueness across calls. 5 tests
//! (counted toward the T34 acceptance 15 tests).

use buff_auth::{password_hash, password_verify, AuthError};

#[test]
fn password_hash_returns_phc_format() {
    let hash = password_hash("hunter2").expect("hash");
    assert!(
        hash.starts_with("$argon2id$"),
        "expected PHC prefix, got: {hash}"
    );
    let parts: Vec<&str> = hash.split('$').filter(|s| !s.is_empty()).collect();
    assert!(parts.len() >= 4, "PHC has algorithm / params / salt / hash");
}

#[test]
fn password_verify_accepts_correct_password() {
    let hash = password_hash("correct horse battery staple").expect("hash");
    let ok = password_verify("correct horse battery staple", &hash).expect("verify shape");
    assert!(ok);
}

#[test]
fn password_verify_returns_false_for_wrong_password() {
    let hash = password_hash("correct horse battery staple").expect("hash");
    let ok = password_verify("battery staple correct horse", &hash).expect("verify shape");
    assert!(!ok, "wrong password must be Ok(false), NOT an Err");
}

#[test]
fn password_verify_errors_on_malformed_hash() {
    let err = password_verify("any", "not-a-real-phc-hash").unwrap_err();
    assert!(matches!(err, AuthError::PasswordHash(_)));
}

#[test]
fn password_hash_uses_fresh_salt_each_call() {
    let a = password_hash("same").expect("a");
    let b = password_hash("same").expect("b");
    assert_ne!(a, b, "salt must differ between calls");
}
