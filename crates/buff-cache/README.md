# buff-cache

> In-memory cache for the **Buff** language. Pure-Rust MVP wrapping the `moka` crate.

`buff-cache` wraps the mature [`moka`](https://crates.io/crates/moka) crate behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses the cache via the `Cache` prelude type:

```buff
let cache = Cache.new(max_capacity: 1000)
cache.set(key: "user:1", value: "alice")
print(cache.get(key: "user:1").or(default: "(missing)"))  // "alice"

cache.set(key: "session:abc", value: "token", ttl: Duration.seconds(60))
thread.sleep(Duration.seconds(61))
print(cache.get(key: "session:abc").or(default: "(expired)"))  // "(expired)"

cache.delete(key: "user:1")
print("entries: ${cache.len()}")
```

**Status: experimental** (T31 v1.16 frameworks wave 5).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Cache` prelude type.

For direct Rust use:

```bash
cargo add buff-cache --path crates/buff-cache
```

## Quick start

```rust
use buff_cache::Cache;
use std::time::Duration;

fn main() {
    let cache = Cache::new(100).expect("100-capacity");

    cache.set("user:1".to_string(), "alice".to_string());
    assert_eq!(cache.get("user:1"), Some("alice".to_string()));

    cache.set_with_ttl(
        "session:abc".to_string(),
        "token".to_string(),
        Duration::from_secs(60),
    );
    assert!(cache.contains("session:abc"));

    cache.delete("user:1");
    assert_eq!(cache.get("user:1"), None);
}
```

## Public API

### `Cache` — concurrent in-memory cache (LRU + per-entry TTL)

| Method | Signature | Notes |
|---|---|---|
| `Cache::new` | `(max_capacity: u64) -> Result<Cache, CacheError>` | Rejects zero capacity. `catch_unwind` boundary. |
| `cache.set` | `(&self, key: String, value: String)` | Insert without TTL (stays until LRU-evicted). |
| `cache.set_with_ttl` | `(&self, key: String, value: String, ttl: Duration)` | Insert with per-entry TTL. TTL=0 acts like `set`. |
| `cache.get` | `(&self, key: &str) -> Option<String>` | Returns `None` if missing OR expired (lazy eviction). |
| `cache.delete` | `(&self, key: &str)` | Removes a single entry. No-op if missing. |
| `cache.contains` | `(&self, key: &str) -> bool` | Expiry-aware (returns `false` for expired entries). |
| `cache.clear` | `(&self)` | Removes all entries (`invalidate_all`). |
| `cache.len` | `(&self) -> u64` | Approximate entry count (moka runs pending tasks first). |
| `cache.is_empty` | `(&self) -> bool` | Convenience: `self.len() == 0`. |

## Behavior

### Eviction

- **LRU semantics** via moka's tinyLFU admission policy: when the cache hits `max_capacity`, the least-recently-used entries are evicted to make room.
- **Per-entry TTL**: each `set_with_ttl` entry stores an `Option<Instant>` deadline. `get` / `contains` check the deadline lazily and invalidate expired entries on access.

### Thread safety

`Cache` is `Send + Sync` (wraps `Arc<moka::sync::Cache<...>>`). The same `Cache` instance can be safely shared across threads via `.clone()` (cheap — bumps the inner `Arc` count).

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Cache`, `CacheError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `new` returns owned `Cache`. `get` returns owned `Option<String>`. `set` consumes owned `String` args. |
| R3 — Error mapping | `new` returns `Result<Cache, CacheError>`. `set`/`get`/`delete` are infallible. |
| R4 — Thread safety | `Cache` is `Send + Sync` (wraps `Arc<moka::sync::Cache>`). |
| R5 — Lifetime hiding | No public lifetime parameters. `Cache` owns its `moka::sync::Cache` handle via `Arc`. |
| R6 — Panic boundary | `new` wraps body in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-cache
cargo clippy -p buff-cache --all-targets -- -D warnings
cargo fmt -p buff-cache --check
```

Tests are hermetic: no external cache server needed (moka is in-process). TTL tests use small `Duration::from_millis` budgets with `thread::sleep` to make timing observable without env hooks. 15 unit tests + 3 insta snapshots.

## Deferred to v1.18+

Per the T31 task spec, the following are explicitly out of scope for the MVP:

- **Distributed Redis backend** (`redis` crate with `tls-rustls`).
- **Cache invalidation pub/sub** (v1.22+).
- **Multi-tier cache orchestration** (v1.22+).

The `cache.set(key, value, ttl: Duration)` surface is shaped so the future Redis backend is a drop-in.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
