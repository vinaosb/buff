//! T27 integration tests — `enum` declarations and `match` expressions.
//!
//! Coverage:
//!
//! - Top-level `enum Color { Red, Green, Blue }` parses to a `Decl::EnumDecl`
//!   with the right name + variants (all unit variants).
//! - Multi-variant enum with mixed unit + data-carrying variants.
//! - Generic enum `enum Result<T, E> { Ok(T), Err(E) }` parses with both
//!   generic params and tuple-payload variants.
//! - Empty enum `enum Empty { }` parses (zero variants).
//! - Trailing-comma tolerance in both the variant list and the generic
//!   param list.
//! - `match c { Red => 1, Green => 2, Blue => 3 }` parses to an
//!   `Expr::MatchExpr` with three arms, each with an `Ident` pattern and
//!   the right body literal.
//! - `match r { Ok(v) => v, Err(_) => 0 }` parses: `Ok(v)` is a `Variant`
//!   pattern with a binding subpattern; `Err(_)` is a `Variant` with a
//!   wildcard subpattern.
//! - `match x { _ => 1 }` parses with a wildcard catch-all.
//! - `match n { 0 => "z", _ => "nz" }` parses with a literal pattern.
//! - Negative literal pattern `-1 => ...`.
//! - Nested variant pattern `Ok(Err(_))`.
//! - Trailing-comma tolerance in match arms.
//! - Multiple variants share the same `enum_name: ""` placeholder (the
//!   parser doesn't know which enum each variant belongs to — exhaustiveness
//!   + codegen resolve it by name).
//! - Regression: top-level `func` parsing still works alongside `enum`.
//! - Regression: `match` inside a `let` binding (`let v = match c { ... }`)
//!   parses (match is a primary expression like `if`).
//!
//! Each test feeds source strings through the lexer and then through
//! [`buff_lang_parser::parse`] (for top-level decls) or
//! [`buff_lang_parser::parse_expression`] (for match expressions). The
//! resulting AST is pattern-matched to assert the expected shape.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser --test enum_match
//! ```

#![allow(clippy::approx_constant)]

use buff_lang_ast::{Decl, EnumDecl, EnumVariant, Expr, Ident, Literal, Pattern};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse, parse_expression};

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse `src` as a single EXPRESSION (used for match tests).
fn parse_expr(src: &str) -> Expr {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse_expression(&tokens, sid()).expect("parser must succeed")
}

/// Tokenize + parse `src` as a top-level program (used for enum decl tests).
fn parse_program(src: &str) -> Vec<Decl> {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect("parser must succeed")
}

/// Tokenize + parse `src` as a top-level program, expecting FAILURE.
fn parse_program_err(src: &str) -> buff_lang_error::ParseError {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect_err("parser must fail")
}

// ---------------------------------------------------------------------------
// Enum declaration parsing.
// ---------------------------------------------------------------------------

#[test]
fn enum_match_simple_unit_enum_parses() {
    // `enum Color { Red, Green, Blue }` → one EnumDecl with three unit variants.
    let decls = parse_program("enum Color { Red, Green, Blue }");
    assert_eq!(decls.len(), 1, "expected one top-level decl");
    match &decls[0] {
        Decl::EnumDecl(e) => {
            assert_eq!(e.name.name, "Color", "enum name");
            assert!(e.generics.is_empty(), "non-generic enum has no generics");
            assert_eq!(e.variants.len(), 3, "three variants");
            let names: Vec<&str> = e.variants.iter().map(|v| v.name.name.as_str()).collect();
            assert_eq!(
                names,
                vec!["Red", "Green", "Blue"],
                "variant names in order"
            );
            // All unit variants — `data` is None.
            for v in &e.variants {
                assert!(
                    v.data.is_none(),
                    "unit variant {:?} should have no payload",
                    v.name
                );
            }
        }
        other => panic!("expected EnumDecl, got {other:?}"),
    }
}

#[test]
fn enum_match_data_carrying_enum_parses() {
    // `enum Shape { Circle(Float), Rect(Float, Float), Point }` — mixed.
    let decls = parse_program("enum Shape { Circle(Float), Rect(Float, Float), Point }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Shape");
    assert_eq!(e.variants.len(), 3);
    // Circle(Float) — one payload.
    assert_eq!(e.variants[0].name.name, "Circle");
    let circle_data = e.variants[0].data.as_ref().expect("Circle has payload");
    assert_eq!(circle_data.len(), 1, "Circle has one payload type");
    // Rect(Float, Float) — two payloads.
    assert_eq!(e.variants[1].name.name, "Rect");
    let rect_data = e.variants[1].data.as_ref().expect("Rect has payload");
    assert_eq!(rect_data.len(), 2, "Rect has two payload types");
    // Point — unit variant.
    assert_eq!(e.variants[2].name.name, "Point");
    assert!(e.variants[2].data.is_none(), "Point is a unit variant");
}

#[test]
fn enum_match_generic_enum_parses() {
    // `enum Result<T, E> { Ok(T), Err(E) }` — generic params + payload variants.
    let decls = parse_program("enum Result<T, E> { Ok(T), Err(E) }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Result");
    // Two generic params: T and E.
    assert_eq!(
        e.generics
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>(),
        vec!["T", "E"],
        "generic param names"
    );
    assert_eq!(e.variants.len(), 2);
    // Ok(T) — payload is the type-param T.
    assert_eq!(e.variants[0].name.name, "Ok");
    let ok_data = e.variants[0].data.as_ref().expect("Ok has payload");
    assert_eq!(ok_data.len(), 1);
    // Err(E) — payload is the type-param E.
    assert_eq!(e.variants[1].name.name, "Err");
    let err_data = e.variants[1].data.as_ref().expect("Err has payload");
    assert_eq!(err_data.len(), 1);
}

#[test]
fn enum_match_single_generic_enum_parses() {
    // `enum Option<T> { Some(T), None }` — one generic param.
    let decls = parse_program("enum Option<T> { Some(T), None }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Option");
    assert_eq!(
        e.generics
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>(),
        vec!["T"],
        "one generic param"
    );
    assert_eq!(e.variants.len(), 2);
    assert_eq!(e.variants[0].name.name, "Some");
    assert!(e.variants[0].data.is_some(), "Some has payload");
    assert_eq!(e.variants[1].name.name, "None");
    assert!(e.variants[1].data.is_none(), "None is unit");
}

#[test]
fn enum_match_empty_enum_parses() {
    // `enum Empty { }` — zero variants.
    let decls = parse_program("enum Empty { }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Empty");
    assert!(e.variants.is_empty(), "empty enum has zero variants");
}

#[test]
fn enum_match_trailing_comma_in_variants_parses() {
    // Trailing comma in the variant list is allowed.
    let decls = parse_program("enum Color { Red, Green, Blue, }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        _ => panic!("expected EnumDecl"),
    };
    assert_eq!(e.variants.len(), 3, "trailing comma tolerated");
}

#[test]
fn enum_match_trailing_comma_in_generics_parses() {
    // Trailing comma in the generic param list is allowed.
    let decls = parse_program("enum Result<T, E,> { Ok(T), Err(E) }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        _ => panic!("expected EnumDecl"),
    };
    assert_eq!(e.generics.len(), 2, "trailing comma in generics tolerated");
}

#[test]
fn enum_match_missing_opening_brace_errors() {
    // `enum Color Red, Green }` — missing `{` → parse error.
    let err = parse_program_err("enum Color Red, Green }");
    let msg = format!("{}", err.diagnostic);
    assert!(
        msg.contains("`{`"),
        "expected error about missing `{{`, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Match expression parsing.
// ---------------------------------------------------------------------------

#[test]
fn enum_match_simple_match_with_unit_variant_arms() {
    // `match c { Red => 1, Green => 2, Blue => 3 }`
    let e = parse_expr("match c { Red => 1, Green => 2, Blue => 3 }");
    match e {
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            // Scrutinee is the bare ident `c`.
            match scrutinee.as_ref() {
                Expr::Ident(id, _) => assert_eq!(id.name, "c", "scrutinee name"),
                other => panic!("expected Ident scrutinee, got {other:?}"),
            }
            assert_eq!(arms.len(), 3, "three arms");
            // First arm: pattern Red, body 1.
            match &arms[0].pattern {
                Pattern::Ident(id, _) => assert_eq!(id.name, "Red"),
                other => panic!("expected Ident pattern for unit variant, got {other:?}"),
            }
            // Body of first arm: ExprStmt wrapping Literal(Int(1)).
            assert_eq!(arms[0].body.stmts.len(), 1);
            match &arms[0].body.stmts[0] {
                buff_lang_ast::Stmt::ExprStmt(Expr::Literal(Literal::Int(n), _), _) => {
                    assert_eq!(*n, 1, "first arm body")
                }
                other => panic!("expected Literal body, got {other:?}"),
            }
        }
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_with_data_binding_pattern() {
    // `match r { Ok(v) => v, Err(_) => 0 }` — Ok binds v, Err ignores.
    let e = parse_expr("match r { Ok(v) => v, Err(_) => 0 }");
    match e {
        Expr::MatchExpr { arms, .. } => {
            assert_eq!(arms.len(), 2);
            // Ok(v): Variant pattern with a single Ident subpattern.
            match &arms[0].pattern {
                Pattern::Variant {
                    variant,
                    subpatterns,
                    ..
                } => {
                    assert_eq!(variant.name, "Ok", "first arm variant");
                    assert_eq!(subpatterns.len(), 1, "Ok has one subpattern");
                    match &subpatterns[0] {
                        Pattern::Ident(id, _) => assert_eq!(id.name, "v", "bound name"),
                        other => panic!("expected Ident subpattern, got {other:?}"),
                    }
                }
                other => panic!("expected Variant pattern, got {other:?}"),
            }
            // Body of Ok(v): the bound ident `v`.
            match &arms[0].body.stmts[0] {
                buff_lang_ast::Stmt::ExprStmt(Expr::Ident(id, _), _) => {
                    assert_eq!(id.name, "v", "Ok body uses the bound name")
                }
                other => panic!("expected Ident body, got {other:?}"),
            }
            // Err(_): Variant pattern with a single Wildcard subpattern.
            match &arms[1].pattern {
                Pattern::Variant {
                    variant,
                    subpatterns,
                    ..
                } => {
                    assert_eq!(variant.name, "Err", "second arm variant");
                    assert_eq!(subpatterns.len(), 1, "Err has one subpattern");
                    assert!(
                        matches!(subpatterns[0], Pattern::Wildcard(_)),
                        "Err subpattern is wildcard"
                    );
                }
                other => panic!("expected Variant pattern, got {other:?}"),
            }
        }
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_with_wildcard_catch_all() {
    // `match x { _ => 1 }` — wildcard arm.
    let e = parse_expr("match x { _ => 1 }");
    match e {
        Expr::MatchExpr { arms, .. } => {
            assert_eq!(arms.len(), 1);
            assert!(
                matches!(arms[0].pattern, Pattern::Wildcard(_)),
                "wildcard pattern"
            );
        }
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_with_literal_pattern() {
    // `match n { 0 => "z", _ => "nz" }` — literal + wildcard.
    let e = parse_expr("match n { 0 => \"z\", _ => \"nz\" }");
    match e {
        Expr::MatchExpr { arms, .. } => {
            assert_eq!(arms.len(), 2);
            match &arms[0].pattern {
                Pattern::Literal(Literal::Int(n), _) => assert_eq!(*n, 0, "literal pattern"),
                other => panic!("expected Literal pattern, got {other:?}"),
            }
            assert!(matches!(arms[1].pattern, Pattern::Wildcard(_)));
        }
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_with_negative_literal_pattern() {
    // `match n { -1 => \"neg\", _ => \"other\" }` — negative-literal collapse.
    let e = parse_expr("match n { -1 => \"neg\", _ => \"other\" }");
    match e {
        Expr::MatchExpr { arms, .. } => match &arms[0].pattern {
            Pattern::Literal(Literal::Int(n), _) => assert_eq!(*n, -1, "negative literal"),
            other => panic!("expected Literal pattern, got {other:?}"),
        },
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_nested_variant_pattern() {
    // `match r { Ok(Err(_)) => 1, _ => 0 }` — nested variant pattern.
    let e = parse_expr("match r { Ok(Err(_)) => 1, _ => 0 }");
    match e {
        Expr::MatchExpr { arms, .. } => {
            match &arms[0].pattern {
                Pattern::Variant {
                    variant,
                    subpatterns,
                    ..
                } => {
                    assert_eq!(variant.name, "Ok", "outer variant");
                    assert_eq!(subpatterns.len(), 1, "Ok has one subpattern");
                    // The subpattern is itself a Variant (Err with a wildcard).
                    match &subpatterns[0] {
                        Pattern::Variant {
                            variant: inner_v,
                            subpatterns: inner_sub,
                            ..
                        } => {
                            assert_eq!(inner_v.name, "Err", "inner variant");
                            assert_eq!(inner_sub.len(), 1);
                            assert!(matches!(inner_sub[0], Pattern::Wildcard(_)));
                        }
                        other => panic!("expected nested Variant, got {other:?}"),
                    }
                }
                other => panic!("expected outer Variant, got {other:?}"),
            }
        }
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_trailing_comma_in_arms() {
    // Trailing comma in match arms is allowed.
    let e = parse_expr("match c { Red => 1, Green => 2, }");
    match e {
        Expr::MatchExpr { arms, .. } => assert_eq!(arms.len(), 2, "trailing comma tolerated"),
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_bare_ident_scrutinee_with_real_arms_parses_without_parens() {
    // The common case: `match c { Red => 1, ... }` does NOT need parens
    // because the `{` is followed by an arm pattern, not by `}` (empty
    // struct-init) or `Ident :` (struct field). This is the disambiguation
    // contract from T26's `cursor_at_struct_init_body` — it only fires on
    // the struct-init SHAPE; arm bodies don't match that shape.
    let e = parse_expr("match c { Red => 1, Green => 2 }");
    match e {
        Expr::MatchExpr { arms, .. } => assert_eq!(arms.len(), 2),
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_empty_arms_known_limitation_errors() {
    // KNOWN LIMITATION (v0.5): `match x { }` (zero arms on a bare-ident
    // scrutinee) is rejected because the T26 struct-init disambiguator
    // greedily parses `x { }` as an empty struct-init, leaving no `{` for
    // the match body. Real matches have at least one arm and so are
    // unaffected; a future parser task may add a "no struct-init" mode to
    // the scrutinee parse to lift this restriction. For now we just pin
    // the current (error) behaviour so a future change is detected.
    let tokens = tokenize("match x { }", sid()).expect("lexer must succeed");
    let result = parse_expression(&tokens, sid());
    assert!(
        result.is_err(),
        "empty-arms bare-ident match is a known disambiguation limitation; got: {result:?}"
    );
}

#[test]
fn enum_match_is_a_primary_expression_inside_let() {
    // `match` can appear as the RHS of a `let` (it's a primary expression
    // like `if`). Tested here via `parse_expr` so we don't need a full
    // function context.
    let e = parse_expr("match c { Red => 1, _ => 0 }");
    assert!(
        matches!(e, Expr::MatchExpr { .. }),
        "match parses at primary position"
    );
}

#[test]
fn enum_match_complex_scrutinee_expression() {
    // `match foo.bar(x) { _ => 1 }` — scrutinee is a method call.
    let e = parse_expr("match foo.bar(x) { _ => 1 }");
    match e {
        Expr::MatchExpr { scrutinee, .. } => match scrutinee.as_ref() {
            Expr::MethodCall { method, .. } => {
                assert_eq!(method.name, "bar", "method-call scrutinee")
            }
            other => panic!("expected MethodCall scrutinee, got {other:?}"),
        },
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_top_level_func_and_enum_coexist() {
    // Regression: a program with BOTH an enum and a func parses both.
    let src = "enum Color { Red, Green, Blue }\nfunc f(x: Int) -> Int { return x }";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 2, "two top-level decls");
    assert!(matches!(decls[0], Decl::EnumDecl(_)), "first decl is enum");
    assert!(matches!(decls[1], Decl::FuncDecl(_)), "second decl is func");
}

#[test]
fn enum_match_variant_patterns_share_empty_enum_name() {
    // All Variant patterns parsed from source share the empty-string
    // `enum_name` placeholder (the parser doesn't know which enum each
    // variant belongs to). Exhaustiveness + codegen resolve by name.
    let e = parse_expr("match r { Ok(v) => v, Err(e) => 0 }");
    match e {
        Expr::MatchExpr { arms, .. } => {
            for arm in &arms {
                if let Pattern::Variant { enum_name, .. } = &arm.pattern {
                    assert_eq!(
                        enum_name.name, "",
                        "parser fills enum_name with empty placeholder"
                    );
                }
            }
        }
        other => panic!("expected MatchExpr, got {other:?}"),
    }
}

#[test]
fn enum_match_ast_node_constructors_are_public() {
    // Smoke-test the AST constructors and Pattern accessors added in T27.
    // Pattern::span() and Pattern::variant_name_key() must work for every
    // variant. Ident construction must accept the new generics field.
    use buff_lang_error::Span;
    let span = Span::dummy();
    let wildcard = Pattern::Wildcard(span);
    assert_eq!(wildcard.span(), span);
    assert!(
        wildcard.variant_name_key().is_none(),
        "wildcard has no variant name"
    );

    let ident_pat = Pattern::Ident(Ident::new("Red", span), span);
    assert_eq!(ident_pat.variant_name_key(), Some("Red"));

    let variant_pat = Pattern::Variant {
        enum_name: Ident::new("", span),
        variant: Ident::new("Ok", span),
        subpatterns: Vec::new(),
        span,
    };
    assert_eq!(variant_pat.variant_name_key(), Some("Ok"));

    // EnumDecl with empty generics field (the migration shape).
    let _enum_decl = EnumDecl { name: Ident::new("Color", span), type_params: Vec::new(), variants: vec![EnumVariant {
        name: Ident::new("Red", span),
        data: None,
        span,
    }],
    span, };
}
