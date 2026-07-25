//! T39 parser tests — or-patterns `A | B` in `match` arms.
//!
//! Verifies the parser accepts the `|`-separated alternative syntax at the
//! top level of a match arm (`Red | Green => ...`) AND in nested subpattern
//! positions (`Some(1 | 2)`, `Ok(Red) | Err(Blue)`), producing a
//! [`Pattern::Or`] AST node. The codegen lowering (or-pattern → Rust
//! `Pat::Or`) is exercised by the codegen crate's own tests; these tests
//! lock the AST shape so a parser regression is caught immediately.

use buff_lang_ast::{Expr, MatchArm, Pattern};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_match, parse_pattern, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse a single pattern.
fn parse_pat(src: &str) -> Pattern {
    let toks = tokenize(src, sid()).expect("lexer should succeed");
    let mut s = TokenStream::new(&toks, sid());
    parse_pattern(&mut s).expect("parse_pattern should succeed")
}

/// Tokenize + parse a `match` expression (the `match` keyword is the first
/// token).
fn parse_match_expr(src: &str) -> Expr {
    let toks = tokenize(src, sid()).expect("lexer should succeed");
    let mut s = TokenStream::new(&toks, sid());
    parse_match(&mut s).expect("parse_match should succeed")
}

/// Extract the arms from a MatchExpr, panicking if `e` is not one.
fn arms_of(e: &Expr) -> &[MatchArm] {
    match e {
        Expr::MatchExpr { arms, .. } => arms,
        _ => panic!("expected MatchExpr, got {e}"),
    }
}

#[test]
fn t39_top_level_two_alternatives() {
    // `Red | Green` → Or([Ident Red, Ident Green]).
    let p = parse_pat("Red | Green");
    match p {
        Pattern::Or(alts, _) => {
            assert_eq!(alts.len(), 2, "two alternatives");
            assert!(alts.iter().all(|a| matches!(a, Pattern::Ident(_, _))));
        }
        other => panic!("expected Or, got {other}"),
    }
}

#[test]
fn t39_top_level_three_alternatives() {
    // `Red | Green | Blue` → Or([Ident, Ident, Ident]).
    let p = parse_pat("Red | Green | Blue");
    match p {
        Pattern::Or(alts, _) => {
            assert_eq!(alts.len(), 3, "three alternatives");
        }
        other => panic!("expected Or, got {other}"),
    }
}

#[test]
fn t39_single_pattern_no_or_wrapping() {
    // A lone `Red` (no `|`) must NOT be wrapped in Or — backward-compatible.
    let p = parse_pat("Red");
    assert!(
        matches!(p, Pattern::Ident(_, _)),
        "lone pattern must not be wrapped in Or"
    );
}

#[test]
fn t39_nested_or_inside_variant_subpattern() {
    // `Some(1 | 2)` — the or-pattern is inside a variant subpattern.
    let p = parse_pat("Some(1 | 2)");
    match p {
        Pattern::Variant { subpatterns, .. } => {
            assert_eq!(subpatterns.len(), 1);
            assert!(
                matches!(subpatterns[0], Pattern::Or(_, _)),
                "subpattern should be an Or"
            );
        }
        other => panic!("expected Variant, got {other}"),
    }
}

#[test]
fn t39_or_of_variant_patterns() {
    // `Ok(Red) | Err(Blue)` — two variant alternatives at the top level.
    let p = parse_pat("Ok(Red) | Err(Blue)");
    match p {
        Pattern::Or(alts, _) => {
            assert_eq!(alts.len(), 2);
            assert!(
                alts.iter().all(|a| matches!(a, Pattern::Variant { .. })),
                "both alternatives should be Variant patterns"
            );
        }
        other => panic!("expected Or, got {other}"),
    }
}

#[test]
fn t39_match_arm_with_or_pattern_parses() {
    // Full match expression with an or-pattern arm.
    // `match color { Red | Green => "go", _ => "stop" }`
    let e = parse_match_expr("match color { Red | Green => \"go\", _ => \"stop\" }");
    let arms = arms_of(&e);
    assert_eq!(arms.len(), 2, "two arms");
    let first_is_or_two = match &arms[0].pattern {
        Pattern::Or(alts, _) => alts.len() == 2,
        _ => false,
    };
    assert!(
        first_is_or_two,
        "first arm should be an Or with 2 alternatives"
    );
    assert!(
        matches!(&arms[1].pattern, Pattern::Wildcard(_)),
        "second arm should be a wildcard"
    );
}

#[test]
fn t39_or_pattern_bindings_union() {
    // `Some(a) | Some(b)` — the Or's bindings are the union of each alt's
    // bindings ([a] from the first Some, [b] from the second → [a, b]).
    // (Rust requires each or-alternative to bind the SAME names; Buff defers
    // that consistency check to rustc. The parser test only verifies the
    // union-flattening mechanism in `Pattern::bindings`.)
    let p = parse_pat("Some(a) | Some(b)");
    let binds = p.bindings();
    let names: Vec<&str> = binds.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b"], "bindings should be the union [a, b]");
}
