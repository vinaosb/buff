# buff-cache

In-memory cache for the Buff language. Pure-Rust MVP wrapping the [`moka`](https://crates.io/crates/moka) crate (sync API, tinyLFU admission + LRU eviction + per-entry TTL via stored `Option<Instant>` deadlines). Per T31 spec: `Cache.new(max_capacity)`, `cache.get(key)`, `cache.set(key, value)`, `cache.set(key, value, ttl)`, `cache.delete(key)`.

**Status: experimental** (T31 v1.16 frameworks wave 5).

## STRUCTURE

```
buff-cache/
├── Cargo.toml            # moka + thiserror + insta deps
├── src/
│   ├── lib.rs            # Cache (main surface, ~145 LOC)
│   └── error.rs          # CacheError enum (~20 LOC)
├── examples/
│   ├── cache_basic.rs         # set/get/delete roundtrip
│   ├── cache_ttl.rs           # per-entry TTL eviction
│   ├── cache_lru.rs           # LRU eviction under capacity pressure
│   └── cache/
│       └── cache_basic.buff   # Buff-side forward-decl (matches .rs)
└── tests/
    └── core.rs           # 15 unit tests + 3 insta snapshots (~250 LOC)
```

Total: ~450 LOC (well under the 2000 LOC T31 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new cache op | `src/lib.rs` (add `pub fn` on `Cache`) + test in `tests/core.rs` |
| Add a new error variant | `src/error.rs` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (8 functions, ≤15 cap)

### `Cache` (8 functions)
- Constructors: `new(max_capacity)` — returns `Result<Cache, CacheError>` (zero capacity rejected)
- Mutators: `set(key, value)` (no TTL), `set_with_ttl(key, value, ttl)`, `delete(key)`, `clear()`
- Accessors: `get(key) -> Option<String>`, `contains(key) -> bool`, `len() -> u64`, `is_empty() -> bool`

## CONVENTIONS

- **Pure-Rust only**: moka's `sync` feature pulls in only `crossbeam-epoch`/`portable-atomic`/`quanta`/`triomphe`/`tagptr` — all pure-Rust, no cc-rs, no native deps. Matches the "no C library, no Docker" hard rule from T126/T127.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. Capacity validation returns `Result`.
- **catch_unwind boundary**: `new` wraps its body in `catch_unwind` per FFI guide R6.
- **Send + Sync**: `Cache` is `Send + Sync` (wraps `Arc<moka::sync::Cache<...>>`); safe to share across `spawn` boundaries per FFI guide R4.
- **Per-entry TTL layered on top of moka**: moka 0.12 supports global TTL via `.time_to_live(Duration)` on the builder, but per-entry TTL requires implementing the `Expiry` trait (non-trivial). For the T31 MVP we store `(value, Option<Instant>)` tuples and check the deadline in `get`/`contains` — gives true per-entry TTL semantics with a single moka instance. Trade-off: expired entries occupy capacity until the next `get` probes them or `max_capacity` evicts them (no background sweep). v1.18+ Redis backend will move per-entry TTL into Redis itself (`EXPIRE` command).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `moka` | Upstream concurrent cache. `buff-cache` is a safe wrapper; never re-exports `moka::*` types directly. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Cache` + `PreludeAssocFn::New` + `PreludeInstanceFn::{Get, Set, SetTtl, Delete, Contains, Clear, Len}`. `ty.rs` has the `Type::Cache` variant + `is_prelude_cache()` predicate. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Cache => "buff_cache::Cache"` arm. `lower_prelude_type_assoc_fn` has the `(Cache, New)` arm. `lower_prelude_type_instance_fn` has all 7 instance-method arms. `program_uses_namespace("Cache")` records `buff-cache` + `moka` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **MSVC host blocker**: `cargo test -p buff-cache` fails on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` here, documented in buff-image's AGENTS.md). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue.
- **moka 0.12 sync feature only**: the `future` feature is NOT enabled — Buff's T31 cache surface is synchronous (mirrors how `crossbeam-channel` is the sync surface for the LSP server). The async-friendly moka API (`moka::future::Cache`) is deferred to v1.18+ when the Redis backend lands and async propagation through cache ops becomes worth the surface area.
- **Cache impls Default** as a 1024-capacity empty cache (used by codegen fallback for panic-free `unwrap_or_default()` paths — matches the Image / DataFrame precedent).

## DEFERRED (v1.18+)

Per the T31 task spec ("If problematic, defer distributed to v1.18+ and ship in-memory MVP only (document in AGENTS.md)"):

- **Distributed Redis backend**: `redis` crate (with `tls-rustls` feature for pure-Rust TLS). The current `Cache.new(max_capacity) / cache.set / cache.get / cache.delete` surface is shaped so the future Redis backend is a drop-in: keys/values are `String`, TTL is a `chrono::Duration` (same one the Buff prelude already surfaces), and the dispatch will be keyed on a new `Backend::{Memory, Redis}` enum inside `Cache` (single match arm per method). The `redis` crate's rustls feature is pure-Rust on paper but pulls a substantial async + connection-pool surface that complicates the MVP smoke test on the Windows MSVC host.
- **Cache invalidation pub/sub**: T31 task spec explicitly defers to v1.22+.
- **Multi-tier cache orchestration**: T31 task spec explicitly defers to v1.22+.
- **Per-entry TTL via moka `Expiry` trait**: future enhancement to replace the `(value, Option<Instant>)` tuple with moka's native per-entry expiry (saves the stored `Option<Instant>` and adds a background sweep so expired entries don't occupy capacity until probed). Migration is internal — the Buff-visible surface stays identical.
