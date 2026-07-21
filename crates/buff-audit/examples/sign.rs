// T26 example: sign a payload, then verify it.
//
// Demonstrates the full Ed25519 sign/verify roundtrip via the
// `Signature.keypair()` / `Signature.sign(data, key)` /
// `Signature.verify(data, sig, key)` surface. Generates a fresh
// keypair from the OS CSPRNG, signs a hello-world payload, prints
// the signature, then re-verifies it against the same public key.

use buff_audit::{keypair, sign, verify};

fn main() {
    let (public_hex, secret_hex) = keypair().expect("keypair");
    println!("public_key = {} ({} chars)", public_hex, public_hex.len());
    println!("secret_key = {} ({} chars)", secret_hex, secret_hex.len());

    let payload = b"hello, audit";
    let sig_hex = sign(payload, &secret_hex).expect("sign");
    println!("signature = {} ({} chars)", sig_hex, sig_hex.len());

    let ok = verify(payload, &sig_hex, &public_hex).expect("verify shape");
    println!("verify original = {}", ok);
    assert!(ok, "fresh signature must verify");

    let tampered = verify(b"TAMPERED", &sig_hex, &public_hex).expect("verify shape");
    println!("verify tampered = {}", tampered);
    assert!(!tampered, "tampered data must NOT verify");
}
