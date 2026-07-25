//! T40: Automatic dispatch threshold logic.
//!
//! Pure O(1) decision function that routes work to [`DispatchKind::SingleThread`],
//! [`DispatchKind::CpuParallel`], or [`DispatchKind::GpuCompute`] based on
//! element count, GPU availability, and VRAM capacity.
//!
//! # Cost
//!
//! Pure integer arithmetic — no I/O, no allocation, no hashing. The decision
//! is always well under 1 μs on any modern CPU, so callers may invoke
//! [`decide`] per dispatch site without buffering.
//!
//! # Determinism
//!
//! [`decide`] is a pure function of its four inputs. Same inputs → same
//! output, on every host, every run. No [`std::collections::HashMap`] /
//! [`std::collections::HashSet`] (project hard rule — see `lib.rs` docs).
//!
//! # Routing
//!
//! | element_count               | gpu_available | data fits VRAM | result           |
//! |-----------------------------|---------------|----------------|------------------|
//! | `<= SINGLE_THREAD_MAX` (< 1000) | (ignored)     | (ignored)      | `SingleThread`   |
//! | `1000..=CPU_PARALLEL_MAX` (≤ 50_000) | (ignored)     | (ignored)      | `CpuParallel`    |
//! | `> CPU_PARALLEL_MAX` (> 50_000)    | `true`        | yes            | `GpuCompute`     |
//! | `> CPU_PARALLEL_MAX`              | `true`        | no             | `CpuParallel`    |
//! | `> CPU_PARALLEL_MAX`              | `false`       | (ignored)      | `CpuParallel`    |
//!
//! The fallback is **always** [`DispatchKind::CpuParallel`]: never fail,
//! never silently fall back to single-thread when parallelism would help.
//!
//! # Future wiring
//!
//! T49 (`@prefer(gpu)` / `@prefer(cpu)` hints) will wrap [`decide`] with a
//! thin override layer. T45 (GPU dispatch) consumes the [`DispatchKind::GpuCompute`]
//! result to invoke the wgpu pipeline. T40 itself only computes the routing
//! decision — it does **not** call any backend.

use crate::DispatchKind;

/// Inclusive upper bound for [`DispatchKind::SingleThread`].
///
/// Element counts `<= SINGLE_THREAD_MAX` (i.e. `< 1000`) route to
/// [`DispatchKind::SingleThread`] regardless of GPU availability — the
/// per-element overhead of parallelism or GPU dispatch dominates at this
/// size.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::threshold::SINGLE_THREAD_MAX;
/// assert_eq!(SINGLE_THREAD_MAX, 999);
/// ```
pub const SINGLE_THREAD_MAX: usize = 999;

/// Inclusive upper bound for [`DispatchKind::CpuParallel`].
///
/// Element counts in `1000..=CPU_PARALLEL_MAX` (i.e. `1000..=50_000`) route
/// to [`DispatchKind::CpuParallel`] (rayon). Counts `> CPU_PARALLEL_MAX`
/// route to [`DispatchKind::GpuCompute`] IF a GPU is available and the data
/// fits VRAM; otherwise they fall back to [`DispatchKind::CpuParallel`].
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::threshold::CPU_PARALLEL_MAX;
/// assert_eq!(CPU_PARALLEL_MAX, 50_000);
/// ```
pub const CPU_PARALLEL_MAX: usize = 50_000;

/// Pure dispatch planner.
///
/// The decision is a pure function of `(element_count, gpu_available,
/// available_vram_bytes, bytes_per_element)` — no instance state. The struct
/// exists only to give T49 (`@prefer` hints) and the runtime glue a
/// documented extension point (e.g. injecting a `Prefer::Gpu` hint that
/// overrides the data-size ladder without changing the free function).
///
/// `Default` and `new()` are equivalent; both yield a planner whose
/// [`DispatchPlanner::decide`] matches the free [`decide`] function exactly.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DispatchPlanner;

impl DispatchPlanner {
    /// Construct a default planner.
    ///
    /// Equivalent to [`DispatchPlanner::default`] but available in `const`
    /// contexts (no `Derived` trait bound required at the call site).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Decide which backend to use, using the standard thresholds.
    ///
    /// Thin wrapper around the free [`decide`] function — see its docs for
    /// the routing table and threshold definitions.
    #[must_use]
    pub fn decide(
        self,
        element_count: usize,
        gpu_available: bool,
        available_vram_bytes: Option<u64>,
        bytes_per_element: u64,
    ) -> DispatchKind {
        decide(
            element_count,
            gpu_available,
            available_vram_bytes,
            bytes_per_element,
        )
    }
}

/// Decide which [`DispatchKind`] to use for a buffer of `element_count`
/// elements occupying `bytes_per_element` bytes each.
///
/// # Thresholds
///
/// - `element_count <= `[`SINGLE_THREAD_MAX`] (i.e. `< 1000`):
///   [`DispatchKind::SingleThread`].
/// - `SINGLE_THREAD_MAX < element_count <= `[`CPU_PARALLEL_MAX`]
///   (i.e. `1000..=50_000`): [`DispatchKind::CpuParallel`].
/// - `element_count > CPU_PARALLEL_MAX` (i.e. `> 50_000`):
///   [`DispatchKind::GpuCompute`] IF `gpu_available` AND the data fits VRAM.
///   Otherwise [`DispatchKind::CpuParallel`] (graceful fallback).
///
/// # VRAM semantics
///
/// - `available_vram_bytes == None` means "unknown — assume the data fits" →
///   GPU stays eligible.
/// - `available_vram_bytes == Some(v)` means "at most `v` bytes free". The
///   data fits iff `element_count * bytes_per_element <= v`. Overflow on the
///   multiplication is treated as "does not fit" → [`DispatchKind::CpuParallel`]
///   fallback.
///
/// # Cost
///
/// Pure integer arithmetic — O(1), allocation-free. The decision is always
/// < 1 μs on any modern CPU.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::{DispatchKind, threshold::decide};
///
/// // Tiny work → single thread.
/// assert_eq!(decide(999, true, None, 8), DispatchKind::SingleThread);
/// // Medium work → CPU parallel.
/// assert_eq!(decide(1_000, true, None, 8), DispatchKind::CpuParallel);
/// // Large work with a GPU → GPU compute.
/// assert_eq!(decide(50_001, true, None, 8), DispatchKind::GpuCompute);
/// // Large work without a GPU → CPU parallel fallback.
/// assert_eq!(decide(50_001, false, None, 8), DispatchKind::CpuParallel);
/// ```
#[must_use]
pub fn decide(
    element_count: usize,
    gpu_available: bool,
    available_vram_bytes: Option<u64>,
    bytes_per_element: u64,
) -> DispatchKind {
    // Tiny work — single thread always wins (GPU/parallel setup overhead
    // dominates at this size). GPU availability is irrelevant here.
    if element_count <= SINGLE_THREAD_MAX {
        return DispatchKind::SingleThread;
    }

    // Medium work — always CPU parallel. GPU would be slower for arrays of
    // this size once the dispatch + transfer overhead is amortized.
    if element_count <= CPU_PARALLEL_MAX {
        return DispatchKind::CpuParallel;
    }

    // Large work — prefer GPU when available AND data fits VRAM.
    if gpu_available && fits_vram(element_count, bytes_per_element, available_vram_bytes) {
        return DispatchKind::GpuCompute;
    }

    // Graceful fallback when no GPU is available or the data exceeds VRAM.
    // Never SingleThread — large data still benefits from rayon.
    DispatchKind::CpuParallel
}

/// Check whether `element_count * bytes_per_element` fits the reported VRAM.
///
/// - `available == None` means "unknown — assume fits".
/// - `available == Some(cap)` means "at most `cap` bytes free"; the data
///   fits iff `element_count * bytes_per_element <= cap`.
/// - Multiplication overflow (data > 2^64 bytes) is treated as "does not
///   fit" — any real VRAM is smaller than 2^64.
///
/// Kept `pub(crate)`: T49 (hints::decide_with_prefer) reuses this exact
/// overflow-aware check when honoring `@prefer(gpu)` so the VRAM-edge
/// behavior stays byte-identical to T40's [`decide`]. T45 / T46 / T47
/// also reach the same predicate through their dispatch paths.
pub(crate) fn fits_vram(
    element_count: usize,
    bytes_per_element: u64,
    available: Option<u64>,
) -> bool {
    match available {
        None => true,
        Some(cap) => {
            // `usize` is at most `u64` on every platform Buff targets
            // (16-, 32-, 64-bit); the widening cast is lossless.
            let count = element_count as u64;
            match count.checked_mul(bytes_per_element) {
                Some(total) => total <= cap,
                None => false,
            }
        }
    }
}

// ===========================================================================
// T5: Dynamic workload-aware dispatch (v1.25 Wave 0, Track B MOAT)
// ===========================================================================
//
// T40's [`decide`] uses static element-count thresholds alone: `< 1000` →
// `SingleThread`, `1000..=50_000` → `CpuParallel`, `> 50_000` → `GpuCompute`
// (if a GPU exists). That is correct on average but cannot adapt to the
// ACTUAL runtime conditions of a specific dispatch site:
//
// * A 60 000-element memory-bound copy (`x => x`, intensity ~0.25 FLOPs/byte)
//   would be routed to the GPU — but the GPU's memory-bandwidth ceiling is
//   no higher than a multi-core CPU's, so the dispatch + transfer overhead
//   makes the GPU *slower*. [`decide`] has no way to know this.
//
// * A 5 000-element compute-heavy fused-multiply-add kernel
//   (`x => x * x + x`, intensity ~6 FLOPs/byte) would be routed to
//   `CpuParallel` — but a GPU with hundreds of ALUs would crush it.
//   [`decide`] has no way to promote it.
//
// T5 fixes both by introducing [`WorkloadContext`] + [`decide_dynamic`]: a
// NEW pure decision function that inspects the real runtime element count,
// GPU availability, AND an optional arithmetic-intensity estimate. The old
// [`decide`] is preserved verbatim for backwards compatibility (it remains
// the static-only path used by existing callers and T49's [`decide`]-based
// hint layering).
//
// # Key invariants (preserved from T40)
//
// * **`DispatchKind` variant ordering is untouched** — `SingleThread` <
//   `CpuParallel` < `GpuCompute` (see [`crate::DispatchKind`]).
// * **"GPU failure invisible; CPU fallback always correct"** — when no GPU
//   is available, [`decide_dynamic`] NEVER returns `GpuCompute`; it falls
//   back to `CpuParallel` for all medium/large inputs.
// * **Pure + deterministic + sub-microsecond** — [`decide_dynamic`] does
//   integer comparisons and at most one `f64` comparison. No I/O, no
//   allocation, no hashing, no thread-local state. The [`WorkloadContext`]
//   is passed by reference (`&WorkloadContext`) so callers pay zero copy.

/// Empirical GPU arithmetic-intensity break-even point, in FLOPs per byte
/// of data transferred.
///
/// Workloads at or above this intensity are **compute-bound** — the GPU's
/// massively parallel ALU array delivers a clear win over CPU rayon.
/// Workloads below it are **memory-bound** — both the GPU and a multi-core
/// CPU are bottlenecked by memory bandwidth, so the GPU's dispatch +
/// PCIe-transfer overhead makes it *slower* than rayon.
///
/// # Why 4.0
///
/// The roofline model gives the GPU-vs-CPU break-even at roughly:
///
/// ```text
///   intensity_break_even = (GPU_peak_FLOPs / GPU_peak_BW)
///                         / (CPU_peak_FLOPs / CPU_peak_BW)
/// ```
///
/// For a modern discrete GPU (~15 TFLOPS, ~500 GB/s) vs a 16-core CPU
/// (~500 GFLOPS, ~100 GB/s):
///
/// ```text
///   GPU operational intensity = 15_000 / 500 = 30 FLOPs/byte
///   CPU operational intensity = 500 / 100     = 5 FLOPs/byte
///   break_even ratio ≈ 30 / 5                 ≈ 6 (conservative)
/// ```
///
/// 4.0 is a deliberately GPU-favorable threshold (lower than the
/// theoretical 6) so that [`decide_dynamic`] promotes to the GPU whenever
/// the workload is *plausibly* compute-bound, erring on the side of using
/// the accelerator. Downstream callers that want a stricter bar can read
/// this constant and apply their own multiplier.
///
/// # Used by
///
/// [`decide_dynamic`] compares `WorkloadContext::arithmetic_intensity`
/// against this constant. [`decide_with_prefer_dynamic`] inherits the
/// same bar through delegation.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::threshold::GPU_ARITHMETIC_INTENSITY_THRESHOLD;
/// assert_eq!(GPU_ARITHMETIC_INTENSITY_THRESHOLD, 4.0);
/// ```
pub const GPU_ARITHMETIC_INTENSITY_THRESHOLD: f64 = 4.0;

// ===========================================================================
// T10: Data-locality-aware dispatch (v1.25 Wave 1)
// ===========================================================================
//
// T5's [`decide_dynamic`] assumes data starts on the CPU (the common case
// for Buff values, which are Rust `Vec<f32>` in RAM). But when a chain of
// GPU operations runs, the OUTPUT of op N is already resident in VRAM —
// feeding it as the INPUT to op N+1 should NOT incur a round-trip through
// CPU RAM. The PCIe download (~16 GB/s) + re-upload would dominate the
// actual compute for all but the most arithmetic-heavy kernels.
//
// T10 extends [`WorkloadContext`] with a [`DataLocation`] field so the
// CALLER (the dispatch site / runtime manager) can tell [`decide_dynamic`]
// where the data currently lives. When `data_location == Gpu` AND a GPU is
// available, [`decide_dynamic`] returns [`DispatchKind::GpuCompute`]
// UNCONDITIONALLY — even for inputs smaller than [`SINGLE_THREAD_MAX`].
// The rationale: the alternative (bring data back to CPU) costs a PCIe
// download that exceeds even a warm GPU dispatch (~100 µs).
//
// # Key invariants (preserved)
//
// * **Default is `Cpu`** — existing callers (and all T5 tests) see zero
//   behavior change. The T10 path only fires when the caller explicitly
//   sets `data_location` to [`DataLocation::Gpu`] via
//   [`.with_data_location()`](WorkloadContext::with_data_location).
// * **GPU-fallback guarantee preserved** — when `gpu_available == false`,
//   the T10 check does not fire (data cannot be on the GPU if there is no
//   GPU), and [`decide_dynamic`] falls through to the normal CPU path.
// * **Pure + deterministic** — [`DataLocation`] is a plain enum; the
//   check is one `==` comparison. Sub-microsecond.

/// T10: Where the input data currently resides at dispatch time.
///
/// Used by [`decide_dynamic`] to avoid redundant CPU↔GPU transfers in
/// chained GPU operations. When data is already on the GPU, the dispatch
/// stays on the GPU — bringing it back to CPU RAM would cost a PCIe
/// download that exceeds the GPU dispatch overhead for nearly all
/// workloads.
///
/// # Variants
///
/// * [`DataLocation::Cpu`] — data lives in CPU RAM (the default; Buff
///   values are Rust `Vec<f32>` in main memory). [`decide_dynamic`] uses
///   the normal threshold + intensity logic.
/// * [`DataLocation::Gpu`] — data is resident in GPU VRAM (typically the
///   output of a prior [`DispatchKind::GpuCompute`] dispatch in a chain).
///   [`decide_dynamic`] returns [`DispatchKind::GpuCompute`] whenever a
///   GPU is available, regardless of element count or intensity.
///
/// # Determinism
///
/// Plain `Copy` enum — no interior mutability, no I/O. The default
/// ([`DataLocation::Cpu`]) is the conservative choice.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::{
///     DataLocation, DispatchKind, WorkloadContext, threshold::decide_dynamic,
/// };
///
/// // Default: data on CPU → 500 elements → SingleThread (tiny band).
/// let ctx = WorkloadContext::new(500, true);
/// assert_eq!(decide_dynamic(&ctx), DispatchKind::SingleThread);
///
/// // T10: data already on GPU → 500 elements → GpuCompute (no transfer).
/// let ctx = WorkloadContext::new(500, true).with_data_location(DataLocation::Gpu);
/// assert_eq!(decide_dynamic(&ctx), DispatchKind::GpuCompute);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum DataLocation {
    /// Data resides in CPU RAM (main memory). This is the default — Buff
    /// values are Rust `Vec<f32>` living in process memory. [`decide_dynamic`]
    /// uses the normal element-count + arithmetic-intensity thresholds.
    ///
    /// When this is set and a GPU is available, dispatching to the GPU
    /// incurs a PCIe upload cost (~16 GB/s) that the cost model factors in.
    #[default]
    Cpu,
    /// Data is resident in GPU VRAM — typically the output of a prior
    /// [`DispatchKind::GpuCompute`] dispatch in a chained operation.
    /// [`decide_dynamic`] prefers [`DispatchKind::GpuCompute`] whenever a
    /// GPU is available, because bringing the data back to CPU RAM would
    /// cost a PCIe download that exceeds the GPU dispatch overhead.
    Gpu,
}

/// T5: Runtime workload context for dynamic dispatch decisions.
///
/// Captures the actual runtime inputs that [`decide_dynamic`] uses to make
/// a workload-aware CPU/GPU dispatch choice. Unlike T40's [`decide`] (which
/// uses static element-count thresholds alone), [`decide_dynamic`] inspects
/// the REAL runtime data size, GPU availability, and an optional
/// arithmetic-intensity estimate to route work optimally.
///
/// # Construction
///
/// Use [`WorkloadContext::new`] for the common case (count + GPU flag),
/// then `.with_intensity(ai)` to attach an arithmetic-intensity estimate:
///
/// ```
/// use buff_lang_runtime::WorkloadContext;
///
/// let ctx = WorkloadContext::new(100_000, true)
///     .with_intensity(8.0); // 8 FLOPs/byte — compute-bound
/// ```
///
/// When `arithmetic_intensity` is left as `None` (unknown), [`decide_dynamic`]
/// treats the workload as GPU-favorable — matching the convention where
/// `available_vram_bytes: None` in [`decide`] means "assume fits". This keeps
/// the dynamic path consistent with the static path for the GPU decision
/// when intensity data is unavailable.
///
/// # Fields
///
/// * `element_count` — actual number of elements in the buffer at dispatch
///   time. Compared against [`SINGLE_THREAD_MAX`] and [`CPU_PARALLEL_MAX`].
/// * `gpu_available` — whether a GPU adapter is present on this host at
///   dispatch time. When `false`, [`decide_dynamic`] always falls back to
///   a CPU path (never `GpuCompute`).
/// * `arithmetic_intensity` — optional FLOPs-per-byte estimate. `None`
///   means "unknown — treat as GPU-favorable". `Some(v)` where
///   `v < `[`GPU_ARITHMETIC_INTENSITY_THRESHOLD`] means "memory-bound —
///   demote GPU-eligible work to `CpuParallel`".
///
/// # Determinism
///
/// `WorkloadContext` is a pure data struct (no interior mutability, no
/// I/O, no clocks). [`decide_dynamic`] is a pure function of
/// `&WorkloadContext` — same inputs → same output, every host, every run.
///
/// # Cost
///
/// [`decide_dynamic`] performs only integer comparisons and at most one
/// `f64` comparison — O(1), allocation-free, sub-microsecond. The
/// `WorkloadContext` is passed by reference so the caller pays zero copy.
///
/// # `PartialEq` but not `Eq`
///
/// `arithmetic_intensity: Option<f64>` cannot implement `Eq` (`f64` has
/// NaN), so this struct derives `PartialEq` only. Two contexts with NaN
/// intensity are never equal (even to themselves), but NaN intensity is
/// treated as "not high" by [`decide_dynamic`] (any comparison with NaN
/// returns `false`), so the dispatch decision is still deterministic for
/// a given NaN payload.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WorkloadContext {
    /// Actual number of elements in the buffer being dispatched.
    ///
    /// This is the REAL runtime length (e.g. `slice.len()`), not a static
    /// estimate. [`decide_dynamic`] compares it against [`SINGLE_THREAD_MAX`]
    /// and [`CPU_PARALLEL_MAX`] to pick the routing band.
    pub element_count: usize,
    /// Whether a GPU adapter is available on this host at dispatch time.
    ///
    /// When `false`, [`decide_dynamic`] never returns
    /// [`DispatchKind::GpuCompute`] — the "GPU failure invisible; CPU
    /// fallback always correct" guarantee is preserved.
    pub gpu_available: bool,
    /// Optional arithmetic-intensity estimate (FLOPs per byte of data
    /// transferred).
    ///
    /// `None` means "unknown — treat as GPU-favorable" (matches
    /// `available_vram_bytes: None` in [`decide`]). `Some(v)` where
    /// `v >= `[`GPU_ARITHMETIC_INTENSITY_THRESHOLD`] means "compute-bound
    /// — GPU wins". `Some(v)` where
    /// `v < GPU_ARITHMETIC_INTENSITY_THRESHOLD` means "memory-bound —
    /// CPU wins (GPU's bandwidth ceiling is no higher)".
    pub arithmetic_intensity: Option<f64>,
    /// T10: Where the input data currently lives.
    ///
    /// Defaults to [`DataLocation::Cpu`] (data in main memory — the common
    /// case). Set to [`DataLocation::Gpu`] when the data is the output of a
    /// prior GPU dispatch (chained operations) so [`decide_dynamic`] avoids
    /// a redundant PCIe round-trip.
    ///
    /// Use [`.with_data_location(loc)`](Self::with_data_location) to set it.
    pub data_location: DataLocation,
    /// T11: Bytes per element in the buffer (e.g. 4 for `f32`, 8 for `f64`).
    ///
    /// When `> 0`, [`decide_dynamic`] uses the multi-factor cost model
    /// (transfer time + launch overhead + occupancy + arithmetic intensity)
    /// instead of the simpler intensity-threshold heuristic. When `0`
    /// (the default), the cost model is bypassed and the T5 intensity
    /// threshold logic runs — keeping all existing T5 tests unchanged.
    ///
    /// Use [`.with_bytes_per_element(n)`](Self::with_bytes_per_element) to set it.
    pub bytes_per_element: u64,
}

impl WorkloadContext {
    /// Construct a workload context with `element_count` + `gpu_available`,
    /// leaving `arithmetic_intensity` as `None` (unknown — treated as
    /// GPU-favorable by [`decide_dynamic`]).
    ///
    /// Available in `const` contexts (no `Default` trait bound required).
    /// Use [`.with_intensity(ai)`](Self::with_intensity) to attach an
    /// intensity estimate.
    ///
    /// # Examples
    ///
    /// ```
    /// use buff_lang_runtime::{WorkloadContext, threshold::decide_dynamic, DispatchKind};
    ///
    /// const CTX: WorkloadContext = WorkloadContext::new(100_000, true);
    /// assert_eq!(decide_dynamic(&CTX), DispatchKind::GpuCompute);
    /// ```
    #[must_use]
    pub const fn new(element_count: usize, gpu_available: bool) -> Self {
        Self {
            element_count,
            gpu_available,
            arithmetic_intensity: None,
            data_location: DataLocation::Cpu,
            bytes_per_element: 0,
        }
    }

    /// Chainable builder to set the arithmetic-intensity estimate.
    ///
    /// Consumes and returns `self` (builder pattern). Pass the estimated
    /// FLOPs-per-byte of the dispatch kernel:
    ///
    /// * `x => x * x` → ~1 FLOP / 4 bytes = 0.25 (memory-bound).
    /// * `x => x * x + x` → ~2 FLOPs / 4 bytes = 0.5 (still memory-bound).
    /// * A 16-term dot-product per element → ~16 FLOPs / 4 bytes = 4.0
    ///   (compute-bound — at the break-even).
    ///
    /// # Examples
    ///
    /// ```
    /// use buff_lang_runtime::{WorkloadContext, threshold::decide_dynamic, DispatchKind};
    ///
    /// // Memory-bound: intensity below threshold → CPU even with GPU.
    /// let ctx = WorkloadContext::new(100_000, true).with_intensity(0.5);
    /// assert_eq!(decide_dynamic(&ctx), DispatchKind::CpuParallel);
    ///
    /// // Compute-bound: intensity above threshold → GPU.
    /// let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
    /// assert_eq!(decide_dynamic(&ctx), DispatchKind::GpuCompute);
    /// ```
    #[must_use]
    pub fn with_intensity(mut self, arithmetic_intensity: f64) -> Self {
        self.arithmetic_intensity = Some(arithmetic_intensity);
        self
    }

    /// T10: Chainable builder to set where the input data currently lives.
    ///
    /// Set to [`DataLocation::Gpu`] when the data is the output of a prior
    /// GPU dispatch (chained operations) so [`decide_dynamic`] avoids a
    /// redundant PCIe round-trip and keeps the dispatch on the GPU.
    ///
    /// Defaults to [`DataLocation::Cpu`] (data in main memory).
    ///
    /// # Examples
    ///
    /// ```
    /// use buff_lang_runtime::{
    ///     DataLocation, DispatchKind, WorkloadContext, threshold::decide_dynamic,
    /// };
    ///
    /// // Default (CPU): 500 elements → SingleThread (tiny band).
    /// let ctx = WorkloadContext::new(500, true);
    /// assert_eq!(decide_dynamic(&ctx), DispatchKind::SingleThread);
    ///
    /// // Data on GPU: 500 elements → GpuCompute (no PCIe transfer needed).
    /// let ctx = ctx.with_data_location(DataLocation::Gpu);
    /// assert_eq!(decide_dynamic(&ctx), DispatchKind::GpuCompute);
    /// ```
    #[must_use]
    pub fn with_data_location(mut self, data_location: DataLocation) -> Self {
        self.data_location = data_location;
        self
    }

    /// T11: Chainable builder to set the bytes-per-element width.
    ///
    /// When set to a non-zero value (e.g. 4 for `f32`, 8 for `f64`),
    /// [`decide_dynamic`] activates the multi-factor cost model
    /// (transfer time + launch overhead + occupancy + arithmetic intensity)
    /// instead of the simpler intensity-threshold heuristic.
    ///
    /// # Examples
    ///
    /// ```
    /// use buff_lang_runtime::{
    ///     DispatchKind, WorkloadContext, threshold::decide_dynamic,
    /// };
    ///
    /// // With bytes_per_element set, the cost model runs:
    /// let ctx = WorkloadContext::new(100_000, true)
    ///     .with_bytes_per_element(4)    // f32
    ///     .with_intensity(8.0);        // compute-bound
    /// assert_eq!(decide_dynamic(&ctx), DispatchKind::GpuCompute);
    /// ```
    #[must_use]
    pub fn with_bytes_per_element(mut self, bytes_per_element: u64) -> Self {
        self.bytes_per_element = bytes_per_element;
        self
    }
}

/// Whether `arithmetic_intensity` is "high" enough for the GPU to win.
///
/// `None` (unknown) → `true` (GPU-favorable) — matches the convention
/// where `available_vram_bytes: None` in [`decide`] means "assume fits".
/// This keeps [`decide_dynamic`] consistent with [`decide`] for the GPU
/// decision when intensity data is unavailable: if [`decide`] would pick
/// `GpuCompute`, [`decide_dynamic`] with `None` intensity does too.
///
/// `Some(v)` → `v >= `[`GPU_ARITHMETIC_INTENSITY_THRESHOLD`]. NaN
/// compares `false` against any threshold, so `Some(f64::NAN)` → `false`
/// (treated as memory-bound — the conservative choice for garbage input).
fn is_gpu_favorable_intensity(arithmetic_intensity: Option<f64>) -> bool {
    arithmetic_intensity.is_none_or(|ai| ai >= GPU_ARITHMETIC_INTENSITY_THRESHOLD)
}

// ===========================================================================
// T11: Refined multi-factor cost model (v1.25 Wave 1)
// ===========================================================================
//
// T5's [`decide_dynamic`] uses a single-factor heuristic: arithmetic
// intensity ≥ 4.0 FLOPs/byte → GPU, else CPU. This is correct on average
// but misses several real-world factors:
//
// * **Transfer time**: uploading data CPU→GPU over PCIe (~16 GB/s) takes
//   real wall-clock time. For data already on the GPU (T10), this is zero.
// * **Launch overhead**: every GPU dispatch pays a fixed cost (~100 µs
//   warm) for command encoding + submission + poll. The CPU has no such
//   overhead.
// * **Occupancy**: the GPU's hundreds of compute units are wasted if the
//   input is too small to fill them. Below ~64 workgroups (4096 elements
//   at workgroup size 64), the GPU is underutilized.
// * **Arithmetic intensity**: the roofline model — compute-bound work
//   favours the GPU's massive ALU array; memory-bound work is bottlenecked
//   by bandwidth on both CPU and GPU.
//
// T11 replaces the single intensity threshold with a **roofline-based cost
// model** that estimates GPU time and CPU time, then picks the faster one.
// The model is activated ONLY when [`WorkloadContext::bytes_per_element`]
// is non-zero (callers opt in). When `bytes_per_element == 0` (the default),
// [`decide_dynamic`] falls back to the T5 intensity threshold — keeping all
// existing tests unchanged.
//
// # Cost
//
// Pure O(1) `f64` arithmetic — a handful of multiplies, divides, and `max`.
// Sub-microsecond on any modern CPU. No I/O, no allocation, no hashing.
//
// # Determinism
//
// Pure function of `&WorkloadContext`. Same inputs → same output on every
// host, every run. The constants are `pub const f64` so callers can inspect
// (and override in their own wrappers) the assumed hardware parameters.

/// T11: PCIe 3.0 x16 effective bandwidth in bytes/sec (~16 GB/s).
///
/// Conservative: PCIe 4.0 reaches ~32 GB/s, but many hosts still run 3.0.
/// Used by the cost model to estimate CPU→GPU transfer time.
pub const PCIE_BANDWIDTH_BYTES_PER_SEC: f64 = 16e9;

/// T11: Fixed overhead per GPU dispatch in seconds (~100 µs warm).
///
/// Covers command encoding + queue submission + `device.poll(Wait)`. The
/// cold-start cost (first dispatch of a shader, ~300 µs–1 ms) is amortized
/// by T47's pipeline cache; this constant reflects the warm steady-state.
pub const GPU_LAUNCH_OVERHEAD_SECS: f64 = 100e-6;

/// T11: Modern discrete GPU peak FP32 throughput in FLOPs/sec (~15 TFLOPS).
///
/// Representative of a mid-range discrete GPU (e.g. RTX 3060 ~12 TFLOPS,
/// RTX 4070 ~29 TFLOPS). Used by the cost model's roofline computation.
pub const GPU_PEAK_FLOPS_PER_SEC: f64 = 15e12;

/// T11: Modern discrete GPU memory bandwidth in bytes/sec (~500 GB/s).
///
/// Representative of GDDR6 on a mid-range card. Used by the cost model's
/// roofline computation for the memory-bound ceiling.
pub const GPU_MEMORY_BANDWIDTH_BYTES_PER_SEC: f64 = 500e9;

/// T11: Multi-core CPU peak FP32 throughput in FLOPs/sec (~500 GFLOPS).
///
/// Representative of a 16-core desktop CPU using AVX2/AVX-512.
pub const CPU_PEAK_FLOPS_PER_SEC: f64 = 500e9;

/// T11: Multi-core CPU memory bandwidth in bytes/sec (~100 GB/s).
///
/// Representative of dual-channel DDR4/DDR5 on a desktop platform.
pub const CPU_MEMORY_BANDWIDTH_BYTES_PER_SEC: f64 = 100e9;

/// T11: Workgroup size used by the cost model for occupancy estimation.
///
/// Matches [`crate::WORKGROUP_SIZE`] (64) — T44 codegen emits
/// `@compute @workgroup_size(64)`. Duplicated here to avoid pulling the
/// wgpu-backed `gpu_pipeline` module into the pure `threshold` module.
const COST_MODEL_WORKGROUP_SIZE: usize = 64;

/// T11: Minimum workgroups for full GPU occupancy (~64 compute units).
///
/// Below this, the GPU's compute units are underutilized and the cost
/// model applies a throughput penalty. 64 CUs × 1 wave = 64 workgroups;
/// below that, effective throughput scales down proportionally.
const GPU_MIN_WORKGROUPS_FOR_OCCUPANCY: u32 = 64;

/// T11: Estimated GPU and CPU execution times from the cost model.
///
/// Returned by [`estimate_costs``. All fields are in seconds. The caller
/// compares `gpu_time < cpu_time` to decide whether the GPU wins.
///
/// # Fields
///
/// * `gpu_time` — total estimated GPU time: launch overhead + PCIe transfer
///   (0 if data is already on GPU) + roofline compute (max of compute-bound
///   and memory-bound times), scaled by an occupancy penalty if the input
///   is too small to fill the GPU.
/// * `cpu_time` — estimated CPU time: roofline compute only (no transfer,
///   no launch overhead — data is already in RAM).
/// * `transfer_time` — the PCIe upload/download component of `gpu_time`
///   (0 when [`DataLocation::Gpu`]).
/// * `launch_overhead` — the fixed GPU dispatch overhead (always
///   [`GPU_LAUNCH_OVERHEAD_SECS`]).
/// * `occupancy_factor` — throughput multiplier applied to GPU compute
///   (1.0 when fully occupied; > 1.0 when underutilized).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CostEstimate {
    /// Total estimated GPU execution time (seconds).
    pub gpu_time: f64,
    /// Estimated CPU execution time (seconds).
    pub cpu_time: f64,
    /// PCIe transfer time component of `gpu_time` (seconds). 0 when data
    /// is already on the GPU (T10 [`DataLocation::Gpu`]).
    pub transfer_time: f64,
    /// Fixed GPU launch overhead ([`GPU_LAUNCH_OVERHEAD_SECS`]).
    pub launch_overhead: f64,
    /// Occupancy penalty multiplier on GPU compute (1.0 = full occupancy).
    pub occupancy_factor: f64,
}

/// T11: Estimate GPU and CPU execution times for the given workload.
///
/// Uses a roofline-based cost model with four factors:
///
/// 1. **Transfer time**: `total_bytes / PCIE_BANDWIDTH` (0 if data on GPU).
/// 2. **Launch overhead**: [`GPU_LAUNCH_OVERHEAD_SECS`] (fixed per dispatch).
/// 3. **Occupancy**: throughput penalty when workgroups < [`GPU_MIN_WORKGROUPS_FOR_OCCUPANCY`].
/// 4. **Arithmetic intensity**: roofline `max(flops/peak, bytes/bandwidth)`.
///
/// # NaN safety
///
/// If `arithmetic_intensity` is `Some(NaN)`, `total_flops` becomes NaN.
/// `f64::max(NaN, x)` returns `x`, so the compute term degenerates to the
/// memory-bound path — conservative and correct.
///
/// # Cost
///
/// Pure O(1) `f64` arithmetic. No allocation, no I/O.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::{WorkloadContext, threshold::estimate_costs};
///
/// let ctx = WorkloadContext::new(1_000_000, true)
///     .with_bytes_per_element(4)
///     .with_intensity(8.0);
/// let costs = estimate_costs(&ctx);
/// // GPU with 1M elements + high intensity should be faster than CPU.
/// assert!(costs.gpu_time < costs.cpu_time);
/// ```
#[must_use]
pub fn estimate_costs(ctx: &WorkloadContext) -> CostEstimate {
    let total_bytes = ctx.element_count as f64 * ctx.bytes_per_element as f64;

    // Default to the GPU-favorable threshold when intensity is unknown,
    // matching T5's convention.
    let intensity = ctx
        .arithmetic_intensity
        .unwrap_or(GPU_ARITHMETIC_INTENSITY_THRESHOLD);
    let total_flops = intensity * total_bytes;

    // --- Transfer time ---
    let transfer_time = match ctx.data_location {
        DataLocation::Gpu => 0.0,
        DataLocation::Cpu => total_bytes / PCIE_BANDWIDTH_BYTES_PER_SEC,
    };

    // --- Occupancy penalty ---
    // Ceiling division: how many workgroups this input would generate.
    let workgroups = ctx.element_count.div_ceil(COST_MODEL_WORKGROUP_SIZE) as u32;
    let occupancy_factor = if workgroups < GPU_MIN_WORKGROUPS_FOR_OCCUPANCY {
        // Underutilized: scale compute time up proportionally.
        // max(1) avoids division by zero (can't happen here since we're
        // past the SingleThread check, but defensive).
        GPU_MIN_WORKGROUPS_FOR_OCCUPANCY as f64 / workgroups.max(1) as f64
    } else {
        1.0
    };

    // --- GPU roofline ---
    let gpu_compute_bound = total_flops / GPU_PEAK_FLOPS_PER_SEC;
    let gpu_memory_bound = total_bytes / GPU_MEMORY_BANDWIDTH_BYTES_PER_SEC;
    // f64::max(NaN, x) = x — NaN intensity degenerates to memory-bound.
    let gpu_compute = gpu_compute_bound.max(gpu_memory_bound) * occupancy_factor;
    let gpu_time = GPU_LAUNCH_OVERHEAD_SECS + transfer_time + gpu_compute;

    // --- CPU roofline ---
    let cpu_compute_bound = total_flops / CPU_PEAK_FLOPS_PER_SEC;
    let cpu_memory_bound = total_bytes / CPU_MEMORY_BANDWIDTH_BYTES_PER_SEC;
    let cpu_time = cpu_compute_bound.max(cpu_memory_bound);

    CostEstimate {
        gpu_time,
        cpu_time,
        transfer_time,
        launch_overhead: GPU_LAUNCH_OVERHEAD_SECS,
        occupancy_factor,
    }
}

/// T11: Whether the multi-factor cost model favours the GPU.
///
/// Returns `true` when [`estimate_costs`] reports `gpu_time < cpu_time`.
/// Used by [`decide_dynamic`] when [`WorkloadContext::bytes_per_element`]
/// is non-zero.
///
/// # Cost
///
/// Delegates to [`estimate_costs`] — pure O(1) `f64` arithmetic.
///
/// # NaN safety
///
/// If any computation produces NaN (e.g. from `Some(NaN)` intensity), the
/// comparison `gpu_time < cpu_time` returns `false` (NaN comparisons are
/// always false), so the GPU is NOT favoured — conservative.
#[must_use]
pub fn cost_model_favors_gpu(ctx: &WorkloadContext) -> bool {
    let costs = estimate_costs(ctx);
    costs.gpu_time < costs.cpu_time
}

/// T5: Decide which [`DispatchKind`] to use, inspecting ACTUAL runtime
/// workload context (element count + GPU availability + arithmetic
/// intensity).
///
/// This is the **dynamic** counterpart to T40's static [`decide`]. It uses
/// the same [`SINGLE_THREAD_MAX`] / [`CPU_PARALLEL_MAX`] band boundaries,
/// but adds two runtime-aware refinements that [`decide`] cannot make:
///
/// # Routing rules (the 4 spec branches)
///
/// | condition | result |
/// |-----------|--------|
/// | `element_count <= `[`SINGLE_THREAD_MAX`] | [`DispatchKind::SingleThread`] |
/// | `element_count <= `[`CPU_PARALLEL_MAX`] AND (no GPU OR low intensity) | [`DispatchKind::CpuParallel`] |
/// | `element_count > `[`CPU_PARALLEL_MAX`] AND `gpu_available` AND high intensity | [`DispatchKind::GpuCompute`] |
/// | `element_count > `[`CPU_PARALLEL_MAX`] AND no GPU | [`DispatchKind::CpuParallel`] |
///
/// Where "high intensity" means `arithmetic_intensity` is `None` (unknown —
/// treated as GPU-favorable) OR `Some(v)` where
/// `v >= `[`GPU_ARITHMETIC_INTENSITY_THRESHOLD`].
///
/// # Dynamic refinements over [`decide`]
///
/// Two additional behaviors emerge from the spec's 4 branches that [`decide`]
/// cannot replicate:
///
/// 1. **Promotion** (medium band → GPU): when `gpu_available` AND intensity
///    is high AND `SINGLE_THREAD_MAX < element_count <= CPU_PARALLEL_MAX`,
///    [`decide_dynamic`] returns [`DispatchKind::GpuCompute`]. Static
///    [`decide`] always returns [`DispatchKind::CpuParallel`] in this band.
///    This is the MOAT: a compute-heavy 5 000-element kernel hits the GPU
///    instead of waiting on rayon.
///
/// 2. **Demotion** (large band → CPU): when `gpu_available` BUT intensity
///    is low (memory-bound) AND `element_count > CPU_PARALLEL_MAX`,
///    [`decide_dynamic`] returns [`DispatchKind::CpuParallel`]. Static
///    [`decide`] returns [`DispatchKind::GpuCompute`] here. A memory-bound
///    100 000-element copy stays on the CPU where rayon's bandwidth is
///    just as good and there's no dispatch overhead.
///
/// # Backwards compatibility
///
/// [`decide`] is **unchanged** — existing callers (T49's
/// [`decide_with_prefer`], the codegen-emitted dispatch sites, all T40
/// tests) keep their exact behavior. [`decide_dynamic`] is a NEW function;
/// callers opt in by constructing a [`WorkloadContext`].
///
/// When `arithmetic_intensity` is `None`, [`decide_dynamic`] with a GPU
/// produces the same `GpuCompute` decision as [`decide`] for large inputs —
/// so callers that don't yet compute intensity see no behavior change
/// beyond the medium-band promotion (which is strictly an improvement).
///
/// # Cost
///
/// Pure O(1) integer + at most one `f64` comparison. No I/O, no
/// allocation, no hashing. Sub-microsecond on any modern CPU. The
/// [`WorkloadContext`] is passed by reference.
///
/// # Determinism
///
/// Pure function of `&WorkloadContext`. No [`std::collections::HashMap`] /
/// [`std::collections::HashSet`], no clocks, no thread-locals. Same inputs
/// → same output, every host, every run.
///
/// # GPU-fallback guarantee
///
/// When `gpu_available == false`, this function NEVER returns
/// [`DispatchKind::GpuCompute`] — the "GPU failure invisible; CPU fallback
/// always correct" invariant from T40 is preserved.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::{WorkloadContext, threshold::decide_dynamic, DispatchKind};
///
/// // Branch 1: tiny work → SingleThread.
/// let ctx = WorkloadContext::new(500, true);
/// assert_eq!(decide_dynamic(&ctx), DispatchKind::SingleThread);
///
/// // Branch 2: medium + no GPU → CpuParallel.
/// let ctx = WorkloadContext::new(10_000, false);
/// assert_eq!(decide_dynamic(&ctx), DispatchKind::CpuParallel);
///
/// // Branch 3: large + GPU + high intensity → GpuCompute.
/// let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
/// assert_eq!(decide_dynamic(&ctx), DispatchKind::GpuCompute);
///
/// // Branch 4: large + no GPU → CpuParallel.
/// let ctx = WorkloadContext::new(100_000, false);
/// assert_eq!(decide_dynamic(&ctx), DispatchKind::CpuParallel);
///
/// // Dynamic demotion: large + GPU + LOW intensity → CpuParallel.
/// let ctx = WorkloadContext::new(100_000, true).with_intensity(0.5);
/// assert_eq!(decide_dynamic(&ctx), DispatchKind::CpuParallel);
/// ```
#[must_use]
pub fn decide_dynamic(ctx: &WorkloadContext) -> DispatchKind {
    // T10: Data-locality-aware dispatch.
    //
    // If the data is ALREADY on the GPU (from a prior GpuCompute dispatch
    // in a chained operation) AND a GPU is available, stay on the GPU.
    // Bringing the data back to CPU RAM would cost a PCIe download
    // (~16 GB/s ≈ 60 µs/MB) that exceeds even a warm GPU dispatch
    // (~100 µs). This check fires BEFORE the SingleThread band so that
    // even tiny inputs stay on the GPU when they're already resident.
    //
    // The "GPU failure invisible; CPU fallback always correct" guarantee
    // is preserved: when `gpu_available == false`, this check does NOT
    // fire (data_location == Gpu with no GPU is a caller bug, but we
    // handle it gracefully by falling through to the normal CPU logic).
    if ctx.data_location == DataLocation::Gpu && ctx.gpu_available {
        return DispatchKind::GpuCompute;
    }

    // Branch 1: element_count ≤ SINGLE_THREAD_MAX → SingleThread.
    // Tiny work: parallel/GPU setup overhead dominates at this size.
    // GPU availability and intensity are irrelevant here.
    if ctx.element_count <= SINGLE_THREAD_MAX {
        return DispatchKind::SingleThread;
    }

    // T11: Refined multi-factor cost model.
    //
    // When the caller provides `bytes_per_element` (> 0), replace the
    // single intensity threshold with a roofline-based cost model that
    // considers transfer time + launch overhead + occupancy + arithmetic
    // intensity. This gives finer-grained decisions than the intensity-
    // only heuristic.
    //
    // When `bytes_per_element == 0` (the default), this branch is skipped
    // and the T5 intensity-threshold logic below runs — keeping all
    // existing T5 tests byte-identical.
    if ctx.bytes_per_element > 0 {
        if ctx.gpu_available && cost_model_favors_gpu(ctx) {
            return DispatchKind::GpuCompute;
        }
        // Cost model says CPU wins, or no GPU available.
        return DispatchKind::CpuParallel;
    }

    // Precompute intensity favorability. None (unknown) → true (GPU-favorable),
    // matching decide()'s convention where unknown VRAM means "assume fits".
    // NaN compares false → treated as "not high" (conservative for garbage input).
    let intensity_high = is_gpu_favorable_intensity(ctx.arithmetic_intensity);

    // Branches 2 + 3: GPU-eligible work.
    //
    // When a GPU is available AND the workload is compute-bound (high or
    // unknown intensity), route to GpuCompute. This covers:
    //   * Branch 3: large band (> CPU_PARALLEL_MAX) + GPU + high → GpuCompute.
    //   * Dynamic PROMOTION: medium band (≤ CPU_PARALLEL_MAX) + GPU + high →
    //     GpuCompute. Static decide() cannot do this — it always picks
    //     CpuParallel in the medium band. decide_dynamic promotes when the
    //     workload is compute-intensive enough to benefit.
    if ctx.gpu_available && intensity_high {
        return DispatchKind::GpuCompute;
    }

    // Branches 2 + 4: CpuParallel fallback.
    //
    // This covers:
    //   * Branch 2: medium band + (no GPU OR low intensity) → CpuParallel.
    //   * Branch 4: large band + no GPU → CpuParallel.
    //   * Dynamic DEMOTION: large band + GPU + low intensity → CpuParallel.
    //     Memory-bound work doesn't benefit from the GPU; rayon's bandwidth
    //     is just as good and there's no dispatch overhead.
    //
    // Never SingleThread — medium/large data still benefits from rayon.
    DispatchKind::CpuParallel
}

/// T6: Explain WHY a dispatch decision was made — human-readable diagnostic.
///
/// Zero-overhead when not called: the `String` is only allocated when this
/// function is invoked. Callers MUST gate on a user-facing `--explain` flag
/// and never construct the string on the hot path.
///
/// # Output format
///
/// Multi-line, one field per line, with branch-trace annotations:
///
/// ```text
/// Dispatch: GpuCompute
///   element_count: 100000
///   gpu_available: true
///   arithmetic_intensity: 8.0 (>= 4.0 threshold → GPU-favorable)
///   SINGLE_THREAD_MAX: 999 (branch not taken: count > 999)
///   CPU_PARALLEL_MAX: 50000 (branch not taken: count > 50000)
///   Decision: GPU available + high intensity → GpuCompute
/// ```
///
/// # Coverage
///
/// All 4 [`DispatchKind`] branches are documented:
///
/// | Branch | Decision line |
/// |--------|---------------|
/// | `SingleThread` | `count <= SINGLE_THREAD_MAX → SingleThread` |
/// | `CpuParallel` (medium, no GPU) | `count <= CPU_PARALLEL_MAX + (no GPU OR low intensity) → CpuParallel` |
/// | `GpuCompute` | `GPU available + high intensity → GpuCompute` |
/// | `CpuParallel` (large, no GPU) | `count > CPU_PARALLEL_MAX + no GPU → CpuParallel` |
#[must_use]
pub fn explain_dispatch(ctx: &WorkloadContext, decision: DispatchKind) -> String {
    let intensity_line = match ctx.arithmetic_intensity {
        Some(ai) => {
            let favorable = if ai >= GPU_ARITHMETIC_INTENSITY_THRESHOLD {
                "GPU-favorable"
            } else {
                "memory-bound"
            };
            format!(
                "  arithmetic_intensity: {ai} (>= {threshold} threshold → {favorable})",
                threshold = GPU_ARITHMETIC_INTENSITY_THRESHOLD,
            )
        }
        None => "  arithmetic_intensity: unknown (treated as GPU-favorable)".to_string(),
    };

    let st_line = if ctx.element_count <= SINGLE_THREAD_MAX {
        format!(
            "  SINGLE_THREAD_MAX: {max} (branch TAKEN: count <= {max})",
            max = SINGLE_THREAD_MAX
        )
    } else {
        format!(
            "  SINGLE_THREAD_MAX: {max} (branch not taken: count > {max})",
            max = SINGLE_THREAD_MAX
        )
    };

    let cpu_parallel_line = if ctx.element_count <= CPU_PARALLEL_MAX {
        format!(
            "  CPU_PARALLEL_MAX: {max} (branch TAKEN: count <= {max})",
            max = CPU_PARALLEL_MAX
        )
    } else {
        format!(
            "  CPU_PARALLEL_MAX: {max} (branch not taken: count > {max})",
            max = CPU_PARALLEL_MAX
        )
    };

    let decision_line = match decision {
        DispatchKind::SingleThread => {
            "  Decision: count <= SINGLE_THREAD_MAX → SingleThread".to_string()
        }
        DispatchKind::CpuParallel => {
            if ctx.element_count > CPU_PARALLEL_MAX {
                if ctx.gpu_available {
                    "  Decision: count > CPU_PARALLEL_MAX + GPU available + low intensity → CpuParallel (demotion)".to_string()
                } else {
                    "  Decision: count > CPU_PARALLEL_MAX + no GPU → CpuParallel (fallback)"
                        .to_string()
                }
            } else if ctx.element_count > SINGLE_THREAD_MAX {
                if ctx.gpu_available
                    && ctx
                        .arithmetic_intensity
                        .is_none_or(|ai| ai >= GPU_ARITHMETIC_INTENSITY_THRESHOLD)
                {
                    // This shouldn't happen in practice (decide_dynamic would return GpuCompute),
                    // but we handle it defensively for the explain function.
                    "  Decision: medium band + GPU + high intensity → CpuParallel (unexpected — see GpuCompute path)".to_string()
                } else if !ctx.gpu_available {
                    "  Decision: medium band + no GPU → CpuParallel".to_string()
                } else {
                    "  Decision: medium band + low intensity → CpuParallel".to_string()
                }
            } else {
                "  Decision: count <= SINGLE_THREAD_MAX → SingleThread (CpuParallel not reached)"
                    .to_string()
            }
        }
        DispatchKind::GpuCompute => {
            if ctx.data_location == DataLocation::Gpu && ctx.gpu_available {
                "  Decision: T10 data-locality → data on GPU → GpuCompute (no PCIe transfer)"
                    .to_string()
            } else {
                "  Decision: GPU available + high intensity → GpuCompute".to_string()
            }
        }
    };

    let data_location_line = match ctx.data_location {
        DataLocation::Cpu => "  data_location: cpu".to_string(),
        DataLocation::Gpu if ctx.gpu_available => {
            "  data_location: gpu (resident — T10 keeps dispatch on GPU)".to_string()
        }
        DataLocation::Gpu => {
            "  data_location: gpu (but no GPU available — falling through to CPU)".to_string()
        }
    };

    // T11: Cost model line (only when bytes_per_element is set).
    let cost_model_line = if ctx.bytes_per_element > 0 {
        let costs = estimate_costs(ctx);
        format!(
            "  cost_model: gpu={:.2}µs cpu={:.2}µs transfer={:.2}µs occupancy={:.1}x",
            costs.gpu_time * 1e6,
            costs.cpu_time * 1e6,
            costs.transfer_time * 1e6,
            costs.occupancy_factor,
        )
    } else {
        String::new()
    };

    if cost_model_line.is_empty() {
        format!(
            "Dispatch: {decision:?}\n\
             element_count: {count}\n\
             gpu_available: {gpu}\n\
             {data_location_line}\n\
             {intensity_line}\n\
             {st_line}\n\
             {cpu_parallel_line}\n\
             {decision_line}",
            count = ctx.element_count,
            gpu = ctx.gpu_available,
        )
    } else {
        format!(
            "Dispatch: {decision:?}\n\
             element_count: {count}\n\
             gpu_available: {gpu}\n\
             {data_location_line}\n\
             {intensity_line}\n\
             {cost_model_line}\n\
             {st_line}\n\
             {cpu_parallel_line}\n\
             {decision_line}",
            count = ctx.element_count,
            gpu = ctx.gpu_available,
        )
    }
}

#[cfg(test)]
mod tests {
    //! Smoke tests at the module level — full behavioral coverage lives in
    //! `tests/threshold_tests.rs` so the QA filter
    //! `cargo test -p buff-lang-runtime dispatch_threshold` matches.

    use super::*;

    #[test]
    fn explain_dispatch_single_thread() {
        let ctx = WorkloadContext::new(500, true);
        let decision = decide_dynamic(&ctx);
        let explain = explain_dispatch(&ctx, decision);
        assert_eq!(decision, DispatchKind::SingleThread);
        assert!(explain.contains("Dispatch: SingleThread"));
        assert!(explain.contains("element_count: 500"));
        assert!(explain.contains("SINGLE_THREAD_MAX: 999 (branch TAKEN: count <= 999)"));
        assert!(explain.contains("Decision: count <= SINGLE_THREAD_MAX → SingleThread"));
    }

    #[test]
    fn explain_dispatch_cpu_parallel_medium_no_gpu() {
        let ctx = WorkloadContext::new(10_000, false);
        let decision = decide_dynamic(&ctx);
        let explain = explain_dispatch(&ctx, decision);
        assert_eq!(decision, DispatchKind::CpuParallel);
        assert!(explain.contains("Dispatch: CpuParallel"));
        assert!(explain.contains("element_count: 10000"));
        assert!(explain.contains("CPU_PARALLEL_MAX: 50000 (branch TAKEN: count <= 50000)"));
        assert!(explain.contains("Decision: medium band + no GPU → CpuParallel"));
    }

    #[test]
    fn explain_dispatch_gpu_compute() {
        let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
        let decision = decide_dynamic(&ctx);
        let explain = explain_dispatch(&ctx, decision);
        assert_eq!(decision, DispatchKind::GpuCompute);
        assert!(explain.contains("Dispatch: GpuCompute"));
        assert!(explain.contains("element_count: 100000"));
        assert!(explain.contains("gpu_available: true"));
        assert!(explain.contains("arithmetic_intensity: 8"));
        assert!(explain.contains("GPU-favorable"));
        assert!(explain.contains("Decision: GPU available + high intensity → GpuCompute"));
    }

    #[test]
    fn explain_dispatch_cpu_parallel_large_no_gpu() {
        let ctx = WorkloadContext::new(100_000, false);
        let decision = decide_dynamic(&ctx);
        let explain = explain_dispatch(&ctx, decision);
        assert_eq!(decision, DispatchKind::CpuParallel);
        assert!(explain.contains("Dispatch: CpuParallel"));
        assert!(explain.contains("element_count: 100000"));
        assert!(explain.contains("gpu_available: false"));
        assert!(explain.contains("CPU_PARALLEL_MAX: 50000 (branch not taken: count > 50000)"));
        assert!(explain
            .contains("Decision: count > CPU_PARALLEL_MAX + no GPU → CpuParallel (fallback)"));
    }

    #[test]
    fn explain_dispatch_unknown_intensity_treated_as_gpu_favorable() {
        let ctx = WorkloadContext::new(100_000, true);
        let decision = decide_dynamic(&ctx);
        let explain = explain_dispatch(&ctx, decision);
        assert_eq!(decision, DispatchKind::GpuCompute);
        assert!(explain.contains("arithmetic_intensity: unknown (treated as GPU-favorable)"));
    }

    #[test]
    fn dispatch_threshold_module_smoke_qa_boundaries() {
        // The 6 QA boundary cases in one place — a fast regression catch.
        assert_eq!(
            decide(999, true, None, 8),
            DispatchKind::SingleThread,
            "999 must be SingleThread"
        );
        assert_eq!(
            decide(1_000, true, None, 8),
            DispatchKind::CpuParallel,
            "1000 must be CpuParallel"
        );
        assert_eq!(
            decide(50_000, true, None, 8),
            DispatchKind::CpuParallel,
            "50_000 must be CpuParallel"
        );
        assert_eq!(
            decide(50_001, true, None, 8),
            DispatchKind::GpuCompute,
            "50_001 with GPU must be GpuCompute"
        );
        assert_eq!(
            decide(50_001, false, None, 8),
            DispatchKind::CpuParallel,
            "50_001 without GPU must fall back to CpuParallel"
        );
        assert_eq!(
            decide(1_000_000, true, Some(1), 1_073_741_824),
            DispatchKind::CpuParallel,
            "data exceeding VRAM must fall back to CpuParallel"
        );
    }
}
