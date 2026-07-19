//! T38b: Mock GPU backend + CPU-fallback oracle + WGSL snapshot harness.
//!
//! These tests prove:
//!
//! 1. **QA case**: `MockGpuBackend.dispatch_map(...)` then
//!    `assert_eq!(backend.recorded_dispatches(), 1)`.
//! 2. Multiple dispatches increment the count.
//! 3. Recorded shader source + input length match what was passed.
//! 4. Mock output equals the [`cpu_fallback_map`] oracle for the same
//!    closure.
//! 5. The mock works with NO real GPU (always — it never touches wgpu).
//! 6. The mock is usable as `Box<dyn GpuBackend>` (object-safe).
//! 7. The mock is `Send + Sync` (cross-thread safe).
//! 8. Empty-input dispatch is a valid edge case (counted as one
//!    dispatch, oracle returns empty Vec).
//! 9. `clear_records` resets the recorded-dispatches counter.
//! 10. WGSL snapshot harness: T44 codegen output is byte-stable via
//!     `insta::assert_snapshot`.
//!
//! All test names contain `gpu_harness` so the QA filter
//! `cargo test -p buff-lang-runtime gpu_harness` matches the whole suite.

use std::sync::Arc;

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::ty::TypeRef;
use buff_lang_ast::{Expr, Literal, Stmt};
use buff_lang_codegen_wgsl::generate_wgsl;
use buff_lang_error::Span;
use buff_lang_runtime::{
    cpu_fallback_map, DispatchRecord, GpuBackend, MockGpuBackend, RuntimeError,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the QA lambda `{ x: Float => x * 2.0 }` for the WGSL snapshot
/// test. Matches the T44 reference lambda exactly so the snapshot stays
/// byte-identical to T44's `wgsl_full_shader_x_times_two.snap`.
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
            span,
        }],
        body,
        return_type: None,
        span,
    }
}

/// Sample WGSL shader source used by the recording tests. NOT generated
/// — just a stable placeholder so the recording logic can be exercised
/// without depending on T44 codegen for the non-snapshot tests.
const SAMPLE_WGSL: &str = "@compute @workgroup_size(64)\nfn main() {}";

// ---------------------------------------------------------------------------
// 1. QA case — single dispatch records exactly one
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_qa_single_dispatch_records_one() {
    // QA case from the task spec:
    //   MockGpuBackend.dispatch() → assert recorded_dispatches == 1
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    assert_eq!(
        backend.recorded_dispatches(),
        0,
        "freshly-constructed backend must have zero dispatches"
    );

    let input = vec![1.0_f32, 2.0, 3.0];
    let out = backend
        .dispatch_map(SAMPLE_WGSL, &input)
        .expect("dispatch_map must succeed on the mock");

    assert_eq!(
        backend.recorded_dispatches(),
        1,
        "QA: after exactly one dispatch_map call, recorded_dispatches must be 1"
    );
    assert_eq!(out, vec![2.0, 4.0, 6.0]);
}

// ---------------------------------------------------------------------------
// 2. Multiple dispatches increment the count
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_multiple_dispatches_increment_count() {
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x + 1.0));
    let input = vec![10.0_f32, 20.0];

    for expected in 1..=5 {
        let _ = backend.dispatch_map(SAMPLE_WGSL, &input);
        assert_eq!(
            backend.recorded_dispatches(),
            expected,
            "after {expected} dispatches, count must match"
        );
    }

    // dispatch_count() is an alias — must agree with recorded_dispatches().
    assert_eq!(backend.dispatch_count(), 5);
}

#[test]
fn test_gpu_harness_dispatch_count_alias_matches_recorded_dispatches() {
    let backend = MockGpuBackend::new(|input: &[f32]| input.to_vec());
    let input = vec![0.0_f32; 4];
    for _ in 0..3 {
        let _ = backend.dispatch_map(SAMPLE_WGSL, &input);
    }
    assert_eq!(backend.dispatch_count(), backend.recorded_dispatches());
    assert_eq!(backend.dispatch_count(), 3);
}

// ---------------------------------------------------------------------------
// 3. Recorded shader + input_len match what was passed
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_records_shader_source_matches_input() {
    let backend = MockGpuBackend::new(|input: &[f32]| input.to_vec());
    let custom_shader = "@compute @workgroup_size(128)\nfn main(@builtin(global_invocation_id) gid: vec3<u32>) { /* body */ }";
    let input = vec![0.0_f32; 7];

    let _ = backend.dispatch_map(custom_shader, &input);

    let recs = backend.records();
    assert_eq!(recs.len(), 1, "exactly one record expected");
    let DispatchRecord { shader, input_len } = &recs[0];
    assert_eq!(
        shader, custom_shader,
        "recorded shader source must be byte-equal to the input"
    );
    assert_eq!(
        *input_len, 7,
        "recorded input_len must match the input slice length"
    );
}

#[test]
fn test_gpu_harness_records_preserve_dispatch_order() {
    // Three dispatches with distinguishable (shader, input_len) pairs.
    // Records must come back in the same order they were dispatched
    // (Mutex<Vec> push order — deterministic).
    let backend = MockGpuBackend::new(|input: &[f32]| input.to_vec());

    let _ = backend.dispatch_map("// first", &[1.0_f32]);
    let _ = backend.dispatch_map("// second", &[2.0_f32, 3.0]);
    let _ = backend.dispatch_map("// third", &[4.0_f32, 5.0, 6.0, 7.0]);

    let recs = backend.records();
    assert_eq!(recs.len(), 3);
    assert_eq!(recs[0].shader, "// first");
    assert_eq!(recs[0].input_len, 1);
    assert_eq!(recs[1].shader, "// second");
    assert_eq!(recs[1].input_len, 2);
    assert_eq!(recs[2].shader, "// third");
    assert_eq!(recs[2].input_len, 4);
}

#[test]
fn test_gpu_harness_records_capture_empty_shader_string() {
    // Empty shader source is a valid edge case — must be recorded as-is,
    // not replaced with a default.
    let backend = MockGpuBackend::new(|_: &[f32]| Vec::new());
    let _ = backend.dispatch_map("", &[]);
    let recs = backend.records();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].shader, "");
    assert_eq!(recs[0].input_len, 0);
}

// ---------------------------------------------------------------------------
// 4. Mock output equals CPU-fallback oracle for the same closure
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_mock_output_matches_cpu_fallback_oracle() {
    let factor = 3.0_f32;
    // Per-element closure used by BOTH the mock and the oracle.
    let mk_out = |input: &[f32]| cpu_fallback_map(input, |x| x * factor + 1.0);

    let backend = MockGpuBackend::new(mk_out);
    let input = vec![1.0_f32, 2.0, 3.0, 4.0, 5.0];

    let gpu_out = backend
        .dispatch_map(SAMPLE_WGSL, &input)
        .expect("dispatch must succeed");
    let cpu_out = cpu_fallback_map(&input, |x| x * factor + 1.0);

    assert_eq!(gpu_out, cpu_out, "mock output must equal CPU oracle");
    assert_eq!(
        gpu_out,
        vec![4.0, 7.0, 10.0, 13.0, 16.0],
        "concrete expected values"
    );
}

#[test]
fn test_gpu_harness_cpu_fallback_oracle_preserves_input_order() {
    // Deterministic per-element map preserves order — this is the
    // contract that the mock output also inherits.
    let input = vec![5.0_f32, 3.0, 8.0, 1.0, 9.0, 2.0, 7.0];
    let out = cpu_fallback_map(&input, |x| x.floor());
    assert_eq!(out, input, "identity closure must round-trip");
}

#[test]
fn test_gpu_harness_cpu_fallback_oracle_empty_input() {
    let out = cpu_fallback_map(&[], |x| x * 2.0);
    assert!(out.is_empty(), "empty input → empty output");
}

// ---------------------------------------------------------------------------
// 5. Mock works with NO real GPU (always)
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_mock_never_touches_wgpu() {
    // The mock does not acquire any GPU resources. Constructing and
    // dispatching on it must succeed even when no GPU is present — there
    // is no `GpuContext::new()` call, no `request_adapter`, no device
    // init. We assert this indirectly: dispatch succeeds unconditionally
    // regardless of host GPU state.
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x.powi(2)));
    let input = vec![2.0_f32, 3.0, 4.0];
    let out = backend
        .dispatch_map(SAMPLE_WGSL, &input)
        .expect("mock dispatch must never fail due to GPU absence");
    assert_eq!(out, vec![4.0, 9.0, 16.0]);
}

#[test]
fn test_gpu_harness_mock_dispatch_never_returns_err() {
    // The mock's dispatch_map is infallible by construction — it returns
    // Ok always (the oracle never errors). Tests can rely on this so
    // they focus on the recording behavior, not error handling.
    let backend = MockGpuBackend::new(|input: &[f32]| input.to_vec());
    for _ in 0..10 {
        let result = backend.dispatch_map(SAMPLE_WGSL, &[0.0_f32; 100]);
        assert!(result.is_ok(), "mock dispatch_map must never return Err");
    }
}

// ---------------------------------------------------------------------------
// 6. Object-safety — usable as Box<dyn GpuBackend>
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_mock_usable_as_box_dyn_gpu_backend() {
    // If GpuBackend were not object-safe, this would fail to compile.
    let backend: Box<dyn GpuBackend> = Box::new(MockGpuBackend::new(|input: &[f32]| {
        cpu_fallback_map(input, |x| x - 1.0)
    }));
    let input = vec![10.0_f32, 20.0, 30.0];
    let out = backend
        .dispatch_map(SAMPLE_WGSL, &input)
        .expect("dyn GpuBackend dispatch must succeed");
    assert_eq!(out, vec![9.0, 19.0, 29.0]);

    // We cannot call recorded_dispatches() through `dyn GpuBackend` (it's
    // not on the trait) — that's intentional. Recording is a mock-only
    // concern. The trait surface stays minimal.
}

#[test]
fn test_gpu_harness_mock_swappable_with_another_backend_via_dyn() {
    // Two mock instances behind the same trait object — simulates
    // swapping a "real" backend for a "mock" backend at runtime (the
    // whole point of the trait abstraction).
    let backends: Vec<Box<dyn GpuBackend>> = vec![
        Box::new(MockGpuBackend::new(|input: &[f32]| {
            cpu_fallback_map(input, |x| x * 2.0)
        })),
        Box::new(MockGpuBackend::new(|input: &[f32]| {
            cpu_fallback_map(input, |x| x * 10.0)
        })),
    ];
    let input = vec![1.0_f32, 2.0, 3.0];
    let mut outputs = Vec::new();
    for backend in &backends {
        outputs.push(
            backend
                .dispatch_map(SAMPLE_WGSL, &input)
                .expect("dispatch must succeed"),
        );
    }
    assert_eq!(outputs[0], vec![2.0, 4.0, 6.0]);
    assert_eq!(outputs[1], vec![10.0, 20.0, 30.0]);
}

// ---------------------------------------------------------------------------
// 7. Send + Sync — cross-thread safe
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_mock_is_send_sync_across_threads() {
    // The trait bound `GpuBackend: Send + Sync` is enforced at compile
    // time — this test goes further and proves the mock is safely
    // usable across OS threads at runtime (Mutex<Vec> is the right
    // shared-state primitive here).
    let backend = Arc::new(MockGpuBackend::new(|input: &[f32]| {
        cpu_fallback_map(input, |x| x * 2.0)
    }));
    let input = vec![1.0_f32, 2.0, 3.0];

    let handles: Vec<std::thread::JoinHandle<()>> = (0..4)
        .map(|_| {
            let backend = Arc::clone(&backend);
            let input = input.clone();
            std::thread::spawn(move || {
                let _ = backend.dispatch_map(SAMPLE_WGSL, &input);
            })
        })
        .collect();

    for handle in handles {
        handle
            .join()
            .expect("worker thread must not panic with the shared mock");
    }

    assert_eq!(
        backend.recorded_dispatches(),
        4,
        "four concurrent dispatches must each be recorded exactly once"
    );
}

// ---------------------------------------------------------------------------
// 8. clear_records resets the recorded-dispatches counter
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_clear_records_resets_count() {
    let backend = MockGpuBackend::new(|input: &[f32]| input.to_vec());
    let _ = backend.dispatch_map(SAMPLE_WGSL, &[1.0_f32]);
    let _ = backend.dispatch_map(SAMPLE_WGSL, &[2.0_f32]);
    assert_eq!(backend.recorded_dispatches(), 2);

    backend.clear_records();
    assert_eq!(backend.recorded_dispatches(), 0, "clear must reset count");
    assert!(
        backend.records().is_empty(),
        "clear must also drop the stored records"
    );

    // The backend is still usable after clear.
    let _ = backend.dispatch_map(SAMPLE_WGSL, &[3.0_f32]);
    assert_eq!(backend.recorded_dispatches(), 1);
}

// ---------------------------------------------------------------------------
// 9. Error bridge — dispatch_map return type carries RuntimeError
//     (even though the mock never errors, the signature must allow it)
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_dispatch_map_result_shape_is_runtime_error() {
    // Compile-time proof that the trait's error type is RuntimeError
    // (so T45 can return GpuUnavailable/GpuInit/Unsupported from real
    // backends). The mock returns Ok always — this test just locks the
    // shape so a future trait refactor that changes the error type
    // breaks here loudly.
    let backend = MockGpuBackend::new(|input: &[f32]| input.to_vec());
    let result: Result<Vec<f32>, RuntimeError> = backend.dispatch_map(SAMPLE_WGSL, &[1.0_f32]);
    let _ = result.expect("Ok path");
}

// ---------------------------------------------------------------------------
// 10. WGSL snapshot harness — T44 codegen output is byte-stable
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_harness_wgsl_snapshot_x_times_two_stable() {
    // Snapshot harness: the same Buff lambda → byte-identical WGSL.
    // T44 owns the codegen; T38b owns the snapshot discipline so that
    // T45 (real GPU dispatch) can wire `generate_wgsl(...)` output as
    // `wgpu::ShaderSource::Wgsl` and rely on byte-stable input.
    let lambda = x_times_two_lambda();
    let src = generate_wgsl(&lambda).expect("codegen must succeed for {x => x * 2.0}");
    insta::assert_snapshot!(src);
}

#[test]
fn test_gpu_harness_wgsl_snapshot_byte_identical_across_calls() {
    // Same lambda, called twice, must produce byte-identical WGSL — the
    // pre-condition for insta snapshot stability.
    let lambda = x_times_two_lambda();
    let a = generate_wgsl(&lambda).expect("first call");
    let b = generate_wgsl(&lambda).expect("second call");
    assert_eq!(a, b, "T44 codegen must be deterministic");
    assert!(a.contains("@compute @workgroup_size(64)"));
    assert!(a.contains("output[i] = x * 2.0;"));
}

#[test]
fn test_gpu_harness_mock_can_consume_generated_wgsl() {
    // End-to-end-ish: feed T44-generated WGSL into the mock. Proves
    // the snapshot-stable WGSL is a valid input to dispatch_map (it
    // doesn't need to be a real WGSL compiler — the mock just records
    // the string). This is the shape T45 will use, except T45 will
    // actually compile+run the shader.
    let lambda = x_times_two_lambda();
    let wgsl = generate_wgsl(&lambda).expect("codegen");
    let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
    let input = vec![1.0_f32, 2.0, 3.0, 4.0];

    let out = backend
        .dispatch_map(&wgsl, &input)
        .expect("dispatch must succeed with generated WGSL");

    assert_eq!(backend.recorded_dispatches(), 1);
    let recs = backend.records();
    assert_eq!(
        recs[0].shader, wgsl,
        "recorded shader must be the generated WGSL"
    );
    assert_eq!(recs[0].input_len, 4);
    assert_eq!(out, vec![2.0, 4.0, 6.0, 8.0]);
}
