//! T96 â€” Standard library prelude: Rust **codegen** integration tests.
//!
//! These tests verify that the prelude function names lower to the correct
//! Rust idioms. They are the codegen half of the T96 acceptance criteria;
//! the type-inference half lives in
//! `crates/buff-lang-types/tests/prelude_functions.rs`.
//!
//! ## Coverage
//!
//! - **Math**: `abs` â†’ `.abs()`, `min`/`max` â†’ `.min()`/`.max()`,
//!   `sqrt`/`floor`/`ceil`/`round` â†’ `((x) as f64).<m>()`, `pow` â†’
//!   `.pow((e) as u32)` for ints and `.powf((e) as f64)` for floats.
//! - **Conversions**: `Int(x)`/`Float(x)`/`Bool(x)` dispatch on the arg
//!   type (`as T` for numerics, `.parse::<T>().unwrap_or(default)` for
//!   strings); `String(x)` â†’ `.to_string()`.
//! - **I/O**: `print("lit")` â†’ `println!("lit")` (no `{}`), `print(x)` â†’
//!   `println!("{}", x)`, `read_line()` â†’ stdin block.
//! - A **snapshot** test pins the combined output of a small program that
//!   uses prelude math + I/O together.

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
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

fn float_expr(n: f32) -> Expr {
    Expr::Literal(Literal::Float(n), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn call_expr(name: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args,
        span: span(),
    }
}

fn neg_int(n: i64) -> Expr {
    Expr::UnaryOp {
        op: buff_lang_ast::op::UnaryOp::Neg,
        operand: Box::new(int_expr(n)),
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
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    })
}

// ---------------------------------------------------------------------------
// 1. abs â†’ .abs()  (with parens around the receiver)
// ---------------------------------------------------------------------------

#[test]
fn prelude_codegen_abs_neg_five() {
    // func main() { print(abs(-5)) }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![call_expr("abs", vec![neg_int(5)])]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    // abs(-5) lowers to `(- 5).abs()` — parens around the receiver so
    // it composes with negation and avoids `5.abs()` (field-access
    // ambiguity). prettyplease prints unary `-` with a trailing space.
    assert!(src.contains(".abs()"), "src = {src}");
    assert!(src.contains("- 5") || src.contains("(-5)"), "src = {src}");
    // The whole thing is wrapped in a println!("{}", ...).
    assert!(src.contains("println!"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse as Rust");
}

// ---------------------------------------------------------------------------
// 2. min / max â†’ .min() / .max()
// ---------------------------------------------------------------------------

#[test]
fn prelude_codegen_min_max() {
    // func main() { print(min(3, 7)); print(max(3, 7)) }
    let f = func_with_stmts(
        "main",
        vec![
            Stmt::ExprStmt(
                call_expr(
                    "print",
                    vec![call_expr("min", vec![int_expr(3), int_expr(7)])],
                ),
                span(),
            ),
            Stmt::ExprStmt(
                call_expr(
                    "print",
                    vec![call_expr("max", vec![int_expr(3), int_expr(7)])],
                ),
                span(),
            ),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains(".min("), "src = {src}");
    assert!(src.contains(".max("), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 3. sqrt / floor / ceil / round â†’ ((x) as f64).<method>()
// ---------------------------------------------------------------------------

#[test]
fn prelude_codegen_float_unary_math() {
    let f = func_with_stmts(
        "main",
        vec![
            Stmt::ExprStmt(
                call_expr("print", vec![call_expr("sqrt", vec![int_expr(16)])]),
                span(),
            ),
            Stmt::ExprStmt(
                call_expr("print", vec![call_expr("floor", vec![float_expr(1.5)])]),
                span(),
            ),
            Stmt::ExprStmt(
                call_expr("print", vec![call_expr("ceil", vec![float_expr(1.5)])]),
                span(),
            ),
            Stmt::ExprStmt(
                call_expr("print", vec![call_expr("round", vec![float_expr(1.5)])]),
                span(),
            ),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("as f64"), "src = {src}");
    assert!(src.contains(".sqrt()"), "src = {src}");
    assert!(src.contains(".floor()"), "src = {src}");
    assert!(src.contains(".ceil()"), "src = {src}");
    assert!(src.contains(".round()"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 4. pow â†’ .pow((e) as u32) for int base, .powf((e) as f64) for float base
// ---------------------------------------------------------------------------

#[test]
fn prelude_codegen_pow_int_base_uses_pow() {
    // func main() { print(pow(2, 10)) }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr(
                "print",
                vec![call_expr("pow", vec![int_expr(2), int_expr(10)])],
            ),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains(".pow("), "src = {src}");
    assert!(src.contains("as u32"), "src = {src}");
    assert!(
        !src.contains(".powf("),
        "int pow should not use powf, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

#[test]
fn prelude_codegen_pow_float_base_uses_powf() {
    // func main() { print(pow(2.0, 10)) }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr(
                "print",
                vec![call_expr("pow", vec![float_expr(2.0), int_expr(10)])],
            ),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains(".powf("), "src = {src}");
    assert!(src.contains("as f64"), "src = {src}");
    assert!(
        !src.contains("as u32"),
        "float pow should not use u32, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 5. Conversions â€” Int / Float / String / Bool
// ---------------------------------------------------------------------------

#[test]
fn prelude_codegen_int_of_string_parses() {
    // func main() { print(Int("42")) }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![call_expr("Int", vec![string_expr("42")])]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    // prettyplease prints turbofish with spaces: `.parse:: < i64 > ()`.
    // Check the components separately so the test survives formatting
    // changes. (The generated source re-parses as valid Rust either way.)
    assert!(src.contains(".parse::"), "src = {src}");
    assert!(src.contains("i64"), "src = {src}");
    assert!(src.contains(".unwrap_or(0)"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

#[test]
fn prelude_codegen_int_of_int_casts() {
    // func main() { let x = 5; print(Int(x)) }
    let f = func_with_stmts(
        "main",
        vec![
            Stmt::LetDecl {
                name: ident("x"),
                value: int_expr(5),
                mutable: false,
                ty: None,
                span: span(),
            },
            Stmt::ExprStmt(
                call_expr("print", vec![call_expr("Int", vec![ident_expr("x")])]),
                span(),
            ),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("as i64"), "src = {src}");
    // No parse call for numeric args.
    assert!(!src.contains(".parse::"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

#[test]
fn prelude_codegen_string_of_int_to_string() {
    // func main() { print(String(42)) }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![call_expr("String", vec![int_expr(42)])]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains(".to_string()"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

#[test]
fn prelude_codegen_bool_of_int_not_equal_zero() {
    // func main() { print(Bool(1)) }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![call_expr("Bool", vec![int_expr(1)])]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    // Numeric â†’ Bool uses `(x) != 0`.
    assert!(src.contains("!= 0"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

#[test]
fn prelude_codegen_bool_of_string_parses() {
    // func main() { print(Bool("true")) }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![call_expr("Bool", vec![string_expr("true")])]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    // prettyplease prints turbofish with spaces; check components.
    assert!(src.contains(".parse::"), "src = {src}");
    assert!(src.contains("bool"), "src = {src}");
    assert!(src.contains("false"), "src = {src}"); // the unwrap_or(false) default
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 6. print / println / read_line
// ---------------------------------------------------------------------------

#[test]
fn prelude_codegen_print_string_literal_no_placeholder() {
    // func main() { print("hello") } â†’ println!("hello") (no {})
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("print", vec![string_expr("hello")]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains(r#"println!("hello")"#), "src = {src}");
    assert!(
        !src.contains(r#""{}""#),
        "string literal should drop placeholder, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

#[test]
fn prelude_codegen_print_non_literal_uses_placeholder() {
    // func main() { let x = 42; print(x) } â†’ println!("{}", x)
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
    assert!(src.contains(r#"println!("{}", "#), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

#[test]
fn prelude_codegen_println_alias_matches_print() {
    // println(x) lowers identically to print(x).
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
            Stmt::ExprStmt(call_expr("println", vec![ident_expr("x")]), span()),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("println!"), "src = {src}");
    assert!(src.contains(r#""{}""#), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

#[test]
fn prelude_codegen_read_line_returns_string() {
    // func main() { let line = read_line() }
    let f = func_with_stmts(
        "main",
        vec![Stmt::LetDecl {
            name: ident("line"),
            value: call_expr("read_line", vec![]),
            mutable: false,
            ty: None,
            span: span(),
        }],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("std::io::stdin()"), "src = {src}");
    assert!(src.contains(".read_line("), "src = {src}");
    assert!(src.contains("String::new()"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 7. Combined snapshot â€” math + I/O together
// ---------------------------------------------------------------------------

#[test]
fn prelude_codegen_combined_snapshot() {
    // func main() {
    //     print(abs(-5));
    //     print(max(3, 7));
    //     print(Int("42"));
    //     print(String(42));
    // }
    let f = func_with_stmts(
        "main",
        vec![
            Stmt::ExprStmt(
                call_expr("print", vec![call_expr("abs", vec![neg_int(5)])]),
                span(),
            ),
            Stmt::ExprStmt(
                call_expr(
                    "print",
                    vec![call_expr("max", vec![int_expr(3), int_expr(7)])],
                ),
                span(),
            ),
            Stmt::ExprStmt(
                call_expr("print", vec![call_expr("Int", vec![string_expr("42")])]),
                span(),
            ),
            Stmt::ExprStmt(
                call_expr("print", vec![call_expr("String", vec![int_expr(42)])]),
                span(),
            ),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    insta::assert_snapshot!(src, @r###"
    fn main() {
        println!("{}", (- 5).abs());
        println!("{}", (3).max(7));
        println!("{}", "42".parse:: < i64 > ().unwrap_or(0));
        println!("{}", 42.to_string());
    }
    "###);
}

// ---------------------------------------------------------------------------
// 8. Non-prelude calls are unchanged (passthrough)
// ---------------------------------------------------------------------------

#[test]
fn prelude_codegen_user_func_call_still_passthrough() {
    // A non-prelude function name (e.g. "my_func") is NOT intercepted â€” it
    // lowers to a plain Rust call expression `my_func(arg)`.
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("my_func", vec![int_expr(1), int_expr(2)]),
            span(),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("my_func(1, 2)"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}
