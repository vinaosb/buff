//! `buff-db` — database access MVP for the Buff language.
//!
//! Wraps [`sqlx`](https://crates.io/crates/sqlx) behind a safe, owned
//! surface that complies with the Buff FFI guide
//! (`crates/buff-lang-ffi-guide/GUIDE.md`).
//!
//! ## Surface
//!
//! | Type | What it is |
//! |---|---|
//! | [`Pool`] | connection pool wrapping `sqlx::any::AnyPool`. |
//! | [`Transaction`] | in-flight DB transaction; commit or rollback. |
//! | [`Row`] / [`DbValue`] | row + cell representation (owned `Vec<DbValue>`). |
//! | [`DbParam`] | bindable query parameter (i64 / f64 / String / bool / bytes / null). |
//! | [`Query`] / [`JoinKind`] | runtime query builder (NOT compile-time macros per T3). |
//! | [`DbError`] | crate-local error enum. |
//!
//! ## Public functions (20-cap; current surface 17)
//!
//! ### `Pool` (6)
//!  1. [`Pool::connect`] — async ctor. `Database.connect(url)` in Buff.
//!  2. [`Pool::from_inner`] — wrap an existing `AnyPool`.
//!  3. [`Pool::inner`] — borrow the underlying `AnyPool`.
//!  4. [`Pool::query`] — `SELECT`, returns `Vec<Row>`.
//!  5. [`Pool::query_one`] — `SELECT` expecting exactly one row.
//!  6. [`Pool::execute`] — DDL/DML, returns `u64` rows affected.
//!
//! ### `Transaction` (4)
//!  7. [`Transaction::commit`] — async, finalizes the transaction.
//!  8. [`Transaction::rollback`] — async, aborts the transaction.
//!  9. [`Transaction::execute`] — execute inside the tx.
//! 10. [`Transaction::query`] — query inside the tx.
//!
//! ### `Row` / `DbValue` (8)
//! 11. [`Row::get`] — column-name lookup.
//! 12. [`Row::to_map`] — flat `BTreeMap<String, String>` view.
//! 13. [`Row::column_names`] — column name slice.
//! 14. [`Row::len`] / [`Row::is_empty`] — cell count.
//! 15. [`DbValue::as_int`] / [`DbValue::as_float`] / [`DbValue::as_text`]
//!     / [`DbValue::as_bool`] / [`DbValue::is_null`] — typed accessors.
//!
//! ### `Query` builder (8 chainable + 1 terminal)
//! 16. [`Query::new`] — ctor (`Query.new("users")` in Buff).
//! 17. [`Query::select`] — column projection.
//! 18. [`Query::filter`] — WHERE predicate.
//! 19. [`Query::inner_join`] / [`Query::left_join`] / [`Query::join`] — JOIN.
//! 20. [`Query::order_by`] / [`Query::limit`] / [`Query::offset`] — ORDER/LIMIT.
//! — [`Query::sql`] — terminal, returns the rendered SQL String.
//!
//! ## FFI safety (per `crates/buff-lang-ffi-guide/GUIDE.md`)
//!
//! - R1: No `*const T` / `*mut T`. Inputs/outputs are owned `String`,
//!   `Vec<u8>`, `Vec<Row>`, and the opaque `Pool` / `Transaction`.
//! - R2: Rust owns every allocation. Buff holds owned `Pool` / `Row` values.
//! - R3: All fallible ops return `Result<T, DbError>`. No panics in
//!   non-test code.
//! - R4: `Pool` / `Row` / `DbValue` / `Query` are `Send + 'static` (the
//!   underlying `sqlx::AnyPool` is `Clone + Send + Sync`). `Transaction`
//!   borrows from `Pool` for its lifetime (NOT `'static` — by design).
//! - R5: No lifetimes exposed in `Pool` / `Row` / `Query` (only
//!   `Transaction<'a>` carries one, and it's anchored to its `Pool`).
//! - R6: No `catch_unwind` needed — no panic sites in the crate body.
//!
//! ## Scope
//!
//! MVP per T18 spec: SQLite + PostgreSQL drivers via `sqlx::any`. NO
//! migrations (deferred to v1.18+). NO compile-time SQL validation
//! (deferred to v1.19+ macro work). NO MySQL/MSSQL/Oracle (T18 must-not #5).

mod error;
mod pool;
mod query;
mod row;

pub use error::{DbError, Result};
pub use pool::{DbParam, Pool, Transaction};
pub use query::{JoinKind, Query};
pub use row::{DbValue, Row};
