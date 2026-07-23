# buff-pipeline

DAG-based ETL pipeline framework for Buff. **EXPERIMENTAL** (T14, v1.13 frameworks wave 3).

Linear chain of stages connected by bounded `buff_lang_runtime::Channel<T>` MPSC queues. Each stage is a `tokio::spawn` task reading from a `Receiver<T>` and writing to a `Sender<U>`. Bounded buffer capacity provides natural backpressure.

## OVERVIEW

The `Pipeline<T>` type carries a **continuation closure** (`spawner`) that, when invoked with a `Sender<T>`, spawns the entire upstream chain (source + all stages) as tokio tasks. Each builder method (`.stage()` / `.map()` / etc.) consumes `self`, wraps the previous spawner in a new `Channel<T,U>` pair + task, and returns a new `Pipeline<U>`. This threads heterogeneous type parameters through the Rust type system without `dyn Any` type erasure.

| Component | Role |
|-----------|------|
| `Pipeline<T>` | Typed linear-chain builder; `run()` drains to `Vec<T>` |
| `Source` | Factory namespace: `from_csv` streaming chunked reader |
| `Sink` | Factory namespace: `to_csv` / `to_json` / `collect` |
| Stages | `map` / `filter` / `batch` / `window` / `parallel` |

## STRUCTURE

```
buff-pipeline/
├── Cargo.toml             # buff-lang-runtime + tokio + rayon + csv + serde + serde_json + thiserror
├── src/
│   ├── lib.rs             # crate docs + pub use + forbid(clippy::*) lints + DEFAULT_BUFFER
│   ├── error.rs           # PipelineError (5 variants, thiserror) + PipelineResult + From impls
│   ├── pipeline.rs        # Pipeline<T> continuation struct + new/source/stage/sink/run + Debug + Default
│   ├── source.rs          # Source namespace + from_csv (streaming, rayon-parallel chunk parse)
│   ├── sink.rs            # Sink namespace + to_csv/to_json/collect
│   └── stage.rs           # map/filter/batch/window/parallel stage helpers (impl<T> Pipeline<T>)
├── tests/
│   ├── unit_tests.rs      # 24 integration tests (10+ required) + 5 inline insta snapshots + 2 proptests
│   └── snapshots/         # README.md (inline snapshots used — no .snap files on disk)
└── examples/pipeline/     # 3 .buff forward-declaration examples
```

~1181 LOC src / ~1755 LOC total (well under 4000 cap).

## PUBLIC API (15 fns, <= 40 cap)

| # | Type | Function | Notes |
|---|------|----------|-------|
| 1 | `Pipeline<()>` | `new()` | Empty builder; returns `Pipeline<()>` with no-op spawner |
| 2 | `Pipeline<()>` | `source(data: Vec<T>)` | In-memory source to `Pipeline<T>` |
| 3 | `Pipeline<T>` | `with_buffer(n)` | Override inter-stage channel capacity (default 64) |
| 4 | `Pipeline<T>` | `stage(name, fn)` | Generic named transform stage `T -> U` |
| 5 | `Pipeline<T>` | `sink(fn)` | Side-effecting passthrough `Fn(&T)` |
| 6 | `Pipeline<T>` | `run()` | Drain to `Vec<T>` (builds multi-thread tokio runtime) |
| 7 | `Pipeline<T>` | `map(fn)` | Sugar: `stage("map", fn)` |
| 8 | `Pipeline<T>` | `filter(pred)` | Keep items where `Fn(&T) -> bool` is true |
| 9 | `Pipeline<T>` | `batch(size)` | Group N items into `Vec<T>` |
| 10 | `Pipeline<T>` | `window(size, reduce)` | Collect N items, reduce, emit `U` |
| 11 | `Pipeline<T>` | `parallel(workers, fn)` | N tokio tasks via round-robin dispatcher |
| 12 | `Source` | `from_csv(path, chunk_size)` | Streaming CSV reader (spawn_blocking + rayon) |
| 13 | `Sink` | `collect(pipeline)` | Sugar: `pipeline.run()` |
| 14 | `Sink` | `to_csv(path, rows)` | Write `Vec<Vec<String>>` to CSV |
| 15 | `Sink` | `to_json(path, rows)` | Write `T: Serialize` to pretty JSON |

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a PipelineError variant | `src/error.rs` |
| Add a new stage helper | `src/stage.rs` (extend `impl<T> Pipeline<T>` — mind the 40-fn cap) |
| Change source behavior | `src/source.rs` |
| Change sink behavior | `src/sink.rs` |
| Change the continuation / concurrency model | `src/pipeline.rs` |
| Change default buffer size | `src/lib.rs::DEFAULT_BUFFER` |

## CONVENTIONS

- **NO `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code (project hard rule). Enforced via `#![cfg_attr(not(test), forbid(clippy::unwrap_used))]` + `expect_used` + `panic` at crate root.
- **`BTreeMap`/`BTreeSet` only** where used (no HashMap/HashSet — project hard rule). This crate currently uses neither.
- **Errors via `thiserror::Error`** derive. `PipelineError` has 5 variants: `Io`, `Csv`, `Json`, `Runtime`, `Config`. `From<std::io::Error>` / `From<csv::Error>` / `From<serde_json::Error>` impls enable `?` propagation.
- **Channel-based concurrency** — every stage runs as a `tokio::spawn` task reading from `buff_lang_runtime::Channel<T>` and writing to `Channel<U>`. Bounded buffers (default 64) provide backpressure naturally.
- **Continuation pattern** — `Pipeline<T>` stores `Box<dyn FnOnce(Sender<T>) -> JoinHandle<()> + Send + 'static>`. Each builder wraps the previous spawner; no `dyn Any` type erasure needed.
- **`parallel(workers, fn)` stage** — round-robin dispatcher task distributes items to N worker input channels (one per worker). Each worker clones the transform, applies it, sends to a shared output sender. A supervisor task awaits all workers. Output order is NOT preserved.
- **`Source::from_csv` streaming** — runs inside `tokio::task::spawn_blocking`; reads `chunk_size` records, parses via `rayon::par_iter`, pushes rows via `sender.0.blocking_send(row)` (parks the thread on backpressure). Bounded memory regardless of CSV size.

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
| `buff-lang-runtime` | Provides `Channel<T>` MPSC primitive (T2). All inter-stage queues use it. `Sender<T>: Clone` is a manual impl in buff-lang-runtime (since v1.22.0) that correctly omits the `T: Clone` bound — mirrors tokio upstream. |
| `buff-dataframe` (T7) | Potential source/sink for batch ops. Not yet integrated in MVP. |
| `buff-lang-codegen-rust` | Future codegen layer that lowers `Pipeline.*` / `Source.*` / `Sink.*` Buff calls into `buff_pipeline::*` Rust paths. Deferred to a follow-up task. |

## DEPS

- `buff-lang-runtime` (path) — `Channel<T>` + `Sender<T>` + `Receiver<T>` (T2)
- `tokio` (workspace 1.40, `full` + `test-util`) — runtime, spawn, spawn_blocking
- `rayon` (workspace 1.10) — parallel CSV row parsing in `Source::from_csv` chunks
- `csv` (workspace 1) — streaming CSV reader + writer
- `serde` (workspace 1, `derive`) — `Serialize` bound for `Sink::to_json`
- `serde_json` (workspace 1) — JSON sink
- `thiserror` (workspace 1.0) — `PipelineError` derive
- `insta` (dev-only, workspace 1.40) — 5 inline snapshot tests
- `proptest` (dev-only, workspace 1.5) — 2 property tests
- `tempfile` (dev-only, workspace 3) — hermetic temp dirs for CSV round-trip tests

## NOTES

- **MSVC host blocker**: `cargo test -p buff-pipeline` was verified to pass on this Windows host after manually configuring `LIB` / `INCLUDE` environment variables to include the VS 18 Insiders MSVC `lib\onecore\x64` path + Windows SDK 10.0.26100.0 UCRT paths. The pre-existing `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` issue (documented in buff-image's AGENTS.md) is caused by empty `LIB`/`INCLUDE` env vars on this host. CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue.
- **Pre-existing dependency clippy fixes**: `buff-lang-debug-info` (3 doc-comment + 1 `match_result_ok` fixes) and `buff-lang-runtime::Channel::new` (`new_ret_no_self` allow) had pre-existing clippy lints that blocked `cargo clippy -p buff-pipeline --all-targets -- -D warnings` from completing. These were fixed as minimal infrastructure unblocks (no logic changes).
- **Sender<T> clone**: `buff_lang_runtime::Sender<T>` has a manual `impl<T> Clone for Sender<T>` (since v1.22.0) that correctly omits the `T: Clone` bound — mirrors tokio upstream (`tokio::sync::mpsc::Sender<T>` is Clone for any `T`). The `parallel` stage uses `out_sender.clone()` directly.
- **Inline snapshots**: The 5 insta snapshots are inline (`assert_snapshot!(value, @"expected")`) — no `.snap` files on disk. See `tests/snapshots/README.md` for the rationale + migration path.

## LICENSE

Dual-licensed under MIT or Apache-2.0, matching the rest of the Buff workspace.
