# buff-observe

Structured observability for the Buff language. EXPERIMENTAL.

Pure-Rust MVP wrapping [`tracing`](https://docs.rs/tracing) +
[`opentelemetry`](https://docs.rs/opentelemetry): spans, structured
fields, and metrics (Counter / Histogram / Gauge) with console (default)
and OTLP (gRPC) export.

## Quick start

```rust
use buff_observe::{Tracer, Span, Counter};

Tracer::bootstrap().unwrap();
let mut span = Span::new("request".to_string());
span.field("user".to_string(), serde_json::json!("alice"));
span.enter();

let counter = Counter::new("hits".to_string());
counter.inc();
```

## Public API

| Category | Function | Notes |
|---|---|---|
| Tracer | `Tracer.bootstrap()` | Sets up the tracing subscriber layer |
| Span | `Span.new(name)` | Owned span |
| Span | `span.field(k, v)` / `span.enter()` | Builder + record |
| Metrics | `Counter.new(name)` / `.inc()` / `.inc_by(n)` | Monotonic counter |
| Metrics | `Histogram.new(name)` / `.observe(value)` | Distribution |
| Metrics | `Gauge.new(name)` / `.set(value)` | Point-in-time value |

## Conventions

- **All public types are `Send + Sync`** (`tracing::Span` is `Send + Sync`).
- **No `unwrap`/`expect`/`panic!`** in non-test code (project hard rule).
  `Tracer::bootstrap` wraps its body in `catch_unwind` (FFI guide R6).
- **No public lifetime parameters / raw pointers** (FFI guide R1/R5).

## Integration with Buff language

`Tracer` / `Span` / `Counter` / `Histogram` / `Gauge` are registered as
namespace-only prelude types. The assoc + instance fns resolve to
`Type::Unknown` for MVP; the coordinated `Type::*` variants + codegen
lowering arms are a follow-up sibling task. `buff check` validates the
syntax today; `buff run` integration lands with that sibling task.

## Examples

- `examples/observe.rs` — minimal span + counter smoke.
- `examples/observe_spans.rs` — nested-span demo.

## License

MIT OR Apache-2.0 (same as the rest of the workspace).
