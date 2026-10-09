//! Regression tests for multi-level dedents landing on `else` lines (ITER-36).
//!
//! Root cause these tests pin: when the LAST statement of an `if`/`else if`
//! arm is itself a layout block (nested `if`, `for`, ...), the arm's closing
//! and the nested block's closing produce TWO (or more) `Dedent` tokens in a
//! row before an `else`/`else if` that belongs to the OUTER chain.
//! [`TokenStream::peek_kind`] transparently skips leftover `Dedent` tokens,
//! so the inner `if` used to "steal" the outer `else`, swallow the rest of
//! the enclosing function body, and surface as a bogus
//! "function declarations must be top-level" error on the NEXT top-level
//! declaration.
//!
//! The corpus files blocked by this bug (self-host bootstrap, ITER-36):
//!
//! - `self-host/lexer/lexer.buff`            (error at 1040:1)
//! - `self-host/lexer/string_interp.buff`    (error at 163:1)
//! - `self-host/codegen/dependency_detection.buff` (error at 316:1)
//! - `self-host/codegen/passes.buff`         (error at 221:1)
//!
//! Layout invariant under test: an `else` attaches to the nearest `if` at
//! the SAME (or outer) indentation level — a `Dedent` between a then-block
//! and an `else` is a hard boundary the else-attachment must NOT cross.
//! Newlines between the block and the `else` remain transparent (brace-form
//! `if c { ... }` followed by a newline then `else` still attaches).

use buff_lang_ast::{Decl, Expr, Stmt};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

fn sid() -> SourceId {
    SourceId(0)
}

/// Parse `src` and assert exactly `n` top-level declarations come out.
fn parse_n_decls(src: &str, n: usize) -> Vec<Decl> {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let decls = parse(&tokens, sid()).expect("parser should succeed");
    assert_eq!(
        decls.len(),
        n,
        "expected exactly {n} top-level decls, got {}: {decls:#?}",
        decls.len()
    );
    decls
}

/// Borrow the `Expr::IfExpr` out of a statement slot, or panic with context.
fn as_if_expr(stmt: &Stmt) -> &Expr {
    match stmt {
        Stmt::ExprStmt(expr, _) => expr,
        other => panic!("expected ExprStmt, got {other:#?}"),
    }
}

/// Unwrap an `Expr::IfExpr` or panic with context.
fn unwrap_if_expr(
    expr: &Expr,
) -> (
    &buff_lang_ast::Expr,
    &buff_lang_ast::Block,
    Option<&buff_lang_ast::Block>,
) {
    match expr {
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => (cond, then_block, else_block.as_ref()),
        other => panic!("expected IfExpr, got {other:#?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. The four corpus shapes (RED before the ITER-36 fix)
// ---------------------------------------------------------------------------

/// Shape from `self-host/lexer/string_interp.buff` / `lexer.buff`: an
/// `else if` arm whose ONLY statement is a nested `if`; the outer `else`
/// sits at a dedent of two levels from the nested body.
#[test]
fn else_if_arm_nested_if_last_then_outer_else() {
    let src = "\
func f(start: Int) -> Int:
    if start == 1:
        start = start + 1
    else if start == 2:
        if start < 100:
            start = start + 1
    else:
        start = start + 2
    return start

func g() -> Int:
    return 1";
    let decls = parse_n_decls(src, 2);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    // Body: [if-chain, return] — the `return` must stay in f's body.
    assert_eq!(f.body.stmts.len(), 2, "if-chain + return expected in body");
    let (outer_cond, _outer_then, outer_else) = unwrap_if_expr(as_if_expr(&f.body.stmts[0]));
    match outer_cond {
        Expr::BinaryOp { .. } => {}
        other => panic!("outer cond should be a comparison, got {other:#?}"),
    }
    // The outer if HAS an else (the `else: start = start + 2` arm).
    let outer_else = outer_else.expect("outer if must own the trailing else");
    // `else if` desugars: the outer else block holds the nested else-if.
    let (_, else_if_then, else_if_else) = unwrap_if_expr(as_if_expr(&outer_else.stmts[0]));
    assert_eq!(
        else_if_then.stmts.len(),
        1,
        "else-if arm holds only the nested if"
    );
    // The NESTED if (last stmt of the else-if arm) must NOT steal the else.
    let (_, _, nested_else) = unwrap_if_expr(as_if_expr(&else_if_then.stmts[0]));
    assert!(
        nested_else.is_none(),
        "nested if must not steal the outer chain's else"
    );
    // The else-if member of the chain owns it instead.
    assert!(
        else_if_else.is_some(),
        "else-if member must own the trailing else"
    );
}

/// Shape from `self-host/codegen/dependency_detection.buff`: the FIRST
/// arm ends with a nested `if` and the chain's `else` follows a two-level
/// dedent.
#[test]
fn then_arm_nested_if_last_stmt_then_else() {
    let src = "\
func f(start: Int) -> Int:
    if start == 1:
        if start < 100:
            start = start + 1
        if start < 50:
            start = start + 2
    else:
        start = start + 3
    return start

func g() -> Int:
    return 1";
    let decls = parse_n_decls(src, 2);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    assert_eq!(f.body.stmts.len(), 2, "if-chain + return expected in body");
    let (_, outer_then, outer_else) = unwrap_if_expr(as_if_expr(&f.body.stmts[0]));
    assert_eq!(outer_then.stmts.len(), 2, "two nested ifs in then arm");
    // Second nested if is the last stmt — it must NOT own the else.
    let (_, _, nested_else) = unwrap_if_expr(as_if_expr(&outer_then.stmts[1]));
    assert!(
        nested_else.is_none(),
        "nested if must not steal the outer chain's else"
    );
    assert!(outer_else.is_some(), "outer if must own the trailing else");
}

/// Shape from `self-host/codegen/passes.buff` / `lexer.buff`: an `else if`
/// arm holding a `for` loop followed by a sibling `if`; the outer `else`
/// lands after a two-level dedent.
#[test]
fn else_if_arm_for_then_if_then_outer_else() {
    let src = "\
func f(start: Int) -> Int:
    let mut i = start
    if i == 1:
        i = i + 1
    else if i == 2:
        i = i + 1
        for i < 100:
            i = i + 1
        if i < 100:
            i = i + 1
    else:
        i = i + 2
    return i

func g() -> Int:
    return 1";
    let decls = parse_n_decls(src, 2);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    assert_eq!(f.body.stmts.len(), 3, "let + if-chain + return expected");
    let (_, _then, outer_else) = unwrap_if_expr(as_if_expr(&f.body.stmts[1]));
    let outer_else = outer_else.expect("outer if must own the trailing else");
    let (_, else_if_then, else_if_else) = unwrap_if_expr(as_if_expr(&outer_else.stmts[0]));
    // else-if arm: stmt, for, if — exactly 3 statements.
    assert_eq!(else_if_then.stmts.len(), 3, "else-if arm: stmt + for + if");
    let (_, _, nested_if_else) = unwrap_if_expr(as_if_expr(&else_if_then.stmts[2]));
    assert!(
        nested_if_else.is_none(),
        "trailing nested if must not steal the else"
    );
    assert!(
        else_if_else.is_some(),
        "else-if member must own the trailing else"
    );
}

/// `for` as the last statement of an `else if` arm (the literal
/// `passes.buff`/`find_matching_brace` shape: `for` + `if` siblings, no
/// leading plain statement).
#[test]
fn else_if_arm_for_and_if_only_then_outer_else() {
    let src = "\
func f(start: Int) -> Int:
    let mut i = start
    if i == 1:
        i = i + 1
    else if i == 2:
        for i < 100:
            i = i + 1
        if i < 100:
            i = i + 1
    else:
        i = i + 2
    return i

func g() -> Int:
    return 1";
    let decls = parse_n_decls(src, 2);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    assert_eq!(f.body.stmts.len(), 3, "let + if-chain + return expected");
}

/// End-to-end trimmed `string_interp.buff` shape: the trap inside a `for`
/// loop inside the function, then a fresh top-level `func` — the original
/// failure mode ("function declarations must be top-level" at the NEXT
/// declaration).
#[test]
fn corpus_string_interp_shape_two_funcs() {
    let src = "\
func find_matching_brace(start: Int) -> Result<Int, LexerError>:
    let mut i = start
    for i < 100:
        let b = 1
        if b == 1:
            i = i + 1
        else if b == 2:
            i = i + 1
            for i < 100:
                i = i + 1
            if i < 100:
                i = i + 1
        else:
            i = i + 1
    return i

func split_interp_spec(start: Int) -> Int:
    return start";
    let decls = parse_n_decls(src, 2);
    let second_name = match &decls[1] {
        Decl::FuncDecl(f) => f.name.name.clone(),
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    assert_eq!(second_name, "split_interp_spec");
}

// ---------------------------------------------------------------------------
// 2. Controls: shapes that must keep working (were green before, must stay
//    green after the fix)
// ---------------------------------------------------------------------------

/// Two-level dedent landing on a PLAIN statement (not an `else`) — never
/// steals anything; pins the non-else path of the same dedent family.
#[test]
fn double_dedent_onto_plain_statement_still_parses() {
    let src = "\
func f(start: Int) -> Int:
    if start == 1:
        if start < 100:
            start = start + 1
        if start < 50:
            start = start + 2
    return start

func g() -> Int:
    return 1";
    let decls = parse_n_decls(src, 2);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    assert_eq!(f.body.stmts.len(), 2, "if + return expected in body");
}

/// Single-dedent `else` (the common chain form) keeps attaching.
#[test]
fn single_dedent_else_attaches_to_chain() {
    let src = "\
func f(start: Int) -> Int:
    if start == 1:
        start = start + 1
    else if start < 100:
        start = start + 2
    else:
        start = start + 3
    return start";
    let decls = parse_n_decls(src, 1);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    let (_, _, outer_else) = unwrap_if_expr(as_if_expr(&f.body.stmts[0]));
    assert!(outer_else.is_some(), "chain else must still attach");
}

/// Python-style semantics: an `else` at the OUTER if's column binds to the
/// outer if even when the inner if is the last statement of the outer arm.
/// (Pre-fix this mis-attached to the inner if.)
#[test]
fn else_at_outer_level_binds_to_outer_if() {
    let src = "\
func f(x: Int) -> Int:
    if x == 1:
        x = x + 1
        if x < 100:
            x = x + 1
    else:
        x = x + 2
    return x";
    let decls = parse_n_decls(src, 1);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    let (_, outer_then, outer_else) = unwrap_if_expr(as_if_expr(&f.body.stmts[0]));
    let (_, _, nested_else) = unwrap_if_expr(as_if_expr(&outer_then.stmts[1]));
    assert!(
        nested_else.is_none(),
        "inner if (deeper column) must not own the outer else"
    );
    assert!(
        outer_else.is_some(),
        "else at the outer if's column binds to the outer if"
    );
}

/// Brace-form dangling-else still binds to the nearest if (unchanged
/// single-line behaviour).
#[test]
fn brace_form_dangling_else_unchanged() {
    let src = "\
func f(x: Int) -> Int:
    if x == 1 { x = 10 } else { x = 20 }
    return x";
    let decls = parse_n_decls(src, 1);
    let f = match &decls[0] {
        Decl::FuncDecl(f) => f,
        other => panic!("expected FuncDecl, got {other:#?}"),
    };
    let (_, _, outer_else) = unwrap_if_expr(as_if_expr(&f.body.stmts[0]));
    assert!(outer_else.is_some(), "brace-form else must still attach");
}
