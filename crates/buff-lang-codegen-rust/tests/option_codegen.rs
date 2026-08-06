//! T28 integration tests — Rust codegen for the prelude `Option<T>` enum
//! variants `None` and `Some(x)`.
//!
//! Coverage:
//!
//! - `None` (a bare `Expr::Ident("None")`) lowers to Rust `None` (Rust's
//!   std `Option::None` is in scope by default).
//! - `Some(x)` (a `Expr::FuncCall { callee: Ident("Some"), args: [x] }`)
//!   lowers to Rust `Some(x)` (the std `Option::Some` constructor).
//! - `Some(42)` → `Some(42)`, `Some("hi")` → `Some("hi")`.
//! - End-to-end: `let x = Some(42)` re-parses as valid Rust.
//! - `match opt { Some(x) => x, None => 0 }` re-parses (safe-unwrap via the
//!   T27 match mechanism; `if let` is T72, deferred).
//!
//! These tests build the Buff AST by hand (mirroring the T27 `enum_codegen`
//! suite). The codegen needs NO special-casing for None/Some: a bare `None`
//! ident lowers to `syn::ExprPath` (`None`), and a `Some(x)` call lowers via
//! the regular `Expr::FuncCall` path (it is NOT a prelude function, so the
//! callee becomes a path and the args form a Rust call → `Some(x)`). Both
//! map 1:1 to Rust's std `Option` because Buff deliberately mirrors Rust's
//! Option spelling.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test option_codegen
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, MatchArm, Pattern, Stmt};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

/// `Some(arg)` as the parser-realistic FuncCall shape.
fn some_expr(arg: Expr) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr("Some")),
        args: vec![arg],
        span: span(),
    }
}

/// `None` as a bare ident.
fn none_expr() -> Expr {
    ident_expr("None")
}

fn block(stmts: Vec<Stmt>) -> Block {
    Block {
        stmts,
        span: span(),
    }
}

fn func_with_stmts(name: &str, stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: Vec::new(),
        return_type: None,
        body: block(stmts),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    })
}

fn codegen_stmts(stmts: Vec<Stmt>) -> String {
    generate_rust(&[func_with_stmts("f", stmts)]).expect("codegen must succeed")
}

fn codegen_one_expr(expr: Expr) -> String {
    codegen_stmts(vec![Stmt::ExprStmt(expr, span())])
}

fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// None codegen
// ---------------------------------------------------------------------------

#[test]
fn none_codegen_lowers_to_rust_none() {
    // `None` (bare ident) -> Rust `None` (std Option::None in scope).
    let src = codegen_one_expr(none_expr());
    assert!(
        src.contains("None"),
        "expected `None` in generated Rust: {src}"
    );
    must_reparse(&src);
}

#[test]
fn none_let_binding_codegen_snapshot() {
    // `let x = None` -> `let x = None;`.
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("x"),
        value: none_expr(),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    insta::assert_snapshot!(src, @r###"
    fn f() {
        let x = None;
    }
    "###);
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Some(x) codegen
// ---------------------------------------------------------------------------

#[test]
fn some_int_codegen_lowers_to_rust_some_call() {
    // `Some(42)` -> Rust `Some(42)`.
    let src = codegen_one_expr(some_expr(int_expr(42)));
    assert!(
        src.contains("Some(42)"),
        "expected `Some(42)` in generated Rust: {src}"
    );
    must_reparse(&src);
}

#[test]
fn some_string_codegen_lowers_to_rust_some_call() {
    // `Some("hi")` -> Rust `Some(\"hi\")`.
    let src = codegen_one_expr(some_expr(string_expr("hi")));
    assert!(
        src.contains("Some(\"hi\".to_string())"),
        "expected `Some(\"hi\".to_string())` in generated Rust: {src}"
    );
    must_reparse(&src);
}

#[test]
fn some_let_binding_codegen_snapshot() {
    // `let x = Some(42)` -> `let x: Option<i64> = Some(42);`. The codegen
    // runs the TypeInferencer and emits the inferred `Option<Int<64>>`
    // annotation (T28 made `Some(x)` infer `Option<T>`), so Rust doesn't
    // need to infer the inner type. (`None` stays unannotated because its
    // inner is Unknown — there's no concrete inner to write.)
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("x"),
        value: some_expr(int_expr(42)),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    insta::assert_snapshot!(src, @r###"
    fn f() {
        let x: Option<i64> = Some(42);
    }
    "###);
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Safe-unwrap via match (T27 mechanism; if-let is T72, deferred).
// ---------------------------------------------------------------------------

#[test]
fn match_on_option_with_some_binding_and_none_codegen() {
    // `match opt { Some(x) => x, None => 0 }` lowers to the same Rust shape.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("opt")),
        arms: vec![
            MatchArm {
                pattern: Pattern::Variant {
                    enum_name: ident(""),
                    variant: ident("Some"),
                    subpatterns: vec![Pattern::Ident(ident("x"), span())],
                    span: span(),
                },
                guard: None,
                body: block(vec![Stmt::ExprStmt(ident_expr("x"), span())]),
                span: span(),
            },
            MatchArm {
                pattern: Pattern::Ident(ident("None"), span()),
                guard: None,
                body: block(vec![Stmt::ExprStmt(int_expr(0), span())]),
                span: span(),
            },
        ],
        span: span(),
    };
    let src = codegen_one_expr(mt);
    assert!(src.contains("match opt {"), "expected match in: {src}");
    assert!(
        src.contains("Some(x)"),
        "expected `Some(x)` pattern in: {src}"
    );
    assert!(src.contains("None =>"), "expected `None =>` arm in: {src}");
    must_reparse(&src);
}

#[test]
fn end_to_end_option_program_reparse() {
    // A small program exercising None, Some, and match — all in one file.
    // The generated source must re-parse as valid Rust (syn-level).
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("a"),
            value: none_expr(),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::LetDecl {
            name: ident("b"),
            value: some_expr(int_expr(7)),
            mutable: false,
            ty: None,
            span: span(),
        },
    ];
    let src = codegen_stmts(stmts);
    assert!(src.contains("let a = None;"), "None binding in: {src}");
    // Some(7) infers Option<Int<64>> -> codegen emits `Option<i64>` annotation.
    assert!(
        src.contains("let b: Option<i64> = Some(7);"),
        "Some binding with inferred Option annotation in: {src}"
    );
    must_reparse(&src);
}
