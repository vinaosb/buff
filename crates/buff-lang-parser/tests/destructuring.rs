//! T71 integration tests — destructuring `let` bindings.
//!
//! Coverage:
//!
//! - `let (x, y) = point` → `Stmt::LetPattern` with a `Pattern::Tuple`.
//! - `let (a, _, c) = t` → tuple pattern with a wildcard sub-pattern.
//! - `let Point { x, y } = p` → `Stmt::LetPattern` with a `Pattern::Struct`
//!   using shorthand fields (`{ x, y }` == `{ x: x, y: y }`).
//! - `let Point { x: a, y: b } = p` → struct pattern with explicit
//!   `field: subpattern` entries.
//! - Regression: `let x = 5` STILL produces `Stmt::LetDecl` (unchanged path).
//! - Regression: `let mut name = value` still works (bare-name `mut`).
//!
//! Each test feeds source strings through the lexer and then through
//! [`buff_lang_parser::parse_statement`]. The resulting AST is pattern-matched
//! to assert the expected shape.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser destructuring
//! ```

#![allow(clippy::approx_constant)]

use buff_lang_ast::{Expr, Ident, Pattern, Stmt};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_statement, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse a single statement. Panics on lexer or parser failure.
fn parse_stmt(src: &str) -> Stmt {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_statement(&mut stream).expect("parser should succeed")
}

/// Convenience: assert the statement is a `LetPattern` and hand back its
/// (`pattern`, `value`).
fn as_let_pattern(stmt: &Stmt) -> (&Pattern, &Expr) {
    match stmt {
        Stmt::LetPattern { pattern, value, .. } => (pattern, value),
        other => panic!("expected Stmt::LetPattern, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Tuple destructuring.
// ---------------------------------------------------------------------------

#[test]
fn destructuring_tuple() {
    // `let (x, y) = point` → LetPattern(Tuple([Ident x, Ident y]), value=Ident point).
    let stmt = parse_stmt("let (x, y) = point");
    let (pat, value) = as_let_pattern(&stmt);
    let subs = match pat {
        Pattern::Tuple(s, _) => s,
        other => panic!("expected Pattern::Tuple, got {other:?}"),
    };
    assert_eq!(subs.len(), 2, "tuple pattern should have 2 sub-patterns");
    assert!(
        matches!(&subs[0], Pattern::Ident(id, _) if id.name == "x"),
        "first sub = {:?}",
        subs[0]
    );
    assert!(
        matches!(&subs[1], Pattern::Ident(id, _) if id.name == "y"),
        "second sub = {:?}",
        subs[1]
    );
    // The RHS is the bare identifier `point`.
    assert!(
        matches!(value, Expr::Ident(id, _) if id.name == "point"),
        "value = {value:?}"
    );
}

#[test]
fn destructuring_wildcard_in_tuple() {
    // `let (a, _, c) = t` → Tuple([Ident a, Wildcard, Ident c]).
    let stmt = parse_stmt("let (a, _, c) = t");
    let (pat, _) = as_let_pattern(&stmt);
    let subs = match pat {
        Pattern::Tuple(s, _) => s,
        other => panic!("expected Pattern::Tuple, got {other:?}"),
    };
    assert_eq!(subs.len(), 3);
    assert!(matches!(&subs[0], Pattern::Ident(id, _) if id.name == "a"));
    assert!(
        matches!(&subs[1], Pattern::Wildcard(_)),
        "second = {:?}",
        subs[1]
    );
    assert!(matches!(&subs[2], Pattern::Ident(id, _) if id.name == "c"));
}

#[test]
fn destructuring_tuple_single_then_value() {
    // A parenthesised single binding `let (x) = v` is allowed (1-element
    // tuple pattern). Ensures the `(`-driven path doesn't demand 2+ entries.
    let stmt = parse_stmt("let (x) = v");
    let (pat, _) = as_let_pattern(&stmt);
    let subs = match pat {
        Pattern::Tuple(s, _) => s,
        other => panic!("expected Pattern::Tuple, got {other:?}"),
    };
    assert_eq!(subs.len(), 1);
    assert!(matches!(&subs[0], Pattern::Ident(id, _) if id.name == "x"));
}

// ---------------------------------------------------------------------------
// Struct destructuring.
// ---------------------------------------------------------------------------

#[test]
fn destructuring_struct_shorthand() {
    // `let Point { x, y } = p` → Struct { Point, [(x, Ident x), (y, Ident y)] }.
    let stmt = parse_stmt("let Point { x, y } = p");
    let (pat, value) = as_let_pattern(&stmt);
    let (name, fields) = match pat {
        Pattern::Struct { name, fields, .. } => (name, fields),
        other => panic!("expected Pattern::Struct, got {other:?}"),
    };
    assert_eq!(name.name, "Point");
    assert_eq!(fields.len(), 2, "fields = {fields:?}");
    // Shorthand: field name == binding name.
    assert_eq!(fields[0].0.name, "x");
    assert!(
        matches!(&fields[0].1, Pattern::Ident(id, _) if id.name == "x"),
        "first field sub = {:?}",
        fields[0].1
    );
    assert_eq!(fields[1].0.name, "y");
    assert!(
        matches!(&fields[1].1, Pattern::Ident(id, _) if id.name == "y"),
        "second field sub = {:?}",
        fields[1].1
    );
    assert!(
        matches!(value, Expr::Ident(id, _) if id.name == "p"),
        "value = {value:?}"
    );
}

#[test]
fn destructuring_struct_explicit_field() {
    // `let Point { x: a, y: b } = p` → fields bind a/b (not x/y).
    let stmt = parse_stmt("let Point { x: a, y: b } = p");
    let (pat, _) = as_let_pattern(&stmt);
    let (name, fields) = match pat {
        Pattern::Struct { name, fields, .. } => (name, fields),
        other => panic!("expected Pattern::Struct, got {other:?}"),
    };
    assert_eq!(name.name, "Point");
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0.name, "x");
    assert!(
        matches!(&fields[0].1, Pattern::Ident(id, _) if id.name == "a"),
        "{:?}",
        fields[0].1
    );
    assert_eq!(fields[1].0.name, "y");
    assert!(
        matches!(&fields[1].1, Pattern::Ident(id, _) if id.name == "b"),
        "{:?}",
        fields[1].1
    );
}

#[test]
fn destructuring_struct_empty() {
    // `let Empty { } = e` → Struct with zero fields (structurally valid).
    let stmt = parse_stmt("let Empty { } = e");
    let (pat, _) = as_let_pattern(&stmt);
    let (name, fields) = match pat {
        Pattern::Struct { name, fields, .. } => (name, fields),
        other => panic!("expected Pattern::Struct, got {other:?}"),
    };
    assert_eq!(name.name, "Empty");
    assert!(fields.is_empty(), "fields = {fields:?}");
}

// ---------------------------------------------------------------------------
// Regressions: the existing bare-name `let` path must be untouched.
// ---------------------------------------------------------------------------

#[test]
fn destructuring_plain_let_still_works() {
    // `let x = 5` → Stmt::LetDecl (NOT LetPattern).
    let stmt = parse_stmt("let x = 5");
    match stmt {
        Stmt::LetDecl {
            name,
            value,
            mutable,
            ..
        } => {
            assert_eq!(name.name, "x");
            assert!(!mutable);
            assert!(
                matches!(value, Expr::Literal(buff_lang_ast::Literal::Int(5), _)),
                "value = {value:?}"
            );
        }
        other => panic!("plain `let x = 5` must stay LetDecl, got {other:?}"),
    }
}

#[test]
fn destructuring_plain_let_mut_still_works() {
    // `let mut y = 0` → Stmt::LetDecl with mutable: true.
    let stmt = parse_stmt("let mut y = 0");
    match stmt {
        Stmt::LetDecl { name, mutable, .. } => {
            assert_eq!(name.name, "y");
            assert!(mutable);
        }
        other => panic!("plain `let mut y = 0` must stay LetDecl, got {other:?}"),
    }
}

#[test]
fn destructuring_plain_let_with_type_still_works() {
    // `let n: Int = 7` → Stmt::LetDecl with a type annotation (still the
    // bare-name path, NOT a struct pattern).
    let stmt = parse_stmt("let n: Int = 7");
    match stmt {
        Stmt::LetDecl { name, ty, .. } => {
            assert_eq!(name.name, "n");
            assert!(ty.is_some(), "type annotation must survive");
        }
        other => panic!("`let n: Int = 7` must stay LetDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Error path: malformed destructuring must produce a ParseError (no panic).
// ---------------------------------------------------------------------------

#[test]
fn destructuring_malformed_tuple_reports_error() {
    // `let (x, ,) = v` — empty subpattern slot (a lone `,` after a binding
    // is NOT a trailing comma because another `,` follows, so the parser must
    // reject it). Note `(x, )` is intentionally NOT used here: a single
    // trailing comma is a valid 1-element tuple `(x,)`, same as in Rust.
    use buff_lang_error::ParseError;
    let tokens = tokenize("let (x, ,) = v", sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    let err = parse_statement(&mut stream).expect_err("malformed pattern must error");
    let _ = err; // ParseError — a proper diagnostic, not a panic.
    let _: &ParseError = &err;
}

#[test]
fn destructuring_unclosed_tuple_reports_error() {
    // `let (x, y = v` — missing closing `)`.
    let tokens = tokenize("let (x, y = v", sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    assert!(
        parse_statement(&mut stream).is_err(),
        "unclosed tuple pattern must error, not panic"
    );
}

// ---------------------------------------------------------------------------
// Pattern.bindings() helper (AST-level, exercised via the parsed pattern).
// ---------------------------------------------------------------------------

#[test]
fn destructuring_bindings_collected_from_tuple() {
    let stmt = parse_stmt("let (a, _, c) = t");
    let (pat, _) = as_let_pattern(&stmt);
    let names: Vec<String> = pat.bindings().into_iter().map(|i: Ident| i.name).collect();
    assert_eq!(names, vec!["a".to_string(), "c".to_string()]);
}

#[test]
fn destructuring_bindings_collected_from_struct() {
    let stmt = parse_stmt("let Point { x: a, y } = p");
    let (pat, _) = as_let_pattern(&stmt);
    let names: Vec<String> = pat.bindings().into_iter().map(|i: Ident| i.name).collect();
    assert_eq!(names, vec!["a".to_string(), "y".to_string()]);
}
