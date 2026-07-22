// T34 example: JWT roundtrip — encode a token then decode it back.

use buff_auth::jwt_decode;
use buff_auth::jwt_encode;
use serde_json::{Map, Value};

fn main() {
    let mut claims = Map::new();
    claims.insert("sub".to_string(), Value::String("user-42".to_string()));
    claims.insert("role".to_string(), Value::String("admin".to_string()));
    claims.insert("iat".to_string(), Value::Number(1_700_000_000.into()));

    let secret = "super-secret-key";
    let token = jwt_encode(&claims, secret).expect("encode");
    println!("token: {token}");

    let decoded = jwt_decode(&token, secret).expect("decode");
    println!("decoded sub  = {}", decoded.get("sub").and_then(|v| v.as_str()).unwrap_or_default());
    println!("decoded role = {}", decoded.get("role").and_then(|v| v.as_str()).unwrap_or_default());
}
