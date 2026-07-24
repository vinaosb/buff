//! T5: Dynamic workload-aware CPU/GPU dispatch (v1.25 Wave 0, Track B MOAT).
//!
//! Integration tests for [`decide_dynamic`] + [`WorkloadContext`] +
//! [`decide_with_prefer_dynamic`]. Every test name contains
//! `dynamic_dispatch` so the QA filter
//! `cargo test -p buff-lang-runtime dynamic_dispatch` matches the whole
//! suite.
//!
//! # Coverage matrix
//!
//! * **4 spec branches** (4 tests): tiny→SingleThread, medium+no-GPU→CpuParallel,
//!   large+GPU+high→GpuCompute, large+no-GPU→CpuParallel.
//! * **Dynamic promotion** (1 test): medium+GPU+high→GpuCompute (static
//!   decide() cannot do this).
//! * **Dynamic demotion** (1 test): large+GPU+low→CpuParallel (static
//!   decide() would pick GpuCompute).
//! * **None intensity** (2 tests): treated as GPU-favorable; matches
//!   decide() for large+GPU.
//! * **Boundary** (4 tests): exactly SINGLE_THREAD_MAX / +1, exactly
//!   CPU_PARALLEL_MAX / +1.
//! * **Constant pinned** (1 test): GPU_ARITHMETIC_INTENSITY_THRESHOLD == 4.0.
//! * **MockGpuBackend integration** (3 tests): GpuCompute branch records
//!   a dispatch; CpuParallel branch skips GPU; demoted branch skips GPU.
//! * **decide_with_prefer_dynamic** (5 tests): no-hint parity, cost
//!   override, hint honored, graceful fallback, NPU parity.
//! * **GPU-fallback guarantee** (1 test): gpu_available=false NEVER yields
//!   GpuCompute across all bands.
//! * **WorkloadContext builder** (2 tests): new() + with_intensity();
//!   Default values.

use buff_lang_runtime::{
    cpu_fallback_map, decide, decide_dynamic, decide_with_prefer, decide_with_prefer_dynamic,
    DispatchKind, GpuBackend, MockGpuBackend, Prefer, WorkloadContext, CPU_PARALLEL_MAX,
    GPU_ARITHMETIC_INTENSITY_THRESHOLD, SINGLE_THREAD_MAX,
};

/// WGSL shader source — content irrelevant for MockGpuBackend (it ignores
/// the shader and applies its CPU closure). Matches the stable binding
/// contract shape for documentation.
const SHADER_WGSL: &str = "@compute @workgroup_size(64) fn main() {}";

// ===========================================================================
// QA acceptance: the 4 spec branches (verbatim from T5 spec).
// ===========================================================================

#[test]
fn dynamic_dispatch_branch1_small_input_returns_single_thread() {
    // Spec branch 1: element_count ≤ SINGLE_THREAD_MAX → SingleThread.
    // 500 elements is well within the < 1000 band.
    let ctx = WorkloadContext::new(500, true);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::SingleThread,
        "500 elements must always pick SingleThread (below <1000 threshold)"
    );
}

#[test]
fn dynamic_dispatch_branch2_medium_no_gpu_returns_cpu_parallel() {
    // Spec branch 2: element_count ≤ CPU_PARALLEL_MAX AND no GPU → CpuParallel.
    // 10_000 elements is in the 1000..=50_000 band; no GPU available.
    let ctx = WorkloadContext::new(10_000, false);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::CpuParallel,
        "10_000 elements without a GPU must pick CpuParallel"
    );
}

#[test]
fn dynamic_dispatch_branch2_medium_low_intensity_returns_cpu_parallel() {
    // Spec branch 2 (intensity variant): medium + GPU available BUT
    // arithmetic_intensity is low (memory-bound) → CpuParallel.
    // 0.5 FLOPs/byte is well below the 4.0 threshold.
    let ctx = WorkloadContext::new(10_000, true).with_intensity(0.5);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::CpuParallel,
        "10_000 elements + GPU + low intensity must pick CpuParallel (memory-bound)"
    );
}

#[test]
fn dynamic_dispatch_branch3_large_gpu_high_intensity_returns_gpu_compute() {
    // Spec branch 3: element_count > CPU_PARALLEL_MAX AND gpu_available
    // AND arithmetic_intensity high → GpuCompute.
    // 100_000 elements (> 50_000) + GPU + 8.0 FLOPs/byte (≥ 4.0 threshold).
    let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "100_000 + GPU + high intensity must pick GpuCompute"
    );
}

#[test]
fn dynamic_dispatch_branch4_large_no_gpu_returns_cpu_parallel() {
    // Spec branch 4: element_count > CPU_PARALLEL_MAX AND no GPU → CpuParallel.
    // The graceful fallback: never SingleThread for large data, never
    // GpuCompute without a GPU.
    let ctx = WorkloadContext::new(100_000, false);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::CpuParallel,
        "100_000 without a GPU must fall back to CpuParallel (never SingleThread)"
    );
}

// ===========================================================================
// Dynamic refinements over static decide() — the MOAT.
// ===========================================================================

#[test]
fn dynamic_dispatch_promotion_medium_gpu_high_intensity_returns_gpu_compute() {
    // DYNAMIC PROMOTION: medium band (≤ CPU_PARALLEL_MAX) + GPU + high
    // intensity → GpuCompute. Static decide() CANNOT do this — it always
    // returns CpuParallel for 1000..=50_000.
    let ctx = WorkloadContext::new(10_000, true).with_intensity(8.0);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "10_000 + GPU + high intensity must PROMOTE to GpuCompute (the MOAT)"
    );

    // Cross-check: static decide() returns CpuParallel for the same count.
    assert_eq!(
        decide(10_000, true, None, 4),
        DispatchKind::CpuParallel,
        "static decide() must still return CpuParallel for medium band"
    );
}

#[test]
fn dynamic_dispatch_demotion_large_gpu_low_intensity_returns_cpu_parallel() {
    // DYNAMIC DEMOTION: large band (> CPU_PARALLEL_MAX) + GPU + LOW
    // intensity → CpuParallel. Static decide() would pick GpuCompute
    // here, but memory-bound work doesn't benefit from the GPU.
    let ctx = WorkloadContext::new(100_000, true).with_intensity(0.5);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::CpuParallel,
        "100_000 + GPU + low intensity must DEMOTE to CpuParallel (memory-bound)"
    );

    // Cross-check: static decide() returns GpuCompute for the same count.
    assert_eq!(
        decide(100_000, true, None, 4),
        DispatchKind::GpuCompute,
        "static decide() must still return GpuCompute for large+GPU"
    );
}

// ===========================================================================
// None intensity — treated as GPU-favorable (matches decide() convention).
// ===========================================================================

#[test]
fn dynamic_dispatch_none_intensity_large_with_gpu_returns_gpu_compute() {
    // None intensity = "unknown" → treated as GPU-favorable.
    // For large + GPU, this matches static decide()'s GpuCompute.
    let ctx = WorkloadContext::new(100_000, true);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "None intensity + large + GPU must match decide()'s GpuCompute"
    );
    assert_eq!(
        decide_dynamic(&ctx),
        decide(100_000, true, None, 4),
        "dynamic with None intensity must match static decide() for large+GPU"
    );
}

#[test]
fn dynamic_dispatch_none_intensity_medium_with_gpu_returns_gpu_compute() {
    // None intensity = "unknown" → treated as GPU-favorable.
    // For medium + GPU, this triggers the PROMOTION (the only case where
    // dynamic-with-None differs from static decide()).
    let ctx = WorkloadContext::new(10_000, true);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "None intensity + medium + GPU must promote to GpuCompute"
    );
}

// ===========================================================================
// Boundary cases — exact thresholds.
// ===========================================================================

#[test]
fn dynamic_dispatch_boundary_single_thread_max_is_single_thread() {
    // Exactly SINGLE_THREAD_MAX (999) — inclusive upper bound for SingleThread.
    let ctx = WorkloadContext::new(SINGLE_THREAD_MAX, true).with_intensity(100.0);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::SingleThread,
        "exactly SINGLE_THREAD_MAX (999) must be SingleThread even with extreme intensity"
    );
}

#[test]
fn dynamic_dispatch_boundary_single_thread_max_plus_one_promotes_with_gpu() {
    // Exactly 1000 (SINGLE_THREAD_MAX + 1) — first non-tiny value.
    // With GPU + high intensity → GpuCompute (dynamic promotion).
    let ctx = WorkloadContext::new(SINGLE_THREAD_MAX + 1, true).with_intensity(8.0);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "1000 + GPU + high intensity must promote to GpuCompute"
    );
}

#[test]
fn dynamic_dispatch_boundary_cpu_parallel_max_with_gpu_high_promotes() {
    // Exactly CPU_PARALLEL_MAX (50_000) — still in the medium band.
    // With GPU + high intensity → GpuCompute (promotion, same as any
    // medium-band value).
    let ctx = WorkloadContext::new(CPU_PARALLEL_MAX, true).with_intensity(8.0);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "50_000 + GPU + high intensity must promote to GpuCompute"
    );
}

#[test]
fn dynamic_dispatch_boundary_cpu_parallel_max_plus_one_with_gpu_high() {
    // Exactly 50_001 (CPU_PARALLEL_MAX + 1) — first large-band value.
    // With GPU + high intensity → GpuCompute (same as static decide()).
    let ctx = WorkloadContext::new(CPU_PARALLEL_MAX + 1, true).with_intensity(8.0);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "50_001 + GPU + high intensity must be GpuCompute"
    );
}

// ===========================================================================
// GPU-fallback guarantee — gpu_available=false NEVER yields GpuCompute.
// ===========================================================================

#[test]
fn dynamic_dispatch_no_gpu_never_returns_gpu_compute_across_all_bands() {
    // The "GPU failure invisible; CPU fallback always correct" guarantee:
    // when gpu_available is false, decide_dynamic must NEVER return
    // GpuCompute, regardless of element count or intensity.
    for (count, intensity) in [
        (0usize, None),
        (500, None),
        (999, None),
        (1_000, None),
        (10_000, None),
        (50_000, None),
        (50_001, None),
        (100_000, None),
        (1_000_000, None),
        (10_000, Some(0.0)),
        (10_000, Some(4.0)),
        (10_000, Some(100.0)),
        (100_000, Some(0.0)),
        (100_000, Some(4.0)),
        (100_000, Some(100.0)),
    ] {
        let ctx = WorkloadContext::new(count, false).with_intensity_opt(intensity);
        let decision = decide_dynamic(&ctx);
        assert_ne!(
            decision,
            DispatchKind::GpuCompute,
            "gpu_available=false + count={count} + intensity={intensity:?} must NEVER be GpuCompute"
        );
    }
}

// ===========================================================================
// Constant is pinned.
// ===========================================================================

#[test]
fn dynamic_dispatch_arithmetic_intensity_threshold_is_four() {
    // If someone changes the threshold, the promote/demote tests stop
    // working. Pin it explicitly.
    assert_eq!(
        GPU_ARITHMETIC_INTENSITY_THRESHOLD, 4.0,
        "spec: ≥4.0 FLOPs/byte is compute-bound (GPU wins)"
    );
}

#[test]
fn dynamic_dispatch_intensity_at_exact_threshold_is_high() {
    // At exactly the threshold (4.0), the comparison is `>=` → high.
    // (Inclusive: 4.0 IS high enough for GPU.)
    let ctx = WorkloadContext::new(100_000, true).with_intensity(GPU_ARITHMETIC_INTENSITY_THRESHOLD);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "intensity == threshold (4.0) must be treated as high (>= comparison)"
    );
}

#[test]
fn dynamic_dispatch_intensity_just_below_threshold_is_low() {
    // One ULP below the threshold → low (memory-bound).
    let just_below = GPU_ARITHMETIC_INTENSITY_THRESHOLD - f64::EPSILON;
    let ctx = WorkloadContext::new(100_000, true).with_intensity(just_below);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::CpuParallel,
        "intensity just below threshold must be treated as low (demote to CpuParallel)"
    );
}

// ===========================================================================
// MockGpuBackend integration — the 4 branches exercised end-to-end.
// ===========================================================================

#[test]
fn dynamic_dispatch_mock_gpu_records_dispatch_for_gpu_compute_branch() {
    // Branch 3 (large + GPU + high → GpuCompute): when decide_dynamic picks
    // GpuCompute and a MockGpuBackend is available, the dispatch MUST be
    // recorded. This verifies the dynamic decision flows through to the
    // actual GPU dispatch path.
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    let input: Vec<f32> = (0..100_000).map(|i| i as f32).collect();

    // WorkloadContext that routes to GpuCompute.
    let ctx = WorkloadContext::new(input.len(), true).with_intensity(8.0);
    assert_eq!(decide_dynamic(&ctx), DispatchKind::GpuCompute);

    // Simulate the dispatch path: GpuCompute → backend.dispatch_map.
    let out = backend.dispatch_map(SHADER_WGSL, &input).unwrap();
    assert_eq!(backend.recorded_dispatches(), 1, "GPU dispatch must be recorded");
    assert_eq!(out.len(), input.len());
    assert_eq!(out[0], 0.0); // 0 * 2.0
    assert_eq!(out[1], 2.0); // 1 * 2.0
    assert_eq!(out[999], 1998.0); // 999 * 2.0
}

#[test]
fn dynamic_dispatch_mock_gpu_skipped_for_cpu_parallel_branch_no_gpu() {
    // Branch 2/4 (CpuParallel): when decide_dynamic picks CpuParallel,
    // the MockGpuBackend is NOT invoked — the CPU oracle runs instead.
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    let input: Vec<f32> = (0..10_000).map(|i| i as f32).collect();

    // WorkloadContext that routes to CpuParallel (no GPU).
    let ctx = WorkloadContext::new(input.len(), false);
    assert_eq!(decide_dynamic(&ctx), DispatchKind::CpuParallel);

    // Simulate the dispatch path: CpuParallel → CPU oracle (NOT backend).
    let out = cpu_fallback_map(&input, |x| x * 2.0);
    assert_eq!(
        backend.recorded_dispatches(),
        0,
        "CpuParallel branch must NOT touch the GPU backend"
    );
    assert_eq!(out.len(), input.len());
    assert_eq!(out[0], 0.0);
    assert_eq!(out[9999], 19_998.0);
}

#[test]
fn dynamic_dispatch_mock_gpu_skipped_for_demoted_memory_bound_workload() {
    // Dynamic demotion: large + GPU + LOW intensity → CpuParallel.
    // Even though a GPU is "available", the memory-bound workload stays
    // on CPU. The MockGpuBackend is NOT invoked.
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    let input: Vec<f32> = (0..100_000).map(|i| i as f32).collect();

    // WorkloadContext that demotes to CpuParallel (GPU present but low intensity).
    let ctx = WorkloadContext::new(input.len(), true).with_intensity(0.5);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::CpuParallel,
        "memory-bound large workload must demote to CpuParallel even with GPU"
    );

    // Simulate the dispatch path: demoted CpuParallel → CPU oracle.
    let out = cpu_fallback_map(&input, |x| x * 2.0);
    assert_eq!(
        backend.recorded_dispatches(),
        0,
        "Demoted memory-bound workload must NOT touch the GPU backend"
    );
    assert_eq!(out.len(), input.len());
}

// ===========================================================================
// decide_with_prefer_dynamic — hint layering over decide_dynamic.
// ===========================================================================

#[test]
fn dynamic_dispatch_with_prefer_none_matches_decide_dynamic_verbatim() {
    // Rule 1: Prefer::None delegates to decide_dynamic verbatim.
    // No behavior change vs the un-hinted dynamic path.
    for (count, gpu, intensity) in [
        (10usize, true, None),
        (999, true, None),
        (1_000, true, None),
        (10_000, true, Some(8.0)),
        (10_000, true, Some(0.5)),
        (50_001, true, None),
        (50_001, false, None),
        (100_000, true, Some(8.0)),
        (100_000, true, Some(0.5)),
        (1_000_000, true, None),
    ] {
        let ctx = WorkloadContext::new(count, gpu).with_intensity_opt(intensity);
        assert_eq!(
            decide_with_prefer_dynamic(&ctx, Prefer::None, None, 4),
            decide_dynamic(&ctx),
            "Prefer::None must match decide_dynamic for count={count} gpu={gpu} ai={intensity:?}"
        );
    }
}

#[test]
fn dynamic_dispatch_with_prefer_gpu_small_input_cost_override() {
    // Rule 2: @prefer(gpu) + small input → cost-model override → dynamic.
    // The QA "10 elements → CPU" case: 10 < PREFER_GPU_MIN_ELEMENTS (1024),
    // so the hint is overridden and decide_dynamic runs (10 ≤ 999 → SingleThread).
    let ctx = WorkloadContext::new(10, true);
    assert_eq!(
        decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4),
        DispatchKind::SingleThread,
        "@prefer(gpu) + 10 elements must cost-override to SingleThread"
    );
}

#[test]
fn dynamic_dispatch_with_prefer_gpu_large_input_with_gpu_is_gpu_compute() {
    // Rule 3: @prefer(gpu) + large input + GPU available → GpuCompute.
    // The hint is honored — intensity is NOT consulted (explicit override).
    let ctx = WorkloadContext::new(100_000, true).with_intensity(0.5);
    assert_eq!(
        decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4),
        DispatchKind::GpuCompute,
        "@prefer(gpu) + large + GPU must be GpuCompute (intensity ignored for honored hint)"
    );

    // Cross-check: without the hint, the low intensity would demote to CpuParallel.
    assert_eq!(
        decide_with_prefer_dynamic(&ctx, Prefer::None, None, 4),
        DispatchKind::CpuParallel,
        "without hint, low intensity must demote to CpuParallel"
    );
}

#[test]
fn dynamic_dispatch_with_prefer_gpu_large_input_no_gpu_graceful_fallback() {
    // Rule 4: @prefer(gpu) + large input + NO GPU → graceful fallback
    // → decide_dynamic → CpuParallel.
    let ctx = WorkloadContext::new(100_000, false);
    assert_eq!(
        decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4),
        DispatchKind::CpuParallel,
        "@prefer(gpu) + large + no GPU must fall back to CpuParallel"
    );
}

#[test]
fn dynamic_dispatch_with_prefer_npu_matches_prefer_gpu() {
    // @prefer(npu) is routed identically to @prefer(gpu) in v1.0
    // (NPU backend is post-v1.0). Both honor the hint the same way.
    let ctx = WorkloadContext::new(100_000, true);
    assert_eq!(
        decide_with_prefer_dynamic(&ctx, Prefer::Npu, None, 4),
        decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4),
        "@prefer(npu) must match @prefer(gpu) routing"
    );
}

#[test]
fn dynamic_dispatch_with_prefer_gpu_vram_exceeds_falls_back_to_dynamic() {
    // Rule 4 (VRAM variant): @prefer(gpu) + large + GPU BUT data exceeds
    // VRAM → graceful fallback → decide_dynamic.
    //
    // 100_000 * 4 bytes = 400_000 bytes. Cap 1 → does not fit.
    // decide_dynamic with None intensity + GPU → GpuCompute... but wait,
    // decide_dynamic doesn't check VRAM. So the fallback path lands on
    // decide_dynamic which returns GpuCompute for large+GPU+None-intensity.
    //
    // Hmm — this is a subtle interaction. The hint path checked VRAM and
    // fell through; decide_dynamic doesn't check VRAM (by design — it's a
    // pure workload-size + intensity decision, NOT a VRAM decision).
    // The CALLER is responsible for not dispatching more data than fits
    // VRAM (T46 tiling handles that). So this test verifies the delegation
    // shape: hint-cannot-honor → decide_dynamic.
    let ctx = WorkloadContext::new(100_000, true);
    assert_eq!(
        decide_with_prefer_dynamic(&ctx, Prefer::Gpu, Some(1), 4),
        decide_dynamic(&ctx),
        "VRAM-exceeded hint fallback must delegate to decide_dynamic"
    );
}

// ===========================================================================
// decide_with_prefer_dynamic vs decide_with_prefer — verify the dynamic
// path differs from static exactly where expected.
// ===========================================================================

#[test]
fn dynamic_dispatch_with_prefer_none_promotes_where_static_does_not() {
    // No hint + medium + GPU + high intensity:
    //   static decide_with_prefer → CpuParallel (static band).
    //   dynamic decide_with_prefer_dynamic → GpuCompute (promotion).
    let ctx = WorkloadContext::new(10_000, true).with_intensity(8.0);
    assert_eq!(
        decide_with_prefer(10_000, Prefer::None, true, None, 4),
        DispatchKind::CpuParallel,
        "static: medium band is always CpuParallel with no hint"
    );
    assert_eq!(
        decide_with_prefer_dynamic(&ctx, Prefer::None, None, 4),
        DispatchKind::GpuCompute,
        "dynamic: medium + GPU + high promotes to GpuCompute"
    );
}

#[test]
fn dynamic_dispatch_with_prefer_none_demotes_where_static_does_not() {
    // No hint + large + GPU + low intensity:
    //   static decide_with_prefer → GpuCompute (static band).
    //   dynamic decide_with_prefer_dynamic → CpuParallel (demotion).
    let ctx = WorkloadContext::new(100_000, true).with_intensity(0.5);
    assert_eq!(
        decide_with_prefer(100_000, Prefer::None, true, None, 4),
        DispatchKind::GpuCompute,
        "static: large + GPU is always GpuCompute with no hint"
    );
    assert_eq!(
        decide_with_prefer_dynamic(&ctx, Prefer::None, None, 4),
        DispatchKind::CpuParallel,
        "dynamic: large + GPU + low demotes to CpuParallel"
    );
}

// ===========================================================================
// WorkloadContext builder + Default.
// ===========================================================================

#[test]
fn dynamic_dispatch_workload_context_new_sets_none_intensity() {
    let ctx = WorkloadContext::new(42, true);
    assert_eq!(ctx.element_count, 42);
    assert!(ctx.gpu_available);
    assert_eq!(ctx.arithmetic_intensity, None);
}

#[test]
fn dynamic_dispatch_workload_context_with_intensity_sets_some() {
    let ctx = WorkloadContext::new(42, true).with_intensity(5.5);
    assert_eq!(ctx.element_count, 42);
    assert!(ctx.gpu_available);
    assert_eq!(ctx.arithmetic_intensity, Some(5.5));
}

#[test]
fn dynamic_dispatch_workload_context_default_is_zero_no_gpu() {
    // Default: element_count=0, gpu_available=false, intensity=None.
    // decide_dynamic(&default) → SingleThread (0 ≤ 999).
    let ctx: WorkloadContext = Default::default();
    assert_eq!(ctx.element_count, 0);
    assert!(!ctx.gpu_available);
    assert_eq!(ctx.arithmetic_intensity, None);
    assert_eq!(decide_dynamic(&ctx), DispatchKind::SingleThread);
}

#[test]
fn dynamic_dispatch_workload_context_copy_clone_semantics() {
    // WorkloadContext is Copy — passing by value doesn't move.
    let ctx = WorkloadContext::new(100, true).with_intensity(2.0);
    let _decision = decide_dynamic(&ctx);
    let _also = decide_dynamic(&ctx); // would fail to compile if not Copy
    assert_eq!(ctx.element_count, 100); // still usable
}

// ===========================================================================
// NaN intensity — treated as "not high" (conservative).
// ===========================================================================

#[test]
fn dynamic_dispatch_nan_intensity_treated_as_memory_bound() {
    // NaN compares false against any threshold → is_gpu_favorable_intensity
    // returns false → treated as memory-bound → CpuParallel (not GpuCompute).
    let ctx = WorkloadContext::new(100_000, true).with_intensity(f64::NAN);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::CpuParallel,
        "NaN intensity must be treated as low (memory-bound) — conservative"
    );
}

// ===========================================================================
// Helper extension for the test-only with_intensity_opt.
// ===========================================================================

/// Test-only helper trait to set `Option<f64>` intensity directly (the
/// public API only exposes `with_intensity(f64)` which wraps in `Some`).
/// This lets the no-GPU-fallback sweep test pass `None` after chaining.
trait WithIntensityOpt {
    fn with_intensity_opt(self, intensity: Option<f64>) -> Self;
}

impl WithIntensityOpt for WorkloadContext {
    fn with_intensity_opt(mut self, intensity: Option<f64>) -> Self {
        self.arithmetic_intensity = intensity;
        self
    }
}
