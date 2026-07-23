# buff-pipeline

DAG-based ETL pipeline framework for Buff. Bounded `Channel<T>` queues connect stages; backpressure is natural.

**Status: experimental** (T14 v1.22 Wave 10).

## OVERVIEW

Each `Pipeline` is a linear chain of stages. A source pushes items into a `Channel<T>`; each stage consumes its input `Channel<T>` and produces a `Channel<U>` for the next stage; a sink drains the final channel. Bounded buffer capacity provides natural backpressure.

| Component | Role |
|-----------|------|
| `Pipeline` | Linear chain builder + `run()` |
| `Source::from_csv` | Chunked CSV reader → bounded channel |
| `Sink::{to_csv,to_json,collect}` | Drain final channel to file or `Vec<T>` |
| Stages `map`/`filter`/`batch`/`window`/`parallel` | Build common stage kinds |

## STRUCTURE

```
buff-pipeline/
├── Cargo.toml          # workspace deps: buff-lang-runtime (Channel) + thiserror + rayon + csv + serde + serde_json
├── src/
│   ├── lib.rs          # Public API + re-exports
│   ├── error.rs        # PipelineError enum (thiserror)
│   ├── pipeline.rs     # Pipeline + stage/sink/run
│   ├── source.rs       # Source::from_csv chunked reader
│   ├── sink.rs         # Sink::{to_csv,to_json,collect}
│   └── stage.rs        # map/filter/batch/window/parallel stages
└── tests/
    ├── unit_tests.rs   # 10+ integration tests
    └── snapshots/      # insta snapshots
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a PipelineError variant | `src/error.rs` |
| Add a new stage kind | `src/stage.rs` (extend the 5-helper set carefully — 40-fn cap) |
| Change source behavior | `src/source.rs` |
| Change sink behavior | `src/sink.rs` |
| Change the run loop / concurrency | `src/pipeline.rs` |

## PUBLIC API

### `Pipeline` (5)
- `Pipeline::new(buffer_size)` — create builder
- `p.stage(name, transform)` — add a custom stage
- `p.source(data: Vec<T>)` — batch source (pushes all items into Channel<T>)
- `p.sink(terminal_fn)` — terminal drain
- `p.run()` — execute the chain

### `Source` (1)
- `Source::from_csv(path, chunk_size)` — chunked CSV reader

### `Sink` (3)
- `Sink::to_csv(path)`, `Sink::to_json(path)`, `Sink::collect()` (returns `Vec<T>`)

### Stages (5)
- `p.map(fn)`, `p.filter(pred)`, `p.batch(size)`, `p.window(size, fn)`, `p.parallel(workers, fn)`

Total: **14 public fns** (well under 40 cap).

## CONVENTIONS

- **NO `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code (project-wide rule). Enforced via `#![cfg_attr(not(test), forbid(clippy::unwrap_used))]` and friends at crate root.
- **`BTreeMap`/`BTreeSet` only** where used (no HashMap/HashSet — project hard rule).
- **Errors via `thiserror::Error`** derive. `PipelineError` has variants for channel-closed, source-io, sink-io, parse, and capacity errors.
- **Channel-based concurrency** — every stage runs as a `tokio::spawn` task reading from `buff_lang_runtime::Channel<T>` and writing to `Channel<U>`. Bounded buffers provide backpressure naturally.
- **`parallel(workers, fn)` stage** — spawns N worker tasks each pulling from a shared dispatch channel (single-producer-multi-consumer pattern via per-worker channels). Output preserved via supervisor that awaits all workers before draining the final sender.

## DEFERRED (v1.18+)

Per T14 spec:

- **`Stream<T>` consumption** — T2 deferred general Stream type; this crate uses sync `Vec<T>` batches + `Channel<T>`.
- **Kafka/Redis Streams sources** — v1.18+.
- **Exactly-once delivery guarantees** — v1.18+.
- **Pipeline orchestration** (scheduling, retries, checkpointing) — separate concern (v1.18+).
- **`select` expression** for multi-source consumption — workers consume a single Channel each.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-runtime` | Provides `Channel<T>` MPSC primitive (T2). All inter-stage queues use it. |
| `buff-dataframe` (T7) | Optional source/sink for batch ops. (Not yet integrated in MVP.) |
| `buff-lang-codegen-rust` | Future codegen layer that lowers `Pipeline.*` / `Source.*` / `Sink.*` Buff calls into `buff_pipeline::*` Rust paths. |

## DEPS

- `buff-lang-runtime` (path) — Channel<T>
- `thiserror` — error derive
- `rayon` — parallel stage workers (currently uses tokio spawn instead)
- `csv` — chunked CSV source/sink
- `serde` + `serde_json` — JSON sink
- `crossbeam-channel` — sync test paths
- `insta` (dev-only) — snapshot tests
- `proptest` (dev-only) — property tests
- `tempfile` (dev-only) — test fixtures

## LAUNCH

```bash
cargo build -p buff-pipeline
cargo test -p buff-pipeline
cargo clippy -p buff-pipeline --all-targets -- -D warnings
```

## LICENSE

Dual-licensed under MIT or Apache-2.0, matching the rest of the Buff workspace.
