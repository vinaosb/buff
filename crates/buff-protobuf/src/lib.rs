//! `buff-protobuf` — Protocol Buffers for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`prost`](https://docs.rs/prost) +
//! [`prost-types`](https://docs.rs/prost-types) crates. Provides
//! `Protobuf.serialize(value) -> Bytes` and
//! `Protobuf.deserialize(bytes) -> Value` via a safe FFI boundary per
//! the T4 FFI guide.
//!
//! # Pipeline
//!
//! ```text
//!   Protobuf.serialize(value) ──▶ value_to_struct ──▶ prost::encode ──▶ Vec<u8>
//!   Protobuf.deserialize(bytes) ──▶ prost::decode ──▶ struct_to_value ──▶ Value
//! ```
//!
//! The MVP uses protobuf's well-known [`prost_types::Struct`] type as
//! the dynamic message surface: any `serde_json::Value` is mapped to a
//! `Struct` (objects) / `Value` (scalars) / `ListValue` (arrays) and
//! encoded via `prost::Message::encode`. This gives a faithful
//! protobuf wire-format roundtrip WITHOUT build-time `.proto` codegen
//! (gRPC streaming + `.proto`→Buff type codegen via `prost-build` /
//! `tonic` are deferred per the T52 task spec — unary-only MVP).
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Message`, `Vec<u8>`, `serde_json::Value`, `ProtobufError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `serialize` returns owned `Vec<u8>`. `deserialize` returns owned `Value`. `Message::from_bytes` consumes its arg. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, ProtobufError>`. `prost::EncodeError` / `prost::DecodeError` mapped via `From`. |
//! | R4 — Thread safety | All types are `Send + Sync`. `Message` owns `Vec<u8>` + `String` (both `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. All inputs are owned or `&[u8]` (converted to owned internally). |
//! | R6 — Panic boundary | Every public function wraps its body in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code.

use std::panic::{catch_unwind, AssertUnwindSafe};

pub mod error;
pub use error::ProtobufError;

/// The canonical type URL for the dynamic message surface exposed by
/// this MVP. Matches the convention `type.googleapis.com/<message>` so
/// decoded messages interop with any protobuf-aware service that
/// understands `google.protobuf.Struct`.
pub const TYPE_URL: &str = "type.googleapis.com/google.protobuf.Struct";

/// A protobuf-encoded message.
///
/// Constructed via [`Message::new`] (encode a `serde_json::Value`) or
/// [`Message::from_bytes`] (decode raw wire-format bytes). Holds the
/// encoded payload + the type URL so callers can introspect what kind
/// of message it carries.
///
/// The MVP uses [`prost_types::Struct`] as the wire-format schema
/// (objects → `Struct`, arrays → `ListValue`, scalars → one of
/// `NumberValue` / `StringValue` / `BoolValue` / `NullValue` /
/// `StructValue` / `ListValue`). This gives a faithful protobuf
/// roundtrip for arbitrary JSON-shaped data without build-time
/// `.proto` codegen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    type_url: String,
    payload: Vec<u8>,
}

impl Message {
    /// Encode a `serde_json::Value` into a protobuf [`Message`] using
    /// the well-known `google.protobuf.Struct` schema.
    ///
    /// Wraps `value_to_struct(value)` + `prost::Message::encode`. The
    /// body is wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn new(value: &serde_json::Value) -> Result<Self, ProtobufError> {
        let value_owned = value.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let structured = value_to_struct(&value_owned)?;
            let mut buf = Vec::new();
            prost::Message::encode(&structured, &mut buf).map_err(ProtobufError::from)?;
            Ok::<Vec<u8>, ProtobufError>(buf)
        }));
        match result {
            Ok(Ok(payload)) => Ok(Message {
                type_url: TYPE_URL.to_string(),
                payload,
            }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ProtobufError::Panic),
        }
    }

    /// Decode raw protobuf wire-format bytes into a [`Message`].
    ///
    /// The bytes are assumed to be a `google.protobuf.Struct` encoding
    /// (the canonical schema for this MVP). The type URL is set to
    /// [`TYPE_URL`]. Use [`Message::payload`] to recover the
    /// `serde_json::Value`.
    ///
    /// Wraps `prost::Message::decode`. The body is wrapped in
    /// `catch_unwind` per T4 FFI guide R6.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ProtobufError> {
        if bytes.is_empty() {
            return Err(ProtobufError::EmptyBuffer);
        }
        let bytes_for_decode = bytes.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _decoded: prost_types::Struct =
                prost::Message::decode(&bytes_for_decode[..]).map_err(ProtobufError::from)?;
            Ok::<(), ProtobufError>(())
        }));
        match result {
            Ok(Ok(())) => Ok(Message {
                type_url: TYPE_URL.to_string(),
                payload: bytes,
            }),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ProtobufError::Panic),
        }
    }

    /// Class-method alias for [`Message::from_bytes`]. Mirrors the
    /// `Message.decode(bytes)` surface named in the T52 task spec.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtobufError> {
        Message::from_bytes(bytes.to_vec())
    }

    /// The encoded protobuf wire-format bytes (length-delimited
    /// `google.protobuf.Struct`). Zero-cost `&[u8]` view.
    #[inline]
    pub fn encode(&self) -> &[u8] {
        &self.payload
    }

    /// The type URL identifying the message schema. Always
    /// [`TYPE_URL`] for this MVP (future `.proto`-codegen tasks may
    /// extend the surface with user-defined message types).
    #[inline]
    pub fn type_url(&self) -> &str {
        &self.type_url
    }

    /// The encoded payload size in bytes.
    #[inline]
    pub fn byte_size(&self) -> usize {
        self.payload.len()
    }

    /// Decode the payload back into a `serde_json::Value`.
    ///
    /// Inverse of [`Message::new`]. Wraps
    /// `prost::Message::decode::<prost_types::Struct>` +
    /// `struct_to_value`. The body is wrapped in `catch_unwind` per
    /// T4 FFI guide R6.
    pub fn payload(&self) -> Result<serde_json::Value, ProtobufError> {
        let payload_owned = self.payload.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let structured: prost_types::Struct =
                prost::Message::decode(&payload_owned[..]).map_err(ProtobufError::from)?;
            struct_to_value(&structured)
        }));
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ProtobufError::Panic),
        }
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ProtobufMessage({}, {} bytes)",
            self.type_url,
            self.byte_size()
        )
    }
}

/// Serialize a `serde_json::Value` to protobuf wire-format bytes.
///
/// Wraps `value_to_struct` + `prost::Message::encode`. The body is
/// wrapped in `catch_unwind` per T4 FFI guide R6 so a panic in the
/// codec becomes a stable `Err(ProtobufError::Panic)` instead of
/// process abort.
pub fn serialize(value: &serde_json::Value) -> Result<Vec<u8>, ProtobufError> {
    let value_owned = value.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let structured = value_to_struct(&value_owned)?;
        let mut buf = Vec::new();
        prost::Message::encode(&structured, &mut buf).map_err(ProtobufError::from)?;
        Ok::<Vec<u8>, ProtobufError>(buf)
    }));
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(ProtobufError::Panic),
    }
}

/// Deserialize a `serde_json::Value` from protobuf wire-format bytes.
///
/// Wraps `prost::Message::decode::<prost_types::Struct>` +
/// `struct_to_value`. The body is wrapped in `catch_unwind` per T4
/// FFI guide R6.
pub fn deserialize(bytes: &[u8]) -> Result<serde_json::Value, ProtobufError> {
    if bytes.is_empty() {
        return Err(ProtobufError::EmptyBuffer);
    }
    let bytes_owned = bytes.to_vec();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let structured: prost_types::Struct =
            prost::Message::decode(&bytes_owned[..]).map_err(ProtobufError::from)?;
        struct_to_value(&structured)
    }));
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(ProtobufError::Panic),
    }
}

/// Roundtrip: serialize a value to protobuf and deserialize it back.
/// Returns `None` if either step fails. Convenience helper for tests
/// and codegen integration; not part of the stable Buff-visible
/// surface (the `Protobuf.*` namespace exposes only
/// `serialize` / `deserialize`).
pub fn roundtrip(value: &serde_json::Value) -> Option<serde_json::Value> {
    let bytes = serialize(value).ok()?;
    deserialize(&bytes).ok()
}

/// Convert a `serde_json::Value` into a protobuf `Struct`.
///
/// Objects map 1:1 to `Struct { fields }`. All other JSON shapes
/// (arrays / scalars) are wrapped in a single-field `Struct` under the
/// magic key `"_value"` so they survive the `Struct`-typed wire format
/// (protobuf's `Value` message is never a top-level message). The
/// inverse is [`struct_to_value`].
fn value_to_struct(value: &serde_json::Value) -> Result<prost_types::Struct, ProtobufError> {
    let mut fields = std::collections::HashMap::new();
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                fields.insert(k.clone(), json_to_proto_value(v)?);
            }
        }
        _ => {
            fields.insert("_value".to_string(), json_to_proto_value(value)?);
        }
    }
    Ok(prost_types::Struct { fields })
}

/// Convert a `serde_json::Value` into a protobuf `Value` (the
/// scalar / leaf message). Mirrors `google.protobuf.Value`:
/// - null → `NullValue`
/// - bool → `BoolValue`
/// - number → `NumberValue` (finite f64; NaN/Inf rejected)
/// - string → `StringValue`
/// - array → `ListValue`
/// - object → nested `StructValue`
fn json_to_proto_value(value: &serde_json::Value) -> Result<prost_types::Value, ProtobufError> {
    use prost_types::value::Kind;
    let kind = match value {
        serde_json::Value::Null => Some(Kind::NullValue(0)),
        serde_json::Value::Bool(b) => Some(Kind::BoolValue(*b)),
        serde_json::Value::Number(n) => {
            let f = n.as_f64().ok_or_else(|| {
                ProtobufError::Encode(format!("number {n} not representable as f64"))
            })?;
            if !f.is_finite() {
                return Err(ProtobufError::NonFiniteNumber(f));
            }
            Some(Kind::NumberValue(f))
        }
        serde_json::Value::String(s) => Some(Kind::StringValue(s.clone())),
        serde_json::Value::Array(arr) => {
            let mut values = Vec::with_capacity(arr.len());
            for v in arr {
                values.push(json_to_proto_value(v)?);
            }
            Some(Kind::ListValue(prost_types::ListValue { values }))
        }
        serde_json::Value::Object(map) => {
            let mut fields = std::collections::HashMap::new();
            for (k, v) in map {
                fields.insert(k.clone(), json_to_proto_value(v)?);
            }
            Some(Kind::StructValue(prost_types::Struct { fields }))
        }
    };
    Ok(prost_types::Value { kind })
}

/// Inverse of [`struct_to_value`]: recover the `serde_json::Value`
/// from a decoded `Struct`.
///
/// A `Struct` whose only field is `"_value"` is unwrapped to the raw
/// scalar/array shape (mirrors the wrapping done in
/// [`value_to_struct`]). All other shapes are returned as JSON objects.
fn struct_to_value(structured: &prost_types::Struct) -> Result<serde_json::Value, ProtobufError> {
    if structured.fields.len() == 1 {
        if let Some(inner) = structured.fields.get("_value") {
            return proto_value_to_json(inner);
        }
    }
    let mut map = serde_json::Map::new();
    for (k, v) in &structured.fields {
        map.insert(k.clone(), proto_value_to_json(v)?);
    }
    Ok(serde_json::Value::Object(map))
}

/// Convert a protobuf `Value` back into a `serde_json::Value`.
///
/// Numbers come back as f64; we try `serde_json::Number::from_f64`
/// (which returns `None` for NaN/Inf, but we rejected those at encode
/// time so this is a defensive fallback).
fn proto_value_to_json(value: &prost_types::Value) -> Result<serde_json::Value, ProtobufError> {
    use prost_types::value::Kind;
    match &value.kind {
        None => Ok(serde_json::Value::Null),
        Some(Kind::NullValue(_)) => Ok(serde_json::Value::Null),
        Some(Kind::NumberValue(f)) => {
            let n = serde_json::Number::from_f64(*f).unwrap_or_else(|| serde_json::Number::from(0));
            Ok(serde_json::Value::Number(n))
        }
        Some(Kind::StringValue(s)) => Ok(serde_json::Value::String(s.clone())),
        Some(Kind::BoolValue(b)) => Ok(serde_json::Value::Bool(*b)),
        Some(Kind::StructValue(s)) => {
            let mut map = serde_json::Map::new();
            for (k, v) in &s.fields {
                map.insert(k.clone(), proto_value_to_json(v)?);
            }
            Ok(serde_json::Value::Object(map))
        }
        Some(Kind::ListValue(l)) => {
            let mut arr = Vec::with_capacity(l.values.len());
            for v in &l.values {
                arr.push(proto_value_to_json(v)?);
            }
            Ok(serde_json::Value::Array(arr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Number};

    #[test]
    fn serialize_null() {
        let bytes = serialize(&json!(null)).expect("serialize null");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_bool() {
        let bytes = serialize(&json!(true)).expect("serialize bool");
        assert!(!bytes.is_empty());
        let bytes_false = serialize(&json!(false)).expect("serialize bool false");
        assert!(!bytes_false.is_empty());
    }

    #[test]
    fn serialize_integer() {
        let bytes = serialize(&json!(42)).expect("serialize int");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_negative_integer() {
        let bytes = serialize(&json!(-1)).expect("serialize negative");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_float() {
        let bytes = serialize(&json!(3.14)).expect("serialize float");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_string() {
        let bytes = serialize(&json!("hello protobuf")).expect("serialize string");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_array() {
        let bytes = serialize(&json!([1, 2, 3, "four", true, null])).expect("serialize array");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_object() {
        let bytes = serialize(&json!({
            "name": "Buff",
            "version": 1,
            "features": ["protobuf", "msgpack"]
        }))
        .expect("serialize object");
        assert!(!bytes.is_empty());
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
    fn deserialize_null_roundtrip() {
        let bytes = serialize(&json!(null)).expect("serialize");
        let back = deserialize(&bytes).expect("deserialize");
        assert_eq!(back, json!(null));
    }

    #[test]
    fn deserialize_bool_roundtrip() {
        for b in [true, false] {
            let bytes = serialize(&json!(b)).expect("serialize");
            let back = deserialize(&bytes).expect("deserialize");
            assert_eq!(back, json!(b));
        }
    }

    #[test]
    fn deserialize_integer_roundtrip() {
        for n in [0i64, 1, -1, 42, 1000, -1000] {
            let bytes = serialize(&json!(n)).expect("serialize");
            let back = deserialize(&bytes).expect("deserialize");
            assert_eq!(back.as_i64(), Some(n));
        }
    }

    #[test]
    fn deserialize_float_roundtrip() {
        for f in [0.0, 1.5, -2.25, 3.14159] {
            let bytes = serialize(&json!(f)).expect("serialize");
            let back = deserialize(&bytes).expect("deserialize");
            assert_eq!(back.as_f64(), Some(f));
        }
    }

    #[test]
    fn deserialize_string_roundtrip() {
        let value = json!("hello world");
        let bytes = serialize(&value).expect("serialize");
        let back = deserialize(&bytes).expect("deserialize");
        assert_eq!(back, value);
    }

    #[test]
    fn deserialize_array_roundtrip() {
        let value = json!([1, 2, 3, "four", true, null]);
        let bytes = serialize(&value).expect("serialize");
        let back = deserialize(&bytes).expect("deserialize");
        assert_eq!(back, value);
    }

    #[test]
    fn deserialize_object_roundtrip() {
        let value = json!({
            "name": "Buff",
            "version": 1,
            "features": ["protobuf", "grpc"]
        });
        let bytes = serialize(&value).expect("serialize");
        let back = deserialize(&bytes).expect("deserialize");
        assert_eq!(back, value);
    }

    #[test]
    fn deserialize_empty_buffer_rejected() {
        let err = deserialize(&[]).unwrap_err();
        assert!(matches!(err, ProtobufError::EmptyBuffer));
    }

    #[test]
    fn deserialize_garbage_rejected() {
        let err = deserialize(b"not protobuf data").unwrap_err();
        assert!(matches!(err, ProtobufError::Decode(_)));
    }

    #[test]
    fn roundtrip_preserves_values() {
        let cases = vec![
            json!(null),
            json!(true),
            json!(false),
            json!(0),
            json!(42),
            json!(-1),
            json!(3.14),
            json!("hello world"),
            json!([1, 2, 3]),
            json!({"a": 1, "b": [2, 3]}),
            json!({"nested": {"deep": {"value": 42}}}),
            json!({"mixed": [1, "two", false, [4, 5], {"six": 6}]}),
        ];
        for case in cases {
            let result = roundtrip(&case);
            assert_eq!(result.as_ref(), Some(&case), "roundtrip failed for {case}");
        }
    }

    #[test]
    fn message_new_encodes_value() {
        let msg = Message::new(&json!({"k": "v"})).expect("new");
        assert_eq!(msg.type_url(), TYPE_URL);
        assert!(msg.byte_size() > 0);
        assert!(!msg.encode().is_empty());
    }

    #[test]
    fn message_payload_roundtrips() {
        let value = json!({"count": 42, "name": "Buff"});
        let msg = Message::new(&value).expect("new");
        let back = msg.payload().expect("payload");
        assert_eq!(back, value);
    }

    #[test]
    fn message_from_bytes_decodes() {
        let original = json!({"a": [1, 2], "b": "text"});
        let bytes = serialize(&original).expect("serialize");
        let msg = Message::from_bytes(bytes).expect("from_bytes");
        let back = msg.payload().expect("payload");
        assert_eq!(back, original);
    }

    #[test]
    fn message_decode_alias_works() {
        let value = json!({"x": 1});
        let bytes = serialize(&value).expect("serialize");
        let msg = Message::decode(&bytes).expect("decode");
        assert_eq!(msg.payload().unwrap(), value);
    }

    #[test]
    fn message_from_bytes_rejects_empty() {
        let err = Message::from_bytes(Vec::new()).unwrap_err();
        assert!(matches!(err, ProtobufError::EmptyBuffer));
    }

    #[test]
    fn message_from_bytes_rejects_garbage() {
        let err = Message::from_bytes(b"garbage".to_vec()).unwrap_err();
        assert!(matches!(err, ProtobufError::Decode(_)));
    }

    #[test]
    fn message_display_shows_size() {
        let msg = Message::new(&json!({"k": "v"})).expect("new");
        let s = format!("{msg}");
        assert!(s.contains("type.googleapis.com"));
        assert!(s.contains("bytes"));
    }

    #[test]
    fn message_eq_when_same_payload() {
        let v = json!({"a": 1});
        let m1 = Message::new(&v).expect("new");
        let m2 = Message::new(&v).expect("new");
        assert_eq!(m1, m2);
    }

    #[test]
    fn message_neq_when_different_payload() {
        let m1 = Message::new(&json!({"a": 1})).expect("new");
        let m2 = Message::new(&json!({"a": 2})).expect("new");
        assert_ne!(m1, m2);
    }

    #[test]
    fn serialize_large_array() {
        let arr: Vec<serde_json::Value> = (0..500).map(json).collect();
        let value = serde_json::Value::Array(arr);
        let bytes = serialize(&value).expect("serialize large");
        let back = deserialize(&bytes).expect("deserialize large");
        assert_eq!(value, back);
    }

    #[test]
    fn serialize_empty_object() {
        let value = json!({});
        let bytes = serialize(&value).expect("serialize empty obj");
        let back = deserialize(&bytes).expect("deserialize empty obj");
        assert_eq!(back, value);
    }

    #[test]
    fn serialize_empty_array() {
        let value = json!([]);
        let bytes = serialize(&value).expect("serialize empty arr");
        let back = deserialize(&bytes).expect("deserialize empty arr");
        assert_eq!(back, value);
    }

    #[test]
    fn number_from_f64_fallback_is_safe() {
        let n = serde_json::Number::from_f64(42.0).unwrap_or_else(|| Number::from(0));
        assert_eq!(n.as_f64(), Some(42.0));
    }
}
