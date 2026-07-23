# buff-pipeline

> DAG-based ETL pipeline for the **Buff** language. Bounded `Channel<T>` queues connect stages; backpressure is natural.

`buff-pipeline` wires a linear chain of stages via the T2 `buff_lang_runtime::Channel<T>` MPSC primitive. Each stage is a `tokio::spawn` task reading from a `Receiver<T>` and writing to a `Sender<U>`. The bounded buffer (default capacity 64) provides natural backpressure: when a downstream stage is slow, the upstream `send().await` parks until a slot opens.

**Status: experimental** (T14 v1.13 frameworks wave 3).

## Quick start

```rust
use buff_pipeline::Pipeline;

fn main() {
    // [1, 2, 3, 4, 5] -> map(*2) -> filter(>4) -> [6, 8, 10]
    let result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5])
        .map(|x| x * 2)
        .filter(|x| *x > 4)
        .run()
        .expect("pipeline runs");
    assert_eq!(result, vec![6, 8, 10]);
}
```

## CSV streaming ETL

```rust
use buff_pipeline::{Source, Sink};

fn main() -> buff_pipeline::PipelineResult<()> {
    // Stream a CSV through filter + map, bounded memory via Channel backpressure.
    let rows = Source::from_csv("input.csv", 100)?       // chunk_size = 100 rows
        .filter(|row: &Vec<String>| row.len() >= 2)      // drop malformed rows
        .map(|row| row.iter().take(2).cloned().collect::<Vec<String>>())
        .run()?;                                          // drain to Vec<Vec<String>>

    Sink::to_csv("output.csv", rows)?;                   // write back as CSV
    Ok(())
}
```

## Parallel workers

```rust
use buff_pipeline::Pipeline;

fn main() {
    // 4 workers consume from a shared input Channel via round-robin dispatch.
    // Output ORDER is not preserved (workers race); output CONTENT is.
    let mut result = Pipeline::new()
        .source(vec![1, 2, 3, 4, 5, 6, 7, 8])
        .parallel(4, |x| x * x)
        .run()
        .expect("run");
    result.sort(); // restore order for deterministic comparison
    assert_eq!(result, vec![1, 4, 9, 16, 25, 36, 49, 64]);
}
```

## Public API (15 fns, <= 40 cap)

### `Pipeline<T>` (11)

| Method | Signature | Notes |
|---|---|---|
| `Pipeline::new` | `() -> Pipeline<()>` | Empty builder. Call `.source()` next. |
| `Pipeline::source` | `(self, Vec<T>) -> Pipeline<T>` | In-memory source. |
| `Pipeline::with_buffer` | `(self, usize) -> Self` | Override channel capacity (default 64). |
| `Pipeline::stage` | `(self, name, Fn(T)->U) -> Pipeline<U>` | Generic named stage. |
| `Pipeline::sink` | `(self, Fn(&T)) -> Pipeline<T>` | Side-effecting passthrough. |
| `Pipeline::run` | `(self) -> Result<Vec<T>>` | Drain to Vec (builds tokio runtime). |
| `Pipeline::map` | `(self, Fn(T)->U) -> Pipeline<U>` | Sugar for `stage("map", fn)`. |
| `Pipeline::filter` | `(self, Fn(&T)->bool) -> Pipeline<T>` | Keep matching items. |
| `Pipeline::batch` | `(self, usize) -> Pipeline<Vec<T>>` | Group N items per batch. |
| `Pipeline::window` | `(self, usize, Fn(Vec<T>)->U) -> Pipeline<U>` | Reduce windows of N items. |
| `Pipeline::parallel` | `(self, workers, Fn(T)->U) -> Pipeline<U>` | N tokio workers (unordered). |

### `Source` (1)

| Method | Signature | Notes |
|---|---|---|
| `Source::from_csv` | `(path, chunk_size) -> Result<Pipeline<Vec<String>>>` | Streaming CSV reader (rayon-parallel chunk parse). |

### `Sink` (3)

| Method | Signature | Notes |
|---|---|---|
| `Sink::collect` | `(Pipeline<T>) -> Result<Vec<T>>` | Sugar for `pipeline.run()`. |
| `Sink::to_csv` | `(path, Vec<Vec<String>>) -> Result<()>` | Write rows to CSV file. |
| `Sink::to_json` | `(path, &T: Serialize) -> Result<()>` | Write to pretty JSON. |

## Behavior

### Backpressure

Every inter-stage `Channel<T>` has a bounded capacity (default 64, override via `.with_buffer(n)`). When the buffer is full, the upstream `send().await` parks the sending task until a slot opens. This gives end-to-end flow control: a slow sink throttles the source automatically.

### Ordering

`map` / `filter` / `batch` / `window` / `stage` / `sink` preserve input order (single-task per stage). `parallel` does NOT preserve order — workers race and the dispatcher round-robins without tracking completion order. Sort the output `Vec` if you need deterministic ordering after `parallel`.

### Concurrency model

`Pipeline::run` builds a fresh multi-thread tokio runtime, invokes the continuation spawner (which cascades all stage tasks), and drains the final output channel. Each stage is an independent `tokio::spawn` task. Tasks cooperate via `.await` yield points; the runtime schedules them across worker threads.

## Testing

```bash
cargo test -p buff-pipeline          # 24 integration + 4 unit + 12 doctests
cargo clippy -p buff-pipeline --all-targets -- -D warnings
cargo fmt -p buff-pipeline --check
```

Tests are hermetic: CSV tests use `tempfile::tempdir()` for automatic cleanup. Snapshot tests use inline `insta::assert_snapshot!` (no `.snap` files on disk). Property tests use `proptest`.

## Deferred to v1.18+

Per the T14 task spec, the following are explicitly out of scope for the MVP:

- **`Stream<T>` consumption** (deferred at T2 level — this crate uses `Channel<T>` + sync `Vec<T>`).
- **Kafka / Redis Streams sources.**
- **Exactly-once delivery guarantees.**
- **Pipeline orchestration** (scheduling, retries, checkpointing).
- **`select` expression** for multi-source fan-in.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
