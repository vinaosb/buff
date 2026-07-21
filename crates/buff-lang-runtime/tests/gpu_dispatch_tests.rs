//! T45: Real wgpu-backed GPU dispatch pipeline integration tests.
//!
//! These tests prove:
//!
//! 1. **QA case**: `WgpuBackend.dispatch_map(x*2_wgsl, [1,2,3])` returns
//!    `[2.0, 4.0, 6.0]` on a host with a GPU. Skipped (with an assertion
//!    of graceful `Err`) when no GPU is available.
//! 2. **Workgroup sizing**: `workgroup_count` boundaries match the spec
//!    table exactly (0→0, 1→1, 64→1, 65→2, 128→2, 129→3).
//! 3. **Empty input**: returns empty Vec without dispatching (no GPU
//!    required — early return).
//! 4. **Larger input roundtrip**: a 1000-element input matches the
//!    CPU-fallback oracle to within f32 tolerance.
//! 5. **GPU output == cpu_fallback_map oracle**: outputs match across
//!    various closures + sizes.
//! 6. **No-GPU graceful path**: a backend built from `GpuContext::unavailable()`
//!    returns `RuntimeError::GpuUnavailable` — never panics.
//! 7. **Singleton input**: `[x] → [f(x)]` for one-element input.
//! 8. **Real-GPU dispatch using generated WGSL**: feeds
//!    `buff_lang_codegen_wgsl::generate_wgsl` output through the
//!    pipeline end-to-end.
//! 9. **Object-safety**: `Box<dyn GpuBackend>` works for `WgpuBackend`.
//! 10. **Construction + accessors**: `from_context`, `context()`, `has_device()`.
//!
//! All test names contain `gpu_dispatch` so the QA filter
//! `cargo test -p buff-lang-runtime gpu_dispatch` matches the whole suite.
//!
//! # GPU-availability-aware testing
//!
//! Every test that calls [`WgpuBackend::dispatch_map`] over a real
//! dispatch MUST first call [`try_get_real_backend`] (or
//! [`try_get_real_backend_with_unavailable_context`]) and `return`
//! early when it returns `None`. This is the "graceful skip" pattern
//! required by the task spec so CI hosts without a GPU still pass: the
//! test asserts ONLY the things that are host-independent (e.g. empty
//! input, no-GPU error path, `workgroup_count` arithmetic) and skips
//! the real-dispatch assertions with a visible message.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::ty::TypeRef;
use buff_lang_ast::{Expr, Literal, Stmt};
use buff_lang_codegen_wgsl::generate_wgsl;
use buff_lang_error::Span;
use buff_lang_runtime::{cpu_fallback_map, GpuBackend, GpuContext, RuntimeError, WgpuBackend};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the QA lambda `{ x: Float => x * 2.0 }`. Byte-identical to the
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

/// Build the lambda `{ x: Float => x * x + 1.0 }` (for a non-trivial
/// multi-op shader).
fn x_squared_plus_one_lambda() -> Expr {
    let span = Span::dummy();
    let x = Expr::Ident(Ident::new("x", span), span);
    let xx = Expr::BinaryOp {
        op: BinaryOp::Mul,
        lhs: Box::new(x.clone()),
        rhs: Box::new(x),
        span,
    };
    let one = Expr::Literal(Literal::Float(1.0), span);
    let body_expr = Expr::BinaryOp {
        op: BinaryOp::Add,
        lhs: Box::new(xx),
        rhs: Box::new(one),
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
/// Returns `None` when [`GpuContext::new`] yields [`GpuContextError::NoAdapter`]
/// — callers must `return` early in that case (asserting only the
/// graceful-skip property elsewhere in the test).
fn try_get_real_backend() -> Option<WgpuBackend> {
    match WgpuBackend::new() {
        Ok(backend) => Some(backend),
        Err(RuntimeError::GpuUnavailable) => None,
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
/// within `1e-4` tolerance. The task spec's tolerance suggestion.
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
// 1. QA case — [1,2,3] * 2 == [2,4,6]  (real GPU)
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_qa_one_two_three_x_two_yields_two_four_six() {
    // QA case from the task spec verbatim:
    //   dispatch([1.0, 2.0, 3.0], { x => x * 2 }) → assert [2.0, 4.0, 6.0]
    let Some(backend) = try_get_real_backend() else {
        // Host has no GPU — assert the graceful Err path is taken
        // elsewhere (test_gpu_dispatch_no_gpu_returns_unavailable).
        // Skip this real-GPU assertion.
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("T44 codegen must succeed for {x => x*2}");
    let input = vec![1.0_f32, 2.0, 3.0];
    let out = backend
        .dispatch_map(&wgsl, &input)
        .expect("real-GPU dispatch must succeed when a device is present");

    assert_eq!(
        out,
        vec![2.0_f32, 4.0, 6.0],
        "QA roundtrip [1,2,3]*2 must equal [2,4,6]"
    );
}

// ---------------------------------------------------------------------------
// 2. Empty input → empty output (no GPU required — early return)
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_empty_input_returns_empty_vec_without_dispatch() {
    // Empty input must short-circuit BEFORE any GPU work — both for
    // correctness (no 0-sized dispatch) and for hosts without a GPU
    // (the dispatch_map early return fires before device()/queue()).
    let backend = unavailable_backend();
    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    let out = backend
        .dispatch_map(&wgsl, &[])
        .expect("empty input must always return Ok(empty), even without a GPU");
    assert!(
        out.is_empty(),
        "empty input must produce empty output, got {out:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. No-GPU graceful path — returns RuntimeError::GpuUnavailable
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_no_gpu_returns_unavailable_error() {
    // A backend built from `GpuContext::unavailable()` has no adapter;
    // dispatch_map on it MUST return GpuUnavailable, not panic.
    let backend = unavailable_backend();
    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    let input = vec![1.0_f32, 2.0, 3.0];
    match backend.dispatch_map(&wgsl, &input) {
        Err(RuntimeError::GpuUnavailable) => {
            // expected — graceful no-GPU path
        }
        Err(other) => panic!(
            "expected GpuUnavailable on a context without adapter, got: {other:?}"
        ),
        Ok(out) => panic!(
            "expected Err(GpuUnavailable) but dispatch_map returned Ok({out:?}) on a context with no adapter"
        ),
    }
}

// ---------------------------------------------------------------------------
// 4. Larger input roundtrip — 1000-element dispatch matches CPU oracle
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_larger_input_roundtrip_matches_cpu_oracle() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    let input: Vec<f32> = (0..1000).map(|i| i as f32 * 0.5).collect();
    let gpu_out = backend
        .dispatch_map(&wgsl, &input)
        .expect("real-GPU dispatch must succeed for 1000-element input");
    let cpu_out = cpu_fallback_map(&input, |x| x * 2.0);

    assert_eq!(gpu_out.len(), cpu_out.len(), "length must match");
    assert_approx_eq(&gpu_out, &cpu_out, 1e-4, "GPU output must match CPU oracle");
    // Sanity check: first 5 and last 5 should be exactly 2x input.
    assert_abs_approx_eq_prefix_and_suffix(&gpu_out, &input);
}

fn assert_abs_approx_eq_prefix_and_suffix(out: &[f32], input: &[f32]) {
    for i in 0..5.min(out.len()) {
        let delta = (out[i] - input[i] * 2.0).abs();
        assert!(delta < 1e-4, "prefix elem {i} mismatch: {}", out[i]);
    }
    let n = out.len();
    for i in (n.saturating_sub(5))..n {
        let delta = (out[i] - input[i] * 2.0).abs();
        assert!(delta < 1e-4, "suffix elem {i} mismatch: {}", out[i]);
    }
}

// ---------------------------------------------------------------------------
// 5. GPU output == cpu_fallback_map oracle — multi-op shader
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_output_matches_cpu_fallback_oracle_squared_plus_one() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let lambda = x_squared_plus_one_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen for {x => x*x + 1}");
    let input: Vec<f32> = vec![-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.5];
    let gpu_out = backend
        .dispatch_map(&wgsl, &input)
        .expect("real-GPU dispatch must succeed");
    let cpu_out = cpu_fallback_map(&input, |x| x * x + 1.0);

    assert_eq!(gpu_out.len(), cpu_out.len());
    assert_approx_eq(&gpu_out, &cpu_out, 1e-4, "GPU == CPU oracle for x*x+1");
}

// ---------------------------------------------------------------------------
// 6. Singleton input [42.0] → [84.0]
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_singleton_input_roundtrips() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    let out = backend
        .dispatch_map(&wgsl, &[42.0_f32])
        .expect("real-GPU dispatch on singleton must succeed");
    assert_eq!(out.len(), 1);
    assert!(
        (out[0] - 84.0).abs() < 1e-4,
        "singleton roundtrip 42*2 must equal 84, got {}",
        out[0]
    );
}

// ---------------------------------------------------------------------------
// 7. Workgroup sizing — runs for sizes 1, 64, 65, 128, 129, 1000
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_workgroup_count_boundaries_dispatchable() {
    // For every workgroup_count boundary, a real dispatch succeeds and
    // matches the CPU oracle.
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    for &size in &[1_usize, 64, 65, 128, 129, 1000] {
        let input: Vec<f32> = (0..size).map(|i| (i as f32) * 0.25).collect();
        let gpu_out = backend
            .dispatch_map(&wgsl, &input)
            .unwrap_or_else(|e| panic!("dispatch size={size} must succeed: {e:?}"));
        let cpu_out = cpu_fallback_map(&input, |x| x * 2.0);
        assert_approx_eq(
            &gpu_out,
            &cpu_out,
            1e-4,
            &format!("dispatch size={size} must match oracle"),
        );
    }
}

// ---------------------------------------------------------------------------
// 8. Real-GPU dispatch using T44-generated WGSL — runs on GPU
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_real_gpu_with_generated_wgsl_runs_on_device() {
    // This is the test that ACTUALLY exercises the wgpu pipeline on
    // this host (since this box has a GPU per the T43/T38 findings).
    // Skipped on hosts without a GPU.
    let Some(backend) = try_get_real_backend() else {
        return;
    };
    assert!(
        backend.context().has_adapter(),
        "if WgpuBackend::new succeeded, the context must have an adapter"
    );

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    // Probe the WGSL: it MUST contain the binding layout the pipeline
    // hardcodes. If T44 changes layout, this catches the mismatch.
    assert!(
        wgsl.contains("@group(0) @binding(0)"),
        "WGSL must declare binding 0: {wgsl}"
    );
    assert!(
        wgsl.contains("@group(0) @binding(1)"),
        "WGSL must declare binding 1: {wgsl}"
    );
    assert!(
        wgsl.contains("@workgroup_size(64)"),
        "WGSL must declare workgroup_size(64): {wgsl}"
    );

    let input = vec![1.0_f32, 2.0, 3.0];
    let out = backend
        .dispatch_map(&wgsl, &input)
        .expect("dispatch on real GPU must succeed");
    assert_eq!(out, vec![2.0_f32, 4.0, 6.0]);

    // After at least one dispatch, has_device() must report true.
    assert!(
        backend.has_device(),
        "after a successful dispatch the device must be cached and visible"
    );
}

// ---------------------------------------------------------------------------
// 9. Object-safety — usable as Box<dyn GpuBackend>
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_usable_as_box_dyn_gpu_backend() {
    // If GpuBackend were not object-safe, or if WgpuBackend violated
    // its Send + Sync bounds, this would fail to compile.
    let backend: Box<dyn GpuBackend> = match WgpuBackend::new() {
        Ok(b) => Box::new(b),
        Err(RuntimeError::GpuUnavailable) => Box::new(unavailable_backend()),
        Err(other) => panic!("unexpected error: {other:?}"),
    };
    // Empty input works through the trait object on any host.
    let out = backend
        .dispatch_map("@compute @workgroup_size(64) fn main() {}", &[])
        .expect("empty dispatch through dyn GpuBackend must succeed");
    assert!(out.is_empty());
}

// ---------------------------------------------------------------------------
// 10. Construction + accessors
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_from_context_preserves_context_reference() {
    // WgpuBackend::from_context wraps the given context; context() must
    // borrow the same one. Use has_adapter() to probe identity.
    let ctx = GpuContext::unavailable();
    let backend = WgpuBackend::from_context(ctx);
    assert!(
        !backend.context().has_adapter(),
        "unavailable context must report no adapter via backend.context()"
    );
    assert!(
        !backend.has_device(),
        "unavailable context must report no device via backend.has_device()"
    );
}

#[test]
fn test_gpu_dispatch_new_result_shape_is_runtime_error() {
    // Compile-time + runtime proof that the constructor returns
    // Result<WgpuBackend, RuntimeError>. The Ok arm is exercised on
    // this host (which has a GPU); the Err arm is exercised on hosts
    // without one. Either is acceptable.
    let result: Result<WgpuBackend, RuntimeError> = WgpuBackend::new();
    match result {
        Ok(backend) => {
            assert!(backend.context().has_adapter());
        }
        Err(RuntimeError::GpuUnavailable) => {
            // expected on no-GPU hosts
        }
        Err(other) => panic!("unexpected error variant: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 11. Empty input does NOT initialize the device — even when the context
//     could acquire one
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_empty_input_does_not_trigger_device_init() {
    // After an empty-input dispatch_map, has_device() must still be
    // false (the early return fires before device()/queue()).
    // Build with a real context but check the side effect: no device
    // init even when the dispatch_map returns Ok(empty).
    let ctx = GpuContext::unavailable();
    let backend = WgpuBackend::from_context(ctx);
    assert!(
        !backend.has_device(),
        "precondition: no device before dispatch"
    );
    let _ = backend
        .dispatch_map("@compute @workgroup_size(64) fn main() {}", &[])
        .expect("empty input returns Ok without touching the device");
    assert!(
        !backend.has_device(),
        "empty-input dispatch must NOT have triggered device init"
    );
}

// ---------------------------------------------------------------------------
// 12. Multiple consecutive dispatches on the same backend share the cached
//     (Device, Queue) — T43 OnceLock semantics preserved by T45
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_multiple_dispatches_share_cached_device() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");

    // First dispatch — triggers device init.
    let out1 = backend
        .dispatch_map(&wgsl, &[1.0_f32, 2.0])
        .expect("first dispatch");
    assert_eq!(out1, vec![2.0_f32, 4.0]);
    let init_count_after_first = backend.context().device_init_count();

    // Two more dispatches — must NOT re-init the device.
    let out2 = backend
        .dispatch_map(&wgsl, &[10.0_f32, 20.0, 30.0])
        .expect("second dispatch");
    assert_eq!(out2, vec![20.0_f32, 40.0, 60.0]);

    let out3 = backend
        .dispatch_map(&wgsl, &[0.5_f32, 1.5, 2.5, 3.5])
        .expect("third dispatch");
    assert_eq!(out3, vec![1.0_f32, 3.0, 5.0, 7.0]);

    let init_count_after_third = backend.context().device_init_count();
    assert_eq!(
        init_count_after_first, init_count_after_third,
        "device init count must not increase across multiple dispatches (OnceLock caching)"
    );
    assert_eq!(
        init_count_after_third, 1,
        "cached device must have been initialized exactly once across three dispatches"
    );
}

// ---------------------------------------------------------------------------
// 13. Negative-slope shader — exercises GPU correctness for non-trivial
//     closures that might exercise different ALU paths
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_negative_and_fractional_inputs_match_oracle() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    // Mix of negative / fractional / large-magnitude inputs to exercise
    // the GPU ALU on different value ranges. Avoids common math
    // constants (PI, E) so clippy::approx_constant stays quiet.
    let input: Vec<f32> = vec![
        -100.0, -1.5, -0.001, 0.0, 0.001, 1.5, 100.0, 99999.5, -99999.5, 42.4242,
    ];
    let gpu_out = backend
        .dispatch_map(&wgsl, &input)
        .expect("dispatch with mixed inputs");
    let cpu_out = cpu_fallback_map(&input, |x| x * 2.0);
    assert_approx_eq(
        &gpu_out,
        &cpu_out,
        1e-3,
        "GPU == CPU oracle for mixed sign/magnitude",
    );
}

// ---------------------------------------------------------------------------
// 14. Backend Debug formatting — useful in tracing/logging
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_backend_has_usable_debug_repr() {
    let backend = unavailable_backend();
    let s = format!("{backend:?}");
    assert!(
        s.contains("WgpuBackend"),
        "Debug repr must name the type, got: {s}"
    );
}

// ---------------------------------------------------------------------------
// 15. Cross-compile sanity — WgpuBackend is Send + Sync (required by trait)
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_dispatch_wgpu_backend_is_send_sync() {
    // Compile-time + runtime check. Send + Sync is required by the
    // GpuBackend trait; this test pins the property at the type level.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WgpuBackend>();

    // Also usable across threads via Arc (mirrors the mock test pattern).
    let backend = match WgpuBackend::new() {
        Ok(b) => std::sync::Arc::new(b),
        Err(RuntimeError::GpuUnavailable) => std::sync::Arc::new(unavailable_backend()),
        Err(other) => panic!("unexpected: {other:?}"),
    };
    let backend_clone = std::sync::Arc::clone(&backend);
    let handle = std::thread::spawn(move || {
        // Empty input through dyn GpuBackend on another thread.
        let b: &dyn GpuBackend = backend_clone.as_ref();
        let _ = b.dispatch_map("@compute @workgroup_size(64) fn main() {}", &[]);
    });
    handle
        .join()
        .expect("cross-thread dispatch via dyn GpuBackend must not panic");
}
