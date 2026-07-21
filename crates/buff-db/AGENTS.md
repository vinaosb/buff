# buff-db

Database access MVP for the Buff language. Wraps `sqlx` (pure-Rust, no native-tls) behind a safe, owned surface that complies with the Buff FFI guide.

## STRUCTURE

```
buff-db/
├── Cargo.toml          # sqlx (workspace) + thiserror + insta + tokio (dev)
├── src/
│   ├── lib.rs          # 70 lines — module wiring + crate-level docs + re-exports
│   ├── error.rs        # 60 lines — DbError (Pool/Query/Execute/Transaction/InvalidUrl/...) + Result
│   ├── pool.rs         # 230 lines — Pool + Transaction + DbParam
│   ├── query.rs        # 230 lines — Query builder + JoinKind
│   └── row.rs          # 150 lines — Row + DbValue + row_from_any adapter
├── tests/
│   ├── api.rs          # pool construction + execute + query_one smoke
│   └── query.rs        # SELECT/INSERT round-trip + transactions + query builder SQL
└── examples/
    ├── simple_query.buff  # Buff-side forward-decl (`Database.connect`)
    └── simple_query.rs    # matching direct-Rust pipeline
```

~700 LOC total (well under the T18 cap of 1500). 17 public functions (under the 20 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new DB driver (MySQL/MSSQL) | `Cargo.toml` (add sqlx feature) + `pool.rs::validate_driver` |
| Add a new query builder clause | `query.rs` (extend `Query` struct + chainable method) |
| Add a new row accessor | `row.rs` (extend `DbValue` enum or `Row::get_*`) |
| Add a new bindable param type | `pool.rs` (extend `DbParam` + `bind_to` + `From` impl) |
| Add transaction control op | `pool.rs::Transaction` |
| Wire Database to Buff codegen | `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` (assoc fns) + `crates/buff-lang-types/src/prelude_types.rs` |

## CONVENTIONS (this crate only)

- **`sqlx::any::AnyPool`** storage — single surface for SQLite + PostgreSQL (and any future driver). NOT separate `SqlitePool` / `PgPool` types.
- **No `unwrap`/`expect`/`panic!`/`todo!`** in non-test code. All fallible ops return `Result<T, DbError>` or `Option<T>`.
- **`#![forbid(unsafe_code)]` is NOT added** per AGENTS.md (CI enforces via `cargo clippy --all-targets -D warnings`).
- **Async-first** — every DB operation is `async fn`. Wraps the tokio runtime that the codegen layer sets up via `#[tokio::main]`.
- **Pure-Rust** — `sqlx` with `runtime-tokio-rustls` feature (NOT native-tls, per workspace hard rule).
- **BTreeMap storage** for row → map conversion (project hard rule — deterministic ordering).
- **Owned `Vec<DbValue>`** for row cells (NOT borrowed slices — FFI guide R5: no lifetimes exposed on `Row`).
- **`Pool::clone` is cheap** — `AnyPool` is internally `Arc`'d, so cloning just bumps a refcount. Users can pass pool copies into multiple `spawn` closures (FFI guide R4: Send + 'static).

## ANTI-PATTERNS (THIS CRATE)

- ❌ **Compile-time SQL validation** — deferred to v1.19+ macro work (T18 spec must-not #3).
- ❌ **Migrations** — deferred to v1.18+ (T18 spec must-not #4).
- ❌ **MySQL/MSSQL/Oracle** — T18 spec must-not #5; SQLite + PostgreSQL only for MVP.
- ❌ **`unwrap`/`expect`/`panic!`** in non-test code — project hard rule from AGENTS.md.
- ❌ **`diesel` or other ORMs** — T18 spec mandates sqlx only.
- ❌ **native-tls** — workspace hard rule per AGENTS.md (use `runtime-tokio-rustls` only).
- ❌ **libpq or other native C deps** — pure-Rust only (T126/T127 hard rule).
- ❌ **`Type.create()` / `Type.build()` / `new Type()`** ctor forms — Buff §7 permits only `Type.new()` / `Type.from_*()` / `Type.connect()`.

## UNIQUE STYLES

- **`DbParam` enum (not trait-based)** — `pool.query(sql, &[DbParam::Int(1), DbParam::Text("x".into())])` is type-safe; the `From<i64>` / `From<&str>` / etc. impls let callers write `&[1.into(), "x".into()]` ergonomically.
- **Query builder is runtime, NOT compile-time macros** — per T3 / T18 spec: SQL is built via chainable methods (`Query::new("users").select(&["id"]).filter("age > 18").sql()`) and rendered to a String. The compile-time `sqlx::query!` macro is intentionally avoided (would require offline DB introspection at compile time, breaking the single-file `buff run` rustc path).
- **`Pool::url_scheme()`** — runtime accessor for the driver kind (`"sqlite"` / `"postgres"`). Used by `validate_driver()` to reject unsupported schemes early.
- **`Transaction<'a>` borrows from `Pool`** — by design: tx holds a `&mut` borrow on the underlying connection, preventing concurrent use of the same connection. `Drop` impl auto-rolls-back if neither commit nor rollback was called (matches Rust's RAII pattern + sqlx's underlying semantics).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `sqlx` | Upstream SQL toolkit. `buff-db` is a safe wrapper; never re-exports `sqlx::*` types directly. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Database` + `PreludeAssocFn::Connect`. A coordinated `Type::Pool` variant in `ty.rs` + instance-method dispatch arms are sibling tasks outside the T18 shared zone (mirrors the T8 Tensor / T11 Spectrum forward-declaration precedent). |
| `buff-lang-codegen-rust` | `rust_codegen.rs::lower_prelude_type_assoc_fn` has the `(Database, Connect)` arm (emits `buff_db::Pool::connect(url).await.unwrap_or_default()`). `program_uses_namespace("Database")` records `buff-db` + `sqlx` + `tokio` in `extern_crates`. Instance-method lowering (pool.query / pool.execute / tx.commit) is deferred to the sibling task that adds `Type::Pool`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **Host MSVC blocker**: `cargo test -p buff-db` fails on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` on this host). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue.
- **`sqlx::Any` driver unification** is why the surface serves both SQLite and PostgreSQL without per-driver types. The trade-off is a slight runtime type-erasure cost vs. compile-time monomorphization (acceptable for the MVP).
- **`AnyValue` decoding** in `row.rs::read_any_value` tries `i64` → `f64` → `String` → `bool` → `Vec<u8>` in that order, falling back to `Null`. This is defensive against sqlx's heterogeneous per-driver decoding behavior — order matters (Int first so SQLite INTEGER columns don't silently parse as Float).
- **`Pool::connect` validates the URL scheme** before passing to `sqlx::AnyPool::connect`. This catches `mysql://` URLs early with a clear `UnsupportedDriver` error instead of letting sqlx's driver-resolution fail with a more cryptic message.
