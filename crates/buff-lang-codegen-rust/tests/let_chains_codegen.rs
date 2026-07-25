//! T74 integration tests — codegen for let-chains (nested if-let output).
//!
//! The parser desugars `if let a = x, let b = y, ...` into NESTED single-
//! condition `Expr::IfLet` / `Expr::IfExpr` (T74). These tests verify the
//! existing `lower_if_let` / `lower_if_expr` (T72) lower the nested structure
//! into valid Rust `if let ... { if let ... { ... } }` form, with NO codegen
//! change.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust let_chains
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Pattern, Stmt, TypeRef};
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

/// Build a `Some(name)` variant-pattern.
fn some_pattern(binding: &str) -> Pattern {
    Pattern::Variant {
        enum_name: ident("Option"),
        variant: ident("Some"),
        subpatterns: vec![Pattern::Ident(ident(binding), span())],
        span: span(),
    }
}

/// Build a one-stmt body `print(arg)`.
fn print_body(arg: &str) -> Block {
    let arg_expr = if arg.starts_with('"') {
        Expr::Literal(Literal::String(arg.trim_matches('"').to_string()), span())
    } else {
        ident_expr(arg)
    };
    Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::FuncCall {
                callee: Box::new(ident_expr("print")),
                args: vec![arg_expr],
                span: span(),
            },
            span(),
        )],
        span: span(),
    }
}

/// Wrap a list of statements in a zero-arg `fn f() -> Void { ... }`.
fn func_with_stmts(stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident("f"),
        params: Vec::<Param>::new(),
        return_type: Some(TypeRef::Named {
            name: ident("Void"),
            span: span(),
        }),
        body: Block {
            stmts,
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

/// Assert the generated source re-parses as a valid Rust file.
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

/// Build a nested let-chain AST by hand: `outer` wraps `inner`, mimicking the
/// parser's `fold_if_chain` desugar. This proves the codegen lowers a nested
/// IfLet→IfLet→IfExpr structure WITHOUT any codegen change.
fn nested_two_let_chain() -> Expr {
    // inner IfLet(Some(y), b) → body.
    let inner = Expr::IfLet {
        pattern: some_pattern("y"),
        value: Box::new(ident_expr("b")),
        then_block: print_body("y"),
        else_block: None,
        span: span(),
    };
    let inner_then = Block {
        stmts: vec![Stmt::ExprStmt(inner, span())],
        span: span(),
    };
    // outer IfLet(Some(x), a) → wraps inner.
    Expr::IfLet {
        pattern: some_pattern("x"),
        value: Box::new(ident_expr("a")),
        then_block: inner_then,
        else_block: None,
        span: span(),
    }
}

/// Build a 3-level chain: IfLet(Some(a),x) → IfLet(Some(b),y) → IfExpr(a>b) → body.
fn nested_three_level_chain() -> Expr {
    use buff_lang_ast::op::BinaryOp;
    // innermost IfExpr(a > b) → body.
    let cond = Expr::BinaryOp {
        op: BinaryOp::Gt,
        lhs: Box::new(ident_expr("a")),
        rhs: Box::new(ident_expr("b")),
        span: span(),
    };
    let innermost = Expr::IfExpr {
        cond: Box::new(cond),
        then_block: print_body("a"),
        else_block: None,
        span: span(),
    };
    let mid_then = Block {
        stmts: vec![Stmt::ExprStmt(innermost, span())],
        span: span(),
    };
    let middle = Expr::IfLet {
        pattern: some_pattern("b"),
        value: Box::new(ident_expr("y")),
        then_block: mid_then,
        else_block: None,
        span: span(),
    };
    let outer_then = Block {
        stmts: vec![Stmt::ExprStmt(middle, span())],
        span: span(),
    };
    Expr::IfLet {
        pattern: some_pattern("a"),
        value: Box::new(ident_expr("x")),
        then_block: outer_then,
        else_block: None,
        span: span(),
    }
}

// ---------------------------------------------------------------------------
// Nested IfLet → IfLet codegen (two-let chain shape).
// ---------------------------------------------------------------------------

#[test]
fn let_chains_codegen_two_lets_nested() {
    let expr = nested_two_let_chain();
    let stmt = Stmt::ExprStmt(expr, span());
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    // Both let-bindings appear in nested form.
    assert!(
        src.contains("if let Some(x) = a"),
        "expected `if let Some(x) = a`, got:\n{src}"
    );
    assert!(
        src.contains("if let Some(y) = b"),
        "expected `if let Some(y) = b`, got:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3-level chain codegen (QA shape: IfLet → IfLet → IfExpr).
// ---------------------------------------------------------------------------

#[test]
fn let_chains_codegen_three_level_nested() {
    let expr = nested_three_level_chain();
    let stmt = Stmt::ExprStmt(expr, span());
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("if let Some(a) = x"),
        "expected `if let Some(a) = x`, got:\n{src}"
    );
    assert!(
        src.contains("if let Some(b) = y"),
        "expected `if let Some(b) = y`, got:\n{src}"
    );
    assert!(
        src.contains("a > b"),
        "expected `a > b` cond in innermost if, got:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Else-block replication codegen: else appears at EVERY nesting level.
// ---------------------------------------------------------------------------

#[test]
fn let_chains_codegen_else_replicated() {
    // Hand-build: outer IfLet(Some(x), a) wraps inner IfLet(Some(y), b) with
    // the BODY; BOTH levels get a clone of the else-block.
    let inner = Expr::IfLet {
        pattern: some_pattern("y"),
        value: Box::new(ident_expr("b")),
        then_block: print_body("y"),
        else_block: Some(print_body("\"fallback\"")),
        span: span(),
    };
    let inner_then = Block {
        stmts: vec![Stmt::ExprStmt(inner, span())],
        span: span(),
    };
    let outer = Expr::IfLet {
        pattern: some_pattern("x"),
        value: Box::new(ident_expr("a")),
        then_block: inner_then,
        else_block: Some(print_body("\"fallback\"")),
        span: span(),
    };
    let stmt = Stmt::ExprStmt(outer, span());
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    // Count `else` occurrences: 2 (one per level).
    let else_count = src.matches("else").count();
    assert_eq!(
        else_count, 2,
        "expected 2 `else` arms (replicated), got {else_count}:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// End-to-end: lex → parse → codegen on a real Buff source string.
// ---------------------------------------------------------------------------

#[test]
fn let_chains_codegen_end_to_end() {
    use buff_lang_lexer::tokenize;
    use buff_lang_parser::parse;
    // A full Buff program using a let-chain.
    let src_text = "func f():\n    if let Some(a) = x, let Some(b) = y, a > b { print(a) }";
    let sid = buff_lang_error::SourceId(0);
    let tokens = tokenize(src_text, sid).expect("lexer ok");
    let decls = parse(&tokens, sid).expect("parser ok");
    let rust = generate_rust(&decls).expect("codegen ok");
    assert!(
        rust.contains("if let Some(a) = x"),
        "expected `if let Some(a) = x` in:\n{rust}"
    );
    assert!(
        rust.contains("if let Some(b) = y"),
        "expected `if let Some(b) = y` in:\n{rust}"
    );
    assert!(rust.contains("a > b"), "expected `a > b` in:\n{rust}");
    must_reparse(&rust);
}
