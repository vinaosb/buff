//! T25 integration tests — `Map<K, V>` type, map literals, the Map method
//! family (`.get` / `.insert` / `.contains` / `.remove` / `.len`), and the
//! Type-inference for `Type::Map(K, V)`.
//!
//! Coverage:
//!
//! - `{"key": 42}` -> `std::collections::HashMap::from([("key", 42)])`
//! - multi-entry `{"name": "Alice", "age": 30}` lowers to a tuple-array
//! - empty `{:}` -> `std::collections::HashMap::from([])`
//! - `.get(k)` -> `.get(k)` (returns Option<&V> at the Rust level)
//! - `.insert(k, v)` -> `.insert(k, v)` (passthrough; same name in Rust)
//! - `.contains(k)` -> `.contains_key(k)` (Buff hides the `_key` suffix)
//! - `.remove(k)` -> `.remove(k)` (passthrough; same name in Rust)
//! - `.len()` -> `.len()` (passthrough; same name in Rust)
//! - inference: `{"k": 1}` -> `Type::Map(String, Int<8>)` (T22 auto-width)
//! - inference: empty `{:}` -> `Type::Map(Int<64>, Int<64>)` (default fallback)
//! - end-to-end: re-parses as valid Rust via `syn::parse_str`
//!
//! Each test builds a Buff AST by hand, runs it through
//! [`buff_lang_codegen_rust::generate_rust`], and asserts properties of the
//! resulting Rust source. The generated source is also re-parsed via
//! `syn::parse_str::<syn::File>` to guarantee it is valid Rust (syn doesn't
//! type-check, but it pins the syntactic correctness).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test map_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
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

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

/// A placeholder TypeRef for untyped closure params (codegen ignores it).
fn placeholder_ty() -> TypeRef {
    TypeRef::Named {
        name: ident("_"),
        span: span(),
    }
}

/// Build an `Expr::MapLit` from a list of `(key, value)` pairs.
fn map_lit(entries: Vec<(Expr, Expr)>) -> Expr {
    Expr::MapLit {
        entries,
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

/// Build a minimal closure `{ params => body }` as a Lambda node (kept for
/// symmetry with the other test files even though the Map tests don't need
/// it — confirms imports compile).
#[allow(dead_code)]
fn closure(params: &[&str], body: Expr) -> Expr {
    let params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: ident(p),
            ty: placeholder_ty(),
            default_value: None,
            is_comptime: false,
            span: span(),
        })
        .collect();
    Expr::Lambda {
        params,
        body: Block {
            stmts: vec![Stmt::ExprStmt(body, span())],
            span: span(),
        },
        return_type: None,
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
        attributes: Vec::new(),
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

// ---------------------------------------------------------------------------
// 1. Map literal `{"key": value}` -> `HashMap::from([("key", value)])`
// ---------------------------------------------------------------------------

#[test]
fn map_codegen_literal_single_string_int_entry() {
    // `{"key": 42}` -> `std::collections::HashMap::from([("key", 42)])`.
    let src = codegen_one_expr(map_lit(vec![(string_expr("key"), int_expr(42))]));
    assert!(
        src.contains("std::collections::HashMap::from([(\"key\", 42)])"),
        "expected `std::collections::HashMap::from([(\"key\", 42)])` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_literal_multi_entry_mixed_kinds() {
    // `{"name": "Alice", "age": 30}` -> HashMap::from with two tuples.
    let src = codegen_one_expr(map_lit(vec![
        (string_expr("name"), string_expr("Alice")),
        (string_expr("age"), int_expr(30)),
    ]));
    assert!(
        src.contains("std::collections::HashMap::from(["),
        "expected `HashMap::from([` prefix in: {src}"
    );
    assert!(
        src.contains("(\"name\", \"Alice\")"),
        "expected tuple `(\"name\", \"Alice\")` in: {src}"
    );
    assert!(
        src.contains("(\"age\", 30)"),
        "expected tuple `(\"age\", 30)` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_literal_empty_explicit_marker() {
    // `{:}` -> `std::collections::HashMap::from([])`.
    let src = codegen_one_expr(map_lit(vec![]));
    assert!(
        src.contains("std::collections::HashMap::from([])"),
        "expected `HashMap::from([])` for empty map in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_literal_trailing_comma_omitted_in_output() {
    // Even though the parser allows `{"a": 1,}`, the generated Rust should
    // be clean (no trailing comma in the array literal — prettyplease
    // formats it away).
    let src = codegen_one_expr(map_lit(vec![
        (string_expr("a"), int_expr(1)),
        (string_expr("b"), int_expr(2)),
    ]));
    // The closing `])` should immediately follow the last tuple — no
    // trailing comma between them.
    assert!(
        src.contains("(\"b\", 2)])"),
        "expected no trailing comma in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Map methods — get / insert / contains / remove / len
// ---------------------------------------------------------------------------

#[test]
fn map_codegen_get_passthrough_returns_option() {
    // m.get("key") -> m.get("key") (Rust returns Option<&V>).
    let src = codegen_one_expr(method_call(
        ident_expr("m"),
        "get",
        vec![string_expr("key")],
    ));
    assert!(
        src.contains("m.get(\"key\")"),
        "expected `m.get(\"key\")` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_insert_passthrough() {
    // m.insert("k", 1) -> m.insert("k", 1).
    let src = codegen_one_expr(method_call(
        ident_expr("m"),
        "insert",
        vec![string_expr("k"), int_expr(1)],
    ));
    assert!(
        src.contains("m.insert(\"k\", 1)"),
        "expected `m.insert(\"k\", 1)` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_contains_maps_to_contains_key() {
    // m.contains("k") -> m.contains_key("k"). Buff hides the `_key` suffix.
    let src = codegen_one_expr(method_call(
        ident_expr("m"),
        "contains",
        vec![string_expr("k")],
    ));
    assert!(
        src.contains("m.contains_key(\"k\")"),
        "expected `m.contains_key(\"k\")` in: {src}"
    );
    // AND the original Buff name must NOT leak through.
    assert!(
        !src.contains("m.contains(\"k\")"),
        "Buff `.contains` leaked into Rust (should be `contains_key`): {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_remove_passthrough() {
    // m.remove("k") -> m.remove("k").
    let src = codegen_one_expr(method_call(
        ident_expr("m"),
        "remove",
        vec![string_expr("k")],
    ));
    assert!(
        src.contains("m.remove(\"k\")"),
        "expected `m.remove(\"k\")` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_len_passthrough() {
    // m.len() -> m.len().
    let src = codegen_one_expr(method_call(ident_expr("m"), "len", vec![]));
    assert!(src.contains("m.len()"), "expected `m.len()` in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Type inference — MapLit -> Type::Map(K, V)
// ---------------------------------------------------------------------------

#[test]
fn map_codegen_inference_string_int_entry() {
    // `let m = {"k": 42}` -> key is String, value is Int<8> (T22 auto-width).
    // The codegen emits an explicit type annotation on `let` bindings (T12),
    // so we inspect it.
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("m"),
        value: map_lit(vec![(string_expr("k"), int_expr(42))]),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    assert!(
        src.contains("let m: std::collections::HashMap<String, i8> ="),
        "expected `let m: std::collections::HashMap<String, i8> =` (inferred) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_inference_empty_falls_back_to_default() {
    // `let m = {:}` -> empty map; both key and value default to Int<64>.
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("m"),
        value: map_lit(vec![]),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    assert!(
        src.contains("let m: std::collections::HashMap<i64, i64> ="),
        "expected `let m: std::collections::HashMap<i64, i64> =` (default fallback) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn map_codegen_inference_first_entry_wins_for_mixed() {
    // `{"name": "Alice", "age": 30}` — mixed values. Inference picks the
    // FIRST entry's value kind (String), so the annotation is
    // `HashMap<String, String>` (uniformity is deferred to a future task).
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("m"),
        value: map_lit(vec![
            (string_expr("name"), string_expr("Alice")),
            (string_expr("age"), int_expr(30)),
        ]),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    assert!(
        src.contains("let m: std::collections::HashMap<String, String> ="),
        "expected first-entry inference `HashMap<String, String>` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. End-to-end: a small program using Map constructs
// ---------------------------------------------------------------------------

#[test]
fn map_codegen_end_to_end_construct_query_and_mutate() {
    // Build a small program: construct a map, query it, mutate it.
    //   let m = {"x": 1}
    //   m.get("x")
    //   m.insert("y", 2)
    //   m.contains("z")
    //   m.remove("x")
    //   m.len()
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("m"),
            value: map_lit(vec![(string_expr("x"), int_expr(1))]),
            mutable: true,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(
            method_call(ident_expr("m"), "get", vec![string_expr("x")]),
            span(),
        ),
        Stmt::ExprStmt(
            method_call(
                ident_expr("m"),
                "insert",
                vec![string_expr("y"), int_expr(2)],
            ),
            span(),
        ),
        Stmt::ExprStmt(
            method_call(ident_expr("m"), "contains", vec![string_expr("z")]),
            span(),
        ),
        Stmt::ExprStmt(
            method_call(ident_expr("m"), "remove", vec![string_expr("x")]),
            span(),
        ),
        Stmt::ExprStmt(method_call(ident_expr("m"), "len", vec![]), span()),
    ];
    let src = codegen_stmts(stmts);
    // Each method call should produce the expected Rust form. The move
    // analyzer inserts `.clone()` on subsequent uses of `m` (a HashMap is
    // non-Copy), so we match the substring WITHOUT the leading `m.` — the
    // emitted form is `m.clone().<method>(...)`.
    assert!(src.contains(".get(\"x\")"), "missing .get in: {src}");
    assert!(
        src.contains(".insert(\"y\", 2)"),
        "missing .insert in: {src}"
    );
    assert!(
        src.contains(".contains_key(\"z\")"),
        "missing .contains_key in: {src}"
    );
    assert!(src.contains(".remove(\"x\")"), "missing .remove in: {src}");
    assert!(src.contains(".len()"), "missing .len in: {src}");
    // The map literal gets prettyplease-formatted across multiple lines
    // when the surrounding function body wraps; check the key fragments.
    assert!(
        src.contains("std::collections::HashMap::from(["),
        "missing map literal `from([` in: {src}"
    );
    assert!(src.contains("(\"x\", 1)"), "missing tuple in: {src}");
    must_reparse(&src);
}
