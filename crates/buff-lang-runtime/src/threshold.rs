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
/// Kept private: callers should go through [`decide`]. Exposed as a separate
/// fn so T49 (hints) and T45 (GPU dispatch) can reuse the exact same
/// overflow-aware check without re-implementing it.
fn fits_vram(element_count: usize, bytes_per_element: u64, available: Option<u64>) -> bool {
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

#[cfg(test)]
mod tests {
    //! Smoke tests at the module level — full behavioral coverage lives in
    //! `tests/threshold_tests.rs` so the QA filter
    //! `cargo test -p buff-lang-runtime dispatch_threshold` matches.

    use super::*;

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
