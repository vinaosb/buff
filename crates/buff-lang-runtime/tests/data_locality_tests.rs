//! T10: Data-locality-aware CPU/GPU dispatch (v1.25 Wave 1).
//!
//! Integration tests for [`DataLocation`] + the T10 path in [`decide_dynamic`].
//! Every test name contains `data_locality` so the QA filter
//! `cargo test -p buff-lang-runtime data_locality` matches the whole suite.
//!
//! # Coverage matrix
//!
//! * **Core T10 behavior** (3 tests): data on GPU → GpuCompute for tiny /
//!   medium / large inputs.
//! * **Default is Cpu** (2 tests): without setting data_location, behavior
//!   matches T5 exactly (backwards compatibility).
//! * **GPU-fallback guarantee** (2 tests): data_location=Gpu + no GPU →
//!   never GpuCompute.
//! * **Explain output** (2 tests): explain_dispatch includes data_location
//!   line for both Cpu and Gpu.
//! * **Builder** (2 tests): with_data_location chains correctly; Default is Cpu.
//! * **Chained-op simulation** (1 test): op N output on GPU → op N+1 stays
//!   on GPU.

use buff_lang_runtime::{
    decide_dynamic, explain_dispatch, DataLocation, DispatchKind, WorkloadContext,
    GPU_ARITHMETIC_INTENSITY_THRESHOLD, SINGLE_THREAD_MAX,
};

// ===========================================================================
// Core T10: data on GPU → GpuCompute regardless of size.
// ===========================================================================

#[test]
fn data_locality_gpu_resident_tiny_input_promotes_to_gpu() {
    // T10: 500 elements (normally SingleThread) + data already on GPU →
    // GpuCompute. No PCIe transfer needed; GPU dispatch is cheaper than
    // downloading the data back to CPU.
    let ctx = WorkloadContext::new(500, true).with_data_location(DataLocation::Gpu);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "data on GPU + GPU available must pick GpuCompute even for tiny inputs"
    );
}

#[test]
fn data_locality_gpu_resident_medium_input_stays_on_gpu() {
    // 10_000 elements + data on GPU → GpuCompute.
    let ctx = WorkloadContext::new(10_000, true).with_data_location(DataLocation::Gpu);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "data on GPU + medium input must stay on GPU"
    );
}

#[test]
fn data_locality_gpu_resident_large_input_stays_on_gpu() {
    // 100_000 elements + data on GPU → GpuCompute (would be GpuCompute
    // anyway for high intensity, but T10 guarantees it even with low
    // intensity — the data locality overrides the intensity check).
    let ctx = WorkloadContext::new(100_000, true)
        .with_data_location(DataLocation::Gpu)
        .with_intensity(0.5);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "data on GPU must override low-intensity demotion (no transfer back)"
    );

    // Cross-check: WITHOUT T10, the low intensity would demote to CpuParallel.
    let ctx_no_t10 = WorkloadContext::new(100_000, true).with_intensity(0.5);
    assert_eq!(
        decide_dynamic(&ctx_no_t10),
        DispatchKind::CpuParallel,
        "without T10, low intensity must demote to CpuParallel"
    );
}

#[test]
fn data_locality_gpu_resident_at_single_thread_boundary() {
    // Exactly SINGLE_THREAD_MAX (999) + data on GPU → GpuCompute.
    // T10 overrides the SingleThread band entirely.
    let ctx = WorkloadContext::new(SINGLE_THREAD_MAX, true)
        .with_data_location(DataLocation::Gpu)
        .with_intensity(100.0);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "data on GPU at SINGLE_THREAD_MAX boundary must pick GpuCompute"
    );
}

#[test]
fn data_locality_gpu_resident_overrides_intensity_threshold() {
    // Intensity at the exact threshold (4.0) + data on GPU → GpuCompute
    // (trivially, since T10 fires before intensity is checked).
    // But also: intensity JUST BELOW threshold + data on GPU → still
    // GpuCompute (T10 overrides the demotion).
    let just_below = GPU_ARITHMETIC_INTENSITY_THRESHOLD - f64::EPSILON;
    let ctx = WorkloadContext::new(100_000, true)
        .with_data_location(DataLocation::Gpu)
        .with_intensity(just_below);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "T10 must override intensity-based demotion when data is on GPU"
    );
}

// ===========================================================================
// Default is Cpu — backwards compatibility with T5.
// ===========================================================================

#[test]
fn data_locality_default_cpu_tiny_input_is_single_thread() {
    // Default data_location = Cpu → 500 elements → SingleThread (T5 behavior).
    let ctx = WorkloadContext::new(500, true);
    assert_eq!(
        ctx.data_location,
        DataLocation::Cpu,
        "default data_location must be Cpu"
    );
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::SingleThread,
        "default (Cpu) + tiny input must be SingleThread (T5 behavior unchanged)"
    );
}

#[test]
fn data_locality_explicit_cpu_matches_default() {
    // Setting data_location to Cpu explicitly must produce the same
    // decision as the default (not setting it at all).
    for (count, gpu, intensity) in [
        (500usize, true, None),
        (10_000, true, Some(8.0)),
        (10_000, true, Some(0.5)),
        (100_000, true, Some(8.0)),
        (100_000, false, None),
    ] {
        let ctx_default = WorkloadContext::new(count, gpu);
        let ctx_explicit = ctx_default.with_data_location(DataLocation::Cpu);
        assert_eq!(
            decide_dynamic(&ctx_default),
            decide_dynamic(&ctx_explicit),
            "explicit Cpu must match default Cpu for count={count} gpu={gpu} ai={intensity:?}"
        );
    }
}

// ===========================================================================
// GPU-fallback guarantee: data_location=Gpu + no GPU → never GpuCompute.
// ===========================================================================

#[test]
fn data_locality_gpu_resident_but_no_gpu_falls_back_to_cpu() {
    // data_location=Gpu is a caller claim; if no GPU is available, the
    // dispatch falls through to the normal CPU logic. (The data can't
    // really be on the GPU if there's no GPU — but we handle it safely.)
    let ctx = WorkloadContext::new(10_000, false).with_data_location(DataLocation::Gpu);
    assert_ne!(
        decide_dynamic(&ctx),
        DispatchKind::GpuCompute,
        "data_location=Gpu + no GPU must NEVER return GpuCompute"
    );
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::CpuParallel,
        "10_000 elements (medium band) → CpuParallel"
    );
}

#[test]
fn data_locality_gpu_resident_no_gpu_tiny_input_is_single_thread() {
    // data_location=Gpu + no GPU + tiny input → SingleThread (falls through
    // to T5 branch 1).
    let ctx = WorkloadContext::new(500, false).with_data_location(DataLocation::Gpu);
    assert_eq!(
        decide_dynamic(&ctx),
        DispatchKind::SingleThread,
        "data_location=Gpu + no GPU + tiny input → SingleThread (normal T5 path)"
    );
}

// ===========================================================================
// Explain output includes data_location.
// ===========================================================================

#[test]
fn data_locality_explain_includes_data_location_gpu() {
    let ctx = WorkloadContext::new(500, true).with_data_location(DataLocation::Gpu);
    let decision = decide_dynamic(&ctx);
    let explain = explain_dispatch(&ctx, decision);
    assert_eq!(decision, DispatchKind::GpuCompute);
    assert!(
        explain.contains("data_location: gpu (resident"),
        "explain must show data_location: gpu for resident data"
    );
    assert!(
        explain.contains("T10 data-locality"),
        "explain must reference T10 in the decision line"
    );
}

#[test]
fn data_locality_explain_includes_data_location_cpu() {
    let ctx = WorkloadContext::new(500, true);
    let decision = decide_dynamic(&ctx);
    let explain = explain_dispatch(&ctx, decision);
    assert_eq!(decision, DispatchKind::SingleThread);
    assert!(
        explain.contains("data_location: cpu"),
        "explain must show data_location: cpu for default"
    );
}

// ===========================================================================
// Builder + Default.
// ===========================================================================

#[test]
fn data_locality_builder_chains_correctly() {
    // with_data_location returns Self (chainable), and the field is set.
    let ctx = WorkloadContext::new(100_000, true)
        .with_intensity(8.0)
        .with_data_location(DataLocation::Gpu);
    assert_eq!(ctx.element_count, 100_000);
    assert!(ctx.gpu_available);
    assert_eq!(ctx.arithmetic_intensity, Some(8.0));
    assert_eq!(ctx.data_location, DataLocation::Gpu);
}

#[test]
fn data_locality_default_is_cpu() {
    let ctx: WorkloadContext = Default::default();
    assert_eq!(ctx.data_location, DataLocation::Cpu);
}

#[test]
fn data_location_enum_default_is_cpu() {
    let loc: DataLocation = Default::default();
    assert_eq!(loc, DataLocation::Cpu);
}

// ===========================================================================
// Chained-op simulation: op N output on GPU → op N+1 stays on GPU.
// ===========================================================================

#[test]
fn data_locality_chained_gpu_ops_avoid_round_trip() {
    // Simulate a chained GPU operation pipeline:
    //   op1: 100_000 elements from CPU → GpuCompute (output now on GPU)
    //   op2: 100_000 elements already on GPU → GpuCompute (T10 keeps it)
    //   op3: same chain → GpuCompute again
    //
    // Without T10, op2/op3 would need to decide based on size/intensity
    // alone. With T10, the data_location=Gpu flag short-circuits to GPU.
    let input_len = 100_000;

    // op1: data starts on CPU, high intensity → GpuCompute (T5 promotion).
    let op1_ctx = WorkloadContext::new(input_len, true).with_intensity(8.0);
    let op1_decision = decide_dynamic(&op1_ctx);
    assert_eq!(op1_decision, DispatchKind::GpuCompute);

    // op2: op1's output is on GPU → T10 keeps it on GPU.
    let op2_ctx = WorkloadContext::new(input_len, true)
        .with_intensity(8.0)
        .with_data_location(DataLocation::Gpu);
    let op2_decision = decide_dynamic(&op2_ctx);
    assert_eq!(op2_decision, DispatchKind::GpuCompute);

    // op3: even with LOW intensity, data on GPU → stays on GPU (T10).
    let op3_ctx = WorkloadContext::new(input_len, true)
        .with_intensity(0.5)
        .with_data_location(DataLocation::Gpu);
    let op3_decision = decide_dynamic(&op3_ctx);
    assert_eq!(
        op3_decision,
        DispatchKind::GpuCompute,
        "T10 must keep chained ops on GPU even with low intensity"
    );

    // Cross-check: without T10, op3 with low intensity would demote.
    let op3_no_t10 = WorkloadContext::new(input_len, true).with_intensity(0.5);
    assert_eq!(
        decide_dynamic(&op3_no_t10),
        DispatchKind::CpuParallel,
        "without T10, low intensity demotes to CpuParallel (redundant transfer)"
    );
}
