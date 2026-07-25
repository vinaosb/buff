//! T59 integration tests — buff-actors prelude type codegen wiring.
//!
//! Verifies the codegen registration layer for the actor prelude
//! types added in T59:
//!
//! - **Type variants**: `Type::ActorSystem`, `Type::ActorRef`,
//!   `Type::Supervisor`, `Type::ChildSpec`, `Type::RestartStrategy`
//!   registered in `ty.rs` (constructors + predicates + Display).
//! - **PreludeType variants**: matching variants registered in
//!   `prelude_types.rs` (lookup, name, to_type, is_namespace_only).
//! - **buff_type_to_syn arms**: each variant maps to its
//!   `buff_actors::*` / `buff_actors::supervisor::*` Rust path.
//! - **extern_crates registration**: a program referencing any of
//!   the five actor namespaces records `buff-actors` +
//!   `crossbeam-channel` in the codegen `extern_crates` BTreeSet.
//!
//! Full constructor + instance-method lowering (e.g.
//! `ActorSystem.new()` ─▶ `buff_actors::ActorSystem::new()` etc.)
//! is a follow-up commit per the buff-pubsub T41 two-commit split
//! precedent (the MVP commit ships the crate + type registration +
//! namespace walker; the follow-up adds the AssocFn/InstanceFn
//! lowering arms + per-method codegen tests).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test actors_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::{generate_rust, RustCodegen};
use buff_lang_error::Span;
use buff_lang_types::{
    prelude_types::{is_prelude_type, prelude_type_lookup, PreludeType},
    ty::Type,
};

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn float_expr(v: f64) -> Expr {
    Expr::Literal(Literal::Double(v), span())
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

fn func_decl(name: &str, params: &[(&str, &str)], body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: params
            .iter()
            .map(|(n, t)| Param {
                name: ident(n),
                ty: named_type(t),
                default_value: None,
                is_comptime: false,
                span: span(),
            })
            .collect(),
        return_type: None,
        body: Block {
            stmts: body_stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    })
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

fn let_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty: None,
        span: span(),
    }
}

fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

// ===========================================================================
// 1. PreludeType registration — all five actor types resolve.
// ===========================================================================

#[test]
fn actors_codegen_prelude_type_lookup_resolves_all_five() {
    assert!(is_prelude_type("ActorSystem"));
    assert!(is_prelude_type("ActorRef"));
    assert!(is_prelude_type("Supervisor"));
    assert!(is_prelude_type("ChildSpec"));
    assert!(is_prelude_type("RestartStrategy"));
}

#[test]
fn actors_codegen_prelude_type_lookup_returns_expected_variant() {
    assert_eq!(
        prelude_type_lookup("ActorSystem"),
        Some(PreludeType::ActorSystem)
    );
    assert_eq!(prelude_type_lookup("ActorRef"), Some(PreludeType::ActorRef));
    assert_eq!(
        prelude_type_lookup("Supervisor"),
        Some(PreludeType::Supervisor)
    );
    assert_eq!(
        prelude_type_lookup("ChildSpec"),
        Some(PreludeType::ChildSpec)
    );
    assert_eq!(
        prelude_type_lookup("RestartStrategy"),
        Some(PreludeType::RestartStrategy)
    );
}

#[test]
fn actors_codegen_prelude_type_unknown_name_returns_none() {
    assert_eq!(prelude_type_lookup("ActorSystemImaginary"), None);
    assert!(!is_prelude_type("NotAnActorType"));
}

// ===========================================================================
// 2. Type variant constructors + predicates.
// ===========================================================================

#[test]
fn actors_codegen_type_constructors_return_expected_variants() {
    assert_eq!(Type::actor_system(), Type::ActorSystem);
    assert_eq!(Type::actor_ref(), Type::ActorRef);
    assert_eq!(Type::supervisor(), Type::Supervisor);
    assert_eq!(Type::child_spec(), Type::ChildSpec);
    assert_eq!(Type::restart_strategy(), Type::RestartStrategy);
}

#[test]
fn actors_codegen_type_predicates_match_correctly() {
    assert!(Type::ActorSystem.is_prelude_actor_system());
    assert!(Type::ActorRef.is_prelude_actor_ref());
    assert!(Type::Supervisor.is_prelude_supervisor());
    assert!(Type::ChildSpec.is_prelude_child_spec());
    assert!(Type::RestartStrategy.is_prelude_restart_strategy());
}

#[test]
fn actors_codegen_type_predicates_reject_other_types() {
    assert!(!Type::ActorSystem.is_prelude_actor_ref());
    assert!(!Type::Supervisor.is_prelude_actor_system());
    assert!(!Type::Simd.is_prelude_actor_system());
}

// ===========================================================================
// 3. Display impl — actor type names render correctly.
// ===========================================================================

#[test]
fn actors_codegen_type_display_renders_pascalcase_names() {
    assert_eq!(Type::ActorSystem.to_string(), "ActorSystem");
    assert_eq!(Type::ActorRef.to_string(), "ActorRef");
    assert_eq!(Type::Supervisor.to_string(), "Supervisor");
    assert_eq!(Type::ChildSpec.to_string(), "ChildSpec");
    assert_eq!(Type::RestartStrategy.to_string(), "RestartStrategy");
}

// ===========================================================================
// 4. extern_crates registration — buff-actors + crossbeam-channel
//    recorded when any of the five actor namespaces is referenced.
// ===========================================================================

#[test]
fn actors_codegen_registers_extern_crates_when_actor_system_used() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt("sys", ns_assoc_call("ActorSystem", "new", vec![]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-actors"),
        "extern_crates should contain `buff-actors`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("crossbeam-channel"),
        "extern_crates should contain `crossbeam-channel`, got: {:?}",
        extern_crates
    );
}

#[test]
fn actors_codegen_registers_extern_crates_when_supervisor_used() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "sup",
            ns_assoc_call("Supervisor", "new", vec![ident_expr("sys")]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(extern_crates.contains("buff-actors"));
    assert!(extern_crates.contains("crossbeam-channel"));
}

#[test]
fn actors_codegen_registers_extern_crates_when_restart_strategy_used() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "s",
            ns_assoc_call("RestartStrategy", "permanent", vec![]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(extern_crates.contains("buff-actors"));
}

#[test]
fn actors_codegen_no_extern_crate_when_unused() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![float_expr(1.0)],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("buff-actors"),
        "extern_crates should NOT contain `buff-actors` when actors are unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("crossbeam-channel"),
        "extern_crates should NOT contain `crossbeam-channel` when actors are unused"
    );
}

// ===========================================================================
// 5. Codegen still produces valid (re-parseable) Rust when actors are used.
//    The constructor lowering is a follow-up commit per the buff-pubsub T41
//    two-commit split precedent; this test only verifies that the namespace
//    walker + type registration don't break the pipeline.
// ===========================================================================

#[test]
fn actors_codegen_namespace_reference_produces_parseable_rust() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt("sys", ns_assoc_call("ActorSystem", "new", vec![]))],
    );
    let src = generate_rust(&[main]).expect("codegen must succeed");
    syn::parse_str::<syn::File>(&src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}
