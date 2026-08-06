//! T27 integration tests — Rust codegen for `enum` declarations and
//! `match` expressions.
//!
//! Coverage:
//!
//! - `enum Color { Red, Green, Blue }` →
//!   `#[derive(Clone, Debug)] pub enum Color { Red, Green, Blue }`.
//! - `enum Result<T, E> { Ok(T), Err(E) }` →
//!   `pub enum Result<T, E> { Ok(T), Err(E) }` (generic + tuple variants).
//! - `enum Shape { Circle(Float), Rect(Float, Float), Point }` — mixed
//!   unit + tuple-variant enum.
//! - Empty enum `enum Empty { }` → valid Rust empty enum.
//! - `match r { Ok(v) => v, Err(_) => 0 }` → identical-shape Rust match.
//! - `match c { Red => 1, _ => 0 }` — unit variant + wildcard arm.
//! - Match arms lower with the right pattern shapes (Ident, Variant tuple,
//!   Wildcard, Literal).
//! - End-to-end: enum decl + function with a match body re-parses as valid
//!   Rust via `syn::parse_str::<syn::File>`.
//!
//! Each test builds a Buff AST by hand (the codegen is the system under
//! test; the parser is exercised separately in `enum_match.rs`), runs it
//! through [`buff_lang_codegen_rust::generate_rust`], and asserts on the
//! resulting Rust source. Snapshots pin the exact format.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test enum_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{EnumDecl, EnumVariant, FuncDecl, TypeParam};
use buff_lang_ast::{Decl, Expr, Literal, MatchArm, Pattern, Stmt, TypeRef};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn named_ty(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build a unit variant.
fn unit_variant(name: &str) -> EnumVariant {
    EnumVariant {
        name: ident(name),
        data: None,
        span: span(),
    }
}

/// Build a tuple variant with one or more payload types.
fn tuple_variant(name: &str, payload_tys: &[&str]) -> EnumVariant {
    EnumVariant {
        name: ident(name),
        data: Some(payload_tys.iter().map(|t| named_ty(t)).collect()),
        span: span(),
    }
}

/// Build an enum decl.
fn enum_decl(name: &str, generics: &[&str], variants: Vec<EnumVariant>) -> EnumDecl {
    EnumDecl {
        name: ident(name),
        type_params: generics
            .iter()
            .map(|g| TypeParam {
                name: ident(g),
                bounds: Vec::new(),
                span: span(),
            })
            .collect(),
        variants,
        span: span(),
    }
}

/// Build a `Pattern::Ident`.
fn ident_pat(name: &str) -> Pattern {
    Pattern::Ident(ident(name), span())
}

/// Build a `Pattern::Variant` with subpatterns.
fn variant_pat(name: &str, subpats: Vec<Pattern>) -> Pattern {
    Pattern::Variant {
        enum_name: ident(""),
        variant: ident(name),
        subpatterns: subpats,
        span: span(),
    }
}

/// Build a `Pattern::Wildcard`.
fn wildcard_pat() -> Pattern {
    Pattern::Wildcard(span())
}

/// Build a `Pattern::Literal(Int(n))`.
fn int_lit_pat(n: i64) -> Pattern {
    Pattern::Literal(Literal::Int(n), span())
}

/// Build a match arm with a single-expression body.
fn arm(pat: Pattern, body: Expr) -> MatchArm {
    MatchArm {
        pattern: pat,
        guard: None,
        body: Block {
            stmts: vec![Stmt::ExprStmt(body, span())],
            span: span(),
        },
        span: span(),
    }
}

/// Generate Rust source from a single enum declaration.
fn codegen_enum(d: EnumDecl) -> String {
    generate_rust(&[Decl::EnumDecl(d)]).expect("enum codegen must succeed")
}

/// Wrap a list of statements in a no-arg function called `f` and codegen.
fn codegen_stmts(stmts: Vec<Stmt>) -> String {
    let func = FuncDecl {
        name: ident("f"),
        params: Vec::new(),
        return_type: None,
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
    };
    generate_rust(&[Decl::FuncDecl(func)]).expect("codegen must succeed")
}

/// Like [`codegen_stmts`] but emits a single expression statement.
fn codegen_one_expr(expr: Expr) -> String {
    codegen_stmts(vec![Stmt::ExprStmt(expr, span())])
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// Suppress unused warning for Param import (used implicitly via test helpers).
#[allow(dead_code)]
fn _param_smoke() -> Param {
    Param {
        name: ident("_"),
        ty: named_ty("Int"),
        default_value: None,
        is_comptime: false,
        span: span(),
    }
}

// ---------------------------------------------------------------------------
// 1. EnumDecl codegen — basic shape, derives, pub visibility, generics.
// ---------------------------------------------------------------------------

#[test]
fn enum_codegen_simple_unit_enum_snapshot() {
    // `enum Color { Red, Green, Blue }` →
    // `#[derive(Clone, Debug)] pub enum Color { Red, Green, Blue }`
    let src = codegen_enum(enum_decl(
        "Color",
        &[],
        vec![
            unit_variant("Red"),
            unit_variant("Green"),
            unit_variant("Blue"),
        ],
    ));
    insta::assert_snapshot!(src, @r###"
    #[derive(Clone, PartialEq, Debug)]
    pub enum Color {
        Red,
        Green,
        Blue,
    }
    "###);
    must_reparse(&src);
}

#[test]
fn enum_codegen_generic_data_enum_snapshot() {
    // `enum Result<T, E> { Ok(T), Err(E) }` — generic + tuple variants.
    let src = codegen_enum(enum_decl(
        "Result",
        &["T", "E"],
        vec![tuple_variant("Ok", &["T"]), tuple_variant("Err", &["E"])],
    ));
    insta::assert_snapshot!(src, @r###"
    #[derive(Clone, PartialEq, Debug)]
    pub enum Result<T, E> {
        Ok(T),
        Err(E),
    }
    "###);
    must_reparse(&src);
}

#[test]
fn enum_codegen_mixed_unit_and_tuple_variants() {
    // `enum Shape { Circle(Float), Rect(Float, Float), Point }` — mixed.
    let src = codegen_enum(enum_decl(
        "Shape",
        &[],
        vec![
            tuple_variant("Circle", &["Float"]),
            tuple_variant("Rect", &["Float", "Float"]),
            unit_variant("Point"),
        ],
    ));
    assert!(
        src.contains("#[derive(Clone, PartialEq, Debug)]"),
        "expected derive attribute in: {src}"
    );
    assert!(
        src.contains("pub enum Shape"),
        "expected `pub enum Shape` in: {src}"
    );
    // Unit variant Point has no payload.
    assert!(
        src.contains("Point,\n") || src.contains("Point\n") || src.contains("Point,"),
        "expected `Point` unit variant in: {src}"
    );
    // Tuple variants carry their payloads (Float → f32).
    assert!(
        src.contains("Circle(f32)"),
        "expected `Circle(f32)` in: {src}"
    );
    assert!(
        src.contains("Rect(f32, f32)"),
        "expected `Rect(f32, f32)` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn enum_codegen_empty_enum_snapshot() {
    // `enum Empty { }` → valid Rust empty enum.
    let src = codegen_enum(enum_decl("Empty", &[], Vec::new()));
    insta::assert_snapshot!(src, @r###"
    #[derive(Clone, PartialEq, Debug)]
    pub enum Empty {}
    "###);
    must_reparse(&src);
}

#[test]
fn enum_codegen_single_generic_param_option() {
    // `enum Option<T> { Some(T), None }` — one generic param.
    let src = codegen_enum(enum_decl(
        "Option",
        &["T"],
        vec![tuple_variant("Some", &["T"]), unit_variant("None")],
    ));
    assert!(
        src.contains("pub enum Option<T>"),
        "expected generic enum in: {src}"
    );
    assert!(src.contains("Some(T)"), "expected `Some(T)` in: {src}");
    assert!(src.contains("None"), "expected `None` in: {src}");
    must_reparse(&src);
}

#[test]
fn enum_codegen_variant_payload_uses_standard_type_mapping() {
    // Payload types flow through the same `ast_typeref_to_syn` mapping that
    // drives struct fields and let-binding annotations: Int→i64, Float→f32,
    // String→String, Bool→bool, etc.
    let src = codegen_enum(enum_decl(
        "Many",
        &[],
        vec![
            EnumVariant {
                name: ident("A"),
                data: Some(vec![
                    named_ty("Int"),
                    named_ty("Float"),
                    named_ty("String"),
                    named_ty("Bool"),
                ]),
                span: span(),
            },
            unit_variant("B"),
        ],
    ));
    assert!(
        src.contains("A(i64, f32, String, bool)"),
        "expected mapped payload types in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. MatchExpr codegen — pattern shapes and arm body.
// ---------------------------------------------------------------------------

#[test]
fn enum_codegen_match_with_data_binding_pattern() {
    // `match r { Ok(v) => v, Err(_) => 0 }` → identical-shape Rust match.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("r")),
        arms: vec![
            arm(variant_pat("Ok", vec![ident_pat("v")]), ident_expr("v")),
            arm(variant_pat("Err", vec![wildcard_pat()]), int_expr(0)),
        ],
        span: span(),
    };
    let src = codegen_one_expr(mt);
    assert!(src.contains("match r {"), "expected `match r {{` in: {src}");
    assert!(src.contains("Ok(v)"), "expected `Ok(v)` pattern in: {src}");
    assert!(
        src.contains("Err(_)"),
        "expected `Err(_)` pattern in: {src}"
    );
    // The Ok body uses the bound `v`.
    assert!(src.contains("v"), "expected bound `v` in body: {src}");
    // The Err body is `0`.
    assert!(src.contains("0"), "expected `0` body in: {src}");
    must_reparse(&src);
}

#[test]
fn enum_codegen_match_with_unit_variant_and_wildcard_arms() {
    // `match c { Red => 1, _ => 0 }` — Ident pattern + wildcard.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(ident_pat("Red"), int_expr(1)),
            arm(wildcard_pat(), int_expr(0)),
        ],
        span: span(),
    };
    let src = codegen_one_expr(mt);
    assert!(src.contains("match c {"), "expected `match c {{` in: {src}");
    assert!(src.contains("Red =>"), "expected `Red =>` arm in: {src}");
    assert!(
        src.contains("_ =>"),
        "expected `_ =>` wildcard arm in: {src}"
    );
    assert!(src.contains("1"), "expected `1` body in: {src}");
    assert!(src.contains("0"), "expected `0` body in: {src}");
    must_reparse(&src);
}

#[test]
fn enum_codegen_match_all_unit_variants_snapshot() {
    // `match c { Red => 1, Green => 2, Blue => 3 }`.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(ident_pat("Red"), int_expr(1)),
            arm(ident_pat("Green"), int_expr(2)),
            arm(ident_pat("Blue"), int_expr(3)),
        ],
        span: span(),
    };
    let src = codegen_one_expr(mt);
    insta::assert_snapshot!(src, @r###"
    fn f() {
        match c {
            Red => 1,
            Green => 2,
            Blue => 3,
        };
    }
    "###);
    must_reparse(&src);
}

#[test]
fn enum_codegen_match_with_literal_pattern() {
    // `match n { 0 => "z", _ => "nz" }` — literal + wildcard.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("n")),
        arms: vec![
            arm(
                int_lit_pat(0),
                Expr::Literal(Literal::String("z".to_string()), span()),
            ),
            arm(
                wildcard_pat(),
                Expr::Literal(Literal::String("nz".to_string()), span()),
            ),
        ],
        span: span(),
    };
    let src = codegen_one_expr(mt);
    // The literal pattern `0` lowers to a Rust literal pattern.
    assert!(
        src.contains("0 =>"),
        "expected literal pattern `0 =>` in: {src}"
    );
    assert!(src.contains("_ =>"), "expected wildcard arm in: {src}");
    must_reparse(&src);
}

#[test]
fn enum_codegen_match_with_nested_variant_pattern() {
    // `match r { Ok(Err(_)) => 1, _ => 0 }` — nested variant pattern.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("r")),
        arms: vec![
            arm(
                variant_pat("Ok", vec![variant_pat("Err", vec![wildcard_pat()])]),
                int_expr(1),
            ),
            arm(wildcard_pat(), int_expr(0)),
        ],
        span: span(),
    };
    let src = codegen_one_expr(mt);
    assert!(
        src.contains("Ok(Err(_))"),
        "expected nested pattern `Ok(Err(_))` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn enum_codegen_match_yields_value_via_let_binding() {
    // `let v = match c { Red => 1, _ => 0 }` — match is an expression that
    // yields a value. Confirms the codegen path composes with `let`.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(ident_pat("Red"), int_expr(1)),
            arm(wildcard_pat(), int_expr(0)),
        ],
        span: span(),
    };
    let stmt = Stmt::LetDecl {
        name: ident("v"),
        value: mt,
        mutable: false,
        ty: None,
        span: span(),
    };
    let src = codegen_stmts(vec![stmt]);
    // The `let v` binding should appear before the match.
    let let_off = src.find("let").expect("`let` present");
    let match_off = src.find("match").expect("`match` present");
    assert!(let_off < match_off, "let must precede match in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. End-to-end: enum decl + function with a match in one program.
// ---------------------------------------------------------------------------

#[test]
fn enum_codegen_end_to_end_decl_and_match_reparse() {
    // Combines: enum decl + a function whose body constructs a match on a
    // Color value. The generated source must re-parse as valid Rust.
    let color = enum_decl(
        "Color",
        &[],
        vec![
            unit_variant("Red"),
            unit_variant("Green"),
            unit_variant("Blue"),
        ],
    );
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(ident_pat("Red"), int_expr(1)),
            arm(ident_pat("Green"), int_expr(2)),
            arm(ident_pat("Blue"), int_expr(3)),
        ],
        span: span(),
    };
    let func = FuncDecl {
        name: ident("describe"),
        params: vec![Param {
            name: ident("c"),
            ty: named_ty("Color"),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("Int")),
        body: Block {
            stmts: vec![Stmt::Return(Some(mt), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[Decl::EnumDecl(color), Decl::FuncDecl(func)])
        .expect("end-to-end codegen must succeed");
    assert!(
        src.contains("pub enum Color"),
        "missing enum decl in: {src}"
    );
    assert!(src.contains("match c"), "missing match in: {src}");
    assert!(src.contains("Red =>"), "missing Red arm in: {src}");
    must_reparse(&src);
}

#[test]
fn enum_codegen_result_end_to_end_with_binding() {
    // Combines: `enum Result<T,E>` + a function that matches on a Result
    // value with `Ok(v) => v, Err(_) => 0`. Confirms generic + tuple
    // variants compose with match binding end-to-end.
    let result_decl = enum_decl(
        "Result",
        &["T", "E"],
        vec![tuple_variant("Ok", &["T"]), tuple_variant("Err", &["E"])],
    );
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("r")),
        arms: vec![
            arm(variant_pat("Ok", vec![ident_pat("v")]), ident_expr("v")),
            arm(variant_pat("Err", vec![wildcard_pat()]), int_expr(0)),
        ],
        span: span(),
    };
    let func = FuncDecl {
        name: ident("unwrap_or_zero"),
        params: vec![Param {
            name: ident("r"),
            ty: TypeRef::Generic {
                base: Box::new(named_ty("Result")),
                args: vec![named_ty("Int"), named_ty("String")],
                span: span(),
            },
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("Int")),
        body: Block {
            stmts: vec![Stmt::Return(Some(mt), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[Decl::EnumDecl(result_decl), Decl::FuncDecl(func)])
        .expect("end-to-end codegen must succeed");
    assert!(
        src.contains("pub enum Result<T, E>"),
        "missing generic enum in: {src}"
    );
    assert!(src.contains("Ok(T)"), "missing Ok(T) variant in: {src}");
    assert!(src.contains("Err(E)"), "missing Err(E) variant in: {src}");
    assert!(
        src.contains("Ok(v) =>"),
        "missing Ok(v) binding arm in: {src}"
    );
    assert!(src.contains("Err(_) =>"), "missing Err(_) arm in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. T85 — User-defined enum variant path qualification.
// ---------------------------------------------------------------------------

/// T85: user-defined enum variants MUST be qualified with the enum name
/// when emitted as bare identifier references. `Red` belonging to
/// `enum Color` must lower to `Color::Red`, not bare `Red` (which rustc
/// rejects as an unresolved identifier in expression position and treats
/// as a fresh binding pattern in match-arm position).
#[test]
fn t85_user_enum_variant_in_expression_is_qualified() {
    // `enum Color { Red, Green, Blue }` + `func f() { Red }`.
    // The bare `Red` in expression position must lower to `Color::Red`.
    let color = enum_decl(
        "Color",
        &[],
        vec![
            unit_variant("Red"),
            unit_variant("Green"),
            unit_variant("Blue"),
        ],
    );
    let func = FuncDecl {
        name: ident("f"),
        params: Vec::new(),
        return_type: Some(named_ty("Color")),
        body: Block {
            stmts: vec![Stmt::Return(Some(ident_expr("Red")), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[Decl::EnumDecl(color), Decl::FuncDecl(func)])
        .expect("T85 codegen must succeed");
    assert!(
        src.contains("Color::Red"),
        "T85: expected `Color::Red` qualified path in: {src}"
    );
    must_reparse(&src);
}

/// T85: bare variant in a match-arm IDENT pattern must lower to the
/// qualified path pattern `Color::Red` (otherwise rustc treats it as a
/// fresh binding that matches any value).
#[test]
fn t85_user_enum_variant_in_match_ident_pattern_is_qualified() {
    let color = enum_decl(
        "Color",
        &[],
        vec![
            unit_variant("Red"),
            unit_variant("Green"),
            unit_variant("Blue"),
        ],
    );
    // `match c { Red => 1, _ => 0 }` — Red is parsed as Pattern::Ident.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(ident_pat("Red"), int_expr(1)),
            arm(wildcard_pat(), int_expr(0)),
        ],
        span: span(),
    };
    let func = FuncDecl {
        name: ident("f"),
        params: vec![Param {
            name: ident("c"),
            ty: named_ty("Color"),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("Int")),
        body: Block {
            stmts: vec![Stmt::ExprStmt(mt, span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[Decl::EnumDecl(color), Decl::FuncDecl(func)])
        .expect("T85 codegen must succeed");
    assert!(
        src.contains("Color::Red =>"),
        "T85: expected `Color::Red =>` qualified arm in: {src}"
    );
    must_reparse(&src);
}

/// T85: tuple variant via `Pattern::Variant` with EMPTY `enum_name` must
/// also resolve through the registry (`Color::Rgb(r, g, b)` not bare
/// `Rgb(r, g, b)`).
#[test]
fn t85_user_enum_tuple_variant_in_match_resolves_via_registry() {
    let color = enum_decl(
        "Color",
        &[],
        vec![tuple_variant("Rgb", &["Int", "Int", "Int"])],
    );
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(
                variant_pat("Rgb", vec![ident_pat("r"), ident_pat("g"), ident_pat("b")]),
                ident_expr("r"),
            ),
            arm(wildcard_pat(), int_expr(0)),
        ],
        span: span(),
    };
    let func = FuncDecl {
        name: ident("f"),
        params: vec![Param {
            name: ident("c"),
            ty: named_ty("Color"),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("Int")),
        body: Block {
            stmts: vec![Stmt::ExprStmt(mt, span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[Decl::EnumDecl(color), Decl::FuncDecl(func)])
        .expect("T85 codegen must succeed");
    assert!(
        src.contains("Color::Rgb("),
        "T85: expected `Color::Rgb(` qualified tuple-variant arm in: {src}"
    );
    must_reparse(&src);
}

/// T85: prelude enums `Option` / `Result` are EXCLUDED — their variants
/// (`Some`/`None`/`Ok`/`Err`) live in the Rust prelude and must stay
/// unqualified. Even when the user explicitly declares an `enum Option`,
/// Buff stays out of the way (shadowing the prelude is the user's choice).
#[test]
fn t85_prelude_enums_are_not_qualified() {
    // `enum Option<T> { Some(T), None }` + `match o { Some(v) => v, None => 0 }`.
    // The variant patterns MUST stay as `Some(v)` / `None`, NOT
    // `Option::Some(v)` / `Option::None`.
    let option_decl = enum_decl(
        "Option",
        &["T"],
        vec![tuple_variant("Some", &["T"]), unit_variant("None")],
    );
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("o")),
        arms: vec![
            arm(variant_pat("Some", vec![ident_pat("v")]), ident_expr("v")),
            arm(variant_pat("None", vec![]), int_expr(0)),
        ],
        span: span(),
    };
    let func = FuncDecl {
        name: ident("f"),
        params: vec![Param {
            name: ident("o"),
            ty: TypeRef::Generic {
                base: Box::new(named_ty("Option")),
                args: vec![named_ty("Int")],
                span: span(),
            },
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("Int")),
        body: Block {
            stmts: vec![Stmt::ExprStmt(mt, span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[Decl::EnumDecl(option_decl), Decl::FuncDecl(func)])
        .expect("T85 codegen must succeed");
    assert!(
        src.contains("Some(") && !src.contains("Option::Some"),
        "T85: prelude Some MUST stay unqualified; got: {src}"
    );
    assert!(
        src.contains("None") && !src.contains("Option::None"),
        "T85: prelude None MUST stay unqualified; got: {src}"
    );
    must_reparse(&src);
}

/// T85: variant-name COLLISION across two user enums removes the entry
/// from the registry — bare references stay unqualified so rustc raises
/// the right "ambiguous" error and the user is forced to write `A::X`
/// or `B::X` explicitly.
#[test]
fn t85_variant_name_collision_is_left_unqualified() {
    // Two enums both declaring a variant `X`: ambiguous, so the bare `X`
    // reference must NOT be auto-qualified (rustc will emit the right
    // ambiguous-associated-type diagnostic).
    let enum_a = enum_decl("A", &[], vec![unit_variant("X")]);
    let enum_b = enum_decl("B", &[], vec![unit_variant("X")]);
    let func = FuncDecl {
        name: ident("f"),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts: vec![Stmt::ExprStmt(ident_expr("X"), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[
        Decl::EnumDecl(enum_a),
        Decl::EnumDecl(enum_b),
        Decl::FuncDecl(func),
    ])
    .expect("T85 codegen must succeed");
    assert!(
        !src.contains("A::X") && !src.contains("B::X"),
        "T85: colliding variant X must stay bare (no auto-qualify); got: {src}"
    );
}

// ---------------------------------------------------------------------------
// 5. T86 — Match return-position trailing-semicolon fix.
// ---------------------------------------------------------------------------

/// T86: `return match n { A => 1, _ => 0 }` must emit arm body blocks
/// WITHOUT the trailing `;` so the block yields the value (not `()`).
///
/// Without the fix, each arm body lowered as `{ 1; }` whose Rust type is
/// `()`, causing a type mismatch against the function's declared return
/// type. With the fix, the arm body becomes `{ 1 }` (tail expression).
#[test]
fn t86_return_match_strips_arm_body_trailing_semicolon() {
    // `func f(n: Int) -> Int { return match n { 0 => 1, _ => 99 } }`
    // The arm bodies `1` and `99` MUST lower as tail expressions (no
    // trailing `;`) so the function returns the matched value.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("n")),
        arms: vec![
            arm(int_lit_pat(0), int_expr(1)),
            arm(wildcard_pat(), int_expr(99)),
        ],
        span: span(),
    };
    let func = FuncDecl {
        name: ident("f"),
        params: vec![Param {
            name: ident("n"),
            ty: named_ty("Int"),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("Int")),
        body: Block {
            stmts: vec![Stmt::Return(Some(mt), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[Decl::FuncDecl(func)]).expect("T86 codegen must succeed");
    // The arm body `1` MUST NOT have a trailing `;` (would be `{ 1; }`
    // without the fix, yielding `()` instead of `i64`).
    assert!(
        !src.contains("1;"),
        "T86: arm body must NOT have trailing `;` in return-position match: {src}"
    );
    assert!(
        !src.contains("99;"),
        "T86: wildcard arm body must NOT have trailing `;`: {src}"
    );
    must_reparse(&src);
}

/// T86: standalone match (e.g. `match c { Red => 1, _ => 0 }` as an
/// ExprStmt) ALSO gets its arm body stripped of the trailing `;`.
///
/// Originally the strip was gated on return-position depth, but that left
/// implicit tail-expression returns broken (see [`lower_match_expr`] doc).
/// The strip is now unconditional: `{ expr }` is strictly more general than
/// `{ expr; }` — valid Rust whether the value is wanted or discarded.
#[test]
fn t86_standalone_match_keeps_arm_body_trailing_semicolon() {
    // `match c { Red => 1, _ => 0 }` as a standalone ExprStmt. Arm
    // bodies strip to `{ 1 }` shape (tail expression).
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(ident_pat("Red"), int_expr(1)),
            arm(wildcard_pat(), int_expr(0)),
        ],
        span: span(),
    };
    let src = codegen_one_expr(mt);
    // Arm body in standalone position is ALSO stripped (always-strip
    // rule — `{ expr }` is valid Rust in statement context too).
    assert!(
        !src.contains("1;"),
        "T86: standalone match arm body must NOT have trailing `;` (always strip): {src}"
    );
    must_reparse(&src);
}

/// T86: `let v = match c { ... }` — the match is the RHS of a let binding.
/// Arm bodies are stripped here too (always-strip rule). The let binding
/// captures the value of the match, so each arm MUST yield a value (not
/// `()`), which is exactly what the unconditional strip guarantees.
#[test]
fn t86_let_binding_match_keeps_arm_body_trailing_semicolon() {
    // `let v = match c { Red => 1, _ => 0 }` — let binding. The match
    // yields the arm value into `v`, so arms strip to `{ 1 }` shape.
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(ident_pat("Red"), int_expr(1)),
            arm(wildcard_pat(), int_expr(0)),
        ],
        span: span(),
    };
    let stmt = Stmt::LetDecl {
        name: ident("v"),
        value: mt,
        mutable: false,
        ty: None,
        span: span(),
    };
    let src = codegen_stmts(vec![stmt]);
    // Let-binding context: arm bodies are stripped (always-strip rule).
    assert!(
        !src.contains("1;"),
        "T86: let-binding match arm body must NOT have trailing `;` (always strip): {src}"
    );
    must_reparse(&src);
}

/// T86: end-to-end — `return match c { Red => 1, Green => 2, Blue => 3 }`
/// with the `Color` enum in scope. Combines T85 (qualify `Red` →
/// `Color::Red`) and T86 (strip arm body trailing `;`) so the function
/// typechecks as `-> i64`.
#[test]
fn t86_return_match_with_user_enum_typechecks() {
    let color = enum_decl(
        "Color",
        &[],
        vec![
            unit_variant("Red"),
            unit_variant("Green"),
            unit_variant("Blue"),
        ],
    );
    let mt = Expr::MatchExpr {
        scrutinee: Box::new(ident_expr("c")),
        arms: vec![
            arm(ident_pat("Red"), int_expr(1)),
            arm(ident_pat("Green"), int_expr(2)),
            arm(ident_pat("Blue"), int_expr(3)),
        ],
        span: span(),
    };
    let func = FuncDecl {
        name: ident("describe"),
        params: vec![Param {
            name: ident("c"),
            ty: named_ty("Color"),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: Some(named_ty("Int")),
        body: Block {
            stmts: vec![Stmt::Return(Some(mt), span())],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let src = generate_rust(&[Decl::EnumDecl(color), Decl::FuncDecl(func)])
        .expect("T86 codegen must succeed");
    // T85: user variants qualified.
    assert!(
        src.contains("Color::Red"),
        "T85+T86: expected `Color::Red` in: {src}"
    );
    // T86: no trailing `;` on arm bodies (so arm type is `i64` not `()`).
    assert!(
        !src.contains("1;") && !src.contains("2;") && !src.contains("3;"),
        "T86: return-position match arm bodies must NOT have trailing `;`: {src}"
    );
    must_reparse(&src);
}
