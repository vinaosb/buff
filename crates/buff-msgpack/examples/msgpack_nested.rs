//! Example: serialize a complex nested value to MessagePack.

use serde_json::json;

fn main() {
    let value = json!({
        "name": "Buff",
        "version": 1,
        "features": ["msgpack", "serde", "json"],
        "metadata": {
            "author": "Buff Team",
            "stars": 42,
            "active": true
        }
    });

    let bytes = buff_msgpack::serialize(&value).expect("serialize");
    println!("Serialized {} bytes", bytes.len());

    let decoded = buff_msgpack::deserialize(&bytes).expect("deserialize");
    println!(
        "Decoded: {}",
        serde_json::to_string_pretty(&decoded).unwrap()
    );

    assert_eq!(value, decoded);
    println!("Nested roundtrip OK!");
}
