# buff-dataframe

Columnar DataFrame MVP for Buff. CPU-only per Metis G7. Wraps `csv` + `serde_json` behind a safe, owned-`Vec` surface.

## STRUCTURE

```
src/
├── lib.rs        # 90 lines — module wiring + crate-level docs + public re-exports
├── error.rs      # 70 lines — DfError (Io/Csv/Json/SchemaMismatch/UnknownColumn/TypeMismatch/Empty) + Result
├── series.rs     # 215 lines — Series enum (Int/Float/String/Bool) + ColumnKind
├── dataframe.rs  # 380 lines — DataFrame struct + from_rows/csv/json + select/filter/sort/head/join/group_by/to_table_string + RowView
├── groupby.rs    # 175 lines — GroupBy intermediate + AggOp (Sum/Mean/Min/Max/Count) + aggregate()
└── read.rs       # 175 lines — load_csv (csv::ReaderBuilder + has_headers(false)) + load_json (JSON-lines)
```

~1100 LOC total (well under the T7 cap of 2500). 25 public functions (at cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new column dtype | `series.rs` (extend `Series` + `ColumnKind`) + `read.rs::infer_column_kind`/`build_series` + `dataframe.rs::compare_cells`/`iterate_as_string` |
| Add a new relational op | `dataframe.rs` (new public method on `DataFrame`) |
| Add a new aggregation | `groupby.rs` (new `AggOp` variant + match arm in `aggregate()`) |
| Change CSV/JSON parsing | `read.rs` |
| Add column-kind inference rule | `dataframe.rs::infer_column_kind` |
| Wire DataFrame to Buff codegen | `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` (assoc fns) + `lower_prelude_type_instance_fn` (instance methods) + `crates/buff-lang-types/src/prelude_types.rs` |

## CONVENTIONS (this crate only)

- **`BTreeMap` storage** — never `HashMap`. Project hard rule (deterministic ordering, snapshot-stable output).
- **No `unwrap`/`expect`/`panic!`/`todo!`** in non-test code. All fallible ops return `Result<T, DfError>` or `Option<T>`.
- **`#![forbid(unsafe_code)]` is NOT added** per AGENTS.md (CI enforces via `cargo clippy --all-targets -D warnings`).
- **All ops eager** — no lazy execution / query plan in MVP (deferred to v1.18+ per T7 spec).
- **CPU-only** — no GPU dispatch, no parallelism primitives (Metis G7).
- **In-memory only** — entire CSV/JSON is loaded into `Vec`s; no streaming / chunked reads.
- **Schema inferred at load time** — `infer_column_kind` decides `Int`/`Float`/`Bool`/`String` per column from cell content (empty cells ignored; `Int` if all parse as i64, `Float` if all parse as f64, `Bool` if all are `true`/`false`, else `String`).
- **Tests**: `tests/` directory — 11 test files covering load_csv/load_json/from_rows/select/filter/sort/head/join/group_by/agg/error.
- **Snapshot tests**: `tests/dataframe_csv_inference.rs`, `tests/dataframe_groupby.rs`, etc.

## ANTI-PATTERNS (THIS CRATE)

- ❌ **Streaming / chunked reads** — out of scope, deferred to v1.18+.
- ❌ **Parquet / Arrow / IPC formats** — out of scope, deferred to v1.18+.
- ❌ **Lazy execution / query plan** — out of scope, deferred to v1.18+.
- ❌ **GPU dispatch** — explicitly forbidden by Metis G7.
- ❌ **Polars dependency** — kept out per T7 spec ("Do NOT add `polars` as direct dependency if it requires complex build setup — wrap simple parts only"). The CSV + JSON surface here is sufficient for the MVP.
- ❌ **`unwrap`/`expect`/`panic!`** in non-test code — project hard rule from AGENTS.md.

## UNIQUE STYLES

- **`RowView` transient accessor** — passed to `filter(predicate)` closures so the user can write `|row| row.get_int("age") > 18` without exposing the `BTreeMap` internals or the `Series` enum.
- **`AggOp` enum (not string-typed)** — `agg(col, AggOp::Mean)` is type-safe; the `parse("mean")` helper is provided for string-driven dispatch from Buff codegen (which lowers Buff `agg("col", "mean")` to `AggOp::parse("mean").unwrap_or(AggOp::Count)`).
- **String-coerced aggregation** — `GroupBy::agg(col, op)` always returns a `DataFrame` whose value column is `String` (the format-then-reinfer roundtrip lets us mix Int/Float/Bool min/max/sum cleanly without complicating the surface). Users re-coerce with `get_column(name).as_int_slice()` etc.
- **`from_rows` is the universal ctor** — `from_csv`/`from_json` parse to `Vec<Vec<String>>` then delegate to `from_rows`, which infers column kinds once for the whole frame. This keeps the inference logic in ONE place (DRY).
