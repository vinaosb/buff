//! # buff-dataframe
//!
//! Columnar DataFrame MVP for the Buff language. CPU-only per Metis G7.
//! Wraps the pure-Rust `csv` + `serde_json` crates behind a safe, owned-`Vec`
//! surface that complies with the Buff FFI guide.
//!
//! ## Surface
//!
//! | Type | What it is |
//! |---|---|
//! | [`DataFrame`] | schema-aware columnar table (`BTreeMap` name → [`Series`]). |
//! | [`Series`]    | typed column (`Int` / `Float` / `String` / `Bool`). |
//! | [`GroupBy`]   | intermediate value produced by `df.group_by(col)`, supports `.agg(...)`. |
//! | [`ColumnKind`] | tag enum for the four supported column dtypes. |
//! | [`AggOp`]     | tag enum for `agg(col, op)`: `Sum` / `Mean` / `Min` / `Max` / `Count`. |
//! | [`DfError`]   | crate-local error enum (file-not-found, schema mismatch, etc.). |
//!
//! ## Public functions (25-cap; current surface 25)
//!
//! ### `DataFrame` (16)
//!  1. [`DataFrame::from_csv`]    — load CSV (header row + inferred column kinds).
//!  2. [`DataFrame::from_json`]   — load JSON-lines (one JSON object per line).
//!  3. [`DataFrame::from_rows`]   — in-memory ctor (header + `Vec<Vec<String>>`).
//!  4. [`DataFrame::column_names`] — `Vec<&str>` of column names in declared order.
//!  5. [`DataFrame::ncols`]       — column count.
//!  6. [`DataFrame::len`]         — row count.
//!  7. [`DataFrame::is_empty`]    — `rows == 0`.
//!  8. [`DataFrame::get_column`]  — `Option<&Series>` lookup by name.
//!  9. [`DataFrame::select`]      — projection (returns new DataFrame).
//! 10. [`DataFrame::filter`]      — boolean-mask filter (closure returns `bool`).
//! 11. [`DataFrame::sort`]        — ascending lexicographic sort by column.
//! 12. [`DataFrame::head`]        — first `n` rows.
//! 13. [`DataFrame::join`]        — inner equi-join on a single column.
//! 14. [`DataFrame::group_by`]    — `GroupBy` value (one row per distinct key).
//! 15. [`DataFrame::agg`]         — per-group or whole-column aggregate.
//! 16. [`DataFrame::to_table_string`] — pretty-printed fixed-width table.
//!
//! ### `GroupBy` (3)
//! 17. [`GroupBy::agg`]           — per-group aggregate `(col, op) -> DataFrame`.
//! 18. [`GroupBy::len`]           — group count.
//! 19. [`GroupBy::into_df`]       — collapse into a DataFrame carrying the
//!     grouping marker (used by codegen to dispatch `.agg(...)` on the
//!     DataFrame receiver type).
//!
//! ### `Series` (6)
//! 20. [`Series::len`]
//! 21. [`Series::kind`]           — `ColumnKind` tag.
//! 22. [`Series::as_int_slice`]    — `Option<&[i64]>`.
//! 23. [`Series::as_float_slice`]  — `Option<&[f64]>`.
//! 24. [`Series::as_string_slice`] — `Option<&[String]>`.
//! 25. [`Series::as_bool_slice`]   — `Option<&[bool]>`.
//!
//! ## FFI safety (per `crates/buff-lang-ffi-guide/GUIDE.md`)
//!
//! - R1: No `*const T` / `*mut T`. Inputs/outputs are owned `Vec<T>` and `String`.
//! - R2: Rust owns every allocation. Buff holds owned `DataFrame` / `Series` values.
//! - R3: All fallible ops return [`Result`]/[`Option`]. No panics in non-test code.
//! - R4: [`DataFrame`] / [`Series`] / [`GroupBy`] are `Send + Sync` (plain owned data).
//! - R5: No lifetimes exposed — accessors borrow for the call only.
//! - R6: No `catch_unwind` needed — no panic sites in the crate body.
//!
//! ## Scope
//!
//! CPU-only (Metitis G7). No Parquet (v1.18+). No streaming (load-into-memory).
//! No GPU dispatch. No lazy execution (deferred to v1.18+ per T7 spec).

mod dataframe;
mod error;
mod groupby;
mod read;
mod series;

pub use dataframe::DataFrame;
pub use error::{DfError, Result};
pub use groupby::{AggOp, GroupBy};
pub use series::{ColumnKind, Series};
