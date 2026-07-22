// T31 example: per-entry TTL eviction.
//
// Demonstrates the per-entry TTL surface: insert two entries with
// distinct deadlines, observe that the short-TTL entry disappears
// after its deadline while the long-TTL entry remains. Uses real
// wall-clock time (small ms budgets) so the timing is observable
// without env hooks.

use buff_cache::Cache;
use std::thread;
use std::time::Duration;

fn main() {
    let cache = Cache::new(10).expect("10-capacity");

    cache.set_with_ttl(
        "fast".to_string(),
        "1".to_string(),
        Duration::from_millis(50),
    );
    cache.set_with_ttl(
        "slow".to_string(),
        "2".to_string(),
        Duration::from_secs(60),
    );

    println!("at t=0ms: fast={:?} slow={:?}", cache.get("fast"), cache.get("slow"));

    thread::sleep(Duration::from_millis(80));
    println!(
        "at t=80ms: fast={:?} slow={:?} (fast TTL expired)",
        cache.get("fast"),
        cache.get("slow")
    );

    println!("final entry count: {}", cache.len());
}
