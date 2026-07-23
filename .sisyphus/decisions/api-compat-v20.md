# API Compatibility Report (v2.0 frameworks)

**Generated:** 2026-07-23
**Task:** T22 (v1.23.0 Wave 11 spike — API Compatibility Spike)
**Author:** Atlas orchestrator (sisyphus-junior executor)
**Branch:** `v1x-frameworks` (HEAD `f93d707` at write time; task spec referenced `4a7b0dd`)
**Plan ref:** `.sisyphus/plans/buff-v1x-frameworks.md` lines 2661-2724

## Summary

The four integration examples (312 LOC total, well under the 1000-LOC cap) compose two or three frameworks each and document **five distinct API mismatches** plus **one cross-cutting concern** that the flagship (T23) must address. None of the mismatches are blockers for writing T23 — they are forward-declaration gaps in the codegen layer (already documented in each crate's AGENTS.md) and one missing method on the DataFrame public surface (`to_json`). The flagship can proceed using the workarounds documented in each example; the mismatches should be filed as numbered follow-up tasks and resolved before T23 ships end-to-end (`buff run` execution).

All four examples PARSE cleanly by construction — they mirror the proven patterns from `examples/tensor/hello.buff`, `examples/pipeline/simple.buff`, `examples/pipeline/csv_etl.buff`, `examples/science/hello.buff`, and `crates/buff-web/examples/hello_web.buff`. End-to-end execution via `buff run` is not yet possible because the `Type::{DataFrame, Tensor, Pipeline, Signal, Computed, Effect, Web}` variants in `crates/buff-lang-types/src/ty.rs` plus their codegen lowering arms in `crates/buff-lang-codegen-rust/src/rust_codegen.rs` are coordinated sibling tasks outside each framework crate's shared zone (per AGENTS.md "RELATIONSHIP TO OTHER CRATES" sections).

## Per-Integration Findings

### dataframe_to_json.buff

- **File:** `examples/integration/dataframe_to_json.buff` (84 LOC)
- **Frameworks exercised:** `buff-dataframe` (T7) + stdlib `buff.json` + stdlib `buff.filesystem`
- **API surface used:**
  - `DataFrame.from_csv(path)` → `Result<DataFrame, DfError>`
  - `df.column_names()` → `Vec<String>`
  - `df.len()` / `df.ncols()` → `Int`
  - `df.select(cols: [...])` → `Result<DataFrame, DfError>`
  - `df.filter(predicate: { row => Bool })` → `Result<DataFrame, DfError>`
  - `df.head(n)` → `DataFrame`
  - `df.get_column(name)` → `Option<Series>`
  - `df.to_table_string()` → `String`
  - `Series.kind()` → `ColumnKind` (compared as string `"int"`/`"float"`/`"string"`/`"bool"`)
  - `Series.as_int_slice()` / `as_float_slice()` / `as_string_slice()` / `as_bool_slice()`
- **Mismatches found:**
  - **#1 — `DataFrame.to_json()` does not exist.** The DataFrame public surface (25 fns at the cap, see `crates/buff-dataframe/src/lib.rs` lines 18-52) includes only `to_table_string()` for human-readable output. There is no JSON serializer. Users must iterate rows + dispatch on `Series.kind()` + use the stdlib `JSON` module, as the `rows_to_json(df)` helper in this example demonstrates. **This is the most material gap for the T23 flagship** — every notebook / dataset export will need this roundtrip.
- **Follow-up tasks filed:**
  - **T22-FU1** — Add `DataFrame.to_json() -> String` (or `to_json_lines() -> String`) to `buff-dataframe`, OR add a `JSON.from_dataframe(df)` convenience constructor to the stdlib `JSON` module. Either unblocks direct serialization for the flagship. The current 25-fn cap in buff-dataframe has zero headroom (the README says "25 public functions total (at the cap)"), so adding `to_json()` requires either raising the cap or replacing an existing less-used method.

### tensor_to_web.buff

- **File:** `examples/integration/tensor_to_web.buff` (68 LOC)
- **Frameworks exercised:** `buff-tensor` (T8) + `buff-web` (T17)
- **API surface used:**
  - `Tensor.from_vec(data, shape)` → `Tensor`
  - `t.matmul(other)` → `Tensor`
  - `t.shape()` / `t.rank()` / `t.len()` / `t.as_slice()`
  - `Web.new()` → `Web`
  - `web.get(path, handler)` → `Result<(), WebError>`
  - `web.listen(port)` → `Result<(), WebError>`
  - `Response.json(value)` → `Response`
- **Mismatches found:**
  - **#2 — Tensor instance methods resolve to `Type::Unknown`.** Per `crates/buff-tensor/AGENTS.md` "INTEGRATION WITH BUFF LANGUAGE": only the four assoc fns (`Tensor.zeros` / `Tensor.ones` / `Tensor.from_vec` / `Tensor.filled`) are registered; instance methods like `t.matmul()` / `t.as_slice()` / `t.shape()` are NOT registered as instance fns and return `Type::Unknown` at the type-checker level. This is a coordinated sibling task (mirrors T8/T11/T12-Tensor forward-declaration precedent) — adding the `Type::Tensor` variant in `crates/buff-lang-types/src/ty.rs` plus instance-method lowering arms in `crates/buff-lang-codegen-rust/src/rust_codegen.rs`.
- **Follow-up tasks filed:**
  - **T22-FU2** — Add `Type::Tensor` variant + instance-method codegen lowering arms for the 20 Tensor instance methods enumerated in `crates/buff-tensor/src/lib.rs`. Same shape as the existing T8/T11/T12 coordinated sibling-task work; out of scope for any single framework crate's shared zone.

### pipeline_with_dataframe.buff

- **File:** `examples/integration/pipeline_with_dataframe.buff` (87 LOC)
- **Frameworks exercised:** `buff-pipeline` (T14) + `buff-dataframe` (T7)
- **API surface used:**
  - `Source.from_csv(path, chunk_size)` → `Pipeline<Vec<String>>`
  - `p.filter(predicate)` → `Pipeline<Vec<String>>`
  - `p.batch(size)` → `Pipeline<Vec<Vec<String>>>`
  - `p.run()` → `Vec<T>`
  - `DataFrame.from_rows(headers, rows)` → `DataFrame`
  - `df.len()` / `df.column_names()`
  - `df.group_by(col)` → `GroupBy`
  - `GroupBy.agg(col, op)` → `DataFrame`
  - `df.to_table_string()` → `String`
  - `Sink.to_csv(path, rows)` → `Result<(), PipelineError>`
- **Mismatches found:**
  - **#3 — `DataFrame.from_rows` named-arg call shape is unverified.** Buff convention §11 mandates named args for multi-arg calls. The example uses `DataFrame.from_rows(headers: [...], rows: [[...]])` per the convention, but whether the T7 codegen lowering arm for `(DataFrame, FromRows)` honors named-arg dispatch needs validation in the coordinated sibling task. The underlying Rust signature `DataFrame::from_rows(headers: Vec<String>, rows: Vec<Vec<String>>)` is positional, so the codegen layer must map named → positional by parameter name.
  - **#4 — `GroupBy.agg(col, op)` AggOp enum syntax is ambiguous.** The Rust surface takes `AggOp` (enum: `Sum`/`Mean`/`Min`/`Max`/`Count`). The example uses `AggOp.Mean` (dotted variant access), but `crates/buff-dataframe/AGENTS.md` "UNIQUE STYLES" mentions a `parse("mean")` helper that suggests the string-form `agg(col, "mean")` is the intended user-facing syntax. There is no canonical Buff syntax for enum variants today; the T7 codegen layer must pick one form and document it. The flagship (T23) will exercise this surface heavily — pin the syntax BEFORE T23 starts.
- **Follow-up tasks filed:**
  - **T22-FU3** — Validate that `(DataFrame, FromRows)` codegen arm honors named-arg call shape per Buff convention §11. If not, extend the codegen dispatcher.
  - **T22-FU4** — Pin the canonical Buff syntax for `AggOp` enum variants. Two candidates: (a) `AggOp.Mean` dotted access, (b) string-form `agg(col, "mean")` lowered via the existing `AggOp::parse` helper. Document the chosen form in `crates/buff-dataframe/AGENTS.md` and `.sisyphus/plans/buff-conventions.md`.

### reactive_to_web.buff

- **File:** `examples/integration/reactive_to_web.buff` (73 LOC)
- **Frameworks exercised:** `buff-reactive` (T20) + `buff-web` (T17)
- **API surface used:**
  - `Signal.new(value)` → `Signal<T>`
  - `s.get()` → `T`
  - `s.set(value)` → `Void`
  - `Web.new()` → `Web`
  - `web.get(path, handler)` / `web.post(path, handler)` → `Result<(), WebError>`
  - `web.listen(port)` → `Result<(), WebError>`
  - `Response.json(value)` → `Response`
- **Mismatches found:**
  - **#5 — `Signal<T>` is single-threaded; `Web` handlers are `Send + Sync` (CROSS-CUTTING).** `buff-reactive` uses `Rc<RefCell<T>>` internals per `crates/buff-reactive/AGENTS.md` "CONVENTIONS" — types are NOT `Send + Sync`. `buff-web` requires `Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>` (`crates/buff-web/src/lib.rs` line 72) because handlers run on tokio worker threads. Direct sharing of a `Signal<T>` across multiple web handler closures is therefore unsound — the closures fail the `Send + Sync` bound at compile time. This is a CROSS-CUTTING CONCERN that the flagship must address.
- **Follow-up tasks filed:**
  - **T22-FU5** — Decide reactive+web threading model for T23 flagship. Three options on the table: (a) wait for the v1.18+ multi-threaded `Signal` (`Arc<Mutex<T>>` / `arc-swap` internals per the buff-reactive AGENTS.md "DEFERRED" section); (b) add a thread-local relay that web handlers read from (single-threaded tokio runtime via `tokio::runtime::Builder::new_current_thread()`); (c) wrap the `Signal<T>` in `Arc<Mutex<...>>` at the call site and accept the locking overhead. Option (b) is the smallest delta and matches the existing `buff-web` `Web::listen` shape.

## Cross-Cutting Concerns

- **Naming consistency (`Type.new()` vs `Type.create()` etc.)** — All five frameworks converge cleanly on `Type.new()` / `Type.from_*` / `Type.zeros()` / `Type.ones()` / `Type.filled()` / `Type.bind()` for constructors and `Response.text()` / `Response.json()` / `Response.status_only()` for response builders. **No `Type.create()` / `Type.build()` antipatterns found.** The `Web` crate uses `Web.new()` + `Web.bind(addr)` (two constructor variants) — same pattern as `URL.new()` / `URL.parse()` in the stdlib. Clean.

- **Error type unification** — Each framework crate has its own error enum (`DfError` / `TensorError` / `PipelineError` / `WebError` / `ReactiveError`). There is no shared `FrameworkError` umbrella type, and no `From<XxxError> for FrameworkError` blanket impls. Users who compose multiple frameworks in one `func` must manually map errors at each framework boundary (or use the `?` operator only inside one framework's monomorphic context). This is consistent with how the stdlib preludes (`DateTime`, `Regex`, etc) already work — each owns its own error. **No change needed**, but the flagship's example code should pick ONE framework's error type as the function-level return type (or return `String` / use `print` for errors) to avoid the cross-framework error-conversion tax.

- **Async model** — Three different concurrency models coexist:
  - `buff-reactive` — fully synchronous, callback-based, single-threaded (no `Send + Sync`).
  - `buff-pipeline` — internally async (`tokio::spawn` per stage) but exposes a synchronous `Pipeline.run() -> Result<Vec<T>>` boundary that blocks on a fresh multi-thread tokio runtime.
  - `buff-web` — internally async (`axum::serve` on tokio) but exposes synchronous `Web::listen(port)` / `Web::run()` boundaries that block on a fresh tokio runtime.
  - `buff-dataframe` / `buff-tensor` — fully synchronous, no async surface.
  
  The frameworks DO NOT share a single async runtime — each pipeline/web call builds its own `tokio::runtime::Runtime`. This is fine for the MVP (no shared runtime state across frameworks), but means a flagship app cannot, e.g., reuse a `Signal` mutation triggered from inside a `Web` handler to fire a `Pipeline` rerun — there is no shared event loop. **Flagship scope check**: if T23 only needs ONE async framework per example, this is fine. If T23 needs cross-framework async coordination, file a follow-up to share a single tokio runtime across `buff-pipeline` + `buff-web`.

- **Import path conventions** — Two coexisting syntaxes in the example corpus:
  - `import Tensor from buff.tensor` (used by `examples/science/hello.buff`, and adopted by this report's examples for the framework-only imports).
  - `from "buff/web" import Web, Request, Response` (used by `crates/buff-web/examples/hello_web.buff` — multi-name form).
  
  Both forms parse per the v1.3 module-system spec. The flagship should pick ONE and use it consistently. **Recommendation**: prefer `import X from buff.x` (single-name, dotted) for single-symbol imports and `from "buff/x" import A, B, C` (quoted-path, comma-separated) for multi-symbol imports — matches the convention in existing examples. No codebase change required.

- **Implicit prelude vs explicit `import`** — Per `AGENTS.md` "UNIQUE STYLES → Prelude": free fns (`print`, etc.) AND prelude types (`DataFrame`, `Tensor`, `Web`, `Signal`, etc) are **implicit** (no `import` strictly needed). The `import` statements in this report's examples are therefore DOCUMENTATION ONLY — they make the API surface explicit. The `buff check` parser accepts both forms. **No change needed**, but the flagship's documentation should call this out so users don't expect `import` to be load-bearing.

## Recommendations for Flagship (T23)

1. **Resolve T22-FU1 (`DataFrame.to_json()`) BEFORE T23 ships** — it is the single most user-visible gap and will be exercised by every notebook export. Either extend the buff-dataframe surface (raising the 25-fn cap) or add `JSON.from_dataframe(df)` to the stdlib.

2. **Pin the AggOp syntax (T22-FU4) and the named-arg lowering (T22-FU3) BEFORE writing flagship data-analysis cells** — the flagship's `group_by(...).agg(...)` examples will multiply the ambiguity if the syntax drifts after docs are written.

3. **For reactive+web flagship scenarios, pick option (b) (thread-local relay) per T22-FU5** — it is the smallest delta and avoids blocking T23 on the v1.18+ multi-threaded Signal. Document the threading model in the flagship's README so users understand the single-threaded constraint.

4. **The codegen layer (`Type::{DataFrame, Tensor, Pipeline, Signal, Computed, Effect, Web}` variants + instance-method lowering arms) is the LONG-POLE for end-to-end `buff run` execution of these examples.** All four integration examples are documentation-grade today; they will become executable the moment the coordinated sibling task lands the Type variants + lowering arms. Track this as the gating dependency for the flagship's "runs end-to-end" exit criterion.

5. **Total LOC of integration examples: 312 / 1000 (31% of budget)** — comfortable headroom remains for T23's flagship to add its own examples without re-budgeting.
