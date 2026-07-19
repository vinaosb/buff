//! Buff Runtime crate — Async runtime, parallel execution, and GPU compute support.
//!
//! # T38 scaffold
//!
//! This crate hosts the Buff heterogeneous runtime: CPU parallel dispatch
//! (rayon), GPU compute (wgpu/WebGPU), and the async host (tokio). The
//! scaffold in T38 defines the trait surface that later waves implement:
//!
//! * [`RuntimeError`] — fallible result for every runtime operation.
//! * [`GpuContext`] — a handle to a wgpu adapter (device+queue arrive in T43).
//!   Returns a graceful error when no GPU adapter is available.
//! * [`CpuDispatcher`] — owns a rayon thread pool, ready for T39's `par_map`.
//! * [`Dispatcher`] trait + [`DispatchKind`] — the shape that CPU and GPU
//!   backends will both implement (T39, T45). Kept object-safe so callers
//!   can hold a `Box<dyn Dispatcher>` and pick a backend at execution time
//!   (T40 thresholds).
//!
//! Real parallel/GPU logic is deferred: see T39 (CPU `par_map`), T43 (lazy
//! GPU device init via `OnceLock`), T45 (GPU dispatch pipeline).
//!
//! # Determinism
//!
//! All map/set types in this crate are [`std::collections::BTreeMap`] /
//! [`std::collections::BTreeSet`] — never `HashMap`/`HashSet` — to keep
//! behavior reproducible across hosts (project hard rule).

pub mod cpu;
pub mod dispatch;
pub mod error;
pub mod gpu;

pub use cpu::{CpuDispatcher, CpuDispatcherError};
pub use dispatch::{DispatchKind, Dispatcher};
pub use error::RuntimeError;
pub use gpu::{AdapterInfoSnapshot, GpuContext, GpuContextError};
