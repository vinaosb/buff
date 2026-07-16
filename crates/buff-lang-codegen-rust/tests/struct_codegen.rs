//! T26 integration tests — user-defined `struct` types: StructDecl codegen,
//! StructInit (literal) codegen, field-access-vs-method-call disambiguation,
//! and the `#[repr(C)]` hook for future GPU dispatch.
//!
//! Coverage:
//!
//! - `struct Person { name: String, age: Int }` →
//!   `#[derive(Clone, Debug)] pub struct Person { pub name: String, pub age: i64 }`
//! - `struct Point { x: Float, y: Float }` → pub x: f32, pub y: f32
//! - empty struct → still `#[derive]` + `pub struct Name { }`
//! - multi-type struct fields (Bool, Byte, Double, Char, Decimal)
//! - `Person { name: "Alice", age: 30 }` → valid Rust struct-init expression
//! - field access `p.name` → `p.name` (NOT a method call)
//! - field access on a struct-init expression (chained)
//! - method call vs field disambiguation: `s.char_count()` stays a method
//! - repr(C) hook: when flagged, struct gets `#[repr(C)]` between derive and `pub struct`
//! - default: no repr(C)
//! - end-to-end: re-parses as valid Rust via `syn::parse_str`
//! - parser disambiguates `Person { x: 1 }` (struct init) from `{ x => x }` (closure) and `{"k": v}` (map)
//!
//! Each test builds a Buff AST by hand (or runs the parser for end-to-end
//! cases), runs it through [`buff_lang_codegen_rust::generate_rust`] (or the
//! lower-level `RustCodegen` for the repr(C) hook), and asserts properties of
//! the resulting Rust source. The generated source is also re-parsed via
//! `syn::parse_str::<syn::File>` to guarantee it is valid Rust.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test struct_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{FuncDecl, StructDecl};
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
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

fn float_expr(n: f32) -> Expr {
    Expr::Literal(Literal::Float(n), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
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

/// Build a `struct` declaration AST node with the given name + fields.
fn struct_decl(name: &str, fields: Vec<(&str, TypeRef)>) -> StructDecl {
    StructDecl {
        name: ident(name),
        fields: fields.into_iter().map(|(n, t)| (ident(n), t)).collect(),
        traits: Vec::new(),
        span: span(),
    }
}

/// Build `receiver.method(args...)` as an AST node.
fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: ident(method),
        args,
        span: span(),
    }
}

/// Build `Type { field: value, ... }` as a StructInit AST node.
fn struct_init(type_name: &str, fields: Vec<(&str, Expr)>) -> Expr {
    Expr::StructInit {
        type_name: ident(type_name),
        fields: fields.into_iter().map(|(n, v)| (ident(n), v)).collect(),
        span: span(),
    }
}

/// Wrap a list of statements in a no-arg function called `f`.
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
        span: span(),
    };
    generate_rust(&[Decl::FuncDecl(func)]).expect("codegen must succeed")
}

/// Like [`codegen_stmts`] but emits a single expression statement.
fn codegen_one_expr(expr: Expr) -> String {
    codegen_stmts(vec![Stmt::ExprStmt(expr, span())])
}

/// Generate Rust source from a single struct declaration.
fn codegen_struct(d: StructDecl) -> String {
    generate_rust(&[Decl::StructDecl(d)]).expect("struct codegen must succeed")
}

/// Generate Rust source from a struct decl followed by a function with the
/// given statements. Used for tests that exercise both the type definition
/// and an expression that constructs / uses it.
fn codegen_struct_and_func(d: StructDecl, stmts: Vec<Stmt>) -> String {
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
        span: span(),
    };
    generate_rust(&[Decl::StructDecl(d), Decl::FuncDecl(func)])
        .expect("struct + fn codegen must succeed")
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// Suppress unused warning for Param import (used implicitly via closures in
// other test files in this crate; kept here for symmetry).
#[allow(dead_code)]
fn _param_smoke() -> Param {
    Param {
        name: ident("_"),
        ty: named_ty("Int"),
        span: span(),
    }
}

// ---------------------------------------------------------------------------
// 1. StructDecl — basic shape, derives, pub fields
// ---------------------------------------------------------------------------

#[test]
fn struct_codegen_decl_point_two_floats_snapshot() {
    // `struct Point { x: Float, y: Float }` →
    // `#[derive(Clone, Debug)] pub struct Point { pub x: f32, pub y: f32 }`
    let src = codegen_struct(struct_decl(
        "Point",
        vec![("x", named_ty("Float")), ("y", named_ty("Float"))],
    ));
    insta::assert_snapshot!(src, @r###"
    #[derive(Clone, Debug)]
    pub struct Point {
        pub x: f32,
        pub y: f32,
    }
    "###);
    must_reparse(&src);
}

#[test]
fn struct_codegen_decl_person_string_int() {
    // `struct Person { name: String, age: Int }` →
    // `pub name: String, pub age: i64`
    let src = codegen_struct(struct_decl(
        "Person",
        vec![("name", named_ty("String")), ("age", named_ty("Int"))],
    ));
    assert!(
        src.contains("#[derive(Clone, Debug)]"),
        "expected derive attribute in: {src}"
    );
    assert!(
        src.contains("pub struct Person"),
        "expected `pub struct Person` in: {src}"
    );
    assert!(
        src.contains("pub name: String"),
        "expected `pub name: String` in: {src}"
    );
    assert!(
        src.contains("pub age: i64"),
        "expected `pub age: i64` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn struct_codegen_decl_empty_struct() {
    // An empty struct still gets derives + pub struct + empty body.
    let src = codegen_struct(struct_decl("Empty", Vec::new()));
    assert!(
        src.contains("#[derive(Clone, Debug)]"),
        "expected derive in: {src}"
    );
    assert!(
        src.contains("pub struct Empty"),
        "expected pub struct in: {src}"
    );
    // Re-parsing confirms the empty body is valid Rust.
    must_reparse(&src);
}

#[test]
fn struct_codegen_decl_various_primitive_field_types() {
    // Multiple primitive field types map to their Rust equivalents.
    let src = codegen_struct(struct_decl(
        "Many",
        vec![
            ("a", named_ty("Bool")),
            ("b", named_ty("Byte")),
            ("c", named_ty("Double")),
            ("d", named_ty("Char")),
            ("e", named_ty("Decimal")),
        ],
    ));
    assert!(src.contains("pub a: bool"), "expected bool in: {src}");
    assert!(src.contains("pub b: u8"), "expected u8 in: {src}");
    assert!(src.contains("pub c: f64"), "expected f64 in: {src}");
    assert!(src.contains("pub d: char"), "expected char in: {src}");
    assert!(
        src.contains("pub e: rust_decimal::Decimal"),
        "expected rust_decimal::Decimal in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. StructInit — `Type { field: value, ... }` -> Rust struct-init expr
// ---------------------------------------------------------------------------

#[test]
fn struct_codegen_init_two_fields_string_int() {
    // `Person { name: "Alice", age: 30 }` → Rust struct-init expr.
    let src = codegen_one_expr(struct_init(
        "Person",
        vec![("name", string_expr("Alice")), ("age", int_expr(30))],
    ));
    assert!(
        src.contains("Person { name: \"Alice\", age: 30 }"),
        "expected `Person {{ name: \"Alice\", age: 30 }}` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn struct_codegen_init_two_floats_point() {
    // `Point { x: 1.0f, y: 2.0f }` → struct-init expr with f32 values. The
    // generated Rust uses the explicit `1.0f32` form because prettyplease
    // pins the float suffix on typed literals.
    let src = codegen_one_expr(struct_init(
        "Point",
        vec![("x", float_expr(1.0)), ("y", float_expr(2.0))],
    ));
    assert!(
        src.contains("Point { x: 1.0f32, y: 2.0f32 }"),
        "expected `Point {{ x: 1.0f32, y: 2.0f32 }}` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn struct_codegen_init_trailing_comma_normalised() {
    // Multi-field init should produce valid Rust even with many fields.
    let src = codegen_one_expr(struct_init(
        "Triple",
        vec![("a", int_expr(1)), ("b", int_expr(2)), ("c", int_expr(3))],
    ));
    assert!(src.contains("Triple {"), "expected `Triple {{` in: {src}");
    assert!(src.contains("a: 1"), "expected `a: 1` in: {src}");
    assert!(src.contains("b: 2"), "expected `b: 2` in: {src}");
    assert!(src.contains("c: 3"), "expected `c: 3` in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Field access — `obj.field` lowers to `obj.field` (Expr::Field),
//    NOT `obj.field()` (method call).
// ---------------------------------------------------------------------------

#[test]
fn struct_codegen_field_access_zero_arg_method_call_emits_field() {
    // `p.name` parses as `MethodCall(p, "name", [])`. With the T26 heuristic,
    // an empty-args method whose name is NOT a known builtin lowers to a
    // syn::Expr::Field (not a method call).
    let src = codegen_one_expr(method_call(ident_expr("p"), "name", Vec::new()));
    assert!(
        src.contains("p.name"),
        "expected field access `p.name` in: {src}"
    );
    // Make sure it's NOT emitted as a method call (`p.name()` would be wrong).
    assert!(
        !src.contains("p.name()"),
        "should NOT have method-call form `p.name()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn struct_codegen_field_access_distinct_fields() {
    // Two different field names on the same receiver both emit field access.
    // NOTE: Buff is move-by-default (T33a), so the SECOND use of `p` triggers
    // the move analyzer's `.clone()` insertion (`p.clone().age`). That's the
    // correct codegen; both accesses are still FIELD accesses (not methods).
    let src = codegen_stmts(vec![
        Stmt::ExprStmt(method_call(ident_expr("p"), "name", Vec::new()), span()),
        Stmt::ExprStmt(method_call(ident_expr("p"), "age", Vec::new()), span()),
    ]);
    assert!(src.contains("p.name"), "expected `p.name` in: {src}");
    assert!(
        // Either `p.age` (first use, no clone) or `p.clone().age` (second
        // use after move). Both are field accesses, not method calls.
        src.contains("p.clone().age") || src.contains("p.age"),
        "expected field access `p.age` or `p.clone().age` in: {src}"
    );
    assert!(
        !src.contains("p.name()"),
        "`p.name` should be a field access, not method call in: {src}"
    );
    assert!(
        !src.contains(".age()"),
        "no `.age()` method-call form in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn struct_codegen_field_access_on_struct_init_chained() {
    // `(Person { ... }).age` — chained: a struct-init followed by field
    // access. Confirms the field-access path composes with struct-init.
    let init = struct_init(
        "Person",
        vec![("name", string_expr("Bob")), ("age", int_expr(42))],
    );
    let src = codegen_one_expr(method_call(init, "age", Vec::new()));
    // We don't assert the exact prettyplease formatting of the parenthesised
    // struct-init, but the field access `age` must appear (not `age()`).
    assert!(
        !src.contains(".age()"),
        "chained access should be a field, not method call, in: {src}"
    );
    // The generated source must still be valid Rust.
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. Method-call-vs-field disambiguation: known builtins stay methods
// ---------------------------------------------------------------------------

#[test]
fn struct_codegen_known_string_method_stays_method_call() {
    // `s.char_count()` is a known zero-arg string builtin; it must stay a
    // method call (`s.chars().count()`), NOT be rewritten as `s.char_count`.
    let src = codegen_one_expr(method_call(ident_expr("s"), "char_count", Vec::new()));
    assert!(
        src.contains("s.chars().count()"),
        "expected `s.chars().count()` in: {src}"
    );
    assert!(
        !src.contains(".char_count"),
        "should NOT see the raw Buff method name in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn struct_codegen_known_collection_len_stays_method_call() {
    // `v.len()` is a known zero-arg builtin; it must stay a method call,
    // NOT become a field access `v.len`.
    let src = codegen_one_expr(method_call(ident_expr("v"), "len", Vec::new()));
    assert!(src.contains("v.len()"), "expected `v.len()` in: {src}");
    must_reparse(&src);
}

#[test]
fn struct_codegen_known_one_arg_method_stays_method_call() {
    // `m.get("k")` is a known one-arg builtin; the disambiguation only fires
    // on EMPTY-args calls, so this must still be a method call.
    let src = codegen_one_expr(method_call(ident_expr("m"), "get", vec![string_expr("k")]));
    assert!(
        src.contains("m.get(\"k\")"),
        "expected `m.get(\"k\")` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. repr(C) hook — opt-in mechanism; full GPU detection lands in v1.0.
// ---------------------------------------------------------------------------

#[test]
fn struct_codegen_repr_c_emitted_when_struct_marked() {
    // When a struct name is marked via the repr(C) hook, the generated struct
    // declaration carries `#[repr(C)]` BETWEEN the derive attribute and the
    // `pub struct` line. Full GPU-dispatch auto-detection is deferred to v1.0;
    // T26 provides the emission mechanism only.
    use buff_lang_codegen_rust::RustCodegen;
    let d = struct_decl(
        "GpuPoint",
        vec![("x", named_ty("Float")), ("y", named_ty("Float"))],
    );
    let mut codegen = RustCodegen::new();
    codegen.mark_struct_repr_c("GpuPoint");
    let file = codegen
        .generate(&[Decl::StructDecl(d)])
        .expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    assert!(
        src.contains("#[derive(Clone, Debug)]"),
        "expected derive attribute in: {src}"
    );
    assert!(
        src.contains("#[repr(C)]"),
        "expected `#[repr(C)]` in: {src}"
    );
    assert!(
        src.contains("pub struct GpuPoint"),
        "expected `pub struct GpuPoint` in: {src}"
    );
    // Ordering: derive attribute must come before repr(C), which must come
    // before `pub struct`. We check this by comparing byte offsets.
    let derive_off = src.find("#[derive(Clone, Debug)]").expect("derive present");
    let repr_off = src.find("#[repr(C)]").expect("repr(C) present");
    let struct_off = src.find("pub struct GpuPoint").expect("pub struct present");
    assert!(
        derive_off < repr_off && repr_off < struct_off,
        "ordering: derive < repr(C) < pub struct — got offsets {derive_off}, {repr_off}, {struct_off} in:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn struct_codegen_no_repr_c_by_default() {
    // Default codegen path produces NO `#[repr(C)]` (the hook is opt-in).
    let src = codegen_struct(struct_decl("Plain", vec![("x", named_ty("Int"))]));
    assert!(
        !src.contains("#[repr(C)]"),
        "should NOT emit `#[repr(C)]` by default; got: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 6. End-to-end: struct decl + struct init + field access in one program
// ---------------------------------------------------------------------------

#[test]
fn struct_codegen_end_to_end_decl_init_and_field_access() {
    // Combines: struct decl + `let p = Person { ... }` + `p.age` access.
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("p"),
            value: struct_init(
                "Person",
                vec![("name", string_expr("Alice")), ("age", int_expr(30))],
            ),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(method_call(ident_expr("p"), "age", Vec::new()), span()),
    ];
    let src = codegen_struct_and_func(
        struct_decl(
            "Person",
            vec![("name", named_ty("String")), ("age", named_ty("Int"))],
        ),
        stmts,
    );
    // Struct decl part.
    assert!(
        src.contains("pub struct Person"),
        "missing struct decl: {src}"
    );
    // Struct init part.
    assert!(
        src.contains("Person { name: \"Alice\", age: 30 }"),
        "missing struct init in: {src}"
    );
    // Field access part (NOT a method call).
    assert!(src.contains("p.age"), "missing field access in: {src}");
    assert!(
        !src.contains("p.age()"),
        "field access should not be method call in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 7. Parser disambiguation: `Person { x: 1 }` (struct init) vs
//    `{ x => x }` (closure) vs `{"k": v}` (map). Confirms the parser produces
//    an `Expr::StructInit` for `Ident { field: value }` and that map / closure
//    parsing is unaffected (T25 regression check).
// ---------------------------------------------------------------------------

#[test]
fn struct_codegen_parser_disambiguates_ident_brace_as_struct_init() {
    // `Person { x: 1 }` should parse as StructInit (not map, not closure).
    let tokens = buff_lang_lexer::tokenize("Person { x: 1 }", buff_lang_error::SourceId(0))
        .expect("lex must succeed");
    let expr = buff_lang_parser::parse_expression(&tokens, buff_lang_error::SourceId(0))
        .expect("parse must succeed");
    match expr {
        Expr::StructInit {
            type_name, fields, ..
        } => {
            assert_eq!(type_name.name, "Person", "type name in: {type_name:?}");
            assert_eq!(fields.len(), 1, "expected 1 field, got {}", fields.len());
            assert_eq!(fields[0].0.name, "x", "field name");
        }
        other => panic!("expected StructInit, got {other:?}"),
    }
}

#[test]
fn struct_codegen_parser_map_literal_still_works_t25_regression() {
    // `{"k": 42}` should still parse as MapLit (T25 regression check).
    let tokens = buff_lang_lexer::tokenize("{\"k\": 42}", buff_lang_error::SourceId(0))
        .expect("lex must succeed");
    let expr = buff_lang_parser::parse_expression(&tokens, buff_lang_error::SourceId(0))
        .expect("parse must succeed");
    assert!(
        matches!(expr, Expr::MapLit { ref entries, .. } if entries.len() == 1),
        "expected MapLit with 1 entry, got {expr:?}"
    );
}

#[test]
fn struct_codegen_parser_closure_still_works_t25_regression() {
    // `{ x => x + 1 }` should still parse as Lambda (T25 regression check).
    let tokens = buff_lang_lexer::tokenize("{ x => x + 1 }", buff_lang_error::SourceId(0))
        .expect("lex must succeed");
    let expr = buff_lang_parser::parse_expression(&tokens, buff_lang_error::SourceId(0))
        .expect("parse must succeed");
    assert!(
        matches!(expr, Expr::Lambda { .. }),
        "expected Lambda, got {expr:?}"
    );
}
