# buff-pipeline

DAG-based ETL pipeline framework for Buff. EXPERIMENTAL.

Bounded `Channel<T>` queues connect stages; backpressure is natural. Each
stage is a Buff closure reading from `Channel<T>` and writing to `Channel<U>`.

MVP per T14 (`.sisyphus/plans/buff-v1x-frameworks.md#L1975`):
- **Pipeline** (5 fns): `new`, `stage`, `source`, `sink`, `run`
- **Source** (1 fn): `from_csv` (chunked)
- **Sink** (3 fns): `to_csv`, `to_json`, `collect`
- **Stage helpers** (5 fns): `map`, `filter`, `batch`, `window`, `parallel`

Built on `buff-lang-runtime::Channel<T>` (T2). Does NOT use `Stream<T>`
(deferred to v1.18+). Does NOT use `select` (workers consume single Channel).

## Quick start

```rust
use buff_pipeline::{Pipeline, StageExt, SinkExt};

let mut p = Pipeline::new(64);
p.source(vec![1, 2, 3, 4, 5]);
p.map(|x| x * 2);
p.filter(|x| *x > 4);
let out: Vec<i32> = p.run().collect();
assert_eq!(out, vec![6, 8, 10]);
```

## Public API (14 fns, ≤ 40 cap per T14)

| Category | Function |
|---|---|
| Pipeline | `Pipeline::new(buffer_size)`, `p.stage(name, fn)`, `p.source(vec)`, `p.sink(fn)`, `p.run()` |
| Source | `Source::from_csv(path, chunk_size)` |
| Sink | `Sink::to_csv(path)`, `Sink::to_json(path)`, `Sink::collect()` |
| Stages | `p.map(fn)`, `p.filter(pred)`, `p.batch(size)`, `p.window(size, fn)`, `p.parallel(workers, fn)` |

Total: 14 public fns.

## Conventions

- **Layout**: each stage runs as a `tokio::spawn` task on its own thread.
- **Backpressure**: bounded Channel buffers; source blocks when downstream is slow.
- **Order**: items emerge in input order (no shuffling).
- **Errors**: `Result<_, PipelineError>`. No `unwrap`/`expect`/`panic!` in non-test code (project hard rule; enforced via `#![forbid(clippy::*)]` at crate root).

## Scope caps (T14 spec)

| Cap | MVP value | v1.18+ target |
|---|---|---|
| Sources | `from_csv` + `source(Vec<T>)` | Kafka / Redis Streams |
| Sinks | `to_csv` / `to_json` / `collect` | Parquet / Arrow |
| Concurrency | per-stage tokio task | multi-source `select` |
| Delivery | at-least-once (channel close) | exactly-once |
| Orchestration | NONE (caller-driven) | scheduler + retries + checkpoints |

## Integration with Buff language

Codegen integration is deferred. The crate is callable from Rust tests/examples today;
`buff run` integration lands with the coordinated sibling task that adds
`Type::Pipeline` to `crates/buff-lang-types/src/ty.rs` + the codegen lowering arm in
`crates/buff-lang-codegen-rust/src/rust_codegen.rs`.

## Examples

Three `.buff` examples at `examples/pipeline/`:
- `simple.buff` — map/filter on small list ([1,2,3,4,5] → [6,8,10])
- `csv_etl.buff` — stream CSV through filter+map to output CSV
- `parallel.buff` — `p.parallel(workers: 4, expensive_fn)`

## License

MIT OR Apache-2.0 (same as the rest of the workspace).
