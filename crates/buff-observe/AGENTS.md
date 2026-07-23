# buff-observe

Structured observability for the Buff language. EXPERIMENTAL.

Wraps the [`tracing`](https://docs.rs/tracing) +
[`opentelemetry`](https://docs.rs/opentelemetry) ecosystem: spans,
structured fields, and metrics (Counter / Histogram / Gauge) with console
(default) and OTLP (gRPC) export. Pure-Rust MVP per T21.

## STRUCTURE

```
src/
├── lib.rs        # Public API re-exports + crate-level docs (FFI safety table).
├── error.rs      # ObserveError enum (thiserror) + From impls.
├── tracer.rs     # Tracer.bootstrap() — sets up the tracing subscriber layer.
├── span.rs       # Span.new(name) / .field(k,v) / .enter() — owned Span wrapper.
└── metrics.rs    # Counter / Histogram / Gauge new + inc/observe/set.

examples/
├── observe.rs          # Minimal span + counter smoke.
└── observe_spans.rs    # Nested-span demo.

tests/
└── core.rs       # API + snapshot tests (insta).
```

## PUBLIC API

```text
// Tracer:
Tracer.bootstrap()  -> Result<(), ObserveError>

// Span:
Span.new(name: String) -> Span
span.field(key: String, value: Value) -> Span   // builder
span.enter()                                       // records the span

// Metrics:
Counter.new(name)   / counter.inc() / counter.inc_by(n)
Histogram.new(name) / histogram.observe(value)
Gauge.new(name)     / gauge.set(value)
```

~12 public fns (well under any per-crate cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Change error variants | `src/error.rs` |
| Change tracer bootstrap / subscriber wiring | `src/tracer.rs` |
| Change span model / fields / enter | `src/span.rs` |
| Change Counter / Histogram / Gauge | `src/metrics.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule). `Tracer::bootstrap` wraps its body in
  `catch_unwind` (FFI guide R6 — a panic becomes `Err(ObserveError::Panic)`).
- **All public types are `Send + Sync`** (`tracing::Span` is `Send + Sync`).
- **No public lifetime parameters / raw pointers** (FFI guide R1/R5). All
  data is owned.
- **Errors via `Result<T, ObserveError>`** for every fallible op.
- **BTreeMap/BTreeSet only** where collections are used (project rule).

## INTEGRATION WITH BUFF LANGUAGE (codegen lowering — DEFERRED)

`Tracer` / `Span` / `Counter` / `Histogram` / `Gauge` are registered as
**namespace-only** prelude types in
`crates/buff-lang-types/src/prelude_types.rs`. The assoc + instance fns
currently resolve to `Type::Unknown` at the Buff Type level.

The coordinated `Type::Span` / `Type::Counter` / … variants in
`crates/buff-lang-types/src/ty.rs` plus codegen lowering arms in
`crates/buff-lang-codegen-rust/src/rust_codegen.rs` are a follow-up task
outside the T21 shared zone (sibling-task coordination concern — same
shape as the T7/T8/T9 forward-declaration precedent). This forward-
declaration lets `buff check` validate `Span.new("x")` syntax today;
`buff run` codegen integration lands when the coordinated sibling task
does.

## DEPS

All workspace-pinned:
- `tracing` 0.1 — spans + structured fields.
- `tracing-subscriber` 0.3 — fmt layer for console output.
- `opentelemetry` 0.27 — OTel SDK for OTLP export.
- `opentelemetry-otlp` 0.27 — OTLP gRPC exporter (pure-Rust via tonic + prost).
- Dev: `insta`.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T21.
- FFI guide: `crates/buff-lang-ffi-guide/GUIDE.md` (6 hard rules).
- Pattern: OpenTelemetry + tracing bridge.
