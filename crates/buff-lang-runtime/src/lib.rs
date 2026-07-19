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
//! * [`decide`] / [`DispatchPlanner`] — T40's pure threshold logic that
//!   routes a dispatch site to [`DispatchKind::SingleThread`],
//!   [`DispatchKind::CpuParallel`], or [`DispatchKind::GpuCompute`] based
//!   on element count + GPU availability + VRAM capacity. O(1),
//!   allocation-free.
//! * [`GpuBackend`] / [`MockGpuBackend`] / [`cpu_fallback_map`] — T38b's
//!   mock GPU backend + CPU-fallback oracle so T45/T46/T47 dispatch
//!   logic can be unit-tested WITHOUT a real GPU. The mock records every
//!   dispatch in a `Mutex<Vec<DispatchRecord>>` and produces the "GPU"
//!   output via a caller-provided CPU closure.
//! * [`WgpuBackend`] / [`workgroup_count`] — T45's REAL wgpu-backed GPU
//!   dispatch pipeline. Implements [`GpuBackend`] over a [`GpuContext`]'s
//!   cached `(Device, Queue)`: uploads `&[f32]` to a storage buffer,
//!   runs a `ceil(len/64)`-workgroup compute pass, and reads the output
//!   back via `map_async` + `device.poll(PollType::Wait)`. Returns empty
//!   Vec on empty input; returns [`RuntimeError::GpuUnavailable`] on
//!   hosts without a GPU (graceful — never panics).
//! * [`tile_ranges`] / [`max_elements_per_tile`] / [`dispatch_tiled`] /
//!   [`TiledDispatcher`] / [`dispatch_map_with_tiling`] — T46's VRAM-aware
//!   tiling dispatcher. Splits a large `&[f32]` into tiles that fit VRAM,
//!   dispatches each tile through [`WgpuBackend`] (or any [`GpuBackend`]),
//!   concatenates per-tile outputs in input order, and falls back to a
//!   caller-provided CPU oracle when no GPU is available or even one tile
//!   can't fit. The VRAM budget formula is
//!   `max_elements_per_tile(vram, bpe) = vram / (3 * bpe)` (3 buffers per
//!   dispatch — input + output + staging).
//! * [`PipelineCache`] / [`BufferPool`] / [`ColdStartBackend`] — T47's
//!   cold-start mitigation. [`PipelineCache`] is a `BTreeMap<String,
//!   CachedPipeline>` keyed by WGSL source so the second dispatch of the
//!   same shader reuses the compiled pipeline (cache hit, compile counter
//!   unchanged). [`BufferPool`] reuses GPU buffers across dispatches
//!   (keyed by `(byte_size, usage_flags)`). [`ColdStartBackend`] wraps
//!   T45's [`WgpuBackend`] in an [`std::sync::Arc`], integrates both
//!   caches, and exposes [`ColdStartBackend::spawn_init`] for background
//!   device warm-up + [`ColdStartBackend::wait_ready`] /
//!   [`ColdStartBackend::wait_ready_blocking`] for awaiting it. Implements
//!   [`GpuBackend`], so it's a drop-in replacement for [`WgpuBackend`]
//!   in dispatch sites that want cold-start amortization.
//! * [`Prefer`] / [`PREFER_GPU_MIN_ELEMENTS`] / [`decide_with_prefer`] /
//!   [`dispatch_with_prefer`] / [`prefer_from_name_args`] — T49's
//!   `@prefer(gpu)` / `@prefer(npu)` hint system. [`decide_with_prefer`]
//!   layers a user-supplied [`Prefer`] hint on top of T40's [`decide`]:
//!   honors the hint (routing to [`DispatchKind::GpuCompute`]) when a GPU
//!   is available, the input fits VRAM, AND `element_count >= `
//!   [`PREFER_GPU_MIN_ELEMENTS`] (the empirical GPU-dispatch-overhead
//!   break-even point, pinned to 1024). Below that threshold the
//!   cost-model override kicks in and routes through [`decide`] as if no
//!   hint were present (so `@prefer(gpu)` + 10 elements → CPU).
//!   [`dispatch_with_prefer`] runs the chosen path end-to-end via a
//!   provided [`GpuBackend`] + CPU oracle, masking any GPU-side error
//!   behind the CPU fallback.
//!
//! Real parallel/GPU logic is deferred: see T39 (CPU `par_map`), T43 (lazy
//! GPU device init via `OnceLock`), T45 (real GPU dispatch pipeline —
//! implements [`GpuBackend`] for a wgpu-backed type), T46 (VRAM tiling +
//! CPU fallback), T47 (cold-start pooling — pipeline cache + buffer pool
//! + async init), T49 (`@prefer` hints layered over [`decide`] — DONE).
//!
//! # Determinism
//!
//! All map/set types in this crate are [`std::collections::BTreeMap`] /
//! [`std::collections::BTreeSet`] — never `HashMap`/`HashSet` — to keep
//! behavior reproducible across hosts (project hard rule).

pub mod cold_start;
pub mod cpu;
pub mod dispatch;
pub mod error;
pub mod gpu;
pub mod gpu_pipeline;
pub mod hints;
pub mod mock_gpu;
pub mod threshold;
pub mod tiling;

pub use cold_start::{BufferPool, ColdStartBackend, PipelineCache};
pub use cpu::{CpuDispatcher, CpuDispatcherError};
pub use dispatch::{DispatchKind, Dispatcher};
pub use error::RuntimeError;
pub use gpu::{AdapterInfoSnapshot, GpuContext, GpuContextError};
pub use gpu_pipeline::{workgroup_count, WgpuBackend, WORKGROUP_SIZE};
pub use hints::{
    decide_with_prefer, dispatch_with_prefer, prefer_from_name_args, Prefer,
    PREFER_GPU_MIN_ELEMENTS,
};
pub use mock_gpu::{cpu_fallback_map, DispatchRecord, GpuBackend, MockGpuBackend};
pub use threshold::{decide, DispatchPlanner, CPU_PARALLEL_MAX, SINGLE_THREAD_MAX};
pub use tiling::{
    dispatch_map_with_tiling, dispatch_tiled, max_elements_per_tile, tile_ranges,
    vram_budget_from_device, TiledDispatcher,
};
