# buff-db

> Database access MVP for the **Buff** language. Wraps `sqlx` (SQLite + PostgreSQL).

`buff-db` wraps the [`sqlx`](https://crates.io/crates/sqlx) crate behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses databases via the `Database` prelude type:

```buff
let pool = Database.connect("sqlite::memory:")
# Rust-side only for MVP (instance methods deferred to follow-up task):
# pool.execute("CREATE TABLE users (id INT, name TEXT)", [])
# let rows = pool.query("SELECT * FROM users", [])
```

**Status: experimental** (T18 v1.15 frameworks wave 4).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Database` prelude type.

For direct Rust use:

```bash
cargo add buff-db --path crates/buff-db
```

## Quick start

```rust
use buff_db::{DbParam, Pool};

#[tokio::main]
async fn main() -> Result<(), buff_db::DbError> {
    let pool = Pool::connect("sqlite::memory:").await?;

    pool.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)", &[]).await?;
    pool.execute("INSERT INTO users (id, name) VALUES (?, ?)", &[DbParam::Int(1), DbParam::Text("Ada".into())]).await?;

    let rows = pool.query("SELECT id, name FROM users", &[]).await?;
    for row in &rows {
        let id = row.get("id").and_then(|v| v.as_int()).unwrap_or_default();
        let name = row.get("name").and_then(|v| v.as_text()).unwrap_or_default();
        println!("{id}: {name}");
    }
    Ok(())
}
```

## Public API

### `Pool` — connection pool (wraps `sqlx::any::AnyPool`)

| Method | Signature | Notes |
|---|---|---|
| `Pool::connect` | `async (url: &str) -> Result<Pool, DbError>` | Validates scheme; SQLite + PostgreSQL only. |
| `Pool::from_inner` | `(AnyPool) -> Pool` | Wrap an existing pool. |
| `Pool::inner` | `(&self) -> &AnyPool` | Borrow underlying. |
| `pool.query` | `async (&self, sql: &str, params: &[DbParam]) -> Result<Vec<Row>, DbError>` | SELECT. |
| `pool.query_one` | `async (&self, sql: &str, params: &[DbParam]) -> Result<Row, DbError>` | Single-row SELECT. |
| `pool.execute` | `async (&self, sql: &str, params: &[DbParam]) -> Result<u64, DbError>` | DDL/DML, rows affected. |
| `pool.begin` | `async (&self) -> Result<Transaction, DbError>` | Start a transaction. |

### `Transaction` — in-flight transaction

| Method | Signature | Notes |
|---|---|---|
| `tx.commit` | `async (self) -> Result<(), DbError>` | Finalize. |
| `tx.rollback` | `async (self) -> Result<(), DbError>` | Abort. Drop also rolls back. |
| `tx.execute` | `async (&mut self, sql: &str, params: &[DbParam]) -> Result<u64, DbError>` | Inside tx. |
| `tx.query` | `async (&mut self, sql: &str, params: &[DbParam]) -> Result<Vec<Row>, DbError>` | Inside tx. |

### `Query` — runtime query builder

```rust
use buff_db::Query;

let sql = Query::new("users")
    .select(&["id", "name"])
    .filter("age > 18")
    .order_by("name")
    .limit(10)
    .sql();
assert_eq!(sql, "SELECT id, name FROM users WHERE age > 18 ORDER BY name LIMIT 10");
```

| Method | Notes |
|---|---|
| `Query::new(table)` | ctor |
| `Query::select(&[cols])` | projection (default `*`) |
| `Query::filter(pred)` | WHERE clause (chainable, joined by `AND`) |
| `Query::inner_join(table, on)` | INNER JOIN |
| `Query::left_join(table, on)` | LEFT JOIN |
| `Query::join(kind, table, on)` | explicit kind |
| `Query::order_by(col)` | ORDER BY |
| `Query::limit(n)` / `Query::offset(n)` | pagination |
| `Query::sql()` | render to String (terminal) |

### `Row` / `DbValue` — row + cell accessors

| Method | Notes |
|---|---|
| `row.get(name)` | `Option<&DbValue>` lookup |
| `row.to_map()` | `BTreeMap<String, String>` flat view |
| `row.column_names()` | `&[String]` |
| `value.as_int()` / `as_float()` / `as_text()` / `as_bool()` | typed accessors |
| `value.is_null()` | NULL test |
| `value.to_string_value()` | string-coerced view |

## Supported drivers

| Driver | Status | URL scheme |
|---|---|---|
| SQLite | ✅ MVP | `sqlite://` / `sqlite::memory:` |
| PostgreSQL | ✅ MVP | `postgres://` / `postgresql://` |
| MySQL | ❌ deferred | T18 must-not #5 |
| MSSQL | ❌ deferred | T18 must-not #5 |
| Oracle | ❌ deferred | T18 must-not #5 |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Pool`, `Transaction`, `Row`, `DbValue`, `DbParam`, `Query`, `DbError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `connect` returns owned `Pool`. `query` returns owned `Vec<Row>` (cells are `Vec<DbValue>`). |
| R3 — Error mapping | Every fallible op returns `Result<T, DbError>`. `sqlx::Error` auto-converts via `From`. |
| R4 — Thread safety | `Pool` is `Clone + Send + Sync + 'static`. `Transaction<'a>` borrows from `Pool` for its lifetime (NOT `'static` — by design). |
| R5 — Lifetime hiding | No public lifetime parameters on `Pool` / `Row` / `DbValue` / `Query`. `Transaction<'a>` carries one (anchored to its `Pool`). |
| R6 — Panic boundary | No `catch_unwind` needed — no panic sites in non-test code. |

## Testing

```bash
cargo test -p buff-db
cargo clippy -p buff-db --all-targets -- -D warnings
cargo fmt -p buff-db --check
```

Tests use SQLite in-memory (`sqlite::memory:`) for hermeticity — no PostgreSQL server fixture required.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
