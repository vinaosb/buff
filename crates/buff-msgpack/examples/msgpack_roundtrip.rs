//! Example: serialize a Buff value to MessagePack and deserialize it back.
//!
//! Run: cargo run --example msgpack_roundtrip
//! (or from Buff: MsgPack.serialize(value) / MsgPack.deserialize(bytes))

use serde_json::json;

fn main() {
    // Serialize a simple value
    let value = json!({"hello": "world", "count": 42});
    let bytes = buff_msgpack::serialize(&value).expect("serialize");
    println!(
        "Serialized {} bytes: {:02x?}",
        bytes.len(),
        &bytes[..8.min(bytes.len())]
    );

    // Deserialize back
    let decoded = buff_msgpack::deserialize(&bytes).expect("deserialize");
    println!("Decoded: {}", decoded);

    assert_eq!(value, decoded);
    println!("Roundtrip OK!");
}
