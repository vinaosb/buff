+++
title = "DataFrame"
weight = 50
+++

# DataFrame recipes (buff-dataframe)

Recipes for columnar data analysis with the `DataFrame` prelude type
(T7 v1.13 frameworks wave 2). `DataFrame` wraps the in-tree
`buff-dataframe` crate — schema-aware storage, CSV/JSON load,
relational ops. CPU-only per Metis G7.

> **Status:** `DataFrame` carries an EXPERIMENTAL stability badge
> through v1.18. API may change between minor versions before
> stabilisation.

## Load a CSV into a DataFrame

**Problem**: Read a CSV file with headers into a typed DataFrame.

**Solution**:

```buff
func main():
    let df = DataFrame.from_csv("users.csv")
    print("loaded " + df.len().string() + " rows")
    print(df.to_table_string())
```

**Explanation**:

`DataFrame.from_csv(path)` is the prelude surface for
`buff_dataframe::DataFrame::from_csv` (T7). It uses the `csv` crate
with `has_headers(false)` — every row including the header is parsed
as a `Vector<String>`, then schema-aware column-kind inference runs
once for the whole frame: `Int` if every cell parses as `i64`, `Float`
if every cell parses as `f64`, `Bool` if every cell is `true`/`false`,
else `String`. Empty cells are ignored during inference.

For JSON-lines input, use `DataFrame.from_json(path)` (one JSON
object per line — same shape, parses to rows then delegates to the
same column-kind inference). For other formats (Parquet, Arrow, IPC),
use the FFI surface — those are deferred to v1.18+ per the T7 spec.

## Filter rows

**Problem**: Keep only rows that satisfy a predicate.

**Solution**:

```buff
func adults(df: DataFrame) -> DataFrame:
    return df.filter({ row => Int(row.get("age")) >= 18 })

func main():
    let df = DataFrame.from_csv("users.csv")
    let grown_ups = adults(df)
    print(grown_ups.to_table_string())
```

**Explanation**:

`df.filter(predicate)` returns a new DataFrame containing only the
rows where `predicate(row)` returns `true`. The predicate receives a
`RowView` — a transient accessor that lets you read cell values by
column name (`row.get("age")` returns `String`; coerce with `Int(_)`,
`Float(_)`, `Bool(_)` as needed).

The filter is **eager** — it runs immediately and produces a fresh
DataFrame. No lazy execution / query plan in MVP (deferred to v1.18+
per T7 spec). For chained filters, prefer one combined predicate
(`{ row => cond1(row) and cond2(row) }`) over two `filter` calls —
the combined form scans the frame once.

## Group by and aggregate

**Problem**: Compute per-group statistics (mean age per city).

**Solution**:

```buff
func mean_age_by_city(df: DataFrame) -> DataFrame:
    return df.group_by("city").agg("age", "mean")

func main():
    let df = DataFrame.from_csv("users.csv")
    let report = mean_age_by_city(df)
    print(report.to_table_string())
```

**Explanation**:

`df.group_by(col)` returns a `GroupBy` intermediate value. Call
`.agg(col, op)` on it to compute per-group aggregates. Supported ops
are `"sum"`, `"mean"`, `"min"`, `"max"`, `"count"`. The result is a
fresh DataFrame with one row per group and two columns: the group key
plus the aggregated value (always surfaced as `String` — the
format-then-reinfer roundtrip lets us mix Int/Float/Bool cleanly
without complicating the surface).

For multiple aggregates, chain `.agg` calls or build them up in a
loop. Each `.agg` produces its own DataFrame; merge with `.join`
(see [Join two frames](#join-two-frames) below).

## Join two frames

**Problem**: Combine two DataFrames on a shared key column.

**Solution**:

```buff
func main():
    let users = DataFrame.from_csv("users.csv")
    let orders = DataFrame.from_csv("orders.csv")
    let joined = users.join(orders, on: "user_id")
    print(joined.to_table_string())
```

**Explanation**:

`df.join(other, on: key_col)` returns a new DataFrame containing the
inner-join of `df` and `other` on `key_col`. Rows in either frame that
don't match a row in the other are dropped (inner-join semantics —
outer joins are on the v1.18+ roadmap). Column-name collisions are
resolved by prefixing with the source frame name.

The join is **hash-based** — `buff-dataframe` builds a `BTreeMap` on
the key column of the right frame, then probes it for each row of the
left frame. O(n + m) time, O(m) extra space. For sorted inputs, a
merge-join would be faster on huge frames; that's deferred.

## Export to CSV

**Problem**: Write a DataFrame back to a CSV file.

**Solution**:

```buff
func main():
    let df = DataFrame.from_csv("users.csv")
    let adults = df.filter({ row => Int(row.get("age")) >= 18 })
    let csv_text = Csv.stringify(adults.to_rows())
    File.write("adults.csv", csv_text)?
    print("wrote " + adults.len().string() + " rows")
```

**Explanation**:

`DataFrame.to_rows()` returns the frame as a `Vector<Vector<String>>`
— the shape `Csv.stringify` consumes. The CSV output uses the same
quoting rules as the parser (RFC 4180 — fields with commas, quotes,
or newlines are double-quoted). Schema column ordering is preserved
(`BTreeMap` storage gives deterministic ordering).

For other export formats, swap `Csv.stringify` for `Toml.stringify`
(if the data is key/value shaped) or `Yaml.stringify`. There's no
`DataFrame.to_json` shortcut yet — iterate `df.to_rows()` and emit
one JSON object per line manually (or wait for v1.18+).

## Inspect the schema

**Problem**: List the columns and their inferred kinds.

**Solution**:

```buff
func main():
    let df = DataFrame.from_csv("users.csv")
    let cols = df.columns()
    for col in cols:
        let kind = df.column_kind(col)
        print(col + ": " + kind)
```

**Explanation**:

`df.columns()` returns the ordered `Vector<String>` of column names
(in the order they appear in the source CSV's header row).
`df.column_kind(name)` returns the inferred kind as a `String`:
`"Int"`, `"Float"`, `"Bool"`, or `"String"`. The kind is decided at
load time by `infer_column_kind` — every cell in the column is
checked, and the narrowest kind that fits all cells wins.

For a quick eyeball of the schema, `df.to_table_string()` renders the
frame as an ASCII table (first few rows + header) — useful for REPL
sessions and debugging. For machine-readable introspection, use
`.columns()` + `.column_kind(name)` as above.
