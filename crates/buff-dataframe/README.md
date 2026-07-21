# buff-dataframe

Columnar DataFrame MVP for the Buff language. CPU-only. Schema-aware storage, CSV/JSON load, relational ops.

## Install

The crate is part of the Buff workspace and never published standalone:

```toml
# in a generated Buff Cargo.toml
[dependencies]
buff-dataframe = "2"
```

For direct Rust use during development:

```bash
cargo add --path crates/buff-dataframe
```

## Hello, DataFrame

```rust
use buff_dataframe::{DataFrame, AggOp};

let df = DataFrame::from_rows(
    vec!["name".into(), "age".into(), "city".into()],
    vec![
        vec!["Ada".into(),     "36".into(), "London".into()],
        vec!["Alan".into(),    "41".into(), "London".into()],
        vec!["Grace".into(),   "85".into(), "New York".into()],
    ],
);

let londoners = df.filter(|r| r.get_string("city") == Some("London")).unwrap();
let mean_age  = df.group_by("city").unwrap().agg("age", AggOp::Mean);
println!("{}", mean_age.to_table_string());
```

## Surface

| Type | What it is |
|---|---|
| `DataFrame` | schema-aware columnar table (column name → `Series`). |
| `Series`    | typed column (`Int`/`Float`/`String`/`Bool`). |
| `GroupBy`   | intermediate value produced by `df.group_by(col)`. |
| `ColumnKind`| tag enum for the four supported column dtypes. |
| `AggOp`     | tag enum for `agg(col, op)`: `Sum`/`Mean`/`Min`/`Max`/`Count`. |
| `DfError`   | crate-local error enum. |

25 public functions total (at the cap). See `src/lib.rs` for the full enumerated list with signatures.

## Status

`experimental` — registered in the Buff prelude with the experimental stability badge. API may change between minor versions before v1.18.

## Scope

- ✅ CSV load (`csv` crate, `has_headers(false)`)
- ✅ JSON-lines load (one JSON object per line)
- ✅ `select` / `filter` / `sort` / `head` / `len` / `join` / `group_by(...).agg(...)`
- ✅ Schema-aware column-kind inference at load time
- ❌ Parquet / Arrow / IPC formats (v1.18+)
- ❌ Streaming / chunked reads (v1.18+)
- ❌ Lazy execution / query plan (v1.18+)
- ❌ GPU dispatch (CPU-only per Metis G7)
- ❌ Polars FFI (kept out per T7 spec)

## License

MIT OR Apache-2.0 (same as the rest of the Buff workspace).
