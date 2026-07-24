//! Integration tests for the null-conditional `?.` operator codegen (T70).
//!
//! `receiver ?. name` desugars (in the parser) to
//! `receiver.and_then(|x| x.name)` — an `Option`-chain with short-circuit
//! semantics. Chaining is left-associative:
//!
//! - `u?.name`        → `u.and_then(|x| x.name)`
//! - `opt?.value`     → `opt.and_then(|x| x.value)`
//! - `a?.b?.c`        → `a.and_then(|x| x.b).and_then(|x| x.c)`
//! - `u?.m(arg)`      → `u.and_then(|x| x.m(arg))`  (method-call form)
//!
//! Because the desugar happens entirely in the parser (no new AST variant,
//! no new codegen arm — it emits `Expr::MethodCall { .., method: "and_then",
//! args: [Lambda] }`), these tests exercise the FULL pipeline
//! (lex → parse → codegen) by parsing Buff source strings, then assert the
//! generated Rust contains the expected `.and_then(|x| ...)` shape. A few
//! tests also build the AST by hand to pin the precise node shape codegen
//! consumes.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{SourceId, Span};

// ---------------------------------------------------------------------------
// Helpers — hand-built AST and parse-from-source.
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

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

/// A placeholder TypeRef for untyped closure params (codegen ignores it).
fn placeholder_ty() -> TypeRef {
    TypeRef::Named {
        name: ident("_"),
        span: span(),
    }
}

/// Build a minimal closure `{ params => body }` as a Lambda node.
fn closure(params: &[&str], body: Expr) -> Expr {
    let params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: ident(p),
            ty: placeholder_ty(),
            default_value: None,
            is_comptime: false,
            span: span(),
        })
        .collect();
    Expr::Lambda {
        params,
        body: Block {
            stmts: vec![Stmt::ExprStmt(body, span())],
            span: span(),
        },
        return_type: None,
        span: span(),
    }
}

/// Build `receiver.method(args...)` as an AST node.
fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: ident(method),
        args,
        span: span(),
    }
}

/// Build the desugared form of `receiver?.name`:
/// `receiver.and_then(|x| x.name)`.
fn and_then_field(receiver: Expr, field: &str) -> Expr {
    let body = method_call(ident_expr("x"), field, Vec::new());
    method_call(receiver, "and_then", vec![closure(&["x"], body)])
}

/// Build the desugared form of `receiver?.method(args...)`:
/// `receiver.and_then(|x| x.method(args...))`.
fn and_then_method(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    let body = method_call(ident_expr("x"), method, args);
    method_call(receiver, "and_then", vec![closure(&["x"], body)])
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
/// Used for end-to-end QA assertions (lex → parse → codegen).
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
// QA: `opt?.value` → `opt.and_then(|x| x.value)`  (hand-built AST)
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_opt_value_handbuilt() {
    // Hand-built AST: the parser desugar of `opt?.value` produces exactly
    // MethodCall { receiver: opt, method: and_then,
    //               args: [Lambda(|x| MethodCall(x.value, []))] }.
    let desugared = and_then_field(ident_expr("opt"), "value");
    let src = codegen_stmt(Stmt::ExprStmt(desugared, span()));
    assert!(
        src.contains(".and_then(|x| x.value)"),
        "`opt?.value` should codegen to `opt.and_then(|x| x.value)` (or `<expr>.and_then(|x| x.value)`), got: {src}"
    );
    assert!(
        src.contains("opt"),
        "generated source should mention the receiver `opt`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// `u?.name` → `u.and_then(|x| x.name)`  (end-to-end from source)
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_u_name_e2e() {
    let src = codegen_program("func main():\n    u?.name");
    assert!(
        src.contains(".and_then(|x| x.name)"),
        "`u?.name` should codegen to `.and_then(|x| x.name)`, got: {src}"
    );
    assert!(
        src.contains("u.and_then"),
        "`u?.name` should produce `u.and_then(...)`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// `opt?.value` → `opt.and_then(|x| x.value)`  (end-to-end from source)
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_opt_value_e2e() {
    let src = codegen_program("func main():\n    opt?.value");
    assert!(
        src.contains(".and_then("),
        "`opt?.value` should codegen to `.and_then(...)`, got: {src}"
    );
    assert!(
        src.contains("opt.and_then"),
        "`opt?.value` should produce `opt.and_then(...)`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// Chained: `a?.b?.c` → `a.and_then(|x| x.b).and_then(|x| x.c)`
// (left-associative; each `?.` nests one more `.and_then`)
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_chained_e2e() {
    let src = codegen_program("func main():\n    a?.b?.c");
    // The chain should produce TWO `.and_then` calls in sequence on `a`.
    assert!(
        src.contains("a.and_then"),
        "chained `a?.b?.c` should start with `a.and_then`, got: {src}"
    );
    // Count `.and_then` occurrences — there must be exactly two for `a?.b?.c`.
    let count = src.matches(".and_then").count();
    assert_eq!(
        count, 2,
        "chained `a?.b?.c` should produce 2 `.and_then` calls, got {count} in: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// Chained (hand-built AST): pins the exact node shape the parser produces.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_chained_handbuilt() {
    // The parser desugar of `a?.b?.c` is, left-associatively:
    //   inner = a.and_then(|x| x.b)
    //   outer = inner.and_then(|x| x.c)
    let inner = and_then_field(ident_expr("a"), "b");
    let outer = and_then_field(inner, "c");
    let src = codegen_stmt(Stmt::ExprStmt(outer, span()));
    let count = src.matches(".and_then").count();
    assert_eq!(
        count, 2,
        "hand-built chained `a?.b?.c` should produce 2 `.and_then` calls, got {count} in: {src}"
    );
    assert!(
        src.contains("a.and_then"),
        "chained form should mention `a.and_then`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// Method-call form: `u?.m(arg)` → `u.and_then(|x| x.m(arg))`
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_method_call_e2e() {
    let src = codegen_program("func main():\n    u?.greet(42)");
    assert!(
        src.contains("u.and_then"),
        "`u?.greet(42)` should produce `u.and_then(...)`, got: {src}"
    );
    assert!(
        src.contains(".greet(42)"),
        "`u?.greet(42)` should preserve the method call `greet(42)` inside the lambda, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// Method-call form (hand-built AST).
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_method_call_handbuilt() {
    let desugared = and_then_method(ident_expr("u"), "greet", vec![int_expr(42)]);
    let src = codegen_stmt(Stmt::ExprStmt(desugared, span()));
    assert!(
        src.contains("u.and_then"),
        "method-call form should mention `u.and_then`, got: {src}"
    );
    assert!(
        src.contains(".greet(42)"),
        "method-call form should preserve `.greet(42)`, got: {src}"
    );
    assert_valid_rust(&src);
}

// ---------------------------------------------------------------------------
// Short-circuit (semantic) note: the desugar uses `.and_then`, which on
// `Option<T>` short-circuits `None` (returns `None` without invoking the
// closure). This is the Option-chain contract. We assert the operator name
// is `and_then` so a future change to e.g. `map` would surface here.
// ---------------------------------------------------------------------------

#[test]
fn null_conditional_uses_and_not_map() {
    let src = codegen_program("func main():\n    u?.name");
    assert!(
        src.contains("and_then"),
        "null-conditional must lower to `and_then` (short-circuits None), not `map`, got: {src}"
    );
}
