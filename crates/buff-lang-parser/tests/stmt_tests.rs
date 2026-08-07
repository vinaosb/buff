//! Integration tests for the buff-lang-parser statement parser (T8).
//!
//! These tests exercise the parser end-to-end by feeding source strings
//! through the T6 lexer and then through [`buff_lang_parser::stmt`] and
//! [`buff_lang_parser::parse`] entry points.
//!
//! Coverage:
//!
//! - `let` declarations (basic, `mut`, type annotation)
//! - assignment (simple and compound)
//! - `if`/`else`/`else if` (statement-wrapped IfExpr)
//! - `return` (value and void)
//! - `break` / `continue`
//! - function declarations (no params, params, return type)
//! - `for x in items { ... }` (iterator)
//! - `for cond { ... }` (conditional / while-style)
//! - expression statements (function call as standalone)
//! - type annotation including generics
//! - nested blocks (if inside func body)
//! - error path: `async func foo()` is rejected (T8 does not support async)

#![allow(clippy::approx_constant)]

use buff_lang_ast::{BinaryOp, Block, Expr, FuncDecl, Ident, Literal, Param, Stmt, TypeRef};
use buff_lang_error::{ParseError, SourceId, Span};
use buff_lang_lexer::tokenize;
use buff_lang_parser::{
    parse, parse_block_braces, parse_func_decl, parse_statement, parse_type_ref, TokenStream,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sid() -> SourceId {
    SourceId(0)
}

fn dummy_span() -> Span {
    Span::dummy()
}

fn int_lit(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), dummy_span())
}

fn ident_expr(name: &str) -> Expr {
    Expr::Ident(Ident::new(name, dummy_span()), dummy_span())
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: Ident::new(name, dummy_span()),
        span: dummy_span(),
    }
}

/// Tokenize + parse a single statement. Panics on lexer or parser failure.
fn parse_stmt(src: &str) -> Stmt {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_statement(&mut stream).expect("parser should succeed")
}

/// Like [`parse_stmt`] but asserts the parser produces an error.
fn parse_stmt_err(src: &str) -> ParseError {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_statement(&mut stream).expect_err("parser should fail")
}

/// Tokenize + parse a block (`{ ... }`). The source must start with `{`.
fn parse_block(src: &str) -> Block {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_block_braces(&mut stream).expect("parser should succeed")
}

/// Tokenize + parse a function declaration. The source must start with `func`.
fn parse_func(src: &str) -> FuncDecl {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_func_decl(&mut stream, Vec::new()).expect("parser should succeed")
}

/// Tokenize + parse a type reference in isolation.
fn parse_ty(src: &str) -> TypeRef {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_type_ref(&mut stream).expect("parser should succeed")
}

/// Strip span info by using Display (matches what T7's tests do for shape
/// comparison).
fn shape<T: std::fmt::Display>(node: &T) -> String {
    node.to_string()
}

// ---------------------------------------------------------------------------
// 1. `let` declarations
// ---------------------------------------------------------------------------

#[test]
fn test_let_int() {
    let s = parse_stmt("let x = 42");
    match s {
        Stmt::LetDecl {
            name,
            value,
            mutable,
            ty,
            ..
        } => {
            assert_eq!(name.name, "x");
            assert!(matches!(value, Expr::Literal(Literal::Int(42), _)));
            assert!(!mutable);
            assert!(ty.is_none());
        }
        other => panic!("expected LetDecl, got {other:?}"),
    }
}

#[test]
fn test_let_mut() {
    let s = parse_stmt("let mut y = 0");
    match s {
        Stmt::LetDecl {
            name,
            mutable,
            value,
            ..
        } => {
            assert_eq!(name.name, "y");
            assert!(mutable);
            assert!(matches!(value, Expr::Literal(Literal::Int(0), _)));
        }
        other => panic!("expected LetDecl, got {other:?}"),
    }
}

#[test]
fn test_let_string() {
    let s = parse_stmt("let nome = \"Buff\"");
    match s {
        Stmt::LetDecl { value, .. } => {
            assert!(matches!(value, Expr::Literal(Literal::String(_), _)));
            assert_eq!(shape(&value), "Lit(String(\"Buff\"))");
        }
        other => panic!("expected LetDecl, got {other:?}"),
    }
}

#[test]
fn test_let_with_type() {
    let s = parse_stmt("let x: Int = 42");
    match s {
        Stmt::LetDecl { ty, .. } => {
            let t = ty.expect("type annotation should be present");
            assert!(matches!(t, TypeRef::Named { .. }));
            assert_eq!(shape(&t), "Int");
        }
        other => panic!("expected LetDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2. Assignment
// ---------------------------------------------------------------------------

#[test]
fn test_assignment_simple() {
    let s = parse_stmt("x = 5");
    match s {
        Stmt::Assignment {
            target, op, value, ..
        } => {
            assert!(matches!(target, Expr::Ident(_, _)));
            assert_eq!(op, BinaryOp::Assign);
            assert!(matches!(value, Expr::Literal(Literal::Int(5), _)));
        }
        other => panic!("expected Assignment, got {other:?}"),
    }
}

#[test]
fn test_assignment_compound() {
    let s = parse_stmt("x += 1");
    match s {
        Stmt::Assignment { op, .. } => assert_eq!(op, BinaryOp::AddAssign),
        other => panic!("expected Assignment, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 3. if / else
// ---------------------------------------------------------------------------

#[test]
fn test_if_statement() {
    let s = parse_stmt("if x > 0 { print(\"pos\") }");
    match s {
        Stmt::ExprStmt(Expr::IfExpr { then_block, .. }, _) => {
            assert_eq!(then_block.stmts.len(), 1);
        }
        other => panic!("expected ExprStmt(IfExpr), got {other:?}"),
    }
}

#[test]
fn test_if_else() {
    let s = parse_stmt("if c { 1 } else { 2 }");
    match s {
        Stmt::ExprStmt(Expr::IfExpr { else_block, .. }, _) => {
            assert!(else_block.is_some(), "else block should be present");
        }
        other => panic!("expected ExprStmt(IfExpr), got {other:?}"),
    }
}

#[test]
fn test_if_else_if() {
    let s = parse_stmt("if a { 1 } else if b { 2 } else { 3 }");
    match s {
        Stmt::ExprStmt(Expr::IfExpr { else_block, .. }, _) => {
            let els = else_block.expect("else block present");
            // The else block contains a single ExprStmt wrapping a nested IfExpr.
            assert_eq!(els.stmts.len(), 1);
            match &els.stmts[0] {
                Stmt::ExprStmt(Expr::IfExpr { .. }, _) => {}
                other => panic!("expected nested IfExpr, got {other:?}"),
            }
        }
        other => panic!("expected ExprStmt(IfExpr), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4. return / break / continue
// ---------------------------------------------------------------------------

#[test]
fn test_return_value() {
    let s = parse_stmt("return 42");
    match s {
        Stmt::Return(Some(expr), _) => {
            assert!(matches!(expr, Expr::Literal(Literal::Int(42), _)));
        }
        other => panic!("expected Return(Some), got {other:?}"),
    }
}

#[test]
fn test_return_void() {
    let s = parse_stmt("return");
    match s {
        Stmt::Return(None, _) => {}
        other => panic!("expected Return(None), got {other:?}"),
    }
}

#[test]
fn test_break() {
    let s = parse_stmt("break");
    assert!(matches!(s, Stmt::Break(_)));
}

#[test]
fn test_continue() {
    let s = parse_stmt("continue");
    assert!(matches!(s, Stmt::Continue(_)));
}

// ---------------------------------------------------------------------------
// 5. Function declarations
// ---------------------------------------------------------------------------

#[test]
fn test_func_decl_simple() {
    let f = parse_func("func foo() { }");
    assert_eq!(f.name.name, "foo");
    assert!(f.params.is_empty());
    assert!(f.return_type.is_none());
    assert!(f.body.stmts.is_empty());
    assert!(!f.is_async);
    assert!(!f.is_unsafe);
    assert!(!f.is_extern);
}

#[test]
fn test_func_decl_with_params() {
    let f = parse_func("func add(a: Int, b: Int) -> Int { return a + b }");
    assert_eq!(f.name.name, "add");
    assert_eq!(f.params.len(), 2);
    assert_eq!(f.params[0].name.name, "a");
    assert_eq!(f.params[1].name.name, "b");
    assert!(f.return_type.is_some());
    assert_eq!(
        f.body.stmts.len(),
        1,
        "body should have the return statement"
    );
    // Display check: `a: Int, b: Int`
    let p: Vec<String> = f.params.iter().map(shape).collect();
    assert_eq!(p, vec!["a: Int", "b: Int"]);
}

#[test]
fn test_func_decl_no_return() {
    let f = parse_func("func bar() { print(\"hi\") }");
    assert_eq!(f.name.name, "bar");
    assert!(f.return_type.is_none());
    assert_eq!(f.body.stmts.len(), 1);
    assert!(matches!(f.body.stmts[0], Stmt::ExprStmt(_, _)));
}

#[test]
fn test_func_decl_via_parse_top_level() {
    // Top-level `parse()` should also handle func decls and produce a Vec<Decl>.
    let tokens = tokenize("func foo() { }", sid()).expect("lex");
    let decls = parse(&tokens, sid()).expect("parse");
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        buff_lang_ast::Decl::FuncDecl(f) => assert_eq!(f.name.name, "foo"),
        other => panic!("expected FuncDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6. for loops
// ---------------------------------------------------------------------------

#[test]
fn test_for_in() {
    let s = parse_stmt("for x in items { print(x) }");
    match s {
        Stmt::ForIn {
            var, iter, body, ..
        } => {
            assert_eq!(var.name, "x");
            assert!(matches!(iter, Expr::Ident(_, _)));
            assert_eq!(body.stmts.len(), 1);
        }
        other => panic!("expected ForIn, got {other:?}"),
    }
}

#[test]
fn test_for_while() {
    let s = parse_stmt("for count > 0 { count -= 1 }");
    match s {
        Stmt::ForWhile { cond, body, .. } => {
            // cond should be a binary `count > 0`
            assert!(matches!(
                cond,
                Expr::BinaryOp {
                    op: BinaryOp::Gt,
                    ..
                }
            ));
            assert_eq!(body.stmts.len(), 1);
        }
        other => panic!("expected ForWhile, got {other:?}"),
    }
}

// BUG-9: `while cond { body }` parses to Stmt::While (brace form).
#[test]
fn test_while_braces() {
    let s = parse_stmt("while count > 0 { count -= 1 }");
    match s {
        Stmt::While { cond, body, .. } => {
            // cond should be a binary `count > 0`
            assert!(matches!(
                cond,
                Expr::BinaryOp {
                    op: BinaryOp::Gt,
                    ..
                }
            ));
            assert_eq!(body.stmts.len(), 1);
        }
        other => panic!("expected While, got {other:?}"),
    }
}

// BUG-9: `while cond:` + indent + body + dedent parses to Stmt::While
// (layout form). parse_block handles both brace and layout forms.
#[test]
fn test_while_layout() {
    let src = "while x < 10:\n    print(x)\n    x = x + 1";
    let s = parse_stmt(src);
    match s {
        Stmt::While { cond, body, .. } => {
            // cond should be a binary `x < 10`
            assert!(matches!(
                cond,
                Expr::BinaryOp {
                    op: BinaryOp::Lt,
                    ..
                }
            ));
            assert_eq!(body.stmts.len(), 2, "layout body has two stmts");
        }
        other => panic!("expected While, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 7. Expression statement + blocks
// ---------------------------------------------------------------------------

#[test]
fn test_expr_stmt() {
    let s = parse_stmt("print(x)");
    match s {
        Stmt::ExprStmt(Expr::FuncCall { args, .. }, _) => {
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected ExprStmt(FuncCall), got {other:?}"),
    }
}

#[test]
fn test_block_braces_empty() {
    let b = parse_block("{ }");
    assert!(b.stmts.is_empty());
}

#[test]
fn test_block_braces_multi() {
    let b = parse_block("{ let x = 1\nlet y = 2 }");
    assert_eq!(b.stmts.len(), 2);
}

// ---------------------------------------------------------------------------
// 8. Type references
// ---------------------------------------------------------------------------

#[test]
fn test_type_annotation_named() {
    let t = parse_ty("Int");
    assert!(matches!(t, TypeRef::Named { .. }));
    assert_eq!(shape(&t), "Int");
}

#[test]
fn test_type_annotation_generic() {
    let t = parse_ty("Vector<Int>");
    let display = shape(&t);
    match &t {
        TypeRef::Generic { base, args, .. } => {
            assert!(matches!(**base, TypeRef::Named { .. }));
            assert_eq!(args.len(), 1);
            assert!(matches!(args[0], TypeRef::Named { .. }));
        }
        other => panic!("expected Generic, got {other:?}"),
    }
    assert_eq!(display, "Vector<Int>");
}

#[test]
fn test_type_annotation_in_let() {
    let s = parse_stmt("let v: Vector<Int> = vec");
    match s {
        Stmt::LetDecl { ty, .. } => {
            let t = ty.expect("type annotation present");
            assert!(matches!(t, TypeRef::Generic { .. }));
            assert_eq!(shape(&t), "Vector<Int>");
        }
        other => panic!("expected LetDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 9. Nested constructs
// ---------------------------------------------------------------------------

#[test]
fn test_nested_blocks_if_in_func() {
    let f = parse_func("func foo() { if x { print(1) } }");
    assert_eq!(f.body.stmts.len(), 1);
    match &f.body.stmts[0] {
        Stmt::ExprStmt(Expr::IfExpr { then_block, .. }, _) => {
            assert_eq!(then_block.stmts.len(), 1);
        }
        other => panic!("expected ExprStmt(IfExpr), got {other:?}"),
    }
}

#[test]
fn test_nested_for_in_func() {
    let f = parse_func("func count(items: List<Int>) { for x in items { print(x) } }");
    assert_eq!(f.body.stmts.len(), 1);
    assert!(matches!(f.body.stmts[0], Stmt::ForIn { .. }));
}

// ---------------------------------------------------------------------------
// 10. Error paths
// ---------------------------------------------------------------------------

#[test]
fn test_func_at_stmt_level_errors() {
    let err = parse_stmt_err("func foo() { }");
    assert!(
        err.diagnostic.message.contains("top-level"),
        "message was: {}",
        err.diagnostic.message
    );
}

#[test]
fn test_async_func_top_level_parses_with_is_async_flag() {
    // T31: `async func foo() { }` is now valid Buff syntax. The `async`
    // modifier is consumed by `parse_func_decl` and sets `is_async = true`
    // on the resulting FuncDecl. The dispatcher in `parse()` routes
    // `async func` (KwAsync followed by KwFunc) to `parse_func_decl` so
    // the modifier is handled in one place.
    //
    // (Pre-T31 this test asserted the OPPOSITE — that `async func` was a
    // top-level error — because T8 had deferred the async modifier. T31
    // implements it; this test now pins the GREEN behavior.)
    let tokens = tokenize("async func foo() { }", sid()).expect("lex");
    let decls = parse(&tokens, sid()).expect("async func must now parse");
    assert_eq!(decls.len(), 1, "expected exactly one top-level decl");
    match &decls[0] {
        buff_lang_ast::Decl::FuncDecl(f) => {
            assert_eq!(f.name.name, "foo");
            assert!(f.is_async, "is_async must be true for `async func`");
            assert!(!f.is_unsafe);
            assert!(!f.is_extern);
        }
        other => panic!("expected FuncDecl, got {other:?}"),
    }
}

#[test]
fn test_let_missing_value_errors() {
    let err = parse_stmt_err("let x = ");
    // Either expect or parse_expression will fail — both yield a ParseError.
    assert_eq!(err.diagnostic.severity, buff_lang_error::Severity::Error);
}

#[test]
fn test_return_inside_block_stops_at_brace() {
    // return with no value, followed by `}` — should parse as Return(None).
    let b = parse_block("{ return }");
    assert_eq!(b.stmts.len(), 1);
    assert!(matches!(b.stmts[0], Stmt::Return(None, _)));
}

#[test]
fn test_param_list_with_multiple_args() {
    let f = parse_func("func f(a: Int, b: Int, c: Int) { }");
    let names: Vec<&str> = f.params.iter().map(|p| p.name.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

// ---------------------------------------------------------------------------
// 11. Display / shape sanity
// ---------------------------------------------------------------------------

#[test]
fn test_stmt_display_let() {
    // Build a LetDecl manually and check Display output round-trips.
    let s = Stmt::LetDecl {
        name: Ident::new("x", dummy_span()),
        value: int_lit(42),
        mutable: false,
        ty: None,
        span: dummy_span(),
    };
    assert_eq!(shape(&s), "LetDecl(x = Lit(Int(42)))");
}

#[test]
fn test_for_in_display() {
    let s = Stmt::ForIn {
        var: Ident::new("i", dummy_span()),
        iter: ident_expr("items"),
        body: Block::empty(dummy_span()),
        span: dummy_span(),
    };
    assert_eq!(shape(&s), "ForIn(i in Ident(items) { })");
}

#[test]
fn test_for_while_display() {
    let s = Stmt::ForWhile {
        cond: ident_expr("running"),
        body: Block::empty(dummy_span()),
        span: dummy_span(),
    };
    assert_eq!(shape(&s), "ForWhile(Ident(running) { })");
}

// ---------------------------------------------------------------------------
// 12. Param Display helper
// ---------------------------------------------------------------------------

#[test]
fn test_param_display() {
    let p = Param {
        name: Ident::new("x", dummy_span()),
        ty: named_type("Int"),
        default_value: None,
        is_comptime: false,
        span: dummy_span(),
    };
    assert_eq!(shape(&p), "x: Int");
}

// Sanity check: ensure that our ParseError round-trips through Diagnostic
// severity checks properly.
#[test]
fn test_parse_error_severity() {
    let err = parse_stmt_err("let 42 = x");
    assert_eq!(err.diagnostic.severity, buff_lang_error::Severity::Error);
}
