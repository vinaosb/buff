// T31 example: basic cache set/get/delete roundtrip.
//
// Demonstrates the minimal in-memory cache surface: create with a
// capacity, set two keys, read them back, delete one, observe the
// missing-key path. No TTL — entries stay until LRU-evicted.

use buff_cache::Cache;

fn main() {
    let cache = Cache::new(100).expect("100-capacity cache");
    println!("created: {cache}");

    cache.set("user:1".to_string(), "alice".to_string());
    cache.set("user:2".to_string(), "bob".to_string());

    match cache.get("user:1") {
        Some(v) => println!("user:1 -> {v}"),
        None => println!("user:1 -> (missing)"),
    }

    cache.delete("user:1");
    println!("after delete: user:1 present? {}", cache.contains("user:1"));
    println!("after delete: user:2 present? {}", cache.contains("user:2"));

    println!("final entry count: {}", cache.len());
}
