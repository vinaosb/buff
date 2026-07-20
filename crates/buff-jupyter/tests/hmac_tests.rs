//! Integration tests for the HMAC signing module — sign/verify
//! round-trip + the unsigned-mode fallback.
//!
//! These tests run WITHOUT a live ZMQ socket — they exercise the
//! pure crypto layer directly.

use buff_jupyter::hmac::{sign, verify};

#[test]
fn sign_then_verify_round_trips() {
    let key = "a-session-key";
    let frames = vec![
        b"{\"msg_id\":\"x\"}".to_vec(),
        b"{}".to_vec(),
        b"{}".to_vec(),
        b"{}".to_vec(),
    ];
    let signature = sign(key, &frames);
    assert!(!signature.is_empty());
    assert_eq!(signature.len(), 64); // SHA-256 hex
    verify(key, &frames, &signature).expect("signature must verify");
}

#[test]
fn verify_rejects_tampered_frames() {
    let key = "k";
    let frames = vec![
        b"hdr".to_vec(),
        b"prn".to_vec(),
        b"meta".to_vec(),
        b"cnt".to_vec(),
    ];
    let sig = sign(key, &frames);

    // Tamper with each frame in turn — signature must fail.
    for i in 0..frames.len() {
        let mut tampered = frames.clone();
        tampered[i] = b"TAMPERED".to_vec();
        assert!(
            verify(key, &tampered, &sig).is_err(),
            "frame {i} tamper should fail verification"
        );
    }
}

#[test]
fn verify_rejects_wrong_key() {
    let frames = vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()];
    let sig = sign("real-key", &frames);
    assert!(verify("wrong-key", &frames, &sig).is_err());
}

#[test]
fn unsigned_mode_allows_empty_signature_when_key_empty() {
    let frames = vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()];
    assert!(verify("", &frames, "").is_ok());
}

#[test]
fn unsigned_mode_rejects_nonempty_signature_when_key_empty() {
    let frames = vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()];
    assert!(verify("", &frames, "deadbeef").is_err());
}

#[test]
fn signing_is_lowercase_hex_sha256_length() {
    let key = "another-key";
    let frames = vec![b"x".to_vec(), b"y".to_vec(), b"z".to_vec(), b"w".to_vec()];
    let sig = sign(key, &frames);
    assert_eq!(sig.len(), 64, "SHA-256 hex is 64 chars");
    assert!(
        sig.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "must be lowercase hex, got {sig}"
    );
}

#[test]
fn empty_key_produces_empty_signature() {
    let frames = vec![b"x".to_vec()];
    assert_eq!(sign("", &frames), "");
}
