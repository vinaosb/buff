//! T73 integration tests — codegen for early-return guards.
//!
//! Each test hand-builds a `Stmt::Guard` (or feeds source through the
//! parser), runs it through [`buff_lang_codegen_rust::generate_rust`], and
//! asserts the resulting Rust source contains the expected early-return /
//! let-else patterns.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test guards_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, GuardCondition, Pattern, Stmt, TypeRef};
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

/// Wrap a list of statements in a zero-arg `fn f() { ... }` declaration.
fn func_with_stmts(stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident("f"),
        params: Vec::<Param>::new(),
        return_type: Some(TypeRef::Named {
            name: ident("Int"),
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

fn return_zero_stmt() -> Stmt {
    Stmt::Return(
        Some(Expr::Literal(buff_lang_ast::Literal::Int(0), span())),
        span(),
    )
}

fn return_none_stmt() -> Stmt {
    Stmt::Return(None, span())
}

// ---------------------------------------------------------------------------
// Bool condition: `guard x > 0 else { return 0 }`
// ---------------------------------------------------------------------------

#[test]
fn guards_codegen_bool_condition_early_return() {
    // Build: guard x > 0 else { return 0 }
    let x_gt_zero = Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Gt,
        lhs: Box::new(ident_expr("x")),
        rhs: Box::new(Expr::Literal(buff_lang_ast::Literal::Int(0), span())),
        span: span(),
    };
    let guard = Stmt::Guard {
        conditions: vec![GuardCondition::Bool(x_gt_zero)],
        else_block: Block {
            stmts: vec![return_zero_stmt()],
            span: span(),
        },
        span: span(),
    };
    // Following stmt uses x — to verify the early-return shape.
    let src = generate_rust(&[func_with_stmts(vec![guard, return_zero_stmt()])])
        .expect("codegen must succeed");
    // Expected Rust shape: `if !(x > 0) { return 0; }` (negated condition).
    assert!(src.contains("if !"), "expected `if !` negation in:\n{src}");
    assert!(src.contains("return 0;"), "expected `return 0;` in:\n{src}");
    // Re-parse to verify the generated Rust is syntactically valid.
    syn::parse_str::<syn::File>(&src).expect("generated Rust must parse");
}

// ---------------------------------------------------------------------------
// Let-binding condition: `guard let Some(x) = opt else { return }`
// ---------------------------------------------------------------------------

#[test]
fn guards_codegen_let_binding_let_else() {
    // Build: guard let Some(x) = opt else { return }
    let some_x_pat = Pattern::Variant {
        enum_name: ident(""),
        variant: ident("Some"),
        subpatterns: vec![Pattern::Ident(ident("x"), span())],
        span: span(),
    };
    let guard = Stmt::Guard {
        conditions: vec![GuardCondition::Let {
            pattern: some_x_pat,
            value: ident_expr("opt"),
            span: span(),
        }],
        else_block: Block {
            stmts: vec![return_none_stmt()],
            span: span(),
        },
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![guard])]).expect("codegen must succeed");
    // Expected Rust shape: `let Some(x) = opt else { return; };` (let-else).
    assert!(
        src.contains("let Some(x) = opt"),
        "expected let-else `let Some(x) = opt` in:\n{src}"
    );
    assert!(
        src.contains("else"),
        "expected `else` (let-else) in:\n{src}"
    );
    assert!(
        src.contains("return"),
        "expected `return` in else-block:\n{src}"
    );
    syn::parse_str::<syn::File>(&src).expect("generated Rust must parse");
}

// ---------------------------------------------------------------------------
// Multiple conditions: `guard let Some(x) = opt, x > 0 else { return }`
// ---------------------------------------------------------------------------

#[test]
fn guards_codegen_multiple_conditions_both_emitted() {
    // Build: guard let Some(x) = opt, x > 0 else { return }
    let some_x_pat = Pattern::Variant {
        enum_name: ident(""),
        variant: ident("Some"),
        subpatterns: vec![Pattern::Ident(ident("x"), span())],
        span: span(),
    };
    let x_gt_zero = Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Gt,
        lhs: Box::new(ident_expr("x")),
        rhs: Box::new(Expr::Literal(buff_lang_ast::Literal::Int(0), span())),
        span: span(),
    };
    let guard = Stmt::Guard {
        conditions: vec![
            GuardCondition::Let {
                pattern: some_x_pat,
                value: ident_expr("opt"),
                span: span(),
            },
            GuardCondition::Bool(x_gt_zero),
        ],
        else_block: Block {
            stmts: vec![return_none_stmt()],
            span: span(),
        },
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![guard])]).expect("codegen must succeed");
    // The let-else appears first.
    assert!(
        src.contains("let Some(x) = opt"),
        "expected let-else first in:\n{src}"
    );
    // The negated if appears second.
    assert!(
        src.contains("if !"),
        "expected `if !` negation second in:\n{src}"
    );
    syn::parse_str::<syn::File>(&src).expect("generated Rust must parse");
}

// ---------------------------------------------------------------------------
// End-to-end: feed source through lexer → parser → codegen.
// ---------------------------------------------------------------------------

#[test]
fn guards_codegen_end_to_end_from_source() {
    use buff_lang_error::SourceId;
    use buff_lang_parser::parse;

    let src_text = "func f(x: Int):\n    guard x > 0 else:\n        return 0\n    return x";
    let tokens = buff_lang_lexer::tokenize(src_text, SourceId(0)).expect("lexer");
    let decls = parse(&tokens, SourceId(0)).expect("parser");
    let rust = generate_rust(&decls).expect("codegen");
    // The guard becomes `if !(x > 0) { return 0; }`.
    assert!(rust.contains("if !"), "expected negated if in:\n{rust}");
    assert!(
        rust.contains("return 0;"),
        "expected `return 0;` in:\n{rust}"
    );
    syn::parse_str::<syn::File>(&rust).expect("generated Rust must parse");
}
