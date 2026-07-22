# buff-jobs

> Background job queue + cron scheduler for the **Buff** language. Pure-Rust in-memory MVP.

`buff-jobs` wraps the [`cron`](https://crates.io/crates/cron) crate for cron-expression parsing and ships a process-local in-memory queue with priority deque, retry/backoff, and dead-letter routing. The T35 task spec defers the Redis backend to v1.18+.

**Status: experimental** (T35 v1.x frameworks).

## Quick start

```rust
use buff_jobs::{Backoff, Job, Priority, Queue, Worker};
use std::time::Duration;

let queue = Queue::memory();
queue.enqueue(Job::new("send-welcome-email").unwrap()).unwrap();
queue.enqueue(
    Job::new("generate-thumbnail").unwrap()
        .with_priority(Priority::High)
        .with_max_retries(5)
        .with_backoff(Backoff::exponential(Duration::from_secs(1), Duration::from_secs(60))),
).unwrap();

let worker = Worker::new(queue.clone());
let stats = worker.run(|job| {
    println!("processing: {}", job.payload());
    Ok(())
}).expect("worker.run");

assert_eq!(stats.succeeded, 2);
assert!(queue.is_empty());
```

## Cron scheduling

```rust
use buff_jobs::{Job, Scheduler};
use std::time::Duration;

let scheduler = Scheduler::new();
// Register a cron job (7-field cron expression)
scheduler.cron("0 0 * * * *", Job::new("hourly-report").unwrap()).await.unwrap();
// Register an interval job
scheduler.interval(Duration::from_secs(60), Job::new("health-check").unwrap()).await.unwrap();
```

## Public API

### `Queue` -- in-memory priority deque

| Method | Signature | Notes |
|---|---|---|
| `Queue::memory` | `() -> Queue` | In-memory backend (MVP). Redis is v1.18+. |
| `queue.enqueue` | `(Job) -> Result<JobId, JobsError>` | Priority-ordered insertion. |
| `queue.dequeue` | `() -> Result<Option<Job>, JobsError>` | Pops next-highest-priority. |
| `queue.len` / `is_empty` | `() -> usize` / `() -> bool` | Pending only (excludes in-flight). |
| `queue.dead_letter` | `() -> Vec<Job>` | Jobs that exhausted retries. |
| `queue.stats` | `() -> QueueStats` | Snapshot of all counters. |

### `Job` -- unit of work

| Method | Signature | Notes |
|---|---|---|
| `Job::new` | `(payload: String) -> Result<Job, JobsError>` | Empty payload rejected. UUID v4 id. |
| `job.with_priority` | `(Priority) -> Job` | Builder. Default `Normal`. |
| `job.with_max_retries` | `(u32) -> Job` | Builder. Default `3`. |
| `job.with_backoff` | `(Backoff) -> Job` | Builder. Default `Backoff::fixed(1s)`. |
| `job.id` / `payload` / `priority` / `max_retries` / `backoff` / `attempts` / `status` | accessors | |
| `job.next_retry_delay` | `() -> Result<Option<Duration>, JobsError>` | `None` when budget exhausted. |

### `Worker` -- drains queue, routes by handler result

| Method | Signature | Notes |
|---|---|---|
| `Worker::new` | `(Queue) -> Worker` | Cheap clone (Arc bump). |
| `worker.run` | `(FnMut(&Job) -> JobResult) -> Result<WorkerStats, JobsError>` | Drains queue; honors retry+dead-letter. |

### `Scheduler` -- cron + interval scheduling (async)

| Method | Signature | Notes |
|---|---|---|
| `Scheduler::new` | `() -> Scheduler` | Empty. |
| `scheduler.cron` | `(expr: &str, Job) -> Result<JobId, JobsError>` | 7-field cron. Validates expression. |
| `scheduler.interval` | `(Duration, Job) -> Result<JobId, JobsError>` | Fixed interval. |
| `scheduler.start` | `() -> ()` | Spawns background tick loop. |
| `scheduler.stop` | `() -> ()` | Signals shutdown. |
| `scheduler.remove` | `(JobId) -> bool` | Remove a scheduled job. |

### `Backoff` -- retry-delay schedule

| Variant | Formula |
|---|---|
| `Backoff::fixed(base)` | `delay(N) == base` |
| `Backoff::linear(base)` | `delay(N) == base * N` |
| `Backoff::exponential(base, max)` | `delay(N) == min(base * 2^(N-1), max)` |

### `Priority` -- `Low` / `Normal` / `High` / `Critical`

Higher-priority jobs dequeue first; FIFO within the same priority.

## Retry + dead-letter routing

For each dequeued job, the worker invokes the handler:

1. **`Ok(())`** -- job is ack-completed; `WorkerStats.succeeded` increments.
2. **`Err(reason)`** + attempts remain -- job is re-enqueued; `WorkerStats.retried` increments.
3. **`Err(reason)`** + budget exhausted -- job is routed to the dead-letter queue; `WorkerStats.dead_lettered` increments.

Backoff delays are computed via `Job::next_retry_delay` but the MVP does NOT sleep before re-enqueue.

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 -- No raw pointers | Public surface: `Queue`, `Job`, `Worker`, `Scheduler`, `Backoff`, `Priority`, `JobId`, `JobStatus`, `QueueStats`, `WorkerStats`, `JobsError`. No `*const`/`*mut`. |
| R2 -- Ownership boundary | `enqueue` consumes `Job`. `dequeue` returns owned `Option<Job>`. `dead_letter` returns owned `Vec<Job>`. |
| R3 -- Error mapping | Every fallible op returns `Result<T, JobsError>`. `cron::error::Error` auto-converts via `From`. |
| R4 -- Thread safety | `Queue` is `Send + Sync` (wraps `Arc<Mutex<...>>`). `Job`/`Backoff`/`Priority` are `Send + Sync`. |
| R5 -- Lifetime hiding | No public lifetime parameters. `Job::payload` is owned `String`. |
| R6 -- Panic boundary | `Scheduler::cron` validates cron expression via `cron::Schedule::from_str`. |

## Testing

```bash
cargo test -p buff-jobs
cargo clippy -p buff-jobs --all-targets -- -D warnings
cargo fmt -p buff-jobs --check
```

Tests are hermetic: all queue fixtures are constructed inline via `Job::new`. Snapshots via `insta`.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
