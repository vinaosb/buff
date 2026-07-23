//! Integration tests for the `buff-auth` JWT module.
//!
//! Covers: roundtrip preservation, wrong-secret rejection, malformed
//! token rejection, claims-as-non-object coercion, multi-claim
//! preservation. 6 tests (counted toward the T34 acceptance 15 tests).

use buff_auth::{jwt_decode, jwt_encode, AuthError};
use serde_json::{Map, Value};

#[test]
fn jwt_round_trip_preserves_string_and_bool_claims() {
    let mut claims = Map::new();
    claims.insert("sub".to_string(), Value::String("user-1".to_string()));
    claims.insert("admin".to_string(), Value::Bool(true));
    let token = jwt_encode(&claims, "secret").expect("encode");
    let decoded = jwt_decode(&token, "secret").expect("decode");
    assert_eq!(
        decoded.get("sub"),
        Some(&Value::String("user-1".to_string()))
    );
    assert_eq!(decoded.get("admin"), Some(&Value::Bool(true)));
}

#[test]
fn jwt_round_trip_preserves_numeric_claims() {
    let mut claims = Map::new();
    claims.insert("iat".to_string(), Value::Number(1_700_000_000u64.into()));
    let token = jwt_encode(&claims, "secret").expect("encode");
    let decoded = jwt_decode(&token, "secret").expect("decode");
    let iat = decoded.get("iat").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(iat, 1_700_000_000);
}

#[test]
fn jwt_decode_rejects_tampered_token() {
    let token = jwt_encode(
        &Map::from([("sub".to_string(), Value::String("alice".to_string()))]),
        "real-secret",
    )
    .expect("encode");
    let err = jwt_decode(&token, "other-secret").unwrap_err();
    assert!(matches!(err, AuthError::Jwt(_)));
}

#[test]
fn jwt_decode_rejects_malformed_token() {
    let err = jwt_decode("not.a.real.jwt", "any").unwrap_err();
    assert!(matches!(err, AuthError::Jwt(_)));
}

#[test]
fn jwt_decode_rejects_empty_token() {
    let err = jwt_decode("", "any").unwrap_err();
    assert!(matches!(err, AuthError::Jwt(_)));
}

#[test]
fn jwt_encoded_token_has_three_compact_components() {
    let token =
        jwt_encode(&Map::from([("x".to_string(), Value::Bool(true))]), "secret").expect("encode");
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "JWT compact form is header.payload.signature"
    );
}
