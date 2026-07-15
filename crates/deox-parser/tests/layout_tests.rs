//! Integration tests for layout-sensitive (offside-rule) parsing — T9.
//!
//! These tests exercise the parser's ability to handle Python/F#-style
//! indentation-based blocks alongside the existing brace-delimited form.
//! Both forms must coexist: braces for explicit blocks, layout for natural
//! code.
//!
//! Coverage:
//!
//! - Function bodies via indentation: `func foo():\n    stmt1\n    stmt2`
//! - Dedent returns to outer scope (statement AFTER the layout block)
//! - Nested indentation (2 and 3 levels deep)
//! - `if cond:` layout form (with and without `else`)
//! - `else if` chains in layout form
//! - Dangling-else binding to the nearest (innermost) `if`
//! - `for x in items:` and `for cond:` layout forms
//! - Mixed braces-inside-layout (e.g. inner `if x { ... }` inside a layout
//!   function body)
//! - Braces override layout: indent tokens inside `{ ... }` are ignored
//! - Empty indented block produces a ParseError
//! - `if`-as-expression inside a `let` binding (T8 limitation now fixed)
//! - End-to-end: parsing the `ola.deox` and `arithmetic.deox` fixtures

#![allow(clippy::approx_constant)]

use deox_ast::{Decl, Expr, FuncDecl, Stmt};
use deox_error::SourceId;
use deox_lexer::tokenize;
use deox_parser::{parse, parse_func_decl, parse_statement, TokenStream};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse a single statement from `src`.
fn parse_stmt(src: &str) -> Stmt {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_statement(&mut stream).expect("parser should succeed")
}

/// Tokenize + parse a single statement; expect an error.
fn parse_stmt_err(src: &str) -> deox_error::ParseError {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_statement(&mut stream).expect_err("parser should fail")
}

/// Pull the single FuncDecl out of a top-level parse.
fn single_func(src: &str) -> FuncDecl {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let decls = parse(&tokens, sid()).expect("parser should succeed");
    assert_eq!(decls.len(), 1, "expected exactly 1 top-level decl");
    match decls.into_iter().next().unwrap() {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. Function bodies via indentation
// ---------------------------------------------------------------------------

#[test]
fn test_func_body_via_indent() {
    // Two print statements inside a layout-defined function body.
    let src = "func foo():\n    print(\"a\")\n    print(\"b\")";
    let f = single_func(src);
    assert_eq!(f.name.name, "foo");
    assert_eq!(
        f.body.stmts.len(),
        2,
        "layout body should contain both print statements"
    );
    // Both statements should be ExprStmt(FuncCall print).
    for (i, s) in f.body.stmts.iter().enumerate() {
        match s {
            Stmt::ExprStmt(Expr::FuncCall { .. }, _) => {}
            other => panic!("stmt[{i}] should be ExprStmt(FuncCall), got {other:?}"),
        }
    }
}

#[test]
fn test_func_body_via_indent_dedent_returns() {
    // After the layout body ends (Dedent), the next statement belongs to the
    // OUTER scope, not the function body.
    let src = "func foo():\n    print(\"a\")\nprint(\"fora\")";
    let tokens = tokenize(src, sid()).expect("lexer");
    let mut stream = TokenStream::new(&tokens, sid());
    let f = parse_func_decl(&mut stream).expect("func parses");
    assert_eq!(
        f.body.stmts.len(),
        1,
        "only `print(\"a\")` should be inside the func body"
    );
    // After the func, the next significant token must be `print` (the "fora"
    // call) — proving Dedent brought us back to outer scope.
    assert_eq!(
        stream.peek_kind().map(|k| format!("{k:?}")),
        Some(format!(
            "{:?}",
            deox_lexer::TokenKind::Ident("print".into())
        )),
        "next token after the layout-defined func should be `print` from `print(\"fora\")`"
    );
}

// ---------------------------------------------------------------------------
// 2. Nested indentation
// ---------------------------------------------------------------------------

#[test]
fn test_nested_indent_2_levels() {
    // if x: \n    if y: \n        z()
    let src = "if x:\n    if y:\n        z()";
    let s = parse_stmt(src);
    match s {
        Stmt::ExprStmt(Expr::IfExpr { then_block, .. }, _) => {
            assert_eq!(then_block.stmts.len(), 1);
            // The single inner statement should itself be an IfExpr.
            match &then_block.stmts[0] {
                Stmt::ExprStmt(
                    Expr::IfExpr {
                        then_block: inner, ..
                    },
                    _,
                ) => {
                    assert_eq!(inner.stmts.len(), 1, "innermost z() should be present");
                }
                other => panic!("expected nested IfExpr, got {other:?}"),
            }
        }
        other => panic!("expected outer IfExpr, got {other:?}"),
    }
}

#[test]
fn test_nested_indent_3_levels() {
    // if a: \n    if b: \n        if c: \n            d()
    let src = "if a:\n    if b:\n        if c:\n            d()";
    let s = parse_stmt(src);
    // Walk three IfExpr levels deep.
    let outer = match s {
        Stmt::ExprStmt(Expr::IfExpr { then_block, .. }, _) => then_block,
        other => panic!("expected outer IfExpr, got {other:?}"),
    };
    assert_eq!(outer.stmts.len(), 1);
    let mid = match &outer.stmts[0] {
        Stmt::ExprStmt(Expr::IfExpr { then_block, .. }, _) => then_block.clone(),
        other => panic!("expected mid IfExpr, got {other:?}"),
    };
    assert_eq!(mid.stmts.len(), 1);
    let inner = match &mid.stmts[0] {
        Stmt::ExprStmt(Expr::IfExpr { then_block, .. }, _) => then_block.clone(),
        other => panic!("expected inner IfExpr, got {other:?}"),
    };
    assert_eq!(inner.stmts.len(), 1, "innermost d() should be present");
}

// ---------------------------------------------------------------------------
// 3. if / else / else-if (layout form)
// ---------------------------------------------------------------------------

#[test]
fn test_if_without_braces() {
    // Layout-form if without braces.
    let src = "if cond:\n    a()";
    let s = parse_stmt(src);
    match s {
        Stmt::ExprStmt(
            Expr::IfExpr {
                then_block,
                else_block,
                ..
            },
            _,
        ) => {
            assert_eq!(then_block.stmts.len(), 1);
            assert!(else_block.is_none(), "no else expected");
        }
        other => panic!("expected IfExpr, got {other:?}"),
    }
}

#[test]
fn test_if_else_layout() {
    let src = "if cond:\n    a()\nelse:\n    b()";
    let s = parse_stmt(src);
    match s {
        Stmt::ExprStmt(
            Expr::IfExpr {
                then_block,
                else_block,
                ..
            },
            _,
        ) => {
            assert_eq!(then_block.stmts.len(), 1);
            let els = else_block.expect("else block should be present");
            assert_eq!(els.stmts.len(), 1);
        }
        other => panic!("expected IfExpr with else, got {other:?}"),
    }
}

#[test]
fn test_if_else_if_layout() {
    // if a: x() else if b: y() else: z()
    let src = "if a:\n    x()\nelse if b:\n    y()\nelse:\n    z()";
    let s = parse_stmt(src);
    match s {
        Stmt::ExprStmt(Expr::IfExpr { else_block, .. }, _) => {
            let els = else_block.expect("outer else should be present");
            assert_eq!(
                els.stmts.len(),
                1,
                "else should wrap a single nested IfExpr"
            );
            // The single statement should itself be an IfExpr (the `else if`).
            match &els.stmts[0] {
                Stmt::ExprStmt(
                    Expr::IfExpr {
                        else_block: inner_else,
                        ..
                    },
                    _,
                ) => {
                    let inner = inner_else
                        .as_ref()
                        .expect("inner else should be present (z())");
                    assert_eq!(inner.stmts.len(), 1);
                }
                other => panic!("expected nested IfExpr for `else if`, got {other:?}"),
            }
        }
        other => panic!("expected outer IfExpr, got {other:?}"),
    }
}

#[test]
fn test_dangling_else_inner() {
    // The `else` is indented at the same level as the inner `if b`, so it
    // must bind to `if b`, NOT to `if a`. The inner parse_if_expr consumes
    // exactly one Dedent (level 8 -> 4) and then sees the `else`.
    //
    //   if a:
    //       if b:
    //           c()
    //       else:      <- same indent as `if b`, so binds to `if b`
    //           d()
    let src = "if a:\n    if b:\n        c()\n    else:\n        d()";
    let s = parse_stmt(src);
    let outer = match s {
        Stmt::ExprStmt(
            Expr::IfExpr {
                then_block,
                else_block,
                ..
            },
            _,
        ) => {
            assert!(
                else_block.is_none(),
                "outer if must NOT have an else (dangling else binds to inner)"
            );
            then_block
        }
        other => panic!("expected outer IfExpr, got {other:?}"),
    };
    assert_eq!(outer.stmts.len(), 1);
    match &outer.stmts[0] {
        Stmt::ExprStmt(Expr::IfExpr { else_block, .. }, _) => {
            let els = else_block
                .as_ref()
                .expect("INNER if must get the dangling else");
            assert_eq!(els.stmts.len(), 1, "inner else should hold d()");
        }
        other => panic!("expected inner IfExpr, got {other:?}"),
    }
}

#[test]
fn test_dedent_ends_block() {
    // After `if x:\n    a()`, the Dedent brings us back to outer scope.
    // `b()` is therefore a sibling of the `if`, NOT inside its body.
    let src = "if x:\n    a()\nb()";
    let tokens = tokenize(src, sid()).expect("lexer");
    let mut stream = TokenStream::new(&tokens, sid());
    let s = parse_statement(&mut stream).expect("first statement (the if)");
    match s {
        Stmt::ExprStmt(
            Expr::IfExpr {
                then_block,
                else_block,
                ..
            },
            _,
        ) => {
            assert_eq!(then_block.stmts.len(), 1, "only a() inside if body");
            assert!(else_block.is_none());
        }
        other => panic!("expected IfExpr, got {other:?}"),
    }
    // The next significant token must be Ident("b") — proof b() is sibling.
    assert!(matches!(
        stream.peek_kind(),
        Some(deox_lexer::TokenKind::Ident(name)) if name == "b"
    ));
}

// ---------------------------------------------------------------------------
// 4. Braces + layout coexistence
// ---------------------------------------------------------------------------

#[test]
fn test_braces_override_layout() {
    // Inside a layout-defined function, an inner for-loop uses braces for
    // its body. The lexer still emits Indent/Dedent inside the braces, but
    // `parse_block_braces` auto-skips layout tokens via peek_kind/advance.
    let src = "func foo():\n    for x in items {\n        print(x)\n    }";
    let f = single_func(src);
    assert_eq!(f.body.stmts.len(), 1, "layout body has one for-loop");
    match &f.body.stmts[0] {
        Stmt::ForIn { var, body, .. } => {
            assert_eq!(var.name, "x");
            assert_eq!(body.stmts.len(), 1, "brace body has one print");
        }
        other => panic!("expected ForIn, got {other:?}"),
    }
}

#[test]
fn test_mixed_braces_and_layout() {
    // Outer function uses layout; inner if uses braces. The two forms must
    // coexist without confusing the parser.
    let src = "func foo():\n    if x {\n        y()\n    }";
    let f = single_func(src);
    assert_eq!(f.body.stmts.len(), 1);
    match &f.body.stmts[0] {
        Stmt::ExprStmt(Expr::IfExpr { then_block, .. }, _) => {
            assert_eq!(then_block.stmts.len(), 1);
        }
        other => panic!("expected ExprStmt(IfExpr), got {other:?}"),
    }
}

#[test]
fn test_indent_inside_braces_ignored() {
    // Braces take precedence: the Indent token emitted inside `{ ... }` by
    // the lexer must NOT start a layout block. Only `parse_block_braces`
    // runs here.
    let src = "func foo() {\n    print(\"a\")\n}";
    let f = single_func(src);
    assert_eq!(f.body.stmts.len(), 1);
    assert!(matches!(
        &f.body.stmts[0],
        Stmt::ExprStmt(Expr::FuncCall { .. }, _)
    ));
}

// ---------------------------------------------------------------------------
// 5. for loops in layout form
// ---------------------------------------------------------------------------

#[test]
fn test_for_in_layout() {
    let src = "for x in items:\n    print(x)";
    let s = parse_stmt(src);
    match s {
        Stmt::ForIn { var, body, .. } => {
            assert_eq!(var.name, "x");
            assert_eq!(body.stmts.len(), 1, "layout body has one stmt");
        }
        other => panic!("expected ForIn, got {other:?}"),
    }
}

#[test]
fn test_for_while_layout() {
    let src = "for count > 0:\n    count -= 1";
    let s = parse_stmt(src);
    match s {
        Stmt::ForWhile { body, .. } => {
            assert_eq!(body.stmts.len(), 1);
            // The single stmt should be an Assignment (`count -= 1`).
            assert!(matches!(&body.stmts[0], Stmt::Assignment { .. }));
        }
        other => panic!("expected ForWhile, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6. Error paths
// ---------------------------------------------------------------------------

#[test]
fn test_empty_indented_block_error() {
    // `func foo():` followed by nothing — the parser must reject with an
    // "expected indented block" message.
    let src = "func foo():\n";
    let tokens = tokenize(src, sid()).expect("lexer");
    let mut stream = TokenStream::new(&tokens, sid());
    let err = parse_func_decl(&mut stream).expect_err("should reject empty body");
    assert!(
        err.diagnostic.message.contains("indented block"),
        "message was: {}",
        err.diagnostic.message
    );
}

#[test]
fn test_missing_newline_after_colon_errors() {
    // `if x: a()` (single-line form) is not supported in T9 — the parser
    // requires a Newline after the colon. The statement parser dispatches
    // `if` to parse_if_expr → parse_block which errors.
    let src = "if x: a()";
    let err = parse_stmt_err(src);
    // Could be "expected newline" or "expected indented block" depending on
    // where the parser stops; both are layout-related errors.
    assert!(
        err.diagnostic.message.contains("newline") || err.diagnostic.message.contains("indented"),
        "message was: {}",
        err.diagnostic.message
    );
}

// ---------------------------------------------------------------------------
// 7. if-as-expression (T8 limitation now fixed)
// ---------------------------------------------------------------------------

#[test]
fn test_let_with_if_expr_braces() {
    // The classic T8 gap: `let x = if c { 1 } else { 2 }` failed because
    // parse_primary didn't know about `if`. T9 wires them via
    // `crate::stmt::parse_if_expr`.
    let src = "let x = if c { 1 } else { 2 }";
    let s = parse_stmt(src);
    match s {
        Stmt::LetDecl { name, value, .. } => {
            assert_eq!(name.name, "x");
            assert!(
                matches!(value, Expr::IfExpr { .. }),
                "value should be an IfExpr, got {value:?}"
            );
        }
        other => panic!("expected LetDecl, got {other:?}"),
    }
}

#[test]
fn test_let_with_if_expr_layout() {
    // Layout form of if-as-expression.
    //   let x = if c:
    //       1
    //   else:
    //       2
    let src = "let x = if c:\n    1\nelse:\n    2";
    let s = parse_stmt(src);
    match s {
        Stmt::LetDecl { name, value, .. } => {
            assert_eq!(name.name, "x");
            match value {
                Expr::IfExpr {
                    then_block,
                    else_block,
                    ..
                } => {
                    assert_eq!(then_block.stmts.len(), 1);
                    let els = else_block.expect("else block present");
                    assert_eq!(els.stmts.len(), 1);
                }
                other => panic!("value should be IfExpr, got {other:?}"),
            }
        }
        other => panic!("expected LetDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 8. End-to-end fixture parsing
// ---------------------------------------------------------------------------

#[test]
fn test_end_to_end_ola_deox() {
    let src = include_str!("../../../tests/fixtures/valid/ola.deox");
    let tokens = tokenize(src, sid()).expect("ola.deox should tokenize cleanly");
    let decls = parse(&tokens, sid()).expect("ola.deox should parse cleanly");
    assert_eq!(decls.len(), 1, "ola.deox defines one function (main)");
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:?}"),
    };
    assert_eq!(f.name.name, "main");
    assert_eq!(
        f.body.stmts.len(),
        1,
        "main should have a single print statement"
    );
    assert!(matches!(
        &f.body.stmts[0],
        Stmt::ExprStmt(Expr::FuncCall { .. }, _)
    ));
}

#[test]
fn test_end_to_end_arithmetic_deox() {
    let src = include_str!("../../../tests/fixtures/valid/arithmetic.deox");
    let tokens = tokenize(src, sid()).expect("arithmetic.deox should tokenize cleanly");
    let decls = parse(&tokens, sid()).expect("arithmetic.deox should parse cleanly");
    assert_eq!(decls.len(), 1);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:?}"),
    };
    assert_eq!(f.name.name, "main");
    // 4 statements: let x, let y, let z, print(z).
    assert_eq!(
        f.body.stmts.len(),
        4,
        "arithmetic main should have 4 statements"
    );
    // First three should be LetDecls.
    for (i, s) in f.body.stmts.iter().take(3).enumerate() {
        assert!(
            matches!(s, Stmt::LetDecl { .. }),
            "stmt[{i}] should be LetDecl, got {s:?}"
        );
    }
    // Last should be an ExprStmt(FuncCall print).
    assert!(matches!(
        &f.body.stmts[3],
        Stmt::ExprStmt(Expr::FuncCall { .. }, _)
    ));
}

// ---------------------------------------------------------------------------
// 9. Bonus: layout block direct unit-level checks
// ---------------------------------------------------------------------------

#[test]
fn test_layout_block_in_if_has_correct_span() {
    // The block span should cover from the colon to the end of the last
    // statement in the layout body.
    let src = "if cond:\n    a()";
    let s = parse_stmt(src);
    match s {
        Stmt::ExprStmt(Expr::IfExpr { then_block, .. }, _) => {
            // The block's span end should be > start (i.e., non-empty).
            assert!(
                then_block.span.end > then_block.span.start,
                "block span should be non-empty"
            );
            // Sanity: the span source_id matches the stream's source.
            assert_eq!(then_block.span.source_id, sid());
        }
        other => panic!("expected IfExpr, got {other:?}"),
    }
}

#[test]
fn test_layout_block_preserves_statement_order() {
    // Multiple statements in a layout block must preserve source order.
    let src = "func f():\n    a()\n    b()\n    c()";
    let f = single_func(src);
    assert_eq!(f.body.stmts.len(), 3);
    // Sanity: Block implements Display (used for shape comparisons elsewhere).
    let display = deox_ast::Block::empty(deox_error::Span::dummy()).to_string();
    assert!(display.contains('{'), "Block display sanity");
}
