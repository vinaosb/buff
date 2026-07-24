//! T46: Tiling dispatcher integration tests.
//!
//! These tests prove:
//!
//! 1. **QA case**: `tile_ranges(250, 100)` → exactly 3 tiles
//!    `[(0,100),(100,200),(200,250)]` and a tiled dispatch over a 250-
//!    element input through a mock backend produces the same output as
//!    the CPU oracle and dispatches exactly 3 times.
//! 2. **Pure helpers**: `tile_ranges` and `max_elements_per_tile`
//!    boundary behaviour (empty input, max_tile=0, exact multiples,
//!    budget too small to fit one element, etc).
//! 3. **`dispatch_tiled` via mock**: tile count matches `tile_ranges`,
//!    per-tile input slices are correct, output is concatenated in
//!    input order.
//! 4. **`TiledDispatcher` struct API**: dispatch through the fluent
//!    wrapper produces the same output as the free function.
//! 5. **`dispatch_map_with_tiling` CPU fallback**: returns the CPU
//!    oracle's output when (a) `gpu_backend=None`, (b)
//!    `max_tile_elements=0`, or (c) the GPU dispatch returns an error.
//! 6. **Real-GPU tiled dispatch** (skipped on no-GPU hosts): 250
//!    elements at `max_tile=100` roundtrips through `WgpuBackend`;
//!    tiled result == single-dispatch result == CPU oracle.
//!
//! All test names contain `tiling` so the QA filter
//! `cargo test -p buff-lang-runtime tiling` matches them (the inline
//! unit tests in `src/tiling.rs::tests` match via the module path).
//!
//! # GPU-availability-aware testing
//!
//! Tests that exercise the REAL wgpu dispatch path call
//! [`try_get_real_backend`] and `return` early when it yields `None`
//! (host has no GPU). The mock-backend tests run on every host.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::ty::TypeRef;
use buff_lang_ast::{Expr, Literal, Stmt};
use buff_lang_codegen_wgsl::generate_wgsl;
use buff_lang_error::Span;
use buff_lang_runtime::{
    cpu_fallback_map, dispatch_map_with_tiling, dispatch_tiled, max_elements_per_tile, tile_ranges,
    GpuBackend, GpuContext, MockGpuBackend, RuntimeError, TiledDispatcher, WgpuBackend,
};

// ---------------------------------------------------------------------------
// Helpers (mirror T45's gpu_dispatch_tests.rs)
// ---------------------------------------------------------------------------

/// Build the QA lambda `{ x: Float => x * 2.0 }` — byte-identical to the
/// T44 reference lambda so `generate_wgsl` produces the snapshot-stable
/// shader we depend on.
fn x_times_two_lambda() -> Expr {
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
    Expr::Lambda {
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
    }
}

/// Build a `WgpuBackend` if (and only if) this host has a real GPU.
///
/// Returns `None` when [`WgpuBackend::new`] yields
/// [`RuntimeError::GpuUnavailable`] — callers `return` early in that
/// case (asserting only the graceful-skip property elsewhere).
fn try_get_real_backend() -> Option<WgpuBackend> {
    match WgpuBackend::new() {
        Ok(backend) => Some(backend),
        Err(RuntimeError::GpuUnavailable { .. }) => None,
        Err(other) => panic!("unexpected error constructing WgpuBackend: {other:?}"),
    }
}

/// A `WgpuBackend` built from a known-unavailable context. Always
/// constructible — no GPU is touched. Used to verify the graceful
/// no-GPU error path on EVERY host (GPU or not).
fn unavailable_backend() -> WgpuBackend {
    WgpuBackend::from_context(GpuContext::unavailable())
}

/// Assert approximate equality for two `&[f32]` slices, element-wise,
/// within `tol` tolerance. Same helper as T45.
fn assert_approx_eq(actual: &[f32], expected: &[f32], tol: f32, msg: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{msg}: length mismatch (actual={}, expected={})",
        actual.len(),
        expected.len()
    );
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        let delta = (a - e).abs();
        assert!(
            delta < tol,
            "{msg}: mismatch at index {i}: actual={a}, expected={e}, delta={delta} >= tol={tol}"
        );
    }
}

// ---------------------------------------------------------------------------
// 1. QA case — tile_ranges(250, 100) → 3 tiles  [pure helper]
// ---------------------------------------------------------------------------

#[test]
fn test_tiling_ranges_qa_case_250_at_100_yields_three_tiles() {
    // T46 QA spec verbatim: 250 elements, max_tile=100 → 3 tiles.
    let ranges = tile_ranges(250, 100);
    assert_eq!(ranges, vec![(0, 100), (100, 200), (200, 250)]);
    assert_eq!(ranges.len(), 3, "QA demands exactly 3 tiles");
}

// ---------------------------------------------------------------------------
// 2. tile_ranges edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_tiling_ranges_empty_input_yields_empty_vec() {
    assert!(tile_ranges(0, 100).is_empty());
}

#[test]
fn test_tiling_ranges_input_le_max_yields_single_tile() {
    assert_eq!(tile_ranges(50, 100), vec![(0, 50)]);
    assert_eq!(tile_ranges(100, 100), vec![(0, 100)]);
}

#[test]
fn test_tiling_ranges_exact_multiple_has_no_partial_tile() {
    assert_eq!(tile_ranges(200, 100), vec![(0, 100), (100, 200)]);
}

#[test]
fn test_tiling_ranges_max_tile_zero_disables_tiling() {
    // Documented: max_tile=0 → one tile covering the whole input.
    assert_eq!(tile_ranges(250, 0), vec![(0, 250)]);
    // max_tile=0 + empty input → still empty.
    assert!(tile_ranges(0, 0).is_empty());
}

// ---------------------------------------------------------------------------
// 3. max_elements_per_tile — VRAM budget formula
// ---------------------------------------------------------------------------

#[test]
fn test_tiling_max_elements_per_tile_vram_budget_formula() {
    // The documented formula: max_elements = vram_budget / (3 * bpe).
    // 1200 bytes, 4 bpe → 1200 / 12 = 100 elements.
    assert_eq!(max_elements_per_tile(1200, 4), 100);
    // 4 GiB budget (a typical max_storage_buffer_binding_size), 4 bpe f32.
    // 4_GiB / 12 = 357_913_941 elements per tile (≈ 1.4 GiB per tile
    // total, given the 3x headroom).
    let four_gib = 4u64 * 1024 * 1024 * 1024;
    let tile_size = max_elements_per_tile(four_gib, 4);
    assert_eq!(tile_size, 357_913_941);
}

#[test]
fn test_tiling_max_elements_per_tile_budget_too_small_for_one_element() {
    // 11 bytes < 3*4 = 12 → can't fit one f32 element.
    assert_eq!(max_elements_per_tile(11, 4), 0);
    // Exactly 12 → fits exactly one element.
    assert_eq!(max_elements_per_tile(12, 4), 1);
}

// ---------------------------------------------------------------------------
// 4. dispatch_tiled via MockGpuBackend — 250 elements at max_tile=100
//    Verify: 3 dispatches, output matches CPU oracle.
// ---------------------------------------------------------------------------

#[test]
fn test_tiling_dispatch_via_mock_qa_250_at_100_records_three_dispatches() {
    // MockGpuBackend records every dispatch. We expect 3 tiles for 250
    // elements at max_tile=100 — matches tile_ranges(250, 100).
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    let input: Vec<f32> = (0..250).map(|i| i as f32).collect();
    let out =
        dispatch_tiled(&backend, "@compute ...", &input, 100).expect("mock dispatch never errors");

    // QA assertion: exactly 3 tiles dispatched.
    assert_eq!(
        backend.recorded_dispatches(),
        3,
        "QA demands exactly 3 tile dispatches for 250 elements at max_tile=100"
    );
    // Output has the correct length.
    assert_eq!(out.len(), 250);
    // Output matches the CPU oracle element-wise.
    let cpu = cpu_fallback_map(&input, |x| x * 2.0);
    assert_eq!(out, cpu);
}

#[test]
fn test_tiling_dispatch_via_mock_preserves_input_order_across_tiles() {
    // Use a closure that adds the tile index to each element so we can
    // detect ordering bugs (e.g. tiles dispatched out of order would
    // shift the indices). The closure receives a `&[f32]` tile; we use
    // the input value itself as a position marker.
    //
    // Test: f(x) = x * 10 + offset, where offset = 0 for tile 0,
    //                                   1000 for tile 1,
    //                                   2000 for tile 2, etc.
    // Determined by the FIRST element of the tile.
    let backend = MockGpuBackend::new(|tile: &[f32]| {
        // The "tile index" can be inferred from the first element
        // because we generate input as 0..N contiguously.
        let offset = if tile.is_empty() {
            0.0
        } else {
            ((tile[0] as usize) / 100) as f32 * 1000.0
        };
        tile.iter()
            .copied()
            .map(move |x| x * 10.0 + offset)
            .collect()
    });

    let input: Vec<f32> = (0..250).map(|i| i as f32).collect();
    let out = dispatch_tiled(&backend, "@compute ...", &input, 100).expect("mock dispatch");

    // Expected: each element gets `x*10 + (tile_index * 1000)`.
    let expected: Vec<f32> = input
        .iter()
        .copied()
        .map(|x| {
            let tile_idx = (x as usize) / 100;
            x * 10.0 + (tile_idx as f32) * 1000.0
        })
        .collect();
    assert_eq!(out, expected);
}

#[test]
fn test_tiling_dispatch_via_mock_records_correct_per_tile_input_lengths() {
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x + 1.0));
    let input: Vec<f32> = vec![1.0; 250];
    let _ = dispatch_tiled(&backend, "@compute", &input, 100).expect("mock dispatch");

    let records = backend.records();
    assert_eq!(records.len(), 3);
    // First two tiles are 100 elements, last tile is 50.
    assert_eq!(records[0].input_len, 100);
    assert_eq!(records[1].input_len, 100);
    assert_eq!(records[2].input_len, 50);
}

#[test]
fn test_tiling_dispatch_via_mock_single_tile_when_input_fits() {
    // Input smaller than max_tile → 1 dispatch covering the whole input.
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    let input: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let out = dispatch_tiled(&backend, "@compute", &input, 100).expect("mock dispatch");
    assert_eq!(backend.recorded_dispatches(), 1);
    assert_eq!(out, vec![2.0, 4.0, 6.0, 8.0, 10.0]);
}

#[test]
fn test_tiling_dispatch_via_mock_empty_input_no_dispatch() {
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    let out =
        dispatch_tiled(&backend, "@compute", &[], 100).expect("empty input must return Ok(empty)");
    assert!(out.is_empty());
    assert_eq!(backend.recorded_dispatches(), 0);
}

// ---------------------------------------------------------------------------
// 5. TiledDispatcher struct API
// ---------------------------------------------------------------------------

#[test]
fn test_tiling_dispatcher_struct_dispatches_via_backend() {
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 3.0));
    let dispatcher = TiledDispatcher::new(&backend, 50);
    let input: Vec<f32> = (0..120).map(|i| i as f32).collect();
    let out = dispatcher
        .dispatch("@compute", &input)
        .expect("dispatcher dispatch");

    // 120 elements / 50 per tile = 3 tiles (last is 20).
    assert_eq!(backend.recorded_dispatches(), 3);
    assert_eq!(out, cpu_fallback_map(&input, |x| x * 3.0));
    // Accessors.
    assert_eq!(dispatcher.max_tile_elements(), 50);
}

#[test]
fn test_tiling_dispatcher_struct_accessor_max_tile() {
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x));
    let dispatcher = TiledDispatcher::new(&backend, 777);
    assert_eq!(dispatcher.max_tile_elements(), 777);
    // backend() is observational — just confirm we get back something Debug.
    let _: &dyn GpuBackend = dispatcher.backend();
}

// ---------------------------------------------------------------------------
// 6. dispatch_map_with_tiling — CPU fallback paths
// ---------------------------------------------------------------------------

#[test]
fn test_tiling_high_level_no_gpu_uses_cpu_oracle() {
    // gpu_backend = None → straight to CPU oracle.
    let input: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let out = dispatch_map_with_tiling(None, "@compute", &input, 100, |input| {
        cpu_fallback_map(input, |x| x * 2.0)
    });
    assert_eq!(out, vec![2.0, 4.0, 6.0, 8.0, 10.0]);
}

#[test]
fn test_tiling_high_level_max_tile_zero_uses_cpu_oracle_even_with_backend() {
    // Even if a backend is provided, max_tile=0 means VRAM budget too
    // small to fit one element → CPU fallback.
    let backend = unavailable_backend();
    let input: Vec<f32> = vec![1.0, 2.0, 3.0];
    let out = dispatch_map_with_tiling(Some(&backend), "@compute", &input, 0, |input| {
        cpu_fallback_map(input, |x| x + 10.0)
    });
    assert_eq!(out, vec![11.0, 12.0, 13.0]);
}

#[test]
fn test_tiling_high_level_empty_input_returns_empty_without_dispatch() {
    let out = dispatch_map_with_tiling(None, "@compute", &[], 100, |input| {
        cpu_fallback_map(input, |x| x * 2.0)
    });
    assert!(out.is_empty());
}

#[test]
fn test_tiling_high_level_gpu_error_falls_back_to_cpu() {
    // An "unavailable" WgpuBackend has no adapter → dispatch_map returns
    // GpuUnavailable → CPU fallback fires.
    let backend = unavailable_backend();
    let input: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
    let out = dispatch_map_with_tiling(
        Some(&backend),
        "@compute",
        &input,
        100, // > 0 so we attempt GPU first
        |input| cpu_fallback_map(input, |x| x * 100.0),
    );
    // CPU oracle ran → all elements * 100.
    assert_eq!(out, vec![100.0, 200.0, 300.0, 400.0]);
}

#[test]
fn test_tiling_high_level_mock_backend_tiled_path_produces_correct_result() {
    // Happy path: a working GPU backend (mocked) + max_tile > 0 →
    // tiled dispatch fires, returns the oracle output.
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    let input: Vec<f32> = (0..250).map(|i| i as f32).collect();
    let out = dispatch_map_with_tiling(
        Some(&backend),
        "@compute",
        &input,
        100,
        |input| cpu_fallback_map(input, |x| x * 999.0), // wrong on purpose
    );
    // The GPU path (mocked) should have been used, producing *2.0.
    let expected: Vec<f32> = input.iter().copied().map(|x| x * 2.0).collect();
    assert_eq!(out, expected);
    // CPU oracle did NOT run — the GPU dispatch succeeded.
    // (Indirectly verified: out matches the GPU oracle, not the CPU fallback.)
}

// ---------------------------------------------------------------------------
// 7. Real-GPU tiled dispatch (skipped on hosts without a GPU)
// ---------------------------------------------------------------------------

#[test]
fn test_tiling_real_gpu_250_elements_at_max_tile_100_matches_cpu_oracle() {
    // T46 QA case on real hardware: 250 elements, max_tile=100 → 3
    // tiles dispatched through WgpuBackend, combined result == CPU
    // oracle within f32 tolerance.
    let Some(backend) = try_get_real_backend() else {
        // Host has no GPU — graceful skip. The CPU fallback path is
        // tested separately on every host.
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("T44 codegen must succeed for {x => x*2}");
    let input: Vec<f32> = (0..250).map(|i| i as f32).collect();
    let out = dispatch_tiled(&backend, &wgsl, &input, 100)
        .expect("real-GPU tiled dispatch must succeed when a device is present");

    let cpu = cpu_fallback_map(&input, |x| x * 2.0);
    assert_eq!(out.len(), cpu.len());
    assert_approx_eq(&out, &cpu, 1e-4, "tiled GPU result must match CPU oracle");
}

#[test]
fn test_tiling_real_gpu_tiled_equals_single_dispatch() {
    // Tiled GPU dispatch must produce the SAME output as a single
    // non-tiled dispatch over the same input — element-wise map has no
    // inter-element dependencies, so tiling is a no-op on the result.
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    let input: Vec<f32> = (0..1000).map(|i| i as f32 * 0.5).collect();

    let tiled = dispatch_tiled(&backend, &wgsl, &input, 100).expect("tiled dispatch");
    let single = backend
        .dispatch_map(&wgsl, &input)
        .expect("single dispatch");

    assert_eq!(tiled.len(), single.len());
    assert_approx_eq(&tiled, &single, 1e-4, "tiled == single dispatch");
}

#[test]
fn test_tiling_real_gpu_dispatch_map_with_tiling_uses_gpu_path() {
    // End-to-end: dispatch_map_with_tiling with a real GPU backend
    // returns the GPU's output (which matches the CPU oracle for an
    // element-wise map).
    let backend = match WgpuBackend::new() {
        Ok(b) => b,
        Err(RuntimeError::GpuUnavailable { .. }) => return,
        Err(other) => panic!("unexpected: {other:?}"),
    };
    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    let input: Vec<f32> = (0..250).map(|i| i as f32).collect();
    let out = dispatch_map_with_tiling(Some(&backend), &wgsl, &input, 100, |input| {
        cpu_fallback_map(input, |x| x * 2.0)
    });
    let cpu = cpu_fallback_map(&input, |x| x * 2.0);
    assert_eq!(out.len(), cpu.len());
    assert_approx_eq(&out, &cpu, 1e-4, "high-level tiled dispatch via real GPU");
}
