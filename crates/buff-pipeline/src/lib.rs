//! # buff-pipeline
//!
//! DAG-based ETL pipeline for the Buff language. **EXPERIMENTAL** (T14,
//! v1.13 frameworks wave 3).
//!
//! A [`Pipeline<T>`] is a linear chain of stages wired together by
//! bounded [`buff_lang_runtime::Channel<T>`] MPSC queues. Each stage
//! receives a [`Receiver`](buff_lang_runtime::Receiver) for its input
//! type and sends to a [`Sender`](buff_lang_runtime::Sender) for its
//! output type. The bounded buffer provides **natural backpressure**:
//! when a downstream stage is slow, the upstream stage's `send().await`
//! parks until a slot opens (the canonical bounded-queue flow-control
//! behavior).
//!
//! # Surface
//!
//! | Type | What it is |
//! |------|------------|
//! | [`Pipeline<T>`] | typed linear pipeline whose current output type is `T`. |
//! | [`Source`]      | factory namespace for streaming sources (`from_csv`). |
//! | [`Sink`]        | factory namespace for terminal sinks (`to_csv` / `to_json` / `collect`). |
//! | [`PipelineError`] / [`PipelineResult`](crate::PipelineResult) | crate-local error + Result alias. |
//!
//! ## Public functions (15, ≤ 40 cap per T14 spec)
//!
//! ### `Pipeline` (7)
//!  1. [`Pipeline::new`]    — empty untyped builder (`Pipeline<()>`).
//!  2. [`Pipeline::source`] — in-memory `Vec<T>` source (terminal on `Pipeline<()>`).
//!  3. [`Pipeline::with_buffer`] — override the inter-stage channel buffer.
//!  4. [`Pipeline::stage`]  — generic named stage (`Fn(T) -> U`).
//!  5. [`Pipeline::sink`]   — side-effecting passthrough (`Fn(&T)`).
//!  6. [`Pipeline::run`]    — drain the pipeline into `Vec<T>`.
//!  7. [`Pipeline::map`] / [`Pipeline::filter`] / [`Pipeline::batch`] /
//!     [`Pipeline::window`] / [`Pipeline::parallel`] — five stage
//!     helpers (see [`stage`] module).
//!
//! ### `Source` (1)
//!  8. [`Source::from_csv`] — streaming chunked CSV reader → `Pipeline<Vec<String>>`.
//!
//! ### `Sink` (3)
//!  9. [`Sink::to_csv`]   — write `Vec<Vec<String>>` to CSV.
//! 10. [`Sink::to_json`]  — write `Vec<T: Serialize>` to JSON.
//! 11. [`Sink::collect`]  — run a `Pipeline<T>` and return `Vec<T>`.
//!
//! # Pipeline
//!
//! ```text
//!   source ─▶ [Channel<T0>] ─▶ stage_0 ─▶ [Channel<T1>] ─▶ stage_1 ─▶ … ─▶ [Channel<Tn>] ─▶ run() → Vec<Tn>
//! ```
//!
//! Each arrow is a bounded `Channel<T>` (default capacity 64). The
//! source pushes items into `Channel<T0>`; `stage_0` consumes
//! `Receiver<T0>` and produces to `Sender<T1>`; etc. `Pipeline::run`
//! builds a multi-thread tokio runtime, spawns every stage as a task,
//! and drains the final channel into a `Vec<T>`.
//!
//! # Parallel stage
//!
//! [`Pipeline::parallel`] spawns `N` worker tasks that each consume
//! from their own input channel. A round-robin dispatcher task pulls
//! from the original input channel and distributes items to the worker
//! input channels; each worker applies the transform and sends to a
//! shared (cloned) output sender. The supervisor task awaits all
//! workers, then drops the final sender to signal downstream.
//!
//! Output order is NOT preserved under `parallel` (workers race). The
//! other stage helpers (`map` / `filter` / `batch` / `window`) preserve
//! input order because they use a single-task pipeline stage.
//!
//! # Deferred to v1.18+ (per T14 spec)
//!
//! * `Stream<T>` general async iterable type (deferred at the T2 level —
//!   this crate uses `Channel<T>` + sync `Vec<T>` batches).
//! * Kafka / Redis Streams sources.
//! * Exactly-once delivery guarantees.
//! * Pipeline orchestration (scheduling, retries, watermarks).
//! * `select` expression for multi-source fan-in.
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | Compliance |
//! |------|------------|
//! | R1 — No raw pointers | Public surface: `Pipeline<T>`, `Source`, `Sink`, `PipelineError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | `source` consumes `Vec<T>`. `run` returns owned `Vec<T>`. `stage` consumes `self`. |
//! | R3 — Error mapping | Fallible ops return `PipelineResult<_>`. Infallible ops (`stage`/`map`/...) never panic. |
//! | R4 — Thread safety | `Pipeline<T>: Send` when `T: Send` (spawner closure is `Send + 'static`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `Pipeline<T>` owns its continuation closure. |
//! | R6 — Panic boundary | No `unwrap`/`expect`/`panic!` in non-test code (project hard rule, enforced via `#![forbid(clippy::*)]`). |
//!
//! # Panic-free contract
//!
//! `#![cfg_attr(not(test), forbid(clippy::unwrap_used))]` +
//! `#![cfg_attr(not(test), forbid(clippy::expect_used))]` +
//! `#![cfg_attr(not(test), forbid(clippy::panic))]` at the crate root
//! enforce the no-panic rule at compile time. Integration tests in
//! `tests/` are separate crates and may use `unwrap`/`expect` freely.

#![cfg_attr(not(test), forbid(clippy::unwrap_used))]
#![cfg_attr(not(test), forbid(clippy::expect_used))]
#![cfg_attr(not(test), forbid(clippy::panic))]

pub mod error;
pub mod pipeline;
pub mod sink;
pub mod source;
pub mod stage;

pub use error::{PipelineError, PipelineResult};
pub use pipeline::Pipeline;
pub use sink::Sink;
pub use source::Source;

/// Default bounded channel capacity for inter-stage queues.
///
/// Every stage-to-stage `Channel<T>` is constructed with this capacity
/// unless the caller overrides it via [`Pipeline::with_buffer`]. The
/// value 64 is a conservative middle ground: large enough to amortize
/// per-item tokio task wakeups under steady-state flow, small enough
/// that a slow downstream stage backpressures the upstream within
/// ~64 items (preventing unbounded memory growth on a fast source /
/// slow sink configuration). Matches the buff-cache default-capacity
/// precedent (1024 was considered but 64 is tighter for a pipeline
/// whose stages are typically CPU-light per item).
pub const DEFAULT_BUFFER: usize = 64;
