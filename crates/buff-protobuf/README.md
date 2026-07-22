# buff-protobuf

> Protocol Buffers for the **Buff** language. Pure-Rust MVP (CPU-only).

`buff-protobuf` wraps the pure-Rust [`prost`](https://docs.rs/prost) + [`prost-types`](https://docs.rs/prost-types) crates behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses protobuf via the `Protobuf` prelude namespace:

```buff
let bytes = Protobuf.serialize({"name": "Buff", "version": 1})
let value = Protobuf.deserialize(bytes)
print(value.name)
```

**Status: experimental** (T52 v1.17 frameworks wave 4).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Protobuf` prelude type.

For direct Rust use:

```bash
cargo add buff-protobuf --path crates/buff-protobuf
```

## Quick start

```rust
use buff_protobuf::{deserialize, serialize, Message};
use serde_json::json;

fn main() -> Result<(), buff_protobuf::ProtobufError> {
    let value = json!({
        "name": "Buff",
        "version": 1,
        "features": ["protobuf", "grpc"]
    });

    let bytes = serialize(&value)?;
    let back = deserialize(&bytes)?;
    assert_eq!(back, value);

    let msg = Message::new(&value)?;
    println!("{}", msg);
    println!("payload: {:?}", msg.payload()?);
    Ok(())
}
```

## Public API

### `Message` — encoded protobuf message

| Method | Signature | Notes |
|---|---|---|
| `Message::new` | `(value: &Value) -> Result<Message, ProtobufError>` | Encode via `google.protobuf.Struct`. `catch_unwind` boundary. |
| `Message::from_bytes` | `(bytes: Vec<u8>) -> Result<Message, ProtobufError>` | Decode raw wire-format bytes. |
| `Message::decode` | `(bytes: &[u8]) -> Result<Message, ProtobufError>` | Class-method alias for `from_bytes` (mirrors `Message.decode(bytes)` spec). |
| `msg.encode` | `() -> &[u8]` | Zero-cost view of the encoded payload. |
| `msg.type_url` | `() -> &str` | Always `type.googleapis.com/google.protobuf.Struct` in this MVP. |
| `msg.byte_size` | `() -> usize` | Encoded payload size in bytes. |
| `msg.payload` | `() -> Result<Value, ProtobufError>` | Decode payload back to `serde_json::Value`. |

### Free functions

| Function | Signature | Notes |
|---|---|---|
| `serialize` | `(value: &Value) -> Result<Vec<u8>, ProtobufError>` | Encode `Value` → protobuf wire bytes. |
| `deserialize` | `(bytes: &[u8]) -> Result<Value, ProtobufError>` | Decode protobuf wire bytes → `Value`. |
| `roundtrip` | `(value: &Value) -> Option<Value>` | Convenience helper (test/codegen integration). |

## How it works

The MVP uses protobuf's well-known [`google.protobuf.Struct`](https://protobuf.dev/reference/protobuf/google.protobuf/#struct) type as a dynamic message surface:

| JSON shape | Protobuf mapping |
|---|---|
| `null` | `Value::null_value` |
| `bool` | `Value::bool_value` |
| number (finite `f64`) | `Value::number_value` |
| `string` | `Value::string_value` |
| array | `Value::list_value` (`ListValue`) |
| object | `Struct { fields }` (`Value::struct_value` for nested) |

Non-object JSON shapes (scalars / arrays) are wrapped in a single-field `Struct { "_value": v }` because protobuf's `Value` message is never a top-level message. The inverse unwrap is automatic on decode.

NaN / Infinity are rejected at encode time because protobuf `Value::number_value` is a finite `f64` (and `NaN != NaN` in IEEE-754 would otherwise silently corrupt roundtrips).

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md):

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Message`, `Vec<u8>`, `serde_json::Value`, `ProtobufError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `serialize` returns owned `Vec<u8>`. `deserialize` returns owned `Value`. `Message::from_bytes` consumes its arg. |
| R3 — Error mapping | Every fallible op returns `Result<T, ProtobufError>`. `prost::EncodeError` / `DecodeError` auto-convert via `From`. |
| R4 — Thread safety | `Message` is `Send + Sync` (owns `String` + `Vec<u8>`). |
| R5 — Lifetime hiding | No public lifetime parameters. All inputs owned or `&[u8]`. |
| R6 — Panic boundary | Every public fn wraps its body in `catch_unwind`. |

## Deferred (per T52 spec)

- **gRPC unary**: composition with T17 `buff-web` for HTTP/2 transport. gRPC streaming is explicitly out of MVP scope.
- **`.proto` → Buff type codegen**: `buff proto <file> <out>` CLI subcommand using `prost-build` (wraps protoc). Deferred so the MVP stays pure-Rust at runtime + build-time.
- **Reflection API**: `prost-reflect`-backed runtime schema introspection. Deferred.

## Testing

```bash
cargo test -p buff-protobuf
cargo clippy -p buff-protobuf --all-targets -- -D warnings
cargo fmt -p buff-protobuf --check
```

Tests are hermetic: 31 inline unit tests + 0 insta snapshots (the wire format is fixed by Google's spec, so byte-level snapshots would over-specify prost's encoding choices).

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
