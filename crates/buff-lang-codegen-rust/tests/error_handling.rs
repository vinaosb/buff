//! T30 integration tests — Rust codegen for the error-handling prelude:
//! `Result<T, E>`, the `?` operator, the `Error("msg")` constructor, and
//! custom error enums.
//!
//! Coverage:
//!
//! - `Result<T, E>` type annotation → Rust `Result<T, E>` (1:1, std Result).
//! - `Ok(x)` / `Err(e)` constructors → Rust `Ok(x)` / `Err(e)` (1:1).
//! - `expr?` (the `Expr::Try` node) → Rust's native `?` operator (`expr?`).
//! - `return Error("msg")` → `return Err(Error::new("msg"))`, with the
//!   builtin `Error` struct emitted on-demand (mirrors T24 Matrix pattern).
//! - Custom error enums (`enum MyError { NotFound, Invalid(String) }`) reuse
//!   the T27 `EnumDecl` codegen — these tests confirm the path still works
//!   for error-shaped enums.
//! - `match r { Ok(v) => v, Err(e) => ... }` on a Result re-parses.
//!
//! These tests build the Buff AST by hand (the codegen is the system under
//! test; the parser's `?` postfix is exercised in `parse_question_op`).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test error_handling
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::{EnumDecl, EnumVariant, FuncDecl};
use buff_lang_ast::{Decl, Expr, Literal, MatchArm, Pattern, Stmt, TypeRef};
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

/// Build a named type reference (`Int`, `String`, `Error`, ...).
fn named_ty(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build a generic type reference `Base<arg1, arg2, ...>`.
fn generic_ty(base: &str, args: &[&str]) -> TypeRef {
    TypeRef::Generic {
        base: Box::new(named_ty(base)),
        args: args.iter().map(|t| named_ty(t)).collect(),
        span: span(),
    }
}

/// `Ok(arg)` / `Err(arg)` as the parser-realistic FuncCall shape.
fn variant_call(name: &str, arg: Expr) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args: vec![arg],
        span: span(),
    }
}

/// `Error(arg)` as the prelude error constructor shape.
fn error_call(arg: Expr) -> Expr {
    variant_call("Error", arg)
}

/// `expr?` as the Expr::Try node.
fn try_expr(inner: Expr) -> Expr {
    Expr::Try {
        expr: Box::new(inner),
        span: span(),
    }
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
        span: span(),
    })
}

/// Build a function with an explicit return type.
fn func_typed(name: &str, ret_ty: TypeRef, stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: Vec::new(),
        return_type: Some(ret_ty),
        body: block(stmts),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
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
// Result<T, E> type-annotation codegen
// ---------------------------------------------------------------------------

#[test]
fn error_handling_result_type_annotation_lowers_to_rust_result() {
    // `func read(): Result<String, Error> { ... }` → Rust fn returning
    // `Result<String, Error>`. The generic-lowering path handles Result 1:1
    // (std Result is in scope by default).
    let ret = generic_ty("Result", &["String", "Error"]);
    let decl = func_typed("read", ret, vec![Stmt::Return(None, span())]);
    let src = generate_rust(&[decl]).expect("codegen");
    assert!(
        src.contains("-> Result<String, Error>"),
        "expected Result return type in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_handling_result_int_int_annotation_snapshot() {
    // Pin the exact formatting of a `Result<Int, Int>` return annotation.
    let ret = generic_ty("Result", &["Int", "Int"]);
    let decl = func_typed("g", ret, vec![Stmt::Return(None, span())]);
    let src = generate_rust(&[decl]).expect("codegen");
    insta::assert_snapshot!(src, @r###"
    fn g() -> Result<i64, i64> {
        return;
    }
    "###);
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Ok(x) / Err(e) constructor codegen (1:1 with Rust std Result)
// ---------------------------------------------------------------------------

#[test]
fn error_handling_ok_int_lowers_to_rust_ok_call() {
    // `Ok(42)` → Rust `Ok(42)`.
    let src = codegen_one_expr(variant_call("Ok", int_expr(42)));
    assert!(
        src.contains("Ok(42)"),
        "expected `Ok(42)` in generated Rust: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_handling_err_string_lowers_to_rust_err_call() {
    // `Err("nope")` → Rust `Err(\"nope\")`.
    let src = codegen_one_expr(variant_call("Err", string_expr("nope")));
    assert!(
        src.contains("Err(\"nope\")"),
        "expected `Err(\"nope\")` in generated Rust: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_handling_ok_let_binding_emits_result_annotation() {
    // `let x = Ok(42)` — Ok(42) infers `Result<Int<64>, Unknown>`. The Err
    // inner is Unknown, so `buff_type_to_syn` returns None for the whole
    // Result and codegen falls back to an unannotated `let x = Ok(42);`
    // (Rust infers the Result<i64, _> type from the Ok payload). This
    // mirrors how Option's `None` (Unknown inner) stays unannotated.
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("x"),
        value: variant_call("Ok", int_expr(42)),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    assert!(
        src.contains("let x = Ok(42);"),
        "expected unannotated Ok binding (Unknown Err inner): {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// `?` operator codegen (Expr::Try → Rust native `?`)
// ---------------------------------------------------------------------------

#[test]
fn error_handling_try_op_lowers_to_rust_native_question() {
    // `f()?` → Rust `f()?`. The codegen uses Rust's NATIVE `?` operator
    // (option (a) in the task), not the explicit match desugaring.
    let inner = Expr::FuncCall {
        callee: Box::new(ident_expr("f")),
        args: Vec::new(),
        span: span(),
    };
    let src = codegen_one_expr(try_expr(inner));
    assert!(
        src.contains("f()?"),
        "expected native `f()?` in generated Rust: {src}"
    );
    // The explicit match desugaring must NOT appear.
    assert!(
        !src.contains("match"),
        "must use native `?`, not explicit match: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_handling_try_op_on_ident_snapshot() {
    // `x?` → Rust `x?`. Pin the exact format.
    let src = codegen_one_expr(try_expr(ident_expr("x")));
    insta::assert_snapshot!(src, @r###"
    fn f() {
        x?;
    }
    "###);
    must_reparse(&src);
}

#[test]
fn error_handling_try_op_in_let_binding_snapshot() {
    // `let y = parse()?` → `let y = parse()?;`.
    let parse_call = Expr::FuncCall {
        callee: Box::new(ident_expr("parse")),
        args: Vec::new(),
        span: span(),
    };
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("y"),
        value: try_expr(parse_call),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    insta::assert_snapshot!(src, @r###"
    fn f() {
        let y = parse()?;
    }
    "###);
    must_reparse(&src);
}

#[test]
fn error_handling_try_op_chained_lowers_to_double_question() {
    // `f()??` → Rust `f()??`. Chained `?` works because parse_postfix loops.
    let inner = Expr::FuncCall {
        callee: Box::new(ident_expr("f")),
        args: Vec::new(),
        span: span(),
    };
    let src = codegen_one_expr(try_expr(try_expr(inner)));
    assert!(
        src.contains("f()??"),
        "expected chained `f()??` in generated Rust: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// `return Error("msg")` → `return Err(Error::new("msg"))` + Error struct emit
// ---------------------------------------------------------------------------

#[test]
fn error_handling_return_error_maps_to_err_error_new() {
    // `return Error("fail")` → `return Err(Error::new("fail"))`.
    let src = codegen_stmts(vec![Stmt::Return(
        Some(error_call(string_expr("fail"))),
        span(),
    )]);
    assert!(
        src.contains("return Err(Error::new(\"fail\"));"),
        "expected `return Err(Error::new(\"fail\"));` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_handling_error_struct_emitted_on_demand() {
    // When `Error(...)` is used, the builtin `Error` struct (+ new + Display
    // + std::error::Error impl) is emitted. Mirrors Matrix emit-on-demand.
    let src = codegen_stmts(vec![Stmt::Return(
        Some(error_call(string_expr("boom"))),
        span(),
    )]);
    assert!(
        src.contains("pub struct Error"),
        "expected builtin `Error` struct emission in: {src}"
    );
    assert!(
        src.contains("impl Error"),
        "expected `impl Error` block with `new` fn in: {src}"
    );
    assert!(
        src.contains("impl std::error::Error for Error"),
        "expected `std::error::Error` impl in: {src}"
    );
    assert!(
        src.contains("impl std::fmt::Display for Error"),
        "expected `Display` impl in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_handling_error_struct_not_emitted_when_unused() {
    // A program that never calls `Error(...)` must NOT emit the Error struct
    // (emit-on-demand keeps non-error programs clean). Here we use Ok(1) so
    // there's a Result value but no Error constructor.
    let src = codegen_stmts(vec![Stmt::Return(
        Some(variant_call("Ok", int_expr(1))),
        span(),
    )]);
    assert!(
        !src.contains("pub struct Error"),
        "Error struct must NOT be emitted when Error() is unused: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_handling_return_error_snapshot() {
    // Pin the full generated program for `return Error("nope")`.
    let src = codegen_stmts(vec![Stmt::Return(
        Some(error_call(string_expr("nope"))),
        span(),
    )]);
    insta::assert_snapshot!(src, @r###"
    #[derive(Clone, Debug)]
    pub struct Error {
        pub message: String,
    }
    impl Error {
        pub fn new(message: impl Into<String>) -> Self {
            Self { message: message.into() }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)
        }
    }
    impl std::error::Error for Error {}
    fn f() {
        return Err(Error::new("nope"));
    }
    "###);
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// The task's signature snapshot: `func read(): Result<String, Error> { ... }`
// ---------------------------------------------------------------------------

#[test]
fn error_handling_read_func_result_string_error_snapshot() {
    // The task's RED snapshot:
    //   `func read(): Result<String, Error> { return Error("fail") }`
    // lowers to a Rust fn returning `Result<String, Error>` whose body is
    // `return Err(Error::new("fail"));`, with the Error struct emitted.
    let ret = generic_ty("Result", &["String", "Error"]);
    let decl = func_typed(
        "read",
        ret,
        vec![Stmt::Return(Some(error_call(string_expr("fail"))), span())],
    );
    let src = generate_rust(&[decl]).expect("codegen");
    assert!(
        src.contains("fn read() -> Result<String, Error>"),
        "expected typed read fn in: {src}"
    );
    assert!(
        src.contains("return Err(Error::new(\"fail\"));"),
        "expected Err(Error::new) body in: {src}"
    );
    assert!(
        src.contains("pub struct Error"),
        "expected Error struct emission in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Custom error enums (T27 EnumDecl path; confirms error-shaped enums compile)
// ---------------------------------------------------------------------------

/// Build a unit variant.
fn unit_variant(name: &str) -> EnumVariant {
    EnumVariant {
        name: ident(name),
        data: None,
        span: span(),
    }
}

/// Build a tuple variant with one or more payload types.
fn tuple_variant(name: &str, payload_tys: &[&str]) -> EnumVariant {
    EnumVariant {
        name: ident(name),
        data: Some(payload_tys.iter().map(|t| named_ty(t)).collect()),
        span: span(),
    }
}

#[test]
fn error_handling_custom_error_enum_codegen_snapshot() {
    // `enum MyError { NotFound, Invalid(String) }` → Rust enum with the same
    // shape (reuses the T27 EnumDecl codegen). Full `std::error::Error`
    // trait derivation is deferred to a later task (documented in
    // decisions.md); the enum still compiles via `#[derive(Clone, Debug)]`.
    let e = EnumDecl {
        name: ident("MyError"),
        generics: Vec::new(),
        variants: vec![
            unit_variant("NotFound"),
            tuple_variant("Invalid", &["String"]),
        ],
        span: span(),
    };
    let src = generate_rust(&[Decl::EnumDecl(e)]).expect("codegen");
    insta::assert_snapshot!(src, @r###"
    #[derive(Clone, Debug)]
    pub enum MyError {
        NotFound,
        Invalid(String),
    }
    "###);
    must_reparse(&src);
}

#[test]
fn error_handling_custom_error_enum_then_function_reparse() {
    // A custom error enum followed by a function returning it must re-parse
    // as valid Rust. This exercises the T27 path end-to-end for error types.
    let e = EnumDecl {
        name: ident("MyErr"),
        generics: Vec::new(),
        variants: vec![unit_variant("NotFound"), tuple_variant("Bad", &["String"])],
        span: span(),
    };
    let f = func_typed(
        "lookup",
        named_ty("MyErr"),
        vec![Stmt::Return(Some(ident_expr("NotFound")), span())],
    );
    let src = generate_rust(&[Decl::EnumDecl(e), f]).expect("codegen");
    assert!(src.contains("pub enum MyErr"), "enum in: {src}");
    assert!(src.contains("fn lookup() -> MyErr"), "fn in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// match on Result (Ok/Err arms)
// ---------------------------------------------------------------------------

/// Build a `Pattern::Ident`.
fn ident_pat(name: &str) -> Pattern {
    Pattern::Ident(ident(name), span())
}

/// Build a `Pattern::Variant` with subpatterns.
fn variant_pat(variant: &str, subpats: Vec<Pattern>) -> Pattern {
    Pattern::Variant {
        enum_name: ident(""),
        variant: ident(variant),
        subpatterns: subpats,
        span: span(),
    }
}

#[test]
fn error_handling_match_on_result_with_ok_err_arms() {
    // `match r { Ok(v) => v, Err(_) => 0 }` lowers to the same Rust shape.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("r")),
        arms: vec![
            MatchArm {
                pattern: variant_pat("Ok", vec![ident_pat("v")]),
                body: block(vec![Stmt::ExprStmt(ident_expr("v"), span())]),
                span: span(),
            },
            MatchArm {
                pattern: variant_pat("Err", vec![Pattern::Wildcard(span())]),
                body: block(vec![Stmt::ExprStmt(int_expr(0), span())]),
                span: span(),
            },
        ],
        span: span(),
    };
    let src = codegen_one_expr(mt);
    assert!(src.contains("match r {"), "expected match in: {src}");
    assert!(src.contains("Ok(v)"), "expected `Ok(v)` pattern in: {src}");
    assert!(
        src.contains("Err(_)"),
        "expected `Err(_)` pattern in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// End-to-end: a full error-handling program re-parses as valid Rust.
// ---------------------------------------------------------------------------

#[test]
fn error_handling_end_to_end_program_reparse() {
    // Combines: Error struct (auto-emitted), a Result-returning fn that
    // propagates with `?`, and a `return Error(...)`. Must re-parse.
    let ret = generic_ty("Result", &["String", "Error"]);
    let body = vec![
        Stmt::LetDecl {
            name: ident("x"),
            value: try_expr(ident_expr("inner")),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::Return(Some(error_call(string_expr("done"))), span()),
    ];
    let decl = func_typed("run", ret, body);
    let src = generate_rust(&[decl]).expect("codegen");
    assert!(src.contains("pub struct Error"), "Error struct in: {src}");
    assert!(
        src.contains("fn run() -> Result<String, Error>"),
        "fn sig in: {src}"
    );
    assert!(src.contains("let x = inner?;"), "`?` in: {src}");
    assert!(
        src.contains("return Err(Error::new(\"done\"));"),
        "Error return in: {src}"
    );
    must_reparse(&src);
}
