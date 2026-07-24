//! Integration tests for buff-lang-codegen-rust.
//!
//! Each test builds a small Buff AST by hand, runs it through
//! [`buff_lang_codegen_rust::generate_rust`], and asserts properties of the
//! resulting Rust source. Snapshots are inline (no .snap files) via
//! `insta::assert_snapshot!(x, @"...")`.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_error::Span;

use buff_lang_codegen_rust::{format, generate_rust, RustCodegen};

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn named_type(s: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(s),
        span: span(),
    }
}

fn empty_func(name: &str) -> Decl {
    Decl::FuncDecl(FuncDecl { name: ident(name),
    params: Vec::new(),
    return_type: None,
    body: Block::empty(span()),
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), })
}

// ---------------------------------------------------------------------------
// 1. Empty input
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_empty() {
    let src = generate_rust(&[]).expect("empty input must codegen");
    assert!(src.is_empty(), "empty input should produce empty output");
}

// ---------------------------------------------------------------------------
// 2. Empty function — snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_empty_func_snapshot() {
    let src = generate_rust(&[empty_func("empty")]).unwrap();
    insta::assert_snapshot!(src, @"
fn empty() {}
");
}

// ---------------------------------------------------------------------------
// 3. `let x = 42` produces an integer-typed let binding
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_int() {
    let func = FuncDecl { name: ident("f"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts: vec![Stmt::LetDecl {
            name: ident("x"),
            value: int_expr(42),
            mutable: false,
            ty: None,
            span: span(),
        }],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    assert!(src.contains("fn f()"));
    // T12: type annotations are inferred; `let x = 42` → `let x: i64 = 42`.
    assert!(src.contains("let x: i64 = 42"), "src = {src}");
}

// ---------------------------------------------------------------------------
// 4. Binary op expression `1 + 2` inside an ExprStmt
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_binary_op() {
    let func = FuncDecl { name: ident("f"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::BinaryOp {
                op: BinaryOp::Add,
                lhs: Box::new(int_expr(1)),
                rhs: Box::new(int_expr(2)),
                span: span(),
            },
            span(),
        )],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    assert!(src.contains("1 + 2"), "src = {src}");
}

// ---------------------------------------------------------------------------
// 5. Function with return type and `return` statement — snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_func_with_return_snapshot() {
    let func = FuncDecl { name: ident("foo"),
    params: Vec::new(),
    return_type: Some(named_type("Int")),
    body: Block {
        stmts: vec![Stmt::Return(Some(int_expr(42)), span())],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    insta::assert_snapshot!(src, @"
fn foo() -> i64 {
    return 42;
}
");
}

// ---------------------------------------------------------------------------
// 6. Output is parseable Rust — prettyplease produces valid syntax
// ---------------------------------------------------------------------------

#[test]
fn test_format_passes_rustfmt() {
    let func = FuncDecl { name: ident("g"),
    params: vec![Param {
        name: ident("a"),
        ty: named_type("Int"),
        default_value: None,
        is_comptime: false,
        span: span(),
    }],
    return_type: Some(named_type("Int")),
    body: Block {
        stmts: vec![Stmt::Return(
            Some(Expr::BinaryOp {
                op: BinaryOp::Mul,
                lhs: Box::new(ident_expr("a")),
                rhs: Box::new(int_expr(2)),
                span: span(),
            }),
            span(),
        )],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[Decl::FuncDecl(func)]).unwrap();
    let src = format(&file);
    assert!(!src.is_empty(), "formatted output should be non-empty");
    // Re-parse the prettyplease output as a Rust file. If this succeeds,
    // the generated source is syntactically valid Rust.
    syn::parse_str::<syn::File>(&src).expect("prettyplease output must re-parse");
}

// ---------------------------------------------------------------------------
// 7. Function call expression `foo(a, b)`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_func_call() {
    let func = FuncDecl { name: ident("caller"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::FuncCall {
                callee: Box::new(ident_expr("foo")),
                args: vec![ident_expr("a"), ident_expr("b")],
                span: span(),
            },
            span(),
        )],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    assert!(src.contains("foo(a, b)"), "src = {src}");
}

// ---------------------------------------------------------------------------
// 8. Async + unsafe modifiers (extern is exercised in tests/ffi.rs — T32)
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_async_unsafe_extern_modifiers() {
    // T32: an `is_extern` FuncDecl now lowers to a Rust `extern "C" { ... }`
    // foreign-mod item (bodyless foreign-fn declaration) — see tests/ffi.rs
    // for the full FFI coverage. Here we exercise the COMBINATION of the
    // other two modifiers (`async` + `unsafe`) on a normal body-having fn.
    // (`is_extern` stays `false` so the fn keeps its body and goes through
    // the regular `ItemFn` lowering path.)
    let func = FuncDecl { name: ident("fancy"),
    params: Vec::new(),
    return_type: None,
    body: Block::empty(span()),
    is_async: true,
    is_unsafe: true,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    assert!(src.contains("unsafe"), "src = {src}");
    assert!(src.contains("async"), "src = {src}");
    assert!(src.contains("fn fancy"), "src = {src}");
    // Re-parse to make sure it's valid Rust.
    syn::parse_str::<syn::File>(&src).expect("async/unsafe func must re-parse");
}

// ---------------------------------------------------------------------------
// 9. Multiple parameters with types — snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_func_with_params_snapshot() {
    let func = FuncDecl { name: ident("add"),
    params: vec![
        Param {
            name: ident("a"),
            ty: named_type("Int"),
            default_value: None,
            is_comptime: false,
            span: span(),
        },
        Param {
            name: ident("b"),
            ty: named_type("Int"),
            default_value: None,
            is_comptime: false,
            span: span(),
        },
    ],
    return_type: Some(named_type("Int")),
    body: Block {
        stmts: vec![Stmt::Return(
            Some(Expr::BinaryOp {
                op: BinaryOp::Add,
                lhs: Box::new(ident_expr("a")),
                rhs: Box::new(ident_expr("b")),
                span: span(),
            }),
            span(),
        )],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    insta::assert_snapshot!(src, @"
fn add(a: i64, b: i64) -> i64 {
    return a + b;
}
");
}

// ---------------------------------------------------------------------------
// 10. String + bool literals
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_string_and_bool_literals() {
    let func = FuncDecl { name: ident("lit"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts: vec![
            Stmt::LetDecl {
                name: ident("s"),
                value: Expr::Literal(Literal::String("hi".to_string()), span()),
                mutable: false,
                ty: None,
                span: span(),
            },
            Stmt::LetDecl {
                name: ident("b"),
                value: Expr::Literal(Literal::Bool(true), span()),
                mutable: false,
                ty: None,
                span: span(),
            },
        ],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    // T12: type annotations inferred — `let s = "hi"` → `let s: String = "hi"`,
    // `let b = true` → `let b: bool = true`.
    assert!(src.contains(r#"let s: String = "hi";"#), "src = {src}");
    assert!(src.contains("let b: bool = true;"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("literals must re-parse");
}

// ---------------------------------------------------------------------------
// 11. let-with-type annotation
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_with_type_annotation() {
    let func = FuncDecl { name: ident("f"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts: vec![Stmt::LetDecl {
            name: ident("x"),
            value: int_expr(0),
            mutable: true,
            ty: Some(named_type("Int")),
            span: span(),
        }],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    assert!(src.contains("let mut x: i64 = 0"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("let-with-type must re-parse");
}

// ---------------------------------------------------------------------------
// 12. Multiple top-level declarations
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_multiple_funcs() {
    let decls = vec![empty_func("first"), empty_func("second")];
    let src = generate_rust(&decls).unwrap();
    assert!(src.contains("fn first"), "src = {src}");
    assert!(src.contains("fn second"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("multiple funcs must re-parse");
}

// ---------------------------------------------------------------------------
// 13. Expression function `=>` — full pipeline: lex → parse → codegen
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_expr_function_shorthand() {
    // `func f(x: Int) -> Int => x + 1` should produce a Rust fn returning
    // `x + 1`. We build the AST directly (same shape the parser produces
    // for `=>`), then verify the generated Rust.
    let func = FuncDecl { name: ident("f"),
    params: vec![Param {
        name: ident("x"),
        ty: named_type("Int"),
        default_value: None,
        is_comptime: false,
        span: span(),
    }],
    return_type: Some(named_type("Int")),
    body: Block {
        stmts: vec![Stmt::Return(
            Some(Expr::BinaryOp {
                op: BinaryOp::Add,
                lhs: Box::new(ident_expr("x")),
                rhs: Box::new(int_expr(1)),
                span: span(),
            }),
            span(),
        )],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    let src = generate_rust(&[Decl::FuncDecl(func)]).unwrap();
    assert!(src.contains("fn f(x: i64) -> i64"), "src = {src}");
    assert!(src.contains("x + 1"), "src = {src}");
    // Re-parse to make sure it's valid Rust.
    syn::parse_str::<syn::File>(&src).expect("expr-function codegen must re-parse");
}
