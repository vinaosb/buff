//! T41 parser tests — struct patterns in `match` arms + `..` rest.
//!
//! Verifies the parser accepts struct destructuring patterns in match arms
//! (`match p { Point { x, y } => ... }`), shorthand and explicit-field
//! forms, the T41 `..` rest pattern (`Point { x, .. }`), and that the
//! `rest` flag on [`Pattern::Struct`] is populated correctly.
//!
//! The base struct-pattern feature shipped in T71 (shared `parse_pattern`);
//! T41 adds the `..` rest token and dedicated match-arm coverage/tests.

use buff_lang_ast::{Expr, MatchArm, Pattern};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_match, parse_pattern, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

fn parse_pat(src: &str) -> Pattern {
    let toks = tokenize(src, sid()).expect("lexer should succeed");
    let mut s = TokenStream::new(&toks, sid());
    parse_pattern(&mut s).expect("parse_pattern should succeed")
}

fn parse_match_expr(src: &str) -> Expr {
    let toks = tokenize(src, sid()).expect("lexer should succeed");
    let mut s = TokenStream::new(&toks, sid());
    parse_match(&mut s).expect("parse_match should succeed")
}

fn arms_of(e: &Expr) -> Vec<MatchArm> {
    match e {
        Expr::MatchExpr { arms, .. } => arms.clone(),
        _ => panic!("expected MatchExpr, got {e}"),
    }
}

#[test]
fn t41_struct_pattern_in_match_shorthand() {
    // `match p { Point { x, y } => ... }` — struct pattern with shorthand
    // fields as a match arm.
    let e = parse_match_expr("match p { Point { x, y } => 1, _ => 0 }");
    let arms = arms_of(&e);
    match &arms[0].pattern {
        Pattern::Struct { name, fields, rest, .. } => {
            assert_eq!(name.name, "Point");
            assert_eq!(fields.len(), 2, "two fields");
            assert!(!rest, "no rest pattern");
        }
        other => panic!("expected Struct pattern, got {other}"),
    }
}

#[test]
fn t41_struct_pattern_explicit_fields() {
    // `Point { x: a, y: b }` — explicit field:binding form.
    let p = parse_pat("Point { x: a, y: b }");
    match p {
        Pattern::Struct { fields, .. } => {
            assert_eq!(fields.len(), 2);
            // Each field's subpattern is a distinct binding (a, b).
            assert!(matches!(&fields[0].1, Pattern::Ident(id, _) if id.name == "a"));
            assert!(matches!(&fields[1].1, Pattern::Ident(id, _) if id.name == "b"));
        }
        other => panic!("expected Struct, got {other}"),
    }
}

#[test]
fn t41_rest_pattern_after_fields() {
    // `Point { x, .. }` — rest after one field. Sets rest = true.
    let p = parse_pat("Point { x, .. }");
    match p {
        Pattern::Struct { fields, rest, .. } => {
            assert_eq!(fields.len(), 1, "one named field");
            assert!(rest, "rest flag should be set");
        }
        other => panic!("expected Struct, got {other}"),
    }
}

#[test]
fn t41_rest_pattern_only() {
    // `Point { .. }` — rest with NO named fields. Sets rest = true,
    // fields empty.
    let p = parse_pat("Point { .. }");
    match p {
        Pattern::Struct { fields, rest, .. } => {
            assert!(fields.is_empty(), "no named fields");
            assert!(rest, "rest flag should be set");
        }
        other => panic!("expected Struct, got {other}"),
    }
}

#[test]
fn t41_no_rest_flag_without_dotdot() {
    // `Point { x, y }` (no `..`) must have rest = false.
    let p = parse_pat("Point { x, y }");
    match p {
        Pattern::Struct { rest, .. } => {
            assert!(!rest, "rest should be false without `..`");
        }
        other => panic!("expected Struct, got {other}"),
    }
}

#[test]
fn t41_rest_with_trailing_comma() {
    // `Point { x, .., }` — trailing comma after `..` is allowed.
    let p = parse_pat("Point { x, .., }");
    match p {
        Pattern::Struct { fields, rest, .. } => {
            assert_eq!(fields.len(), 1);
            assert!(rest);
        }
        other => panic!("expected Struct, got {other}"),
    }
}

#[test]
fn t41_struct_pattern_with_guard_in_match() {
    // T41 + T40 composition: struct pattern + guard in one match arm.
    let e = parse_match_expr("match p { Point { x, y } if x > 0 => 1, _ => 0 }");
    let arms = arms_of(&e);
    assert!(
        matches!(&arms[0].pattern, Pattern::Struct { .. }),
        "first arm should be a struct pattern"
    );
    assert!(arms[0].guard.is_some(), "first arm should have a guard");
}
