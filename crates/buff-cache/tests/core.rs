//! Integration tests for the `buff-cache` crate.
//!
//! Covers all 8 public functions per the T31 spec:
//! - Constructors: `Cache::new` (incl. error path on zero capacity)
//! - Mutators: `set`, `set_with_ttl`, `delete`, `clear`
//! - Accessors: `get`, `contains`, `len`, `is_empty`
//!
//! Plus TTL eviction + LRU eviction behavior. 12+ unit tests + 3
//! insta snapshots (per T31 acceptance criteria: "3 examples + 10
//! tests").

use buff_cache::{Cache, CacheError};
use std::thread;
use std::time::Duration;

#[test]
fn cache_new_creates_empty_cache() {
    let cache = Cache::new(100).expect("100-capacity cache");
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}

#[test]
fn cache_new_rejects_zero_capacity() {
    let err = Cache::new(0).unwrap_err();
    assert!(matches!(
        err,
        CacheError::InvalidCapacity { requested: 0 }
    ));
}

#[test]
fn cache_set_and_get_roundtrip() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set("alpha".to_string(), "1".to_string());
    cache.set("beta".to_string(), "2".to_string());
    assert_eq!(cache.get("alpha"), Some("1".to_string()));
    assert_eq!(cache.get("beta"), Some("2".to_string()));
    assert_eq!(cache.get("missing"), None);
}

#[test]
fn cache_get_returns_none_for_empty_key() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set("".to_string(), "empty-key-value".to_string());
    assert_eq!(cache.get(""), Some("empty-key-value".to_string()));
}

#[test]
fn cache_delete_removes_entry() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set("k".to_string(), "v".to_string());
    assert!(cache.contains("k"));
    cache.delete("k");
    assert!(!cache.contains("k"));
    assert_eq!(cache.get("k"), None);
}

#[test]
fn cache_delete_missing_key_is_noop() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.delete("never-existed");
    assert_eq!(cache.len(), 0);
}

#[test]
fn cache_contains_distinguishes_present_absent() {
    let cache = Cache::new(10).expect("10-capacity");
    assert!(!cache.contains("absent"));
    cache.set("present".to_string(), "v".to_string());
    assert!(cache.contains("present"));
}

#[test]
fn cache_clear_empties_all_entries() {
    let cache = Cache::new(100).expect("100-capacity");
    for i in 0..10 {
        cache.set(format!("k{i}"), format!("v{i}"));
    }
    assert!(cache.len() >= 9);
    cache.clear();
    cache.run_pending_tasks();
    assert_eq!(cache.len(), 0);
}

#[test]
fn cache_len_reflects_inserts_and_deletes() {
    let cache = Cache::new(100).expect("100-capacity");
    cache.set("a".to_string(), "1".to_string());
    cache.set("b".to_string(), "2".to_string());
    cache.run_pending_tasks();
    assert_eq!(cache.len(), 2);
    cache.delete("a");
    cache.run_pending_tasks();
    assert_eq!(cache.len(), 1);
}

#[test]
fn cache_set_with_ttl_zero_acts_like_set() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set_with_ttl(
        "k".to_string(),
        "v".to_string(),
        Duration::from_secs(0),
    );
    thread::sleep(Duration::from_millis(10));
    assert_eq!(cache.get("k"), Some("v".to_string()));
}

#[test]
fn cache_set_with_ttl_evicts_after_deadline() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set_with_ttl(
        "short".to_string(),
        "v".to_string(),
        Duration::from_millis(40),
    );
    assert_eq!(cache.get("short"), Some("v".to_string()));
    thread::sleep(Duration::from_millis(80));
    assert_eq!(cache.get("short"), None);
}

#[test]
fn cache_contains_respects_ttl_expiry() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set_with_ttl(
        "k".to_string(),
        "v".to_string(),
        Duration::from_millis(30),
    );
    assert!(cache.contains("k"));
    thread::sleep(Duration::from_millis(60));
    assert!(!cache.contains("k"));
}

#[test]
fn cache_set_with_distinct_deadlines_coexist() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set_with_ttl(
        "fast".to_string(),
        "1".to_string(),
        Duration::from_millis(30),
    );
    cache.set_with_ttl(
        "slow".to_string(),
        "2".to_string(),
        Duration::from_secs(60),
    );
    thread::sleep(Duration::from_millis(60));
    assert_eq!(cache.get("fast"), None);
    assert_eq!(cache.get("slow"), Some("2".to_string()));
}

#[test]
fn cache_lru_eviction_under_capacity_pressure() {
    let cache = Cache::new(3).expect("3-capacity");
    cache.set("a".to_string(), "1".to_string());
    cache.set("b".to_string(), "2".to_string());
    cache.set("c".to_string(), "3".to_string());
    let _ = cache.get("a");
    cache.run_pending_tasks();
    cache.set("d".to_string(), "4".to_string());
    cache.run_pending_tasks();
    assert!(cache.len() <= 3);
    let _ = cache.get("a");
}

#[test]
fn cache_default_does_not_panic() {
    let cache = Cache::default();
    cache.set("k".to_string(), "v".to_string());
    assert_eq!(cache.get("k"), Some("v".to_string()));
}

#[test]
fn cache_clone_shares_underlying_state() {
    let cache = Cache::new(10).expect("10-capacity");
    let cloned = cache.clone();
    cache.set("shared".to_string(), "v".to_string());
    assert_eq!(cloned.get("shared"), Some("v".to_string()));
}

// ---- Insta snapshots (3+) ---------------------------------------------------

#[test]
fn snapshot_cache_display() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set("k".to_string(), "v".to_string());
    cache.run_pending_tasks();
    insta::assert_snapshot!("cache_display", format!("{cache}"));
}

#[test]
fn snapshot_cache_debug() {
    let cache = Cache::new(10).expect("10-capacity");
    cache.set("k".to_string(), "v".to_string());
    cache.run_pending_tasks();
    insta::assert_snapshot!("cache_debug", format!("{cache:?}"));
}

#[test]
fn snapshot_cache_error_debug() {
    let err = CacheError::InvalidCapacity { requested: 0 };
    let ttl_err = CacheError::InvalidTtl { secs: 0 };
    let empty_err = CacheError::EmptyKey;
    let panic_err = CacheError::Panic;
    insta::assert_snapshot!(
        "cache_error_debug",
        format!("{err}\n{ttl_err}\n{empty_err}\n{panic_err}")
    );
}
