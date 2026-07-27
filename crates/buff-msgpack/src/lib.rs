//! `buff-msgpack` — MessagePack binary format for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`rmp-serde`](https://docs.rs/rmp-serde) crate.
//! Provides `MsgPack.serialize(value) -> Bytes` and
//! `MsgPack.deserialize(bytes) -> Value` via a safe FFI boundary per the
//! T4 FFI guide.
//!
//! # Pipeline
//!
//! ```text
//!   MsgPack.serialize(value) ──▶ rmp_serde::to_vec(&value) ──▶ Vec<u8>
//!   MsgPack.deserialize(bytes) ──▶ rmp_serde::from_slice(bytes) ──▶ Value
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Vec<u8>`, `serde_json::Value`, `MsgPackError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `serialize` returns owned `Vec<u8>`. `deserialize` returns owned `Value`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, MsgPackError>`. `rmp_serde::encode::Error` / `rmp_serde::decode::Error` mapped via `From`. |
//! | R4 — Thread safety | All types are `Send + Sync`. |
//! | R5 — Lifetime hiding | No public lifetime parameters. All inputs are owned or `&[u8]` (converted to owned internally). |
//! | R6 — Panic boundary | Every public function wraps its body in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code.

use std::panic::{catch_unwind, AssertUnwindSafe};

pub mod error;
pub use error::MsgPackError;

/// Serialize a `serde_json::Value` to MessagePack binary format.
///
/// Wraps `rmp_serde::to_vec(&value)`. The body is wrapped in
/// `catch_unwind` per T4 FFI guide R6 so a panic in the codec
/// becomes a stable `Err(MsgPackError::Panic)` instead of process
/// abort.
pub fn serialize(value: &serde_json::Value) -> Result<Vec<u8>, MsgPackError> {
    let value_owned = value.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        rmp_serde::to_vec(&value_owned).map_err(MsgPackError::from)
    }));
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(MsgPackError::Panic),
    }
}

/// Deserialize a `serde_json::Value` from MessagePack binary data.
///
/// Wraps `rmp_serde::from_slice::<serde_json::Value>(bytes)`. The body
/// is wrapped in `catch_unwind` per T4 FFI guide R6.
pub fn deserialize(bytes: &[u8]) -> Result<serde_json::Value, MsgPackError> {
    if bytes.is_empty() {
        return Err(MsgPackError::EmptyBuffer);
    }
    let bytes_owned = bytes.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        rmp_serde::from_slice::<serde_json::Value>(&bytes_owned).map_err(MsgPackError::from)
    }));
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(MsgPackError::Panic),
    }
}

/// Roundtrip: serialize a value to MessagePack and deserialize it back.
/// Returns `None` if either step fails.
pub fn roundtrip(value: &serde_json::Value) -> Option<serde_json::Value> {
    let bytes = serialize(value).ok()?;
    deserialize(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialize_integer() {
        let value = json!(42);
        let bytes = serialize(&value).expect("serialize");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_string() {
        let value = json!("hello");
        let bytes = serialize(&value).expect("serialize");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_array() {
        let value = json!([1, 2, 3, "four", true]);
        let bytes = serialize(&value).expect("serialize");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_object() {
        let value = json!({"name": "Buff", "version": 1, "features": ["msgpack"]});
        let bytes = serialize(&value).expect("serialize");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_null() {
        let value = json!(null);
        let bytes = serialize(&value).expect("serialize");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn deserialize_integer() {
        let bytes = rmp_serde::to_vec(&json!(42)).expect("rmp encode");
        let value = deserialize(&bytes).expect("deserialize");
        assert_eq!(value, json!(42));
    }

    #[test]
    fn deserialize_string() {
        let bytes = rmp_serde::to_vec(&json!("hello")).expect("rmp encode");
        let value = deserialize(&bytes).expect("deserialize");
        assert_eq!(value, json!("hello"));
    }

    #[test]
    fn deserialize_array() {
        let expected = json!([1, 2, 3, "four", true]);
        let bytes = rmp_serde::to_vec(&expected).expect("rmp encode");
        let value = deserialize(&bytes).expect("deserialize");
        assert_eq!(value, expected);
    }

    #[test]
    fn deserialize_object() {
        let expected = json!({"name": "Buff", "version": 1});
        let bytes = rmp_serde::to_vec(&expected).expect("rmp encode");
        let value = deserialize(&bytes).expect("deserialize");
        assert_eq!(value, expected);
    }

    #[test]
    fn roundtrip_preserves_values() {
        let cases = vec![
            json!(null),
            json!(true),
            json!(false),
            json!(42),
            json!(-1),
            json!(3.14),
            json!("hello world"),
            json!([1, 2, 3]),
            json!({"a": 1, "b": [2, 3]}),
        ];
        for case in cases {
            let result = roundtrip(&case);
            assert_eq!(result, Some(case));
        }
    }

    #[test]
    fn deserialize_empty_buffer_rejected() {
        let err = deserialize(&[]).unwrap_err();
        assert!(matches!(err, MsgPackError::EmptyBuffer));
    }

    #[test]
    fn deserialize_garbage_rejected() {
        let err = deserialize(b"not msgpack data").unwrap_err();
        assert!(matches!(err, MsgPackError::Decode(_)));
    }

    #[test]
    fn serialize_nested_object() {
        let value = json!({
            "level1": {
                "level2": {
                    "level3": [1, 2, 3]
                }
            }
        });
        let bytes = serialize(&value).expect("serialize nested");
        let back = deserialize(&bytes).expect("deserialize nested");
        assert_eq!(value, back);
    }

    #[test]
    fn serialize_large_array() {
        let arr: Vec<serde_json::Value> = (0..1000).map(|i| json!(i)).collect();
        let value = serde_json::Value::Array(arr);
        let bytes = serialize(&value).expect("serialize large");
        let back = deserialize(&bytes).expect("deserialize large");
        assert_eq!(value, back);
    }
}
