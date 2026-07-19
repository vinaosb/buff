//! T40 GREEN: pure automatic dispatch threshold logic.
//!
//! Decision rules (see `src/threshold.rs`):
//! * `element_count <= 999`           → [`SingleThread`](DispatchKind::SingleThread)
//! * `1000 <= element_count <= 50_000` → [`CpuParallel`](DispatchKind::CpuParallel)
//! * `element_count > 50_000`          → [`GpuCompute`](DispatchKind::GpuCompute) IF
//!   `gpu_available` AND data fits VRAM, else [`CpuParallel`](DispatchKind::CpuParallel).
//!
//! Every test name below contains the substring `dispatch_threshold` so the
//! QA filter `cargo test -p buff-lang-runtime dispatch_threshold` matches
//! them all.
//!
//! Coverage matrix:
//! * 6 QA acceptance cases (verbatim from T40 spec).
//! * Boundary cases (0, 1, exactly 999 / 1000 / 50_000 / 50_001).
//! * VRAM: `None`, exactly-fits, one-byte-over, huge-count overflow.
//! * GPU toggle at multiple sizes.
//! * Constant values pinned.
//! * `DispatchPlanner` struct mirrors free `decide` fn.

use buff_lang_runtime::{
    decide, DispatchKind, DispatchPlanner, CPU_PARALLEL_MAX, SINGLE_THREAD_MAX,
};

// ---------------------------------------------------------------------------
// QA acceptance cases (6) — verbatim from T40 spec.
// ---------------------------------------------------------------------------

#[test]
fn dispatch_threshold_qa_999_elements_returns_single_thread() {
    // QA case #1: decide(999, true, None, _) -> SingleThread.
    assert_eq!(
        decide(999, true, None, 8),
        DispatchKind::SingleThread,
        "999 elements must always pick SingleThread (below <1000 threshold)"
    );
}

#[test]
fn dispatch_threshold_qa_1000_elements_returns_cpu_parallel() {
    // QA case #2: decide(1000, true, None, _) -> CpuParallel.
    assert_eq!(
        decide(1_000, true, None, 8),
        DispatchKind::CpuParallel,
        "1000 elements crosses into CpuParallel band"
    );
}

#[test]
fn dispatch_threshold_qa_50000_elements_returns_cpu_parallel() {
    // QA case #3: decide(50_000, true, None, _) -> CpuParallel.
    assert_eq!(
        decide(50_000, true, None, 8),
        DispatchKind::CpuParallel,
        "50_000 is the inclusive upper bound for CpuParallel"
    );
}

#[test]
fn dispatch_threshold_qa_50001_elements_returns_gpu_compute() {
    // QA case #4: decide(50_001, true, None, _) -> GpuCompute.
    assert_eq!(
        decide(50_001, true, None, 8),
        DispatchKind::GpuCompute,
        "50_001 with GPU available and unknown VRAM must pick GpuCompute"
    );
}

#[test]
fn dispatch_threshold_qa_50001_no_gpu_falls_back_to_cpu_parallel() {
    // QA case #5: decide(50_001, false, None, _) -> CpuParallel.
    assert_eq!(
        decide(50_001, false, None, 8),
        DispatchKind::CpuParallel,
        "without a GPU, large data must fall back to CpuParallel (never SingleThread)"
    );
}

#[test]
fn dispatch_threshold_qa_data_exceeds_vram_falls_back_to_cpu_parallel() {
    // QA case #6: decide(1_000_000, true, Some(small_vram), large_bpe)
    //             -> CpuParallel.
    //
    // 1_000_000 * 1 GiB (1_073_741_824 bytes) would need ~1 PB of VRAM —
    // far above the 1-byte cap we pass in. Must fall back to CpuParallel.
    assert_eq!(
        decide(1_000_000, true, Some(1), 1_073_741_824),
        DispatchKind::CpuParallel,
        "data exceeding VRAM must fall back to CpuParallel even when GPU is available"
    );
}

// ---------------------------------------------------------------------------
// Boundary cases — exact thresholds.
// ---------------------------------------------------------------------------

#[test]
fn dispatch_threshold_zero_elements_returns_single_thread() {
    // Edge: zero elements is the smallest possible input — must be
    // SingleThread (no work to parallelize).
    assert_eq!(
        decide(0, true, None, 8),
        DispatchKind::SingleThread,
        "0 elements must be SingleThread"
    );
}

#[test]
fn dispatch_threshold_one_element_returns_single_thread() {
    // Edge: a single element is the most clearly-sequential case.
    assert_eq!(
        decide(1, true, None, 8),
        DispatchKind::SingleThread,
        "1 element must be SingleThread"
    );
}

#[test]
fn dispatch_threshold_single_thread_boundary_998_still_single_thread() {
    // One below SINGLE_THREAD_MAX — still sequential.
    assert_eq!(
        decide(SINGLE_THREAD_MAX - 1, true, None, 8),
        DispatchKind::SingleThread,
    );
}

#[test]
fn dispatch_threshold_single_thread_boundary_999_is_single_thread() {
    // Exactly SINGLE_THREAD_MAX (999) — inclusive upper bound for SingleThread.
    assert_eq!(
        decide(SINGLE_THREAD_MAX, true, None, 8),
        DispatchKind::SingleThread,
        "exactly SINGLE_THREAD_MAX (999) must be SingleThread"
    );
}

#[test]
fn dispatch_threshold_cpu_parallel_boundary_1000_is_cpu_parallel() {
    // Exactly the first CpuParallel value.
    assert_eq!(
        decide(SINGLE_THREAD_MAX + 1, true, None, 8),
        DispatchKind::CpuParallel,
        "exactly 1000 must be CpuParallel"
    );
}

#[test]
fn dispatch_threshold_cpu_parallel_boundary_49999_is_cpu_parallel() {
    // One below CPU_PARALLEL_MAX.
    assert_eq!(
        decide(CPU_PARALLEL_MAX - 1, true, None, 8),
        DispatchKind::CpuParallel,
    );
}

#[test]
fn dispatch_threshold_cpu_parallel_boundary_50000_is_cpu_parallel() {
    // Exactly CPU_PARALLEL_MAX (50_000) — inclusive upper bound for CpuParallel.
    assert_eq!(
        decide(CPU_PARALLEL_MAX, true, None, 8),
        DispatchKind::CpuParallel,
        "exactly CPU_PARALLEL_MAX (50_000) must be CpuParallel"
    );
}

#[test]
fn dispatch_threshold_gpu_compute_boundary_50001_is_gpu_compute() {
    // Exactly CPU_PARALLEL_MAX + 1 (50_001) — first GpuCompute value when a
    // GPU is available and VRAM is unknown (None == assume fits).
    assert_eq!(
        decide(CPU_PARALLEL_MAX + 1, true, None, 8),
        DispatchKind::GpuCompute,
        "exactly 50_001 with GPU + None VRAM must be GpuCompute"
    );
}

// ---------------------------------------------------------------------------
// VRAM semantics — None, exactly-fits, one-byte-over, overflow.
// ---------------------------------------------------------------------------

#[test]
fn dispatch_threshold_vram_none_at_huge_count_with_gpu_returns_gpu_compute() {
    // None == "assume fits" — large count + GPU available must pick GPU.
    assert_eq!(
        decide(10_000_000, true, None, 8),
        DispatchKind::GpuCompute,
        "None VRAM means unknown — GPU stays eligible"
    );
}

#[test]
fn dispatch_threshold_vram_none_at_huge_count_without_gpu_returns_cpu_parallel() {
    // Same as above but GPU missing — None VRAM doesn't rescue us.
    assert_eq!(
        decide(10_000_000, false, None, 8),
        DispatchKind::CpuParallel,
    );
}

#[test]
fn dispatch_threshold_vram_exactly_fits_returns_gpu_compute() {
    // 50_001 * 8 = 400_008 bytes. VRAM cap exactly 400_008 → fits (<=).
    assert_eq!(
        decide(50_001, true, Some(50_001 * 8), 8),
        DispatchKind::GpuCompute,
        "total_bytes == cap must fit (<=, inclusive)"
    );
}

#[test]
fn dispatch_threshold_vram_one_byte_under_returns_gpu_compute() {
    // 50_001 * 8 = 400_008 bytes. Cap 400_009 → fits.
    assert_eq!(
        decide(50_001, true, Some(50_001 * 8 + 1), 8),
        DispatchKind::GpuCompute,
    );
}

#[test]
fn dispatch_threshold_vram_one_byte_over_returns_cpu_parallel() {
    // 50_001 * 8 = 400_008 bytes. Cap 400_007 → does NOT fit (<=).
    // Must fall back to CpuParallel — even though a GPU is available.
    assert_eq!(
        decide(50_001, true, Some(50_001 * 8 - 1), 8),
        DispatchKind::CpuParallel,
        "total_bytes > cap must fall back to CpuParallel"
    );
}

#[test]
fn dispatch_threshold_vram_zero_cap_excludes_gpu_for_nonempty_data() {
    // Cap 0 + bytes_per_element > 0 + element_count > 0 → does NOT fit.
    assert_eq!(
        decide(50_001, true, Some(0), 8),
        DispatchKind::CpuParallel,
        "zero-cap VRAM with non-zero bytes_per_element must fall back"
    );
}

#[test]
fn dispatch_threshold_vram_zero_cap_zero_bpe_allows_gpu() {
    // Pathological: 0 bytes/element × any count = 0 bytes total, cap 0 →
    // 0 <= 0 → fits. (Practical callers won't hit this, but the math must
    // be consistent — we use `<=`, not `<`.)
    assert_eq!(
        decide(50_001, true, Some(0), 0),
        DispatchKind::GpuCompute,
        "zero-byte elements and zero-byte cap → 0<=0 → fits"
    );
}

#[test]
fn dispatch_threshold_vram_overflow_returns_cpu_parallel() {
    // element_count * bytes_per_element overflows u64 → treated as
    // "does not fit". Build the inputs so the multiply definitely
    // overflows: u64::MAX * 2 = (2^64 - 1) * 2 which overflows.
    //
    // Use a large element_count too (> CPU_PARALLEL_MAX) so the GPU branch
    // is the one we fall out of.
    assert_eq!(
        decide(u64::MAX as usize, true, Some(u64::MAX), 2),
        DispatchKind::CpuParallel,
        "multiplication overflow must be treated as 'does not fit'"
    );
}

// ---------------------------------------------------------------------------
// GPU availability toggling at every band.
// ---------------------------------------------------------------------------

#[test]
fn dispatch_threshold_gpu_toggle_ignored_in_single_thread_band() {
    // GPU availability is irrelevant below the CpuParallel band.
    assert_eq!(decide(500, true, None, 8), DispatchKind::SingleThread);
    assert_eq!(decide(500, false, None, 8), DispatchKind::SingleThread);
}

#[test]
fn dispatch_threshold_gpu_toggle_ignored_in_cpu_parallel_band() {
    // GPU availability is irrelevant in the CpuParallel band — we don't
    // "promote" to GPU at this size, even with a GPU present.
    assert_eq!(decide(20_000, true, None, 8), DispatchKind::CpuParallel);
    assert_eq!(decide(20_000, false, None, 8), DispatchKind::CpuParallel);
}

#[test]
fn dispatch_threshold_gpu_toggle_decisive_in_gpu_band() {
    // Above CPU_PARALLEL_MAX, GPU availability flips the result.
    assert_eq!(
        decide(100_000, true, None, 8),
        DispatchKind::GpuCompute,
        "with GPU: GpuCompute"
    );
    assert_eq!(
        decide(100_000, false, None, 8),
        DispatchKind::CpuParallel,
        "without GPU: CpuParallel fallback"
    );
}

// ---------------------------------------------------------------------------
// bytes_per_element variations.
// ---------------------------------------------------------------------------

#[test]
fn dispatch_threshold_bytes_per_element_one_byte_fits_small_vram() {
    // 100_000 elements * 1 byte = 100_000 bytes. Cap 100_000 → fits.
    assert_eq!(
        decide(100_000, true, Some(100_000), 1),
        DispatchKind::GpuCompute,
    );
}

#[test]
fn dispatch_threshold_bytes_per_element_eight_bytes_same_count_exceeds_vram() {
    // Same count as above (100_000), but 8 bytes/elem → 800_000 bytes,
    // exceeds the 100_000-byte cap from the previous test.
    assert_eq!(
        decide(100_000, true, Some(100_000), 8),
        DispatchKind::CpuParallel,
        "bytes_per_element matters: same count, larger elements exceed VRAM"
    );
}

// ---------------------------------------------------------------------------
// Constants are pinned to documented values.
// ---------------------------------------------------------------------------

#[test]
fn dispatch_threshold_constants_have_documented_values() {
    // If someone changes the threshold constants, the QA cases stop
    // working. Pin them explicitly so a refactor can't silently drift.
    assert_eq!(SINGLE_THREAD_MAX, 999, "spec: <1000 → SingleThread");
    assert_eq!(CPU_PARALLEL_MAX, 50_000, "spec: 1000–50000 → CpuParallel");
    // The two bands are non-overlapping by construction (999 < 50_000) —
    // no need for a runtime `assert!(a < b)` on two constants (clippy:
    // assertions_on_constants).
}

// ---------------------------------------------------------------------------
// DispatchPlanner struct — thin wrapper that must mirror decide().
// ---------------------------------------------------------------------------

#[test]
fn dispatch_threshold_planner_new_default_are_equivalent() {
    // We are explicitly exercising the `Default` impl here — clippy's
    // `default_constructed_unit_structs` lint is correct that production
    // code should write `DispatchPlanner` directly, but in this test we
    // WANT to verify both constructors yield the same value.
    #![allow(clippy::default_constructed_unit_structs)]
    let a = DispatchPlanner::new();
    let b = DispatchPlanner::default();
    assert_eq!(a, b, "new() and default() must yield the same planner");
}

#[test]
fn dispatch_threshold_planner_decide_matches_free_function_across_all_bands() {
    // DispatchPlanner::decide is documented to mirror the free `decide` fn
    // exactly — verify this holds across all three bands and both GPU
    // states.
    let planner = DispatchPlanner::new();
    for &(count, gpu) in &[
        (0usize, true),
        (999, true),
        (1_000, false),
        (50_000, true),
        (50_001, true),
        (50_001, false),
        (1_000_000, true),
    ] {
        let from_fn = decide(count, gpu, None, 8);
        let from_planner = planner.decide(count, gpu, None, 8);
        assert_eq!(
            from_fn, from_planner,
            "planner disagrees with free fn at count={count} gpu={gpu}"
        );
    }
}

#[test]
fn dispatch_threshold_planner_decide_vram_aware_fallback_matches_free_fn() {
    // Same parity check for the VRAM fallback path.
    let planner = DispatchPlanner::new();
    assert_eq!(
        planner.decide(1_000_000, true, Some(1), 1_073_741_824),
        decide(1_000_000, true, Some(1), 1_073_741_824),
    );
    assert_eq!(
        planner.decide(1_000_000, true, Some(u64::MAX), 1),
        decide(1_000_000, true, Some(u64::MAX), 1),
    );
}
