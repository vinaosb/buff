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
    // Branch 1: element_count ≤ SINGLE_THREAD_MAX → SingleThread.
    // Tiny work: parallel/GPU setup overhead dominates at this size.
    // GPU availability and intensity are irrelevant here.
    if ctx.element_count <= SINGLE_THREAD_MAX {
        return DispatchKind::SingleThread;
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
        None => {
            "  arithmetic_intensity: unknown (treated as GPU-favorable)".to_string()
        }
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
                    "  Decision: count > CPU_PARALLEL_MAX + no GPU → CpuParallel (fallback)".to_string()
                }
            } else if ctx.element_count > SINGLE_THREAD_MAX {
                if ctx.gpu_available && ctx.arithmetic_intensity.is_none_or(|ai| ai >= GPU_ARITHMETIC_INTENSITY_THRESHOLD) {
                    // This shouldn't happen in practice (decide_dynamic would return GpuCompute),
                    // but we handle it defensively for the explain function.
                    "  Decision: medium band + GPU + high intensity → CpuParallel (unexpected — see GpuCompute path)".to_string()
                } else if !ctx.gpu_available {
                    "  Decision: medium band + no GPU → CpuParallel".to_string()
                } else {
                    "  Decision: medium band + low intensity → CpuParallel".to_string()
                }
            } else {
                "  Decision: count <= SINGLE_THREAD_MAX → SingleThread (CpuParallel not reached)".to_string()
            }
        }
        DispatchKind::GpuCompute => {
            "  Decision: GPU available + high intensity → GpuCompute".to_string()
        }
    };

    format!(
        "Dispatch: {decision:?}\n\
         element_count: {count}\n\
         gpu_available: {gpu}\n\
         {intensity_line}\n\
         {st_line}\n\
         {cpu_parallel_line}\n\
         {decision_line}",
        count = ctx.element_count,
        gpu = ctx.gpu_available,
    )
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
        assert!(explain.contains("Decision: count > CPU_PARALLEL_MAX + no GPU → CpuParallel (fallback)"));
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
