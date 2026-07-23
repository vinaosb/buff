//! JWT encode/decode via `jsonwebtoken` (rust_crypto backend — pure-Rust, NO `ring`).

use std::panic::{catch_unwind, AssertUnwindSafe};

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde_json::{Map, Value};

use crate::error::AuthError;

/// Encode a JWT signed with HS256 (HMAC-SHA256, the default JWT algorithm).
///
/// `claims` is a heterogeneous JSON-like map (the typical JWT shape);
/// `secret` is the shared HMAC secret. Returns the compact JWS string
/// (`header.payload.signature`). Wraps `jsonwebtoken::encode` in
/// `catch_unwind` per FFI guide R6.
pub fn jwt_encode(claims: &Map<String, Value>, secret: &str) -> Result<String, AuthError> {
    let claims_owned = claims.clone();
    let secret_owned = secret.to_string();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let header = Header::new(Algorithm::HS256);
        let key = EncodingKey::from_secret(secret_owned.as_bytes());
        let value = Value::Object(claims_owned);
        let token = encode(&header, &value, &key)?;
        Ok::<String, AuthError>(token)
    }));
    match result {
        Ok(Ok(token)) => Ok(token),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuthError::Panic),
    }
}

/// Decode + signature-verify a JWT signed with HS256.
///
/// Returns the claims as a heterogeneous JSON-like map (the typical JWT
/// shape). On signature failure / expiry / malformed token, returns
/// `Err(AuthError::Jwt(_))` (NEVER panics). Wraps `jsonwebtoken::decode`
/// in `catch_unwind` per FFI guide R6.
///
/// MVP validation policy: HS256 algorithm + no `exp` enforcement (the
/// caller controls whether to populate `exp`; the wrapper trusts the
/// caller's policy). A future task can expose `Validation` builder
/// surface for `exp`/`iss`/`aud` validation.
pub fn jwt_decode(token: &str, secret: &str) -> Result<Map<String, Value>, AuthError> {
    let token_owned = token.to_string();
    let secret_owned = secret.to_string();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let key = DecodingKey::from_secret(secret_owned.as_bytes());
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
        let data = decode::<Value>(&token_owned, &key, &validation)?;
        match data.claims {
            Value::Object(map) => Ok::<Map<String, Value>, AuthError>(map),
            other => Ok::<Map<String, Value>, AuthError>({
                let mut single = Map::new();
                single.insert("value".to_string(), other);
                single
            }),
        }
    }));
    match result {
        Ok(Ok(map)) => Ok(map),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(AuthError::Panic),
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn jwt_round_trip_preserves_claims() {
        let mut claims = Map::new();
        claims.insert("sub".to_string(), Value::String("alice".to_string()));
        claims.insert("admin".to_string(), Value::Bool(true));
        let secret = "top-secret";
        let token = jwt_encode(&claims, secret).expect("encode");
        let decoded = jwt_decode(&token, secret).expect("decode");
        assert_eq!(
            decoded.get("sub"),
            Some(&Value::String("alice".to_string()))
        );
        assert_eq!(decoded.get("admin"), Some(&Value::Bool(true)));
    }

    #[test]
    fn jwt_decode_rejects_wrong_secret() {
        let mut claims = Map::new();
        claims.insert("sub".to_string(), Value::String("bob".to_string()));
        let token = jwt_encode(&claims, "secret-a").expect("encode");
        let err = jwt_decode(&token, "secret-b").unwrap_err();
        assert!(matches!(err, AuthError::Jwt(_)));
    }

    #[test]
    fn jwt_decode_rejects_garbage() {
        let err = jwt_decode("not.a.jwt", "any").unwrap_err();
        assert!(matches!(err, AuthError::Jwt(_)));
    }
}
