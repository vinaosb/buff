// T31 example: LRU eviction under capacity pressure.
//
// Demonstrates moka's tinyLFU admission policy: when the cache hits
// its max_capacity, the least-recently-used entries are evicted to
// make room. We fill a 3-capacity cache with 3 entries, touch the
// first one to mark it recently-used, then insert a 4th entry and
// observe that the cache stays within capacity.

use buff_cache::Cache;

fn main() {
    let cache = Cache::new(3).expect("3-capacity");

    cache.set("a".to_string(), "1".to_string());
    cache.set("b".to_string(), "2".to_string());
    cache.set("c".to_string(), "3".to_string());
    println!("after 3 inserts: {} entries", cache.len());

    let _ = cache.get("a");
    println!("touched 'a' to bump its recency");

    cache.set("d".to_string(), "4".to_string());
    cache.run_pending_tasks();
    println!(
        "after 4th insert under capacity pressure: {} entries",
        cache.len()
    );
    println!("a still present? {}", cache.contains("a"));
    println!("b still present? {}", cache.contains("b"));

    cache.clear();
    cache.run_pending_tasks();
    println!("after clear: {} entries", cache.len());
}
