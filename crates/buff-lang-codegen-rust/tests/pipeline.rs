//! Integration tests for the pipeline operator `|>` codegen (T69).
//!
//! `LHS |> f(args...)` desugars to `f(LHS, args...)` — the left operand is
//! inserted as the FIRST argument of the right-hand call. Chaining is
//! left-associative: `a |> f() |> g()` → `g(f(a))`.
//!
//! Because the desugar happens entirely in the parser (no new AST variant,
//! no new codegen arm), these tests exercise the FULL pipeline
//! (lex → parse → codegen) by parsing Buff source strings, then assert the
//! generated Rust contains the expected nested-call shape.

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{SourceId, Span};

// ---------------------------------------------------------------------------
// Helpers — hand-built AST (precise shape) and parse-from-source (e2e).
// ---------------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

/// Build a free function call `callee(args...)` AST node.
fn call_expr(callee: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(callee)),
        args,
        span: span(),
    }
}

/// Build a simple function with one statement and codegen it (hand-built AST).
fn codegen_stmt(stmt: Stmt) -> String {
    let func = Decl::FuncDecl(FuncDecl { name: ident("f"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts: vec![stmt],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), });
    generate_rust(&[func]).expect("codegen should succeed")
}

/// Parse a full Buff program string and codegen it to Rust source.
///
/// The program is wrapped so it can be lexed + parsed by the real pipeline.
/// Used for end-to-end QA assertions (e.g. `"hello" |> print()`).
fn codegen_program(src: &str) -> String {
    let tokens = buff_lang_lexer::tokenize(src, SourceId(0)).expect("lexer should succeed");
    let decls = buff_lang_parser::parse(&tokens, SourceId(0)).expect("parser should succeed");
    generate_rust(&decls).expect("codegen should succeed")
}

/// Re-parse the generated Rust to prove it's syntactically valid Rust.
fn assert_valid_rust(src: &str) {
    let _file: syn::File = syn::parse_str(src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// QA: `"hello" |> print()` → `print("hello")`  (end-to-end from source)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_codegen_hello_print() {
    let src = codegen_program("func main():\n    \"hello\" |> print()");
    // The desugar turns `"hello" |> print()` into a `print` call with
    // `"hello"` as its first (and only) argument. Buff's `print` prelude
    // lowers a bare-string-literal arg to Rust's `println!("hello")` (the
    // `{}` format is dropped for literals — see `lower_print`). Asserting
    // `println!("hello")` proves the LHS landed as the print call's arg.
    assert!(
        src.contains("println!(\"hello\")"),
        "`\"hello\" |> print()` should codegen to `println!(\"hello\")` (print prelude maps to `println!`), got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// `x |> f()` → `f(x)`  (hand-built AST + end-to-end)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_codegen_simple() {
    // Hand-built: the parser desugar produces exactly this FuncCall shape.
    // `x |> f()` desugars to FuncCall { callee: f, args: [x] }.
    let desugared = call_expr("f", vec![ident_expr("x")]);
    let src = codegen_stmt(Stmt::ExprStmt(desugared, span()));
    assert!(
        src.contains("f(x)"),
        "`x |> f()` should codegen to `f(x)`, got: {src}"
    );
    assert_valid_rust(&src);

    // End-to-end from source: proves the parser actually produces the shape.
    let src2 = codegen_program("func main():\n    x |> f()");
    assert!(
        src2.contains("f(x)"),
        "end-to-end `x |> f()` should codegen to `f(x)`, got: {src2}"
    );
    assert_valid_rust(&src2);
}

// ---------------------------------------------------------------------------
// `data |> process() |> filter()` → `filter(process(data))`  (chained, e2e)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_codegen_chained() {
    let src = codegen_program("func main():\n    data |> process() |> filter()");
    assert!(
        src.contains("filter(process(data))"),
        "`data |> process() |> filter()` should codegen to `filter(process(data))`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// `x |> f(a, b)` → `f(x, a, b)`  (extra args preserved after LHS, e2e)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_codegen_extra_args() {
    let src = codegen_program("func main():\n    x |> f(a, b)");
    assert!(
        src.contains("f(x, a, b)"),
        "`x |> f(a, b)` should codegen to `f(x, a, b)`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// `x |> f()` (bare-callee RHS, no parens) → `f(x)`  (e2e)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_codegen_bare_callee() {
    // `x |> f` (no parens) is supported as a shorthand for `f(x)`.
    let src = codegen_program("func main():\n    x |> f");
    assert!(
        src.contains("f(x)"),
        "`x |> f` (bare callee) should codegen to `f(x)`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// Hand-built chained shape: `a |> f() |> g()` → `g(f(a))` (precise AST)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_codegen_chained_handbuilt() {
    // The parser desugar of `a |> f() |> g()` is, left-associatively:
    //   inner = f(a)   (FuncCall { callee: f, args: [a] })
    //   outer = g(f(a)) (FuncCall { callee: g, args: [inner] })
    let inner = call_expr("f", vec![ident_expr("a")]);
    let outer = call_expr("g", vec![inner]);
    let src = codegen_stmt(Stmt::ExprStmt(outer, span()));
    assert!(
        src.contains("g(f(a))"),
        "chained pipeline should codegen to `g(f(a))`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// `"hello" |> print()` hand-built (QA mirror via precise AST)
// ---------------------------------------------------------------------------

#[test]
fn pipeline_codegen_string_to_print() {
    let desugared = call_expr("print", vec![string_expr("hello")]);
    let src = codegen_stmt(Stmt::ExprStmt(desugared, span()));
    // Buff's `print` prelude lowers a bare-string-literal arg to
    // `println!("hello")` (the `{}` format is dropped for literals).
    assert!(
        src.contains("println!(\"hello\")"),
        "`\"hello\" |> print()` should codegen to `println!(\"hello\")` (print prelude), got: {src}"
    );
    assert_valid_rust(&src);
}
