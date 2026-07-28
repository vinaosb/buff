//! `buff-cache` — in-memory cache for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`moka`](https://crates.io/crates/moka)
//! crate (sync API, tinyLFU admission + LRU eviction + global TTL).
//! Per-entry TTL semantics layered on top via an `Option<Instant>`
//! expiry marker stored alongside each value.
//!
//! Distributed Redis backend is **deferred to v1.18+** per the T31
//! task spec — see `crates/buff-cache/AGENTS.md` "DEFERRED" section
//! and the root `[workspace.dependencies]` comment in `Cargo.toml`.
//!
//! # Pipeline
//!
//! ```text
//!   Cache.new(max_capacity) ──▶ Cache { moka::sync::Cache<String, (String, Option<Instant>)> }
//!                                     │
//!                                     ├─ cache.set(k, v)         ─▶ insert (v, None)
//!                                     ├─ cache.set(k, v, ttl)    ─▶ insert (v, Some(now+ttl))
//!                                     ├─ cache.get(k)            ─▶ None  if expired / missing
//!                                     ├─ cache.delete(k)         ─▶ invalidate
//!                                     ├─ cache.contains(k)       ─▶ expiry-aware
//!                                     ├─ cache.clear()           ─▶ invalidate_all
//!                                     └─ cache.len()             ─▶ entry_count (approximate)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Cache`, `CacheError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | `new` returns owned `Cache`. `get` returns owned `Option<String>`. `set` consumes its `String` args. |
//! | R3 — Error mapping | Fallible ops (`new`) return `Result<Cache, CacheError>`. `set`/`get`/`delete` are infallible. |
//! | R4 — Thread safety | `Cache` is `Send + Sync` (wraps `moka::sync::Cache` which is itself `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `Cache` owns its `moka::sync::Cache` handle. |
//! | R6 — Panic boundary | `new` wraps its body in `catch_unwind` per FFI guide §6. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Capacity validation returns `Result`.

pub mod error;

pub use error::CacheError;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::{Duration, Instant};

type Inner = moka::sync::Cache<String, (String, Option<Instant>)>;

#[derive(Clone)]
pub struct Cache {
    inner: Arc<Inner>,
}

impl Cache {
    pub fn new(max_capacity: u64) -> Result<Self, CacheError> {
        if max_capacity == 0 {
            return Err(CacheError::InvalidCapacity {
                requested: max_capacity,
            });
        }
        let result = catch_unwind(AssertUnwindSafe(|| {
            let cache: Inner = moka::sync::Cache::builder()
                .max_capacity(max_capacity)
                .build();
            Cache {
                inner: Arc::new(cache),
            }
        }));
        match result {
            Ok(cache) => Ok(cache),
            Err(_) => Err(CacheError::Panic),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match self.inner.get(key) {
            Some((value, expiry)) => match expiry {
                Some(deadline) if Instant::now() >= deadline => {
                    self.inner.invalidate(key);
                    None
                }
                _ => Some(value),
            },
            None => None,
        }
    }

    pub fn set(&self, key: String, value: String) {
        self.inner.insert(key, (value, None));
    }

    pub fn set_with_ttl(&self, key: String, value: String, ttl: Duration) {
        if ttl.is_zero() {
            self.inner.insert(key, (value, None));
            return;
        }
        let deadline = Instant::now().checked_add(ttl).unwrap_or_else(Instant::now);
        self.inner.insert(key, (value, Some(deadline)));
    }

    pub fn delete(&self, key: &str) {
        self.inner.invalidate(key);
    }

    pub fn contains(&self, key: &str) -> bool {
        if let Some((_, Some(deadline))) = self.inner.get(key) {
            if Instant::now() >= deadline {
                self.inner.invalidate(key);
                return false;
            }
        }
        self.inner.contains_key(key)
    }

    pub fn clear(&self) {
        self.inner.invalidate_all();
    }

    pub fn len(&self) -> u64 {
        self.inner.run_pending_tasks();
        self.inner.entry_count()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[allow(dead_code)]
    pub fn run_pending_tasks(&self) {
        self.inner.run_pending_tasks();
    }
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("entries", &self.inner.entry_count())
            .finish()
    }
}

impl Default for Cache {
    fn default() -> Self {
        Cache::new(1024).unwrap_or_else(|_| Cache {
            inner: Arc::new(moka::sync::Cache::new(1024)),
        })
    }
}

impl std::fmt::Display for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cache({} entries)", self.inner.entry_count())
    }
}
