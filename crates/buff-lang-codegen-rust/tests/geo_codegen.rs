//! T45 integration tests - buff-geo prelude types codegen.
//!
//! Verifies that the Rust codegen lowers the three T45 geo types:
//!
//! - **Point** (`Point.new(x, y) -> Point`, `point.x()`, `point.y()`,
//!   `point.distance_to(other)`)
//! - **LineString** (`LineString.from_coords(flat) -> LineString`,
//!   `line_string.length()`)
//! - **Polygon** (`Polygon.new(ring) -> Polygon`, `polygon.area()`,
//!   `polygon.contains(point)`)
//!
//! Each constructor + instance method wraps the `buff_geo` crate's
//! safe API. Fallible constructors (`LineString.new` /
//! `LineString.from_coords` / `Polygon.new` / `Polygon.from_coords`)
//! are panic-free via `unwrap_or_default()` (the wrapper types impl
//! Default). `Point.new` is infallible. Instance methods (`x` / `y` /
//! `distance_to` / `length` / `area` / `contains` / `intersects`)
//! return `f64` / `bool` directly.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test geo_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All three types are prelude types (associated functions + instance
//! methods), so source parsing requires no new keyword / AST node -
//! the existing `MethodCall` shape handles them. We construct ASTs by
//! hand here for the same reasons `crypto_codegen.rs` (T124k),
//! `fs_codegen.rs` (T124j), `format_codegen.rs` (T124i),
//! `web_codegen.rs` (T124h), `system_codegen.rs` (T124g),
//! `regex_codegen.rs` (T124d), `toml_codegen.rs` (T124e), and
//! `utility_codegen.rs` (T124f) do: direct AST construction decouples
//! the codegen-pinning snapshots from any future parser-restructuring
//! work, and lets us test specific edge cases (e.g. wrong arity, ident
//! vs literal arg) without writing Buff source that the parser may
//! reject for orthogonal reasons.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::{generate_rust, RustCodegen};
use buff_lang_error::Span;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn float_expr(v: f64) -> Expr {
    Expr::Literal(Literal::Double(v), span())
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build a free-function decl `func <name>(<params...>) { <body> }`.
fn func_decl(name: &str, params: &[(&str, &str)], body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: params
            .iter()
            .map(|(n, t)| Param {
                name: ident(n),
                ty: named_type(t),
                default_value: None,
                is_comptime: false,
                span: span(),
            })
            .collect(),
        return_type: None,
        body: Block {
            stmts: body_stmts,
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

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

fn let_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty: None,
        span: span(),
    }
}

/// `<namespace>.<method>(args...)` AST node (associated-function call
/// shape). The receiver is the bare namespace Ident (e.g. `Point`,
/// `Polygon`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
/// The receiver is a variable Ident (e.g. `p`, `poly`).
fn instance_call(recv: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(recv)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// Generate Rust for a single helper function `f` containing `stmts`.
fn codegen_stmts_in(name: &str, stmts: Vec<Stmt>) -> String {
    let func = func_decl(name, &[], stmts);
    generate_rust(&[func]).expect("codegen must succeed")
}

/// Generate Rust for a single helper function `f` containing one expr stmt.
fn codegen_one_expr_in(name: &str, expr: Expr) -> String {
    codegen_stmts_in(name, vec![expr_stmt(expr)])
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ===========================================================================
// 1. Point.new â€” two-arg constructor (x: Float, y: Float).
// ===========================================================================

#[test]
fn point_codegen_new_with_literal_args() {
    // Point.new(1.0, 2.0) -> buff_geo::Point::new(1.0d, 2.0d).
    // Infallible â€” no unwrap_or_default needed.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Point", "new", vec![float_expr(1.0), float_expr(2.0)]),
    );
    assert!(
        src.contains("buff_geo::Point::new"),
        "expected `buff_geo::Point::new(` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn point_codegen_new_with_ident_args() {
    // Point.new(x, y) where x and y are variables. The args should
    // splice through as the bare idents.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Point", "new", vec![ident_expr("x"), ident_expr("y")]),
    );
    assert!(
        src.contains("buff_geo::Point::new"),
        "expected `buff_geo::Point::new(` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. point.distance_to â€” one-arg instance method returning Float.
// ===========================================================================

#[test]
fn point_codegen_distance_to_lowers_correctly() {
    // let p = Point.new(x, y)
    // p.distance_to(q)
    // The codegen must lower distance_to to `p.distance_to(q)`.
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "p",
                ns_assoc_call("Point", "new", vec![float_expr(1.0), float_expr(2.0)]),
            ),
            expr_stmt(instance_call("p", "distance_to", vec![ident_expr("q")])),
        ],
    );
    assert!(
        src.contains("buff_geo::Point::new"),
        "expected `buff_geo::Point::new(` for Point.new ctor in: {src}"
    );
    assert!(
        src.contains(".distance_to("),
        "expected `.distance_to(` (instance method) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Polygon.new + polygon.area â€” constructor + zero-arg instance method.
// ===========================================================================

#[test]
fn polygon_codegen_new_and_area_lowers_correctly() {
    // let ring = LineString.from_coords([0.0, 0.0, 1.0, 0.0, ...])
    // let poly = Polygon.new(ring)
    // poly.area()
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "ring",
                ns_assoc_call("LineString", "from_coords", vec![ident_expr("coords")]),
            ),
            let_stmt(
                "poly",
                ns_assoc_call("Polygon", "new", vec![ident_expr("ring")]),
            ),
            expr_stmt(instance_call("poly", "area", vec![])),
        ],
    );
    assert!(
        src.contains("buff_geo::LineString::from_coords"),
        "expected `buff_geo::LineString::from_coords(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free LineString.from_coords) in: {src}"
    );
    assert!(
        src.contains("buff_geo::Polygon::new"),
        "expected `buff_geo::Polygon::new(` in: {src}"
    );
    assert!(
        src.contains(".area()"),
        "expected `.area()` (instance method) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. Point.x / Point.y â€” zero-arg instance methods returning Float.
// ===========================================================================

#[test]
fn point_codegen_x_and_y_lowers_correctly() {
    // let p = Point.new(1.0, 2.0)
    // p.x()
    // p.y()
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "p",
                ns_assoc_call("Point", "new", vec![float_expr(1.0), float_expr(2.0)]),
            ),
            expr_stmt(instance_call("p", "x", vec![])),
            expr_stmt(instance_call("p", "y", vec![])),
        ],
    );
    assert!(
        src.contains(".x()"),
        "expected `.x()` (instance method) in: {src}"
    );
    assert!(
        src.contains(".y()"),
        "expected `.y()` (instance method) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. LineString.length â€” zero-arg instance method returning Float.
// ===========================================================================

#[test]
fn line_string_codegen_length_lowers_correctly() {
    // let ls = LineString.from_coords(coords)
    // ls.length()
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "ls",
                ns_assoc_call("LineString", "from_coords", vec![ident_expr("coords")]),
            ),
            expr_stmt(instance_call("ls", "length", vec![])),
        ],
    );
    assert!(
        src.contains(".length()"),
        "expected `.length()` (instance method) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 6. Polygon.contains / Polygon.intersects â€” bool-returning instance methods.
// ===========================================================================

#[test]
fn polygon_codegen_contains_lowers_correctly() {
    // let poly = Polygon.new(ring)
    // let p = Point.new(1.0, 2.0)
    // poly.contains(p)
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "poly",
                ns_assoc_call("Polygon", "new", vec![ident_expr("ring")]),
            ),
            let_stmt(
                "p",
                ns_assoc_call("Point", "new", vec![float_expr(1.0), float_expr(2.0)]),
            ),
            expr_stmt(instance_call("poly", "contains", vec![ident_expr("p")])),
        ],
    );
    assert!(
        src.contains(".contains("),
        "expected `.contains(` (instance method) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn polygon_codegen_intersects_lowers_correctly() {
    // let a = Polygon.new(ring_a)
    // let b = Polygon.new(ring_b)
    // a.intersects(b)
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "a",
                ns_assoc_call("Polygon", "new", vec![ident_expr("ring_a")]),
            ),
            let_stmt(
                "b",
                ns_assoc_call("Polygon", "new", vec![ident_expr("ring_b")]),
            ),
            expr_stmt(instance_call("a", "intersects", vec![ident_expr("b")])),
        ],
    );
    assert!(
        src.contains(".intersects("),
        "expected `.intersects(` (instance method) in: {src}"
    );
    // intersects takes &Polygon (borrows the arg).
    assert!(
        src.contains("&"),
        "expected `&` (borrow for intersects) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 7. extern_crates registration (narrow walkers).
// ===========================================================================

#[test]
fn geo_codegen_registers_buff_geo_for_point() {
    // A program with Point.new(...) registers buff-geo + geo + geo-types.
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "p",
            ns_assoc_call("Point", "new", vec![float_expr(1.0), float_expr(2.0)]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-geo"),
        "extern_crates should contain `buff-geo`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("geo"),
        "extern_crates should contain `geo`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("geo-types"),
        "extern_crates should contain `geo-types`, got: {:?}",
        extern_crates
    );
}

#[test]
fn geo_codegen_registers_buff_geo_for_polygon() {
    // A program with Polygon.new(...) also registers buff-geo + geo +
    // geo-types (the walker checks all three namespaces).
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "poly",
            ns_assoc_call("Polygon", "new", vec![ident_expr("ring")]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-geo"),
        "extern_crates should contain `buff-geo`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("geo-types"),
        "extern_crates should contain `geo-types`, got: {:?}",
        extern_crates
    );
}

#[test]
fn geo_codegen_no_extern_crate_when_unused() {
    // A program with no Point / LineString / Polygon calls should not
    // register buff-geo / geo / geo-types.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![ident_expr("hi")],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("buff-geo"),
        "extern_crates should NOT contain `buff-geo` when geo types are unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("geo-types"),
        "extern_crates should NOT contain `geo-types` when geo types are unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 8. Full program snapshot â€” pins the end-to-end codegen shape.
// ===========================================================================

#[test]
fn geo_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises the full geo
    // surface from the task spec's acceptance criteria.
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "p",
                ns_assoc_call("Point", "new", vec![float_expr(1.0), float_expr(2.0)]),
            ),
            let_stmt(
                "q",
                ns_assoc_call("Point", "new", vec![float_expr(4.0), float_expr(6.0)]),
            ),
            expr_stmt(instance_call("p", "distance_to", vec![ident_expr("q")])),
            let_stmt(
                "ring",
                ns_assoc_call("LineString", "from_coords", vec![ident_expr("coords")]),
            ),
            let_stmt(
                "poly",
                ns_assoc_call("Polygon", "new", vec![ident_expr("ring")]),
            ),
            expr_stmt(instance_call("poly", "area", vec![])),
            expr_stmt(instance_call("poly", "contains", vec![ident_expr("p")])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}
