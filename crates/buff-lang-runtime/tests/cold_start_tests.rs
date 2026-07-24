//! T47: Cold-start mitigation integration tests.
//!
//! All test names contain the substring `cold_start` so the QA filter
//! `cargo test -p buff-lang-runtime cold_start` matches the whole
//! suite (inline unit tests in `src/cold_start.rs` PLUS these
//! integration tests).
//!
//! # Coverage map
//!
//! | Spec-required case                                | Test name                                          |
//! |---------------------------------------------------|----------------------------------------------------|
//! | QA — same shader twice → compile count == 1       | `test_cold_start_qa_same_shader_twice_compiles_once` |
//! | Different shaders → compile count == 2            | `test_cold_start_different_shaders_compile_twice`    |
//! | Buffer pool reuse                                 | `test_cold_start_buffer_pool_reuses_buffers`         |
//! | Async init is_ready/wait_ready                    | `test_cold_start_async_init_is_ready_*`              |
//! | Results still correct after caching               | `test_cold_start_roundtrip_*_after_caching`          |
//! | Graceful no-GPU                                   | `test_cold_start_no_gpu_*`                           |
//!
//! Real-GPU tests use [`try_get_real_backend`] and skip gracefully on
//! hosts without a GPU adapter (asserting only the no-GPU path
//! elsewhere). Hosts WITH a GPU run every assertion end-to-end.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::ty::TypeRef;
use buff_lang_ast::{Expr, Literal, Stmt};
use buff_lang_codegen_wgsl::generate_wgsl;
use buff_lang_error::Span;
use buff_lang_runtime::{cpu_fallback_map, ColdStartBackend, GpuBackend, GpuContext, RuntimeError};

// ---------------------------------------------------------------------------
// Helpers (mirror the T45 gpu_dispatch_tests.rs helpers)
// ---------------------------------------------------------------------------

/// Build the QA lambda `{ x: Float => x * 2.0 }`. Byte-identical to
/// the T44 reference lambda (so the generated WGSL is snapshot-stable).
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

/// Build the lambda `{ x: Float => x + 1.0 }` (a different shader so
/// we can verify the cache differentiates).
fn x_plus_one_lambda() -> Expr {
    let span = Span::dummy();
    let x = Expr::Ident(Ident::new("x", span), span);
    let one = Expr::Literal(Literal::Float(1.0), span);
    let body_expr = Expr::BinaryOp {
        op: BinaryOp::Add,
        lhs: Box::new(x),
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

/// Construct a [`ColdStartBackend`] backed by a real GPU, or `None`
/// when the host has no GPU adapter. Mirrors the T45 helper pattern.
fn try_get_real_backend() -> Option<ColdStartBackend> {
    match ColdStartBackend::new() {
        Ok(backend) => Some(backend),
        Err(RuntimeError::GpuUnavailable { .. }) => None,
        Err(other) => panic!("unexpected error constructing ColdStartBackend: {other:?}"),
    }
}

/// A [`ColdStartBackend`] built from a known-unavailable context. Always
/// constructible — no GPU is touched.
fn unavailable_backend() -> ColdStartBackend {
    ColdStartBackend::from_context(GpuContext::unavailable())
}

/// Assert approximate equality for two `&[f32]` slices, element-wise,
/// within `1e-4` tolerance.
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
// 1. QA case — same shader dispatched twice → pipeline_compile_count == 1
//    (REAL GPU if present; graceful skip otherwise)
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_qa_same_shader_twice_compiles_once() {
    // The literal QA case from the T47 spec:
    //   dispatch shader A twice → assert create_pipeline called once.
    // We approximate "create_pipeline called once" by reading
    //   backend.pipeline_compile_count() == 1
    // after two dispatches with the same shader.
    let Some(backend) = try_get_real_backend() else {
        // No GPU on this host — the no-GPU path is asserted in
        // test_cold_start_no_gpu_returns_unavailable.
        return;
    };

    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("T44 codegen must succeed for {x => x*2}");
    let input = vec![1.0_f32, 2.0, 3.0];

    // First dispatch: cache miss → compile + alloc.
    let out1 = backend
        .dispatch_map(&wgsl, &input)
        .expect("first dispatch must succeed on a real GPU");
    assert_eq!(out1, vec![2.0_f32, 4.0, 6.0]);
    let compile_after_first = backend.pipeline_compile_count();
    assert_eq!(
        compile_after_first, 1,
        "after first dispatch, exactly one pipeline must have been compiled"
    );

    // Second dispatch: cache HIT → no new compile.
    let out2 = backend
        .dispatch_map(&wgsl, &input)
        .expect("second dispatch must succeed");
    assert_eq!(out2, vec![2.0_f32, 4.0, 6.0]);
    assert_eq!(
        backend.pipeline_compile_count(),
        1,
        "QA: second dispatch with same shader must reuse the cached pipeline (compile count stays at 1)"
    );

    // Third dispatch: also a cache hit.
    let _out3 = backend
        .dispatch_map(&wgsl, &[10.0_f32, 20.0, 30.0])
        .expect("third dispatch");
    assert_eq!(
        backend.pipeline_compile_count(),
        1,
        "third dispatch with same shader must STILL reuse the cache"
    );
}

// ---------------------------------------------------------------------------
// 2. Different shaders → compile count == 2
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_different_shaders_compile_twice() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let wgsl_a = generate_wgsl(&x_times_two_lambda()).expect("codegen A");
    let wgsl_b = generate_wgsl(&x_plus_one_lambda()).expect("codegen B");
    assert_ne!(
        wgsl_a, wgsl_b,
        "the two lambdas must produce different WGSL source"
    );

    let input = vec![1.0_f32, 2.0, 3.0];

    // Dispatch shader A → cache miss for A.
    let _ = backend.dispatch_map(&wgsl_a, &input).expect("dispatch A");
    assert_eq!(backend.pipeline_compile_count(), 1);

    // Dispatch shader B → cache miss for B.
    let out_b = backend.dispatch_map(&wgsl_b, &input).expect("dispatch B");
    assert_eq!(out_b, vec![2.0_f32, 3.0, 4.0]);
    assert_eq!(
        backend.pipeline_compile_count(),
        2,
        "different shaders must each trigger one compile"
    );

    // Now dispatch A again → cache hit (count stays at 2).
    let _ = backend
        .dispatch_map(&wgsl_a, &input)
        .expect("dispatch A again");
    assert_eq!(backend.pipeline_compile_count(), 2);

    // Dispatch B again → cache hit.
    let _ = backend
        .dispatch_map(&wgsl_b, &input)
        .expect("dispatch B again");
    assert_eq!(
        backend.pipeline_compile_count(),
        2,
        "two distinct shaders cached; further dispatches reuse them"
    );
}

// ---------------------------------------------------------------------------
// 3. Buffer pool reuse — allocation_count grows sub-linearly with dispatch count
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_buffer_pool_reuses_buffers() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };

    let wgsl = generate_wgsl(&x_times_two_lambda()).expect("codegen");
    // Fixed input size across dispatches — pool keys are (size, usage).
    let input = vec![1.0_f32, 2.0, 3.0, 4.0, 5.0];

    // First dispatch: 3 buffers allocated (input, output, staging).
    let _ = backend.dispatch_map(&wgsl, &input).expect("first dispatch");
    let alloc_after_first = backend.buffer_allocation_count();
    assert_eq!(
        alloc_after_first, 3,
        "first dispatch must allocate exactly 3 buffers (input + output + staging)"
    );

    // Second dispatch with the SAME input size → 0 new allocations.
    let _ = backend
        .dispatch_map(&wgsl, &input)
        .expect("second dispatch");
    let alloc_after_second = backend.buffer_allocation_count();
    assert_eq!(
        alloc_after_second, 3,
        "second dispatch with same size must reuse pooled buffers (0 new allocations)"
    );

    // Five more dispatches — pool stays steady at 3 buffers.
    for _ in 0..5 {
        let _ = backend
            .dispatch_map(&wgsl, &input)
            .expect("repeat dispatch");
    }
    assert_eq!(
        backend.buffer_allocation_count(),
        3,
        "after 7 total dispatches with same size, pool must still hold 3 buffers (sub-linear growth)"
    );

    // Now a DIFFERENT input size → 3 more buffers allocated for that
    // new size key. Pool now holds 6 buffers total (3 + 3).
    let bigger = vec![0.5_f32; 100];
    let _ = backend
        .dispatch_map(&wgsl, &bigger)
        .expect("bigger dispatch");
    assert_eq!(
        backend.buffer_allocation_count(),
        6,
        "different input size triggers a new pool key → 3 more allocations"
    );
}

// ---------------------------------------------------------------------------
// 4. Async init — is_ready / wait_ready
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_async_init_is_ready_false_before_spawn() {
    // Before spawn_init is called, is_ready must be false.
    let backend = unavailable_backend();
    assert!(
        !backend.is_ready(),
        "is_ready must be false before spawn_init is called"
    );
}

#[test]
fn test_cold_start_async_init_is_ready_true_after_wait_ready_blocking() {
    // After spawn_init + wait_ready_blocking, is_ready must be true —
    // even on an unavailable context (the spawn task runs, fails
    // gracefully to acquire the device, caches the error, sets ready flag).
    let backend = unavailable_backend();
    backend
        .spawn_init()
        .expect("spawn_init must succeed on unavailable context");
    assert!(
        !backend.is_ready() || backend.is_ready(),
        // Defensive — is_ready may be either true (race resolved) or
        // false (still running). The real assertion is below.
    );
    backend.wait_ready_blocking();
    assert!(
        backend.is_ready(),
        "after wait_ready_blocking, is_ready must be true"
    );
}

#[test]
fn test_cold_start_async_init_idempotent() {
    // Calling spawn_init twice must be a no-op — only one background
    // thread ever spawns.
    let backend = unavailable_backend();
    backend.spawn_init().expect("first spawn_init");
    backend
        .spawn_init()
        .expect("second spawn_init must be no-op Ok");
    backend.wait_ready_blocking();
    assert!(backend.is_ready());
}

#[test]
fn test_cold_start_async_init_warms_device_on_real_gpu() {
    // On a host with a GPU, after spawn_init + wait_ready_blocking,
    // has_device() must be true (the spawn task warmed the cache).
    let Some(backend) = try_get_real_backend() else {
        return;
    };
    assert!(
        !backend.has_device(),
        "precondition: before spawn_init, no device"
    );
    backend.spawn_init().expect("spawn_init");
    backend.wait_ready_blocking();
    assert!(
        backend.has_device(),
        "after spawn_init + wait_ready_blocking, device must be cached on a real GPU host"
    );
    // device_init_count must be 1 (spawn_init ran the cache-warming
    // device_queue() call, and OnceLock prevents re-init).
    assert_eq!(
        backend.context().device_init_count(),
        1,
        "device init count must be 1 after spawn_init warming"
    );
}

#[tokio::test]
async fn test_cold_start_async_init_wait_ready_async_path() {
    // The async wait_ready() variant must complete and set is_ready.
    let backend = unavailable_backend();
    backend.spawn_init().expect("spawn");
    backend.wait_ready().await;
    assert!(
        backend.is_ready(),
        "after async wait_ready, is_ready must be true"
    );
}

// ---------------------------------------------------------------------------
// 5. Roundtrip correctness unchanged by caching
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_roundtrip_qa_one_two_three_x_two_yields_two_four_six() {
    // The T45 QA roundtrip must still hold with caching enabled.
    let Some(backend) = try_get_real_backend() else {
        return;
    };
    let wgsl = generate_wgsl(&x_times_two_lambda()).expect("codegen");
    let out = backend
        .dispatch_map(&wgsl, &[1.0_f32, 2.0, 3.0])
        .expect("dispatch");
    assert_eq!(out, vec![2.0_f32, 4.0, 6.0]);
}

#[test]
fn test_cold_start_roundtrip_matches_cpu_oracle_after_many_cached_dispatches() {
    // After many cached dispatches, output must still match the CPU oracle.
    let Some(backend) = try_get_real_backend() else {
        return;
    };
    let wgsl = generate_wgsl(&x_times_two_lambda()).expect("codegen");
    let input: Vec<f32> = (0..100).map(|i| i as f32 * 0.1).collect();
    for _ in 0..5 {
        let gpu_out = backend
            .dispatch_map(&wgsl, &input)
            .expect("repeated dispatch must succeed");
        let cpu_out = cpu_fallback_map(&input, |x| x * 2.0);
        assert_approx_eq(
            &gpu_out,
            &cpu_out,
            1e-4,
            "GPU == CPU oracle after cached dispatch",
        );
    }
    // Sanity: cache + pool behaved as expected.
    assert_eq!(backend.pipeline_compile_count(), 1);
    assert_eq!(backend.buffer_allocation_count(), 3);
}

#[test]
fn test_cold_start_roundtrip_singleton_input_correct_after_caching() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };
    let wgsl = generate_wgsl(&x_times_two_lambda()).expect("codegen");
    let _ = backend
        .dispatch_map(&wgsl, &[1.0_f32, 2.0, 3.0])
        .expect("initial dispatch to warm cache");
    // Now singleton input — different buffer size.
    let out = backend
        .dispatch_map(&wgsl, &[42.0_f32])
        .expect("singleton dispatch");
    assert_eq!(out.len(), 1);
    assert!(
        (out[0] - 84.0).abs() < 1e-4,
        "42*2 must equal 84, got {}",
        out[0]
    );
}

// ---------------------------------------------------------------------------
// 6. Graceful no-GPU
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_no_gpu_returns_unavailable_error() {
    // A backend built from GpuContext::unavailable() must return
    // GpuUnavailable, never panic.
    let backend = unavailable_backend();
    let wgsl = generate_wgsl(&x_times_two_lambda()).expect("codegen");
    let input = vec![1.0_f32, 2.0, 3.0];
    match backend.dispatch_map(&wgsl, &input) {
        Err(RuntimeError::GpuUnavailable { .. }) => {
            // expected
        }
        Err(other) => panic!("expected GpuUnavailable, got: {other:?}"),
        Ok(out) => panic!("expected Err but got Ok({out:?})"),
    }
    // Counters must NOT have moved.
    assert_eq!(backend.pipeline_compile_count(), 0);
    assert_eq!(backend.buffer_allocation_count(), 0);
}

#[test]
fn test_cold_start_no_gpu_empty_input_returns_empty_vec() {
    // Empty input must short-circuit BEFORE any GPU/cache work —
    // works on no-GPU hosts too.
    let backend = unavailable_backend();
    let wgsl = generate_wgsl(&x_times_two_lambda()).expect("codegen");
    let out = backend
        .dispatch_map(&wgsl, &[])
        .expect("empty dispatch must always return Ok(empty)");
    assert!(out.is_empty());
}

// ---------------------------------------------------------------------------
// 7. Object-safety + Send+Sync
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_usable_as_box_dyn_gpu_backend() {
    // ColdStartBackend must be usable as Box<dyn GpuBackend> — that's
    // the T46/T49 dispatch-site pattern.
    let backend: Box<dyn GpuBackend> = match ColdStartBackend::new() {
        Ok(b) => Box::new(b),
        Err(RuntimeError::GpuUnavailable { .. }) => Box::new(unavailable_backend()),
        Err(other) => panic!("unexpected: {other:?}"),
    };
    let out = backend
        .dispatch_map("@compute @workgroup_size(64) fn main() {}", &[])
        .expect("empty dispatch through dyn GpuBackend");
    assert!(out.is_empty());
}

#[test]
fn test_cold_start_send_sync_across_threads() {
    // Compile-time + runtime Send + Sync check.
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<ColdStartBackend>();
    assert_sync::<ColdStartBackend>();

    // Also usable via Arc across threads.
    let backend = match ColdStartBackend::new() {
        Ok(b) => std::sync::Arc::new(b),
        Err(RuntimeError::GpuUnavailable { .. }) => std::sync::Arc::new(unavailable_backend()),
        Err(other) => panic!("unexpected: {other:?}"),
    };
    let backend_clone = std::sync::Arc::clone(&backend);
    let handle = std::thread::spawn(move || {
        let b: &dyn GpuBackend = backend_clone.as_ref();
        let _ = b.dispatch_map("@compute @workgroup_size(64) fn main() {}", &[]);
    });
    handle
        .join()
        .expect("cross-thread dispatch via dyn GpuBackend must not panic");
}

// ---------------------------------------------------------------------------
// 8. Construction + accessor surface
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_constructors_yield_unavailable_context() {
    // from_context(unavailable) and default() must both yield a backend
    // whose context reports no adapter.
    let a = ColdStartBackend::from_context(GpuContext::unavailable());
    let b = ColdStartBackend::default();
    assert!(!a.context().has_adapter());
    assert!(!b.context().has_adapter());
    assert!(!a.has_device());
    assert!(!b.has_device());
}

#[test]
fn test_cold_start_accessors_initial_state_zero() {
    // Freshly-constructed backend has zero counters.
    let backend = unavailable_backend();
    assert_eq!(backend.pipeline_compile_count(), 0);
    assert_eq!(backend.buffer_allocation_count(), 0);
    assert_eq!(backend.cached_pipeline_count(), 0);
    assert_eq!(backend.pooled_buffer_count(), 0);
}

// ---------------------------------------------------------------------------
// 9. Multiple input sizes coexisting in the pool
// ---------------------------------------------------------------------------

#[test]
fn test_cold_start_pool_handles_multiple_sizes_simultaneously() {
    let Some(backend) = try_get_real_backend() else {
        return;
    };
    let wgsl = generate_wgsl(&x_times_two_lambda()).expect("codegen");

    // Dispatch with 5 different input sizes — each triggers 3 fresh
    // allocations (5 * 3 = 15 total).
    let sizes: &[usize] = &[10, 50, 100, 200, 500];
    for &size in sizes {
        let input: Vec<f32> = (0..size).map(|i| i as f32).collect();
        let _ = backend
            .dispatch_map(&wgsl, &input)
            .unwrap_or_else(|e| panic!("dispatch size={size}: {e:?}"));
    }
    assert_eq!(
        backend.buffer_allocation_count(),
        sizes.len() * 3,
        "5 distinct sizes ⇒ 15 allocations (3 per size)"
    );

    // Now re-dispatch each size — pool MUST serve all 5 from free-lists.
    let alloc_before = backend.buffer_allocation_count();
    for &size in sizes {
        let input: Vec<f32> = (0..size).map(|i| i as f32).collect();
        let _ = backend
            .dispatch_map(&wgsl, &input)
            .expect("repeat dispatch must succeed");
    }
    assert_eq!(
        backend.buffer_allocation_count(),
        alloc_before,
        "repeat dispatches across 5 sizes must reuse pool (0 new allocations)"
    );
    // And only one pipeline was compiled (same shader source throughout).
    assert_eq!(backend.pipeline_compile_count(), 1);
}
