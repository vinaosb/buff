//! Roundtrip tests for the Ed25519 sign/verify surface (T26 spec).
//!
//! These tests exercise the core security contract: a signature
//! produced by `sign` MUST verify under `verify`, and any tampering
//! (data, signature, or public key) MUST cause `verify` to return
//! `false` without panicking. There is ONE happy path; the rest are
//! negative cases.

use buff_audit::{keypair, sign, verify};

fn fresh_keypair() -> (String, String) {
    keypair().expect("CSPRNG keypair should always succeed")
}

#[test]
fn roundtrip_sign_then_verify_succeeds() {
    let (pk, sk) = fresh_keypair();
    let data = b"the quick brown fox jumps over the lazy dog";
    let sig = sign(data, &sk).expect("sign with valid secret key");
    let ok = verify(data, &sig, &pk).expect("verify shape");
    assert!(ok, "fresh signature must verify under its own public key");
}

#[test]
fn roundtrip_works_for_empty_data() {
    let (pk, sk) = fresh_keypair();
    let sig = sign(b"", &sk).expect("sign empty");
    let ok = verify(b"", &sig, &pk).expect("verify shape");
    assert!(ok, "empty data is a legal Ed25519 input");
}

#[test]
fn roundtrip_works_for_large_data() {
    let (pk, sk) = fresh_keypair();
    let data: Vec<u8> = (0..10_000u32).map(|i| (i & 0xff) as u8).collect();
    let sig = sign(&data, &sk).expect("sign large");
    let ok = verify(&data, &sig, &pk).expect("verify shape");
    assert!(ok, "10KB data roundtrips");
}

#[test]
fn verify_rejects_tampered_data() {
    let (pk, sk) = fresh_keypair();
    let sig = sign(b"original payload", &sk).expect("sign");
    let ok = verify(b"tampered payload", &sig, &pk).expect("verify shape");
    assert!(!ok, "verify MUST reject tampered data");
}

#[test]
fn verify_rejects_tampered_signature() {
    let (pk, sk) = fresh_keypair();
    let mut sig = sign(b"data", &sk).expect("sign");
    let mut bytes = hex::decode(&sig).expect("hex");
    bytes[0] ^= 0x01;
    sig = hex::encode(&bytes);
    let ok = verify(b"data", &sig, &pk).expect("verify shape");
    assert!(!ok, "verify MUST reject tampered signature");
}

#[test]
fn verify_rejects_wrong_public_key() {
    let (pk_a, sk_a) = fresh_keypair();
    let (pk_b, _sk_b) = fresh_keypair();
    let sig = sign(b"data", &sk_a).expect("sign");
    let ok = verify(b"data", &sig, &pk_b).expect("verify shape");
    assert!(!ok, "verify MUST reject signature under wrong public key");
    let ok = verify(b"data", &sig, &pk_a).expect("verify shape");
    assert!(ok, "verify MUST accept signature under correct public key");
}

#[test]
fn signatures_are_detached_and_deterministic_per_key() {
    let (_pk, sk) = fresh_keypair();
    let sig1 = sign(b"same input", &sk).expect("sign 1");
    let sig2 = sign(b"same input", &sk).expect("sign 2");
    assert_eq!(
        sig1, sig2,
        "Ed25519 is deterministic — same key + same data => same signature"
    );
    assert_eq!(sig1.len(), 128, "signature hex is 64 bytes");
}

#[test]
fn keypair_secret_key_is_not_all_zero() {
    let (_pk, sk) = fresh_keypair();
    let bytes = hex::decode(&sk).expect("hex");
    let any_nonzero = bytes.iter().any(|b| *b != 0);
    assert!(any_nonzero, "CSPRNG key should never be all zeros");
}

#[test]
fn verify_with_invalid_hex_signature_returns_hex_error() {
    let (pk, _) = fresh_keypair();
    let err = verify(b"data", "ZZZZ", &pk).unwrap_err();
    assert!(matches!(err, buff_audit::AuditError::Hex(_)));
}
