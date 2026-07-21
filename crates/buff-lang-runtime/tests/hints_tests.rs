//! T49: `@prefer(gpu)` / `@prefer(npu)` hint system integration tests.
//!
//! All test names contain `hints` so the QA filter
//! `cargo test -p buff-lang-runtime hints` matches the whole suite
//! (across inline `#[cfg(test)] mod tests` in `hints.rs` AND this file).
//!
//! # Coverage matrix
//!
//! * **QA case** (1): `@prefer(gpu)` + 10 elements → `SingleThread` (CPU).
//! * **Large data + GPU available** (1): `@prefer(gpu)` + 100_000 elements →
//!   `GpuCompute`.
//! * **No GPU graceful fallback** (1): `@prefer(gpu)` + large data + no GPU
//!   → `CpuParallel`.
//! * **No-hint parity** (1): `Prefer::None` matches T40's `decide` byte-for-byte.
//! * **NPU parity with GPU** (1): `Prefer::Npu` produces the same routing as
//!   `Prefer::Gpu`.
//! * **Boundary at `PREFER_GPU_MIN_ELEMENTS`** (2): one below → CPU; at
//!   threshold → GpuCompute.
//! * **VRAM-aware override** (2): data exceeds VRAM → CPU even with hint;
//!   fits VRAM → GpuCompute.
//! * **Cost override at small data** (2): `@prefer(gpu)` + 1 element →
//!   SingleThread; `@prefer(gpu)` + 1023 elements → CpuParallel (still CPU).
//! * **prefer_from_name_args primitive** (3): exact matches, multi-arg
//!   rejected, unknown attribute.
//! * **dispatch_with_prefer end-to-end** (5): no-GPU path uses CPU oracle;
//!   mock-GPU path produces oracle output; empty input short-circuits;
//!   real-GPU large-data dispatch matches oracle; graceful fallback when
//!   GPU backend errors.
//! * **AST attribute bridging** (1): a tiny `prefer_from_attributes` helper
//!   over `buff_lang_ast::Attribute` demonstrates the dev-dep-only bridge
//!   pattern (no production ast dep needed in this crate).

use buff_lang_runtime::{
    cpu_fallback_map, decide, decide_with_prefer, dispatch_with_prefer, prefer_from_name_args,
    DispatchKind, GpuContext, MockGpuBackend, Prefer, RuntimeError, WgpuBackend,
    PREFER_GPU_MIN_ELEMENTS,
};

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::ty::TypeRef;
use buff_lang_ast::{Expr, Literal, Stmt};
use buff_lang_codegen_wgsl::generate_wgsl;
use buff_lang_error::Span;
// T0 attribute-named-args bridge: BTreeMap needed for Attribute init.
use std::collections::BTreeMap;

// A small shader source — content doesn't matter for most tests since
// the Mock backend ignores it and the real backend only sees it when
// dispatch_with_prefer picks the GPU path.
const SHADER_WGSL: &str = "@compute @workgroup_size(64) fn main() {}";

// ---------------------------------------------------------------------------
// Helper: prefer_from_attributes — dev-dep-only AST bridge
// ---------------------------------------------------------------------------

/// Dev-only helper demonstrating how a downstream caller (e.g.
/// `buff-lang-types` or the codegen layer) can translate a slice of
/// `buff_lang_ast::Attribute`s into a [`Prefer`] hint WITHOUT coupling
/// `buff-lang-runtime` to `buff-lang-ast`.
///
/// First-match wins (declaration order). Mirrors T48's
/// `has_prefer_gpu_attr` shape but produces a [`Prefer`] enum value
/// instead of a `bool`.
fn prefer_from_attributes(attrs: &[buff_lang_ast::Attribute]) -> Prefer {
    for a in attrs {
        let p = prefer_from_name_args(&a.name.name, &a.args);
        if p != Prefer::None {
            return p;
        }
    }
    Prefer::None
}

// ---------------------------------------------------------------------------
// Helper: real-GPU availability probe (mirrors T45's pattern)
// ---------------------------------------------------------------------------

/// Build a `WgpuBackend` if (and only if) this host has a real GPU.
///
/// Returns `None` on `RuntimeError::GpuUnavailable`; panics on any other
/// construction error (those would indicate a real bug).
fn try_get_real_backend() -> Option<WgpuBackend> {
    match WgpuBackend::new() {
        Ok(backend) => Some(backend),
        Err(RuntimeError::GpuUnavailable) => None,
        Err(other) => panic!("unexpected error constructing WgpuBackend: {other:?}"),
    }
}

/// Helper: build a `MockGpuBackend` that doubles every input element.
fn doubling_mock_backend() -> MockGpuBackend<impl Fn(&[f32]) -> Vec<f32>> {
    MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0))
}

/// Helper: build a `WgpuBackend` from a known-unavailable context for
/// the graceful-no-GPU error path. Always constructible.
fn unavailable_backend() -> WgpuBackend {
    WgpuBackend::from_context(GpuContext::unavailable())
}

// ===========================================================================
// 1. QA case — @prefer(gpu) + 10 elements → CPU (cost-model override)
// ===========================================================================

#[test]
fn hints_qa_prefer_gpu_with_10_elements_routes_to_cpu() {
    // The exact QA case from the task spec verbatim.
    //
    // @prefer(gpu) is set, but element_count (10) is well below
    // PREFER_GPU_MIN_ELEMENTS (1024). The cost-model override kicks in:
    // GPU dispatch overhead would exceed the compute savings, so the
    // dispatch routes through T40's `decide` — which yields SingleThread
    // for any count <= SINGLE_THREAD_MAX (999).
    let decision = decide_with_prefer(10, Prefer::Gpu, true, None, 4);
    assert_eq!(
        decision,
        DispatchKind::SingleThread,
        "QA: @prefer(gpu) + 10 elements MUST route to SingleThread (CPU), \
         got {decision:?}"
    );
    // Sanity: it's NOT GpuCompute.
    assert_ne!(
        decision,
        DispatchKind::GpuCompute,
        "QA: @prefer(gpu) + 10 elements MUST NOT pick GpuCompute"
    );
}

// ===========================================================================
// 2. @prefer(gpu) + large data + GPU available → GpuCompute
// ===========================================================================

#[test]
fn hints_prefer_gpu_large_data_with_gpu_routes_to_gpu_compute() {
    // 100_000 elements with @prefer(gpu), a GPU available, and unknown
    // VRAM (None means "assume fits"). Above PREFER_GPU_MIN_ELEMENTS so
    // the cost-model override does NOT fire. The hint is honored.
    let decision = decide_with_prefer(100_000, Prefer::Gpu, true, None, 4);
    assert_eq!(
        decision,
        DispatchKind::GpuCompute,
        "@prefer(gpu) + 100k elements + GPU available MUST route to GpuCompute"
    );
}

// ===========================================================================
// 3. @prefer(gpu) + large data + NO GPU → CpuParallel (graceful fallback)
// ===========================================================================

#[test]
fn hints_prefer_gpu_large_data_without_gpu_falls_back_to_cpu() {
    // Same as above but no GPU adapter — graceful fallback to T40's
    // `decide`, which yields CpuParallel for > 50_000.
    let decision = decide_with_prefer(100_000, Prefer::Gpu, false, None, 4);
    assert_eq!(
        decision,
        DispatchKind::CpuParallel,
        "@prefer(gpu) + 100k elements + NO GPU MUST fall back to CpuParallel"
    );
}

// ===========================================================================
// 4. prefer == None MUST match T40's decide verbatim (no behavior change)
// ===========================================================================

#[test]
fn hints_prefer_none_matches_decide_verbatim_across_bands() {
    // Sweep across all three T40 bands + edge cases. For every input
    // tuple, Prefer::None must produce the same decision as plain
    // decide(). This is the contract: un-hinted code is unchanged.
    let cases: [(usize, bool, Option<u64>, u64); 8] = [
        (0, true, None, 4),
        (10, true, None, 4),
        (999, true, None, 4),
        (1_000, true, None, 4),
        (50_000, true, None, 4),
        (50_001, true, None, 4),
        (50_001, false, None, 4),
        (1_000_000, true, Some(1), 1_073_741_824), // VRAM overflow case
    ];
    for (count, gpu, vram, bpe) in cases {
        let hinted = decide_with_prefer(count, Prefer::None, gpu, vram, bpe);
        let base = decide(count, gpu, vram, bpe);
        assert_eq!(
            hinted, base,
            "Prefer::None MUST match decide() for (count={count}, gpu={gpu}, vram={vram:?}, bpe={bpe})"
        );
    }
}

// ===========================================================================
// 5. @prefer(npu) maps to "prefer accelerator" — same routing as @prefer(gpu)
// ===========================================================================

#[test]
fn hints_prefer_npu_matches_prefer_gpu_routing() {
    // NPU backend is post-v1.0; @prefer(npu) is interpreted as "prefer
    // accelerator" — try GPU if available + data large enough, else CPU.
    // This means Npu and Gpu produce IDENTICAL routing decisions across
    // all (count, gpu_available, vram, bpe) inputs.
    let cases: [(usize, bool, Option<u64>, u64); 6] = [
        (10, true, None, 4),                      // tiny → CPU (cost override)
        (PREFER_GPU_MIN_ELEMENTS, true, None, 4), // boundary → GPU
        (100_000, true, None, 4),                 // large + GPU → GPU
        (100_000, false, None, 4),                // large + no GPU → CPU
        (1_000_000, true, Some(1), 4),            // VRAM-exceeds → CPU
        (50_000, true, None, 4),                  // mid-band → GPU (hint wins)
    ];
    for (count, gpu, vram, bpe) in cases {
        let gpu_decision = decide_with_prefer(count, Prefer::Gpu, gpu, vram, bpe);
        let npu_decision = decide_with_prefer(count, Prefer::Npu, gpu, vram, bpe);
        assert_eq!(
            gpu_decision, npu_decision,
            "Prefer::Npu must match Prefer::Gpu for (count={count}, gpu={gpu}, vram={vram:?}, bpe={bpe}) \
             — NPU maps to accelerator in v1.0"
        );
    }
}

// ===========================================================================
// 6. Boundary at PREFER_GPU_MIN_ELEMENTS
// ===========================================================================

#[test]
fn hints_prefer_gpu_boundary_just_below_min_routes_to_cpu() {
    // One element below the threshold: cost override fires, routes via
    // T40's decide. PREFER_GPU_MIN_ELEMENTS - 1 = 1023 → CpuParallel
    // (in the 1000..=50_000 band).
    let count = PREFER_GPU_MIN_ELEMENTS - 1;
    assert!(
        count >= 1000,
        "test setup: PREFER_GPU_MIN_ELEMENTS must be > 1000"
    );
    let decision = decide_with_prefer(count, Prefer::Gpu, true, None, 4);
    assert_eq!(
        decision,
        DispatchKind::CpuParallel,
        "@prefer(gpu) + (PREFER_GPU_MIN_ELEMENTS - 1) MUST route to CpuParallel (cost override)"
    );
}

#[test]
fn hints_prefer_gpu_boundary_at_min_routes_to_gpu_compute() {
    // Exactly at the threshold: cost override does NOT fire. With a GPU
    // available + VRAM unknown (None = assume fits), the hint is honored.
    let count = PREFER_GPU_MIN_ELEMENTS;
    let decision = decide_with_prefer(count, Prefer::Gpu, true, None, 4);
    assert_eq!(
        decision,
        DispatchKind::GpuCompute,
        "@prefer(gpu) + PREFER_GPU_MIN_ELEMENTS + GPU MUST route to GpuCompute"
    );
}

// ===========================================================================
// 7. VRAM-aware decisions
// ===========================================================================

#[test]
fn hints_prefer_gpu_with_data_exceeding_vram_routes_to_cpu() {
    // Large input that exceeds VRAM → graceful fallback to T40's decide
    // (which itself returns CpuParallel for > 50_000 with insufficient VRAM).
    let decision = decide_with_prefer(1_000_000, Prefer::Gpu, true, Some(1), 1_073_741_824);
    assert_eq!(
        decision,
        DispatchKind::CpuParallel,
        "@prefer(gpu) + data exceeding VRAM MUST fall back to CpuParallel"
    );
}

#[test]
fn hints_prefer_gpu_with_data_fitting_vram_routes_to_gpu_compute() {
    // Large input that fits the reported VRAM → GpuCompute.
    // 1M f32 elements = 4 MiB; declare VRAM = 8 MiB → fits.
    let decision = decide_with_prefer(1_000_000, Prefer::Gpu, true, Some(8 * 1024 * 1024), 4);
    assert_eq!(
        decision,
        DispatchKind::GpuCompute,
        "@prefer(gpu) + data fitting VRAM + GPU MUST route to GpuCompute"
    );
}

// ===========================================================================
// 8. Cost override at small data (sub-PREFER_GPU_MIN_ELEMENTS bands)
// ===========================================================================

#[test]
fn hints_prefer_gpu_with_singleton_input_routes_to_single_thread() {
    // 1 element is well below SINGLE_THREAD_MAX (999) AND below
    // PREFER_GPU_MIN_ELEMENTS (1024). The cost override fires and T40's
    // decide yields SingleThread.
    let decision = decide_with_prefer(1, Prefer::Gpu, true, None, 4);
    assert_eq!(
        decision,
        DispatchKind::SingleThread,
        "@prefer(gpu) + 1 element MUST route to SingleThread (cost override)"
    );
}

#[test]
fn hints_prefer_gpu_with_zero_elements_routes_to_single_thread() {
    // Empty input — T40's decide yields SingleThread (0 <= 999). The
    // cost override fires before the dispatch can be made; the actual
    // dispatch entry (dispatch_with_prefer) further short-circuits to
    // return an empty Vec.
    let decision = decide_with_prefer(0, Prefer::Gpu, true, None, 4);
    assert_eq!(
        decision,
        DispatchKind::SingleThread,
        "@prefer(gpu) + 0 elements MUST route to SingleThread (cost override)"
    );
}

// ===========================================================================
// 9. prefer_from_name_args primitive (AST-agnostic matcher)
// ===========================================================================

#[test]
fn hints_prefer_from_name_args_matches_exact_prefer_gpu() {
    let args = vec!["gpu".to_string()];
    assert_eq!(prefer_from_name_args("prefer", &args), Prefer::Gpu);
}

#[test]
fn hints_prefer_from_name_args_matches_exact_prefer_npu() {
    let args = vec!["npu".to_string()];
    assert_eq!(prefer_from_name_args("prefer", &args), Prefer::Npu);
}

#[test]
fn hints_prefer_from_name_args_rejects_non_prefer_attribute() {
    // @test, @inline, etc. are NOT prefer hints.
    assert_eq!(prefer_from_name_args("test", &[]), Prefer::None);
    assert_eq!(prefer_from_name_args("inline", &[]), Prefer::None);
    assert_eq!(prefer_from_name_args("", &[]), Prefer::None);
}

// ===========================================================================
// 10. dispatch_with_prefer end-to-end correctness
// ===========================================================================

#[test]
fn hints_dispatch_with_prefer_no_gpu_runs_cpu_oracle() {
    // No GPU backend provided → straight to CPU oracle (doubling map).
    let input: Vec<f32> = (1..=5).map(|i| i as f32).collect();
    let out = dispatch_with_prefer(Prefer::Gpu, None, SHADER_WGSL, &input, None, |input| {
        cpu_fallback_map(input, |x| x * 2.0)
    });
    assert_eq!(out, vec![2.0, 4.0, 6.0, 8.0, 10.0]);
}

#[test]
fn hints_dispatch_with_prefer_with_mock_gpu_routes_through_backend() {
    // Mock backend that doubles; small input (5 elements) so the cost
    // override kicks in and dispatch_with_prefer runs the CPU oracle
    // (which also doubles). The output is the same; we just verify
    // the routing is consistent.
    let backend = doubling_mock_backend();
    let input: Vec<f32> = (1..=5).map(|i| i as f32).collect();
    let out = dispatch_with_prefer(
        Prefer::Gpu,
        Some(&backend),
        SHADER_WGSL,
        &input,
        None,
        |input| cpu_fallback_map(input, |x| x * 2.0),
    );
    // Backend records 0 dispatches — cost override routed to CPU.
    assert_eq!(
        backend.recorded_dispatches(),
        0,
        "cost override must skip GPU"
    );
    assert_eq!(out, vec![2.0, 4.0, 6.0, 8.0, 10.0]);
}

#[test]
fn hints_dispatch_with_prefer_large_input_with_mock_gpu_dispatches_to_backend() {
    // Large input (>= PREFER_GPU_MIN_ELEMENTS) with a mock GPU → the
    // decision is GpuCompute and the backend's dispatch_map is called.
    let backend = doubling_mock_backend();
    let input: Vec<f32> = (1..=PREFER_GPU_MIN_ELEMENTS as i32 * 2)
        .map(|i| i as f32)
        .collect();
    let out = dispatch_with_prefer(
        Prefer::Gpu,
        Some(&backend),
        SHADER_WGSL,
        &input,
        None,
        |input| cpu_fallback_map(input, |x| x * 1000.0), // different oracle
    );
    // Backend MUST have been called once. Output must match the backend's
    // doubling (NOT the 1000x oracle — the GPU path won).
    assert_eq!(
        backend.recorded_dispatches(),
        1,
        "large input + GPU available MUST dispatch through the backend"
    );
    let expected: Vec<f32> = input.iter().map(|x| x * 2.0).collect();
    assert_eq!(out, expected);
}

#[test]
fn hints_dispatch_with_prefer_empty_input_short_circuits() {
    // Empty input must return empty Vec without invoking either backend
    // or the CPU oracle. We assert this with a CPU oracle that would
    // panic if called.
    let backend = doubling_mock_backend();
    let out = dispatch_with_prefer(
        Prefer::Gpu,
        Some(&backend),
        SHADER_WGSL,
        &[],
        None,
        |input| panic!("CPU oracle must not be called for empty input, got {input:?}"),
    );
    assert!(out.is_empty());
    assert_eq!(
        backend.recorded_dispatches(),
        0,
        "empty input must not trigger a GPU dispatch"
    );
}

#[test]
fn hints_dispatch_with_prefer_graceful_fallback_on_gpu_error() {
    // GPU backend is the unavailable WgpuBackend — always returns
    // Err(GpuUnavailable) from dispatch_map. With large input, the
    // decision is GpuCompute (since Some(&backend) → gpu_available=true),
    // but the dispatch fails → fall back to the CPU oracle.
    //
    // This mirrors the production "user has @prefer(gpu), the GPU
    // adapter vanishes mid-flight, dispatch gracefully degrades" path.
    let backend = unavailable_backend();
    let input: Vec<f32> = (1..=2000).map(|i| i as f32).collect();
    let out = dispatch_with_prefer(
        Prefer::Gpu,
        Some(&backend),
        SHADER_WGSL,
        &input,
        None,
        |input| cpu_fallback_map(input, |x| x * 2.0),
    );
    let expected: Vec<f32> = input.iter().map(|x| x * 2.0).collect();
    assert_eq!(
        out, expected,
        "GPU dispatch error must trigger CPU oracle fallback producing the correct output"
    );
}

// ===========================================================================
// 11. AST attribute bridging via prefer_from_name_args (dev-dep pattern)
// ===========================================================================

#[test]
fn hints_prefer_from_attributes_bridges_ast_attribute_to_prefer() {
    use buff_lang_ast::Attribute;

    let span = Span::dummy();

    // A function with @prefer(gpu) attribute.
    let gpu_attr = Attribute {
        name: Ident::new("prefer", span),
        args: vec!["gpu".to_string()],
        named_args: BTreeMap::new(),
        span,
    };
    assert_eq!(prefer_from_attributes(&[gpu_attr]), Prefer::Gpu);

    // A function with @prefer(npu) attribute.
    let npu_attr = Attribute {
        name: Ident::new("prefer", span),
        args: vec!["npu".to_string()],
        named_args: BTreeMap::new(),
        span,
    };
    assert_eq!(prefer_from_attributes(&[npu_attr]), Prefer::Npu);

    // A function with no attributes.
    assert_eq!(prefer_from_attributes(&[]), Prefer::None);

    // A function with an unrelated attribute (e.g. @test).
    let test_attr = Attribute {
        name: Ident::new("test", span),
        args: vec![],
        named_args: BTreeMap::new(),
        span,
    };
    assert_eq!(prefer_from_attributes(&[test_attr]), Prefer::None);

    // First-match wins: @prefer(gpu) BEFORE @prefer(npu).
    let gpu_first = Attribute {
        name: Ident::new("prefer", span),
        args: vec!["gpu".to_string()],
        named_args: BTreeMap::new(),
        span,
    };
    let npu_second = Attribute {
        name: Ident::new("prefer", span),
        args: vec!["npu".to_string()],
        named_args: BTreeMap::new(),
        span,
    };
    assert_eq!(
        prefer_from_attributes(&[gpu_first, npu_second]),
        Prefer::Gpu
    );

    // Multi-arg prefer is intentionally NOT matched (matches T48).
    let multi_arg = Attribute {
        name: Ident::new("prefer", span),
        args: vec!["gpu".to_string(), "force".to_string()],
        named_args: BTreeMap::new(),
        span,
    };
    assert_eq!(prefer_from_attributes(&[multi_arg]), Prefer::None);
}

// ===========================================================================
// 12. Real-GPU dispatch_with_prefer (skipped on no-GPU hosts)
// ===========================================================================

#[test]
fn hints_dispatch_with_prefer_real_gpu_large_input_matches_cpu_oracle() {
    // End-to-end on a real GPU: large input + @prefer(gpu) routes through
    // the real WgpuBackend and produces output matching the CPU oracle.
    //
    // Skipped on hosts without a GPU (the small-input cost-override and
    // no-GPU fallback paths cover those cases above).
    let Some(backend) = try_get_real_backend() else {
        return; // Host has no GPU — graceful skip.
    };

    // Build the QA lambda `{ x: Float => x * 2.0 }` — byte-identical to
    // T44's reference lambda so generate_wgsl produces the snapshot-stable
    // shader.
    let span = Span::dummy();
    let x = Expr::Ident(Ident::new("x", span), span);
    let two = Expr::Literal(Literal::Float(2.0), span);
    let body_expr = Expr::BinaryOp {
        op: BinaryOp::Mul,
        lhs: Box::new(x),
        rhs: Box::new(two),
        span,
    };
    let body = Block {
        stmts: vec![Stmt::ExprStmt(body_expr, span)],
        span,
    };
    let lambda = Expr::Lambda {
        params: vec![Param {
            name: Ident::new("x", span),
            ty: TypeRef::Named {
                name: Ident::new("Float", span),
                span,
            },
            default_value: None,
            is_comptime: false,
            span,
        }],
        body,
        return_type: None,
        span,
    };
    let wgsl =
        generate_wgsl(&lambda).expect("T44 codegen must succeed for {{x: Float => x * 2.0}}");

    // Large input: PREFER_GPU_MIN_ELEMENTS * 2 = 2048 elements. Well
    // above the cost-override threshold so the dispatch actually hits
    // the GPU path.
    let input: Vec<f32> = (1..=(PREFER_GPU_MIN_ELEMENTS as i32 * 2))
        .map(|i| i as f32)
        .collect();
    let out = dispatch_with_prefer(Prefer::Gpu, Some(&backend), &wgsl, &input, None, |input| {
        cpu_fallback_map(input, |x| x * 2.0)
    });

    // GPU output must match the CPU oracle (doubling) within tolerance.
    assert_eq!(out.len(), input.len());
    for (i, (actual, expected)) in out.iter().zip(input.iter().map(|x| x * 2.0)).enumerate() {
        let delta = (actual - expected).abs();
        assert!(
            delta < 1e-4,
            "real-GPU output mismatch at index {i}: actual={actual}, expected={expected}, delta={delta}"
        );
    }
}
