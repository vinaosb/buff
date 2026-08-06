//! T13 integration tests — control-flow codegen: if/else, loops, print.
//!
//! These tests cover the T13 control-flow additions:
//!
//! - `if cond { a } else { b }` → Rust if-expression
//! - `for x in iter { body }` → Rust `for` loop
//! - `for cond { body }` (Buff conditional loop) → Rust `while cond { body }`
//! - `print(arg)` → `println!(...)` macro mapping
//!   (T96: a bare string-literal arg drops the `{}` placeholder so
//!   `print("hello")` → `println!("hello")`; any other arg uses
//!   `println!("{}", arg)`.)
//!
//! Each test builds a small Buff AST by hand, runs it through
//! [`buff_lang_codegen_rust::generate_rust`], and asserts properties of the
//! resulting Rust source.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn bool_expr(b: bool) -> Expr {
    Expr::Literal(Literal::Bool(b), span())
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

fn binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: span(),
    }
}

fn call_expr(name: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args,
        span: span(),
    }
}

fn block(stmts: Vec<Stmt>) -> Block {
    Block {
        stmts,
        span: span(),
    }
}

fn if_expr(cond: Expr, then_block: Block, else_block: Option<Block>) -> Expr {
    Expr::IfExpr {
        cond: Box::new(cond),
        then_block,
        else_block,
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

fn func_with_params_return(
    name: &str,
    params: Vec<Param>,
    ret: Option<TypeRef>,
    stmts: Vec<Stmt>,
) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params,
        return_type: ret,
        body: block(stmts),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    })
}

// ---------------------------------------------------------------------------
// 1. if/else expression
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_if_else() {
    // if c { 1 } else { 2 }
    let expr = if_expr(
        ident_expr("c"),
        block(vec![Stmt::ExprStmt(int_expr(1), span())]),
        Some(block(vec![Stmt::ExprStmt(int_expr(2), span())])),
    );
    let f = func_with_stmts("f", vec![Stmt::ExprStmt(expr, span())]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("if c {"), "src = {src}");
    assert!(src.contains("} else {"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 2. if/else as a let value
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_if_else_as_let_value() {
    // let c = true; let x = if c { 1 } else { 2 }
    // (Declare `c` first so the type inferencer can resolve the if-cond.)
    let value = if_expr(
        ident_expr("c"),
        block(vec![Stmt::ExprStmt(int_expr(1), span())]),
        Some(block(vec![Stmt::ExprStmt(int_expr(2), span())])),
    );
    let f = func_with_stmts(
        "f",
        vec![
            Stmt::LetDecl {
                name: ident("c"),
                value: bool_expr(true),
                mutable: false,
                ty: None,
                span: span(),
            },
            Stmt::LetDecl {
                name: ident("x"),
                value,
                mutable: false,
                ty: None,
                span: span(),
            },
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("let x: i64 ="), "src = {src}");
    assert!(src.contains("if c {"), "src = {src}");
    assert!(src.contains("} else {"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 3. if without else
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_if_no_else() {
    // if c { print("hi") }
    let then_body = block(vec![Stmt::ExprStmt(
        call_expr("print", vec![string_expr("hi")]),
        span(),
    )]);
    let expr = if_expr(ident_expr("c"), then_body, None);
    let f = func_with_stmts("f", vec![Stmt::ExprStmt(expr, span())]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("if c {"), "src = {src}");
    assert!(!src.contains("else"), "no else expected, src = {src}");
    // The print() call should be mapped to println!("{}", "hi").
    assert!(src.contains("println"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 4. for-in loop
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_for_in() {
    // for x in items { print(x) }
    let body = block(vec![Stmt::ExprStmt(
        call_expr("print", vec![ident_expr("x")]),
        span(),
    )]);
    let stmt = Stmt::ForIn {
        var: ident("x"),
        iter: ident_expr("items"),
        body,
        span: span(),
    };
    let f = func_with_stmts("f", vec![stmt]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("for x in items {"), "src = {src}");
    // print(x) → println!("{}", x)
    assert!(src.contains("println"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 5. for-while loop (conditional) — maps to Rust `while`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_for_while() {
    // for count > 0 { count -= 1 }
    let cond = binary(BinaryOp::Gt, ident_expr("count"), int_expr(0));
    let body = block(vec![Stmt::Assignment {
        target: ident_expr("count"),
        op: BinaryOp::SubAssign,
        value: int_expr(1),
        span: span(),
    }]);
    let stmt = Stmt::ForWhile {
        cond,
        body,
        span: span(),
    };
    let f = func_with_stmts("f", vec![stmt]);
    let src = generate_rust(&[f]).unwrap();
    assert!(
        src.contains("while count > 0 {"),
        "expected direct while-loop, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 6. print() with string literal → println!("hello")
//    (T96: bare string literal drops the `{}` placeholder, matching the
//    T96 acceptance `print("hello")` → `println!("hello")` exactly.)
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_print_string_literal() {
    let f = func_with_stmts(
        "f",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![string_expr("hello")]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("println!"), "src = {src}");
    assert!(src.contains(r#""hello""#), "src = {src}");
    // T96: no `{}` placeholder when the arg is a string literal.
    assert!(
        !src.contains(r#""{}""#),
        "T96: print(\"hello\") should be println!(\"hello\"), not println!(\"{{}}\", \"hello\"); src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 7. print(ident) → println!("{}", x)
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_print_ident() {
    // First declare x so the inferencer can see it; then print(x).
    let f = func_with_stmts(
        "f",
        vec![
            Stmt::LetDecl {
                name: ident("x"),
                value: int_expr(42),
                mutable: false,
                ty: None,
                span: span(),
            },
            Stmt::ExprStmt(call_expr("print", vec![ident_expr("x")]), span()),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("println!"), "src = {src}");
    // The ident x must appear inside the macro, not as a free `print(x)`.
    assert!(
        !src.contains("print(x)"),
        "expected print() to be mapped to println!, src = {src}"
    );
    assert!(src.contains("x"), "ident must be present, src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 8. print() with integer literal → println!("{}", 42)
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_print_int_literal() {
    let f = func_with_stmts(
        "f",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![int_expr(42)]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("println!"), "src = {src}");
    assert!(src.contains("42"), "src = {src}");
    assert!(
        !src.contains("print(42)"),
        "expected print() to be mapped to println!, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 9. Nested if inside for-in
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_nested_if_in_for() {
    // for x in items { if x > 0 { print(x) } }
    let if_expr_inner = if_expr(
        binary(BinaryOp::Gt, ident_expr("x"), int_expr(0)),
        block(vec![Stmt::ExprStmt(
            call_expr("print", vec![ident_expr("x")]),
            span(),
        )]),
        None,
    );
    let body = block(vec![Stmt::ExprStmt(if_expr_inner, span())]);
    let stmt = Stmt::ForIn {
        var: ident("x"),
        iter: ident_expr("items"),
        body,
        span: span(),
    };
    let f = func_with_stmts("f", vec![stmt]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("for x in items {"), "src = {src}");
    assert!(src.contains("if x > 0 {"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 10. Full ola.buff-equivalent program — snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_full_program_snapshot() {
    // func main() { print("Olá, Buff!") }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![string_expr("Olá, Buff!")]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    insta::assert_snapshot!(src, @"
fn main() {
    println!(\"Olá, Buff!\");
    {
        if let Ok(__buff_contents) = std::fs::read_to_string(\".env\") {
            for __buff_line in __buff_contents.lines() {
                let __buff_line = __buff_line.trim();
                if __buff_line.is_empty() || __buff_line.starts_with('#') {
                    continue;
                }
                if let Some((__buff_key, __buff_val)) = __buff_line.split_once('=') {
                    let __buff_k = __buff_key.trim().to_string();
                    let __buff_v = __buff_val.trim().to_string();
                    if !__buff_k.is_empty() && std::env::var(&__buff_k).is_err() {
                        unsafe {
                            std::env::set_var(&__buff_k, &__buff_v);
                        }
                    }
                }
            }
        }
    }
}
");
}

// ---------------------------------------------------------------------------
// 11. Function that returns a value
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_func_returns_value() {
    // func add(a: Int, b: Int) -> Int { return a + b }
    let f = func_with_params_return(
        "add",
        vec![
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
        Some(named_type("Int")),
        vec![Stmt::Return(
            Some(binary(BinaryOp::Add, ident_expr("a"), ident_expr("b"))),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("fn add(a: i64, b: i64) -> i64"), "src = {src}");
    assert!(src.contains("return a + b;"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 12. main func with print — runnable shape
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_main_func() {
    // func main() { let x = 42; print(x) }
    let f = func_with_stmts(
        "main",
        vec![
            Stmt::LetDecl {
                name: ident("x"),
                value: int_expr(42),
                mutable: false,
                ty: None,
                span: span(),
            },
            Stmt::ExprStmt(call_expr("print", vec![ident_expr("x")]), span()),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("fn main()"), "src = {src}");
    assert!(src.contains("let x: i64 = 42"), "src = {src}");
    assert!(src.contains("println!"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 13. While loop with break inside (loops compose with break/continue)
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_while_with_break() {
    // func f(n: Int) { for n > 0 { if n == 1 { break } n -= 1 } }
    // Make `n` a typed parameter so the move analyzer treats it as Copy.
    let cond = binary(BinaryOp::Gt, ident_expr("n"), int_expr(0));
    let body = block(vec![
        Stmt::ExprStmt(
            if_expr(
                binary(BinaryOp::Eq, ident_expr("n"), int_expr(1)),
                block(vec![Stmt::Break(span())]),
                None,
            ),
            span(),
        ),
        Stmt::Assignment {
            target: ident_expr("n"),
            op: BinaryOp::SubAssign,
            value: int_expr(1),
            span: span(),
        },
    ]);
    let stmt = Stmt::ForWhile {
        cond,
        body,
        span: span(),
    };
    let f = func_with_params_return(
        "f",
        vec![Param {
            name: ident("n"),
            ty: named_type("Int"),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        None,
        vec![stmt],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("while n > 0 {"), "src = {src}");
    assert!(src.contains("if n == 1 {"), "src = {src}");
    assert!(src.contains("break;"), "src = {src}");
    // No `.clone()` because n is Copy (Int param).
    assert!(!src.contains(".clone()"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 14. Generated code re-parses — syn::parse_str round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_generated_control_flow_reparses() {
    // Compose a function that uses all control-flow constructs at once.
    let body = vec![
        Stmt::LetDecl {
            name: ident("x"),
            value: int_expr(0),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(
            if_expr(
                bool_expr(true),
                block(vec![Stmt::ExprStmt(
                    call_expr("print", vec![string_expr("yes")]),
                    span(),
                )]),
                Some(block(vec![Stmt::ExprStmt(
                    call_expr("print", vec![string_expr("no")]),
                    span(),
                )])),
            ),
            span(),
        ),
        Stmt::ForIn {
            var: ident("i"),
            iter: ident_expr("items"),
            body: block(vec![Stmt::ExprStmt(
                call_expr("print", vec![ident_expr("i")]),
                span(),
            )]),
            span: span(),
        },
    ];
    let f = func_with_stmts("everything", body);
    let src = generate_rust(&[f]).unwrap();
    // Must round-trip through syn — i.e. the generated Rust is syntactically
    // valid even with all control-flow constructs composed together.
    syn::parse_str::<syn::File>(&src)
        .unwrap_or_else(|e| panic!("generated src must re-parse: {e}\n--- src ---\n{src}"));
}
