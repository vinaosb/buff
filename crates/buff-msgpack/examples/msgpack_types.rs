//! Example: serialize various data types to MessagePack.

use serde_json::json;

fn main() {
    let cases = vec![
        json!(null),
        json!(true),
        json!(false),
        json!(42),
        json!(-128),
        json!(3.14159),
        json!("hello 世界"),
        json!([1, 2, 3]),
        json!({"key": "value"}),
    ];

    for (i, value) in cases.iter().enumerate() {
        let bytes = buff_msgpack::serialize(value).expect("serialize");
        let decoded = buff_msgpack::deserialize(&bytes).expect("deserialize");
        println!(
            "Case {}: {:?} -> {} bytes -> {:?}",
            i,
            value,
            bytes.len(),
            decoded
        );
        assert_eq!(*value, decoded);
    }
    println!("All types roundtrip OK!");
}
