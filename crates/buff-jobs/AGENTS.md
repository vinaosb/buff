# buff-jobs

Background job queue + cron scheduler for Buff. Pure-Rust in-memory MVP wrapping the `cron` crate for cron-expression parsing. Dead-letter queue + retry/backoff + priority ordering. Async worker + Redis backend deferred to v1.18+ per the T35 task spec.

**Status: experimental** (T35 v1.x frameworks).

## STRUCTURE

```
buff-jobs/
├── Cargo.toml            # cron + chrono + tokio + uuid + serde + thiserror deps
├── src/
│   ├── lib.rs            # re-exports all public types
│   ├── backoff.rs        # Backoff enum (Fixed/Linear/Exponential) + delay()
│   ├── error.rs          # JobsError enum + JobsResult<T> alias
│   ├── job.rs            # Job, JobId, Priority, JobStatus, JobResult
│   ├── queue.rs          # Queue (in-memory priority deque) + QueueStats
│   ├── scheduler.rs      # Scheduler (cron + interval) + Schedule
│   └── worker.rs         # Worker + WorkerStats + retry/dead-letter routing
└── tests/
    └── core.rs           # 20 tests + 5 insta snapshots
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new backoff strategy | `src/backoff.rs` (add variant to `Backoff` enum + `delay()`) |
| Add a new error variant | `src/error.rs` (+ `From` impl if wrapping an underlying error) |
| Add a new Job builder method | `src/job.rs` (add `pub fn with_*` method) |
| Modify queue behavior | `src/queue.rs` |
| Modify scheduler behavior | `src/scheduler.rs` |
| Modify worker dispatch | `src/worker.rs` |

## PUBLIC API (<=25 fns cap)

### `Job`
- `Job::new(payload)`, `job.with_priority()`, `job.with_max_retries()`, `job.with_backoff()`
- `job.id()`, `job.payload()`, `job.priority()`, `job.max_retries()`, `job.backoff()`, `job.status()`, `job.attempts()`

### `Queue`
- `Queue::memory()`, `queue.enqueue(job)`, `queue.dequeue()`, `queue.len()`, `queue.is_empty()`, `queue.stats()`, `queue.dead_letter()`

### `Worker`
- `Worker::new(queue)`, `worker.run(handler)`

### `Scheduler`
- `Scheduler::new()`, `scheduler.cron(expr, job)`, `scheduler.interval(dur, job)`, `scheduler.start()`, `scheduler.stop()`, `scheduler.remove(id)`, `scheduler.pending_count()`, `scheduler.schedules()`, `scheduler.next_due()`

### `Backoff`
- `Backoff::fixed(base)`, `Backoff::linear(base)`, `Backoff::exponential(base, max)`, `backoff.delay(attempt, max_retries)`

## CONVENTIONS

- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code.
- **Pure-Rust only**: cron 0.13, chrono 0.4, tokio, uuid — no native C deps.
- **Send + Sync**: all types are `Send + Sync` for future async composition.
- **In-memory MVP**: `Queue::memory()` is the only backend. Redis deferred to v1.18+.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `cron` | Upstream cron-expression parser. `buff-jobs` wraps `cron::Schedule::from_str`. |
| `tokio` | Async runtime for the Scheduler's interval/cron tick loop. |
| `buff-lang-ffi-guide` | Defines the hard rules every public function follows. |

## NOTES

- **MSVC host blocker**: `cargo test -p buff-jobs` fails on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers. CI runs on 3-OS matrix and passes.
- **Backoff sleep deferred**: the MVP computes backoff delays but does NOT sleep the worker before re-enqueue.
- **No serde in MVP**: `serde` + `serde_json` are included for `Job` serialization derive but not used by the queue/worker API yet.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T35 (lines 3305-3312).
- FFI guide: `crates/buff-lang-ffi-guide/GUIDE.md`.
