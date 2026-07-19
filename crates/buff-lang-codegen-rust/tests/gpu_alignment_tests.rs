//! T50 integration tests — GPU-bound struct detection + repr(C)/Pod codegen.
//!
//! These tests exercise [`buff_lang_codegen_rust::analyze_gpu_alignment`]
//! via the public [`buff_lang_codegen_rust::generate_rust`] entry point.
//! Every test name contains the substring `gpu_alignment` so the QA
//! filter `cargo test -p buff-lang-codegen-rust gpu_alignment` matches
//! all of them.
//!
//! ## Coverage matrix
//!
//! - **QA + acceptance** — `struct Point { ... }` used in `v.par_map(...)`
//!   → generated Rust contains `#[repr(C)]`, `Copy`, `bytemuck::Pod`,
//!   `bytemuck::Zeroable`.
//! - **Detection signals** — closure param annotation, struct init inside
//!   parallel closure body.
//! - **All 3 combinators** — par_map / par_filter / par_reduce each
//!   trigger the detector.
//! - **Negative cases** — sequential `.map`, no parallel calls, struct
//!   used only outside any parallel closure → NO `#[repr(C)]`.
//! - **Non-regression** — non-GPU-bound struct codegen byte-identical
//!   to pre-T50 (`#[derive(Clone, PartialEq, Debug)]`, no repr(C), no Pod).
//! - **Determinism** — same input, byte-identical output across repeated
//!   `generate_rust` calls.
//! - **Combinator-list parity** — this crate's GPU-combinator list
//!   matches `race_analysis::PARALLEL_COMBINATORS` (single source of
//!   truth — if they ever diverge, race detection + alignment will
//!   disagree with disaster).
//! - **T26 backwards-compat** — `RustCodegen::mark_struct_repr_c` still
//!   emits `#[repr(C)]` (without Pod — manual hook stays opt-in).

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{FuncDecl, StructDecl};
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::{
    analyze_gpu_alignment, generate_rust, race_analysis::PARALLEL_COMBINATORS, RustCodegen,
};
use buff_lang_error::Span;

// ---------------------------------------------------------------------
// AST builder helpers (kept tiny so each test reads like Buff source)
// ---------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}
fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}
fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}
fn named_ty(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}
fn placeholder_ty() -> TypeRef {
    named_ty("_")
}
fn float_expr(n: f32) -> Expr {
    Expr::Literal(Literal::Float(n), span())
}

fn struct_decl(name: &str, fields: Vec<(&str, &str)>) -> Decl {
    Decl::StructDecl(StructDecl {
        name: ident(name),
        fields: fields
            .into_iter()
            .map(|(n, t)| (ident(n), named_ty(t)))
            .collect(),
        traits: Vec::new(),
        span: span(),
    })
}

fn struct_decl_decl(name: &str, fields: Vec<(&str, &str)>) -> StructDecl {
    StructDecl {
        name: ident(name),
        fields: fields
            .into_iter()
            .map(|(n, t)| (ident(n), named_ty(t)))
            .collect(),
        traits: Vec::new(),
        span: span(),
    }
}

fn empty_func_with_stmts(name: &str, stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    })
}

fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: ident(method),
        args,
        span: span(),
    }
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

/// Build a typed-param closure `{ p: TY => <body_expr> }`.
fn typed_closure(param_name: &str, param_ty: TypeRef, body: Expr) -> Expr {
    Expr::Lambda {
        params: vec![Param {
            name: ident(param_name),
            ty: param_ty,
            default_value: None,
            span: span(),
        }],
        body: Block {
            stmts: vec![expr_stmt(body)],
            span: span(),
        },
        return_type: None,
        span: span(),
    }
}

/// Build an untyped-param closure `{ p => <body_expr> }`.
fn untyped_closure(param_name: &str, body: Expr) -> Expr {
    typed_closure(param_name, placeholder_ty(), body)
}

/// Build a struct literal expression: `Type { f: v, ... }`.
fn struct_init(type_name: &str, fields: Vec<(&str, Expr)>) -> Expr {
    Expr::StructInit {
        type_name: ident(type_name),
        fields: fields.into_iter().map(|(n, v)| (ident(n), v)).collect(),
        span: span(),
    }
}

/// Helper to wrap a list of decls into a Vec<Decl> for `generate_rust`.
fn program(decls: Vec<Decl>) -> Vec<Decl> {
    decls
}

// =====================================================================
// 1. QA + ACCEPTANCE — the exact T50 acceptance criterion
// =====================================================================

#[test]
fn gpu_alignment_qa_struct_used_in_par_map_gets_repr_c() {
    // QA case from the T50 task spec (paraphrased):
    //
    //   struct Point { x: Float, y: Float }
    //   func f() {
    //     v.par_map({ p: Point => p.x })
    //   }
    //
    // Acceptance: the generated Rust for `Point` contains `#[repr(C)]`.
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float"), ("y", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        rust.contains("#[repr(C)]"),
        "QA FAILED: generated Rust must contain #[repr(C)] for GPU-bound Point. \
         Got:\n{rust}"
    );
    assert!(
        rust.contains("pub struct Point"),
        "expected Point struct in generated Rust; got:\n{rust}"
    );
}

#[test]
fn gpu_alignment_qa_struct_used_in_par_map_gets_bytemuck_pod_derive() {
    // The T50 task spec also requires bytemuck::Pod on GPU-bound structs.
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float"), ("y", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        rust.contains("bytemuck::Pod"),
        "GPU-bound struct must derive bytemuck::Pod. Got:\n{rust}"
    );
    assert!(
        rust.contains("bytemuck::Zeroable"),
        "GPU-bound struct must derive bytemuck::Zeroable. Got:\n{rust}"
    );
    assert!(
        rust.contains("Copy"),
        "GPU-bound struct must derive Copy (Pod requires it). Got:\n{rust}"
    );
}

#[test]
fn gpu_alignment_qa_full_struct_layout_in_generated_rust() {
    // Full-layout snapshot: assert the GPU-bound struct emits with the
    // EXACT attribute set the design specifies (std derives + bytemuck
    // + repr(C)). This pins the layout so future regressions surface.
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float"), ("y", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    // The exact expected prefix of the struct (everything up to the
    // opening brace). prettyplease formatting is stable so this is
    // byte-identical across runs.
    let expected = "#[derive(Clone, Copy, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable)]\n#[repr(C)]\npub struct Point";
    assert!(
        rust.contains(expected),
        "GPU-bound struct must emit exact attribute set. Expected substring:\n{expected}\n\nGot:\n{rust}"
    );
}

// =====================================================================
// 2. DETECTION SIGNALS — param annotation + struct init in body
// =====================================================================

#[test]
fn gpu_alignment_detects_via_struct_init_in_closure_body() {
    // "struct flows OUT" signal: the closure body CONSTRUCTS a fresh
    // struct value per element.
    //
    //   v.par_map({ x => Point { x: x, y: 0.0 } })
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float"), ("y", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![untyped_closure(
                    "x",
                    struct_init(
                        "Point",
                        vec![("x", ident_expr("x")), ("y", float_expr(0.0))],
                    ),
                )],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        rust.contains("#[repr(C)]"),
        "struct construction inside par_map closure body must trigger gpu-bound. Got:\n{rust}"
    );
}

#[test]
fn gpu_alignment_detects_via_typed_param_in_vector_generic() {
    // Edge case: the param's type is a Generic with a user-struct arg
    // (e.g. `points: Vector<Point>`). The recursive walker should find
    // `Point` inside the generic args.
    let vec_point = TypeRef::Generic {
        base: Box::new(named_ty("Vector")),
        args: vec![named_ty("Point")],
        span: span(),
    };
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![typed_closure("points", vec_point, ident_expr("points"))],
            ))],
        ),
    ]);
    let found = analyze_gpu_alignment(&decls);
    assert!(
        found.contains("Point"),
        "Vector<Point> in param annotation must mark Point as gpu-bound; got {found:?}"
    );
}

// =====================================================================
// 3. ALL 3 PARALLEL COMBINATORS TRIGGER DETECTION
// =====================================================================

#[test]
fn gpu_alignment_par_filter_triggers_detection() {
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_filter",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        rust.contains("#[repr(C)]"),
        "par_filter with Point param must trigger gpu-bound. Got:\n{rust}"
    );
}

#[test]
fn gpu_alignment_par_reduce_triggers_detection() {
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_reduce",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        rust.contains("#[repr(C)]"),
        "par_reduce with Point param must trigger gpu-bound. Got:\n{rust}"
    );
}

#[test]
fn gpu_alignment_par_map_triggers_detection() {
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        rust.contains("#[repr(C)]"),
        "par_map with Point param must trigger gpu-bound. Got:\n{rust}"
    );
}

// =====================================================================
// 4. NEGATIVE CASES — sequential .map, no parallel calls, etc.
// =====================================================================

#[test]
fn gpu_alignment_sequential_map_does_not_trigger() {
    // The CRITICAL negative case: `.map` is NOT a parallel combinator.
    // A struct used only inside a sequential `.map` closure must NOT
    // get #[repr(C)] — its codegen must remain byte-identical to the
    // pre-T50 output.
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "map",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        !rust.contains("#[repr(C)]"),
        "sequential .map must NOT trigger #[repr(C)]. Got:\n{rust}"
    );
    assert!(
        !rust.contains("bytemuck::Pod"),
        "sequential .map must NOT trigger bytemuck::Pod. Got:\n{rust}"
    );
}

#[test]
fn gpu_alignment_no_parallel_calls_no_repr_c() {
    // A program with structs but no parallel combinators anywhere:
    // struct codegen must remain in the regular (non-GPU) path.
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float"), ("y", "Float")]),
        empty_func_with_stmts("f", vec![expr_stmt(ident_expr("v"))]),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        !rust.contains("#[repr(C)]"),
        "no parallel calls ⇒ no #[repr(C)]. Got:\n{rust}"
    );
}

#[test]
fn gpu_alignment_struct_used_only_outside_parallel_closure_unchanged() {
    // A struct used in regular code (e.g. a constructor call) but NOT
    // inside a parallel closure body must NOT be marked gpu-bound.
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(struct_init(
                "Point",
                vec![("x", float_expr(1.0))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(
        !rust.contains("#[repr(C)]"),
        "struct used outside parallel closures must stay unchanged. Got:\n{rust}"
    );
}

// =====================================================================
// 5. NON-REGRESSION — non-GPU-bound struct byte-identical to pre-T50
// =====================================================================

#[test]
fn gpu_alignment_non_gpu_struct_keeps_regular_derives() {
    // A struct that does NOT flow through a parallel combinator must
    // still get the regular T107 derive list (Clone, PartialEq, Debug,
    // and Hash when all fields are Hash-safe). This is the
    // non-regression guarantee: non-GPU structs see ZERO change from T50.
    let decls = program(vec![
        struct_decl("Color", vec![("r", "Int"), ("g", "Int"), ("b", "Int")]),
        empty_func_with_stmts("f", vec![expr_stmt(ident_expr("v"))]),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    // Hash-safe (all Int fields) → must include Hash.
    let expected = "#[derive(Clone, PartialEq, Hash, Debug)]\npub struct Color";
    assert!(
        rust.contains(expected),
        "non-GPU Hash-safe struct must keep regular derives + Hash. \
         Expected substring:\n{expected}\n\nGot:\n{rust}"
    );
    // Negative asserts: GPU-only attributes absent.
    assert!(!rust.contains("#[repr(C)]"));
    assert!(!rust.contains("Copy"));
    assert!(!rust.contains("bytemuck::Pod"));
}

#[test]
fn gpu_alignment_only_gpu_struct_affected_when_two_structs_in_program() {
    // A program with TWO structs: one flows through a parallel closure
    // (Point), the other doesn't (Color). Only Point gets the GPU
    // treatment; Color stays in the regular path.
    let decls = program(vec![
        struct_decl("Color", vec![("r", "Int")]),
        struct_decl("Point", vec![("x", "Float")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let rust = generate_rust(&decls).expect("codegen should succeed");
    // Point is GPU-bound → has all GPU attributes.
    assert!(
        rust.contains("#[derive(Clone, Copy, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable)]\n#[repr(C)]\npub struct Point"),
        "Point (GPU-bound) must have full GPU attribute set. Got:\n{rust}"
    );
    // Color is NOT GPU-bound → regular T107 path (no Copy, no Pod, no repr(C)).
    // All Int fields → Hash-safe → includes Hash.
    assert!(
        rust.contains("#[derive(Clone, PartialEq, Hash, Debug)]\npub struct Color"),
        "Color (non-GPU) must keep regular derives. Got:\n{rust}"
    );
}

// =====================================================================
// 6. DETERMINISM — same input yields byte-identical output
// =====================================================================

#[test]
fn gpu_alignment_deterministic_across_repeated_codegen_runs() {
    // The T29 flaky-test lesson: never rely on hash-seed-dependent
    // iteration for codegen output. Run the same input through
    // generate_rust 5 times and assert byte-identical output.
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float"), ("y", "Float")]),
        struct_decl("Color", vec![("r", "Int")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
            ))],
        ),
    ]);
    let mut outputs: Vec<String> = Vec::with_capacity(5);
    for _ in 0..5 {
        outputs.push(generate_rust(&decls).expect("codegen should succeed"));
    }
    for (i, o) in outputs.iter().enumerate() {
        assert_eq!(
            *o, outputs[0],
            "non-deterministic codegen output at run {i}: GPU-alignment + repr(C) \
             must be stable across repeated invocations"
        );
    }
}

// =====================================================================
// 7. DETECTOR-LEVEL TESTS — direct analysis assertions
// =====================================================================

#[test]
fn gpu_alignment_detector_returns_empty_set_for_no_structs() {
    // A program with parallel calls but NO user structs → empty result.
    let decls = program(vec![empty_func_with_stmts(
        "f",
        vec![expr_stmt(method_call(
            ident_expr("v"),
            "par_map",
            vec![untyped_closure("x", ident_expr("x"))],
        ))],
    )]);
    let found = analyze_gpu_alignment(&decls);
    assert!(
        found.is_empty(),
        "no user structs ⇒ empty set; got {found:?}"
    );
}

#[test]
fn gpu_alignment_detector_finds_multiple_structs() {
    // Two parallel calls, two different structs:
    //   v.par_map({ p: Point => ... })
    //   u.par_filter({ c: Color => ... })
    // Both must be reported (BTreeSet, ascending iteration).
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]),
        struct_decl("Color", vec![("r", "Int")]),
        empty_func_with_stmts(
            "f",
            vec![
                expr_stmt(method_call(
                    ident_expr("v"),
                    "par_map",
                    vec![typed_closure("p", named_ty("Point"), ident_expr("p"))],
                )),
                expr_stmt(method_call(
                    ident_expr("u"),
                    "par_filter",
                    vec![typed_closure("c", named_ty("Color"), ident_expr("c"))],
                )),
            ],
        ),
    ]);
    let found = analyze_gpu_alignment(&decls);
    assert!(
        found.contains("Point"),
        "Point must be detected; got {found:?}"
    );
    assert!(
        found.contains("Color"),
        "Color must be detected; got {found:?}"
    );
    assert_eq!(found.len(), 2, "exactly 2 structs expected; got {found:?}");
    // BTreeSet iteration is ascending — collect into a Vec to verify.
    let ordered: Vec<&String> = found.iter().collect();
    assert_eq!(
        ordered,
        vec![&"Color".to_string(), &"Point".to_string()],
        "BTreeSet iteration must be ascending by name"
    );
}

#[test]
fn gpu_alignment_detector_ignores_non_user_struct_names() {
    // A param annotation `x: Int` names a builtin, not a user struct.
    // `Int` must NOT appear in the gpu-bound set (builtins have their
    // own fixed lowering and don't take user attributes).
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]), // exists but unused in par
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                ident_expr("v"),
                "par_map",
                vec![typed_closure("x", named_ty("Int"), ident_expr("x"))],
            ))],
        ),
    ]);
    let found = analyze_gpu_alignment(&decls);
    assert!(
        !found.contains("Int"),
        "builtin type Int must NOT be in gpu-bound set; got {found:?}"
    );
    assert!(
        !found.contains("Point"),
        "Point is not used in any parallel closure; got {found:?}"
    );
    assert!(found.is_empty());
}

#[test]
fn gpu_alignment_combinator_list_matches_race_analysis() {
    // Single-source-of-truth contract: this crate's GPU-dispatch
    // combinator list MUST match race_analysis::PARALLEL_COMBINATORS.
    // If they ever diverge, race detection + alignment will disagree
    // (e.g. a struct flagged gpu-bound but no race check applied to its
    // closure, or vice versa) with potentially-undefined-behaviour
    // consequences. This test pins the contract.
    //
    // We can't access gpu_alignment::GPU_DISPATCH_COMBINATORS directly
    // (private), but we can verify BEHAVIOURAL parity: every name in
    // PARALLEL_COMBINATORS must trigger detection, and every name NOT
    // in PARALLEL_COMBINATORS must not.
    for combinator in PARALLEL_COMBINATORS {
        let combinator = *combinator;
        let decls = program(vec![
            struct_decl("Probe", vec![("x", "Int")]),
            empty_func_with_stmts(
                "f",
                vec![expr_stmt(method_call(
                    ident_expr("v"),
                    combinator,
                    vec![typed_closure("p", named_ty("Probe"), ident_expr("p"))],
                ))],
            ),
        ]);
        let found = analyze_gpu_alignment(&decls);
        assert!(
            found.contains("Probe"),
            "combinator `{combinator}` from PARALLEL_COMBINATORS must trigger detection; \
             got {found:?}"
        );
    }
    // Negative: a few non-parallel combinators must NOT trigger.
    for non_parallel in &["map", "filter", "reduce", "for_each", "collect"] {
        let non_parallel = *non_parallel;
        let decls = program(vec![
            struct_decl("Probe", vec![("x", "Int")]),
            empty_func_with_stmts(
                "f",
                vec![expr_stmt(method_call(
                    ident_expr("v"),
                    non_parallel,
                    vec![typed_closure("p", named_ty("Probe"), ident_expr("p"))],
                ))],
            ),
        ]);
        let found = analyze_gpu_alignment(&decls);
        assert!(
            !found.contains("Probe"),
            "non-parallel combinator `{non_parallel}` must NOT trigger detection; \
             got {found:?}"
        );
    }
}

// =====================================================================
// 8. T26 BACKWARDS COMPAT — mark_struct_repr_c still works (no Pod)
// =====================================================================

#[test]
fn gpu_alignment_t26_manual_mark_struct_repr_c_still_emits_repr_c_without_pod() {
    // The T26 manual hook (`RustCodegen::mark_struct_repr_c`) predates
    // the T50 gpu-alignment analysis. It MUST keep working: calling it
    // emits #[repr(C)] WITHOUT the bytemuck derives (the manual hook is
    // "user wants repr(C) only", not "user wants full GPU Pod").
    //
    // This guarantees T26's existing struct_codegen test
    // (`struct_codegen_repr_c_emitted_when_struct_marked`) continues to
    // pass byte-identically.
    let sd = struct_decl_decl("Tagged", vec![("n", "Int")]);
    let mut codegen = RustCodegen::new();
    codegen.mark_struct_repr_c("Tagged");
    let file = codegen
        .generate(&[Decl::StructDecl(sd)])
        .expect("codegen should succeed");
    let rust = buff_lang_codegen_rust::format_file(&file);
    assert!(
        rust.contains("#[repr(C)]"),
        "manual mark_struct_repr_c must still emit #[repr(C)]. Got:\n{rust}"
    );
    // The manual hook uses the regular struct path (with Hash derive
    // when Hash-safe) + repr(C). It does NOT add Pod/Copy.
    assert!(
        !rust.contains("bytemuck::Pod"),
        "manual mark_struct_repr_c must NOT add bytemuck::Pod (T26 backwards compat). \
         Got:\n{rust}"
    );
}

// =====================================================================
// 9. CHAINED COMBINATORS — u.par_filter({...}).par_map({...})
// =====================================================================

#[test]
fn gpu_alignment_chained_par_combinators_detect_both_signals() {
    // A chained call where the outer call's closure takes one struct
    // and the inner call's closure produces another. Both signals
    // must be detected (the walker recurses through the receiver).
    //
    //   u.par_filter({ p: Point => true })
    //    .par_map({ p: Point => Color { r: 0 } })
    let decls = program(vec![
        struct_decl("Point", vec![("x", "Float")]),
        struct_decl("Color", vec![("r", "Int")]),
        empty_func_with_stmts(
            "f",
            vec![expr_stmt(method_call(
                method_call(
                    ident_expr("u"),
                    "par_filter",
                    vec![typed_closure(
                        "p",
                        named_ty("Point"),
                        Expr::Literal(Literal::Bool(true), span()),
                    )],
                ),
                "par_map",
                vec![untyped_closure(
                    "p",
                    struct_init("Color", vec![("r", Expr::Literal(Literal::Int(0), span()))]),
                )],
            ))],
        ),
    ]);
    let found = analyze_gpu_alignment(&decls);
    assert!(
        found.contains("Point"),
        "Point must be detected via par_filter; got {found:?}"
    );
    assert!(
        found.contains("Color"),
        "Color must be detected via par_map body struct-init; got {found:?}"
    );
}

// =====================================================================
// 10. PROGRAM WITHOUT ANY STRUCTS — empty set, no false positives
// =====================================================================

#[test]
fn gpu_alignment_no_structs_in_program_yields_empty_set_and_no_repr_c() {
    // A program with a parallel call but no struct declarations at all.
    // The detector must return an empty set AND the generated Rust
    // must contain no #[repr(C)] (no struct to attach it to).
    let decls = program(vec![empty_func_with_stmts(
        "f",
        vec![expr_stmt(method_call(
            ident_expr("v"),
            "par_map",
            vec![untyped_closure("x", ident_expr("x"))],
        ))],
    )]);
    let found = analyze_gpu_alignment(&decls);
    assert!(found.is_empty());
    let rust = generate_rust(&decls).expect("codegen should succeed");
    assert!(!rust.contains("#[repr(C)]"));
    assert!(!rust.contains("bytemuck"));
}
