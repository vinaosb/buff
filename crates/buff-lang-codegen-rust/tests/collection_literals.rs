//! T67 acceptance tests — Collection literals.
//!
//! Verifies that `[1, 2, 3]` → `vec![1, 2, 3]` and `{"k": v}` →
//! `std::collections::HashMap::from([("k", v)])` codegen works end-to-end.
//!
//! The functionality was already implemented by T23 (ArrayLit) and T25
//! (MapLit); this file adds the acceptance-test name `collection_literals`
//! so the T67 acceptance command runs ≥3 tests.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust collection_literals
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
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

/// Build an `Expr::ArrayLit` from a list of element expressions.
fn array_lit(elements: Vec<Expr>) -> Expr {
    Expr::ArrayLit {
        elements,
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

/// Wrap a list of statements in a no-arg function called `f`.
fn codegen_stmts(stmts: Vec<Stmt>) -> String {
    let func = FuncDecl { name: ident("f"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts,
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    generate_rust(&[Decl::FuncDecl(func)]).expect("codegen must succeed")
}

/// Like [`codegen_stmts`] but emits a single expression statement.
fn codegen_one_expr(expr: Expr) -> String {
    codegen_stmts(vec![Stmt::ExprStmt(expr, span())])
}

/// Assert the generated source re-parses as a valid Rust file.
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// 1. Array literal `[1, 2, 3]` -> `vec![1, 2, 3]`
// ---------------------------------------------------------------------------

#[test]
fn collection_literals_array_ints() {
    let src = codegen_one_expr(array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]));
    assert!(
        src.contains("vec![1, 2, 3]"),
        "expected `vec![1, 2, 3]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn collection_literals_empty_array() {
    let src = codegen_one_expr(array_lit(vec![]));
    assert!(src.contains("vec![]"), "expected `vec![]` in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Map literal `{"k": v}` -> `HashMap::from([("k", v)])`
// ---------------------------------------------------------------------------

#[test]
fn collection_literals_map_string_key() {
    let src = codegen_one_expr(map_lit(vec![(string_expr("k"), int_expr(42))]));
    assert!(
        src.contains("std::collections::HashMap::from([(\"k\", 42)])"),
        "expected `HashMap::from([(\"k\", 42)])` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn collection_literals_empty_map() {
    let src = codegen_one_expr(map_lit(vec![]));
    assert!(
        src.contains("std::collections::HashMap::from([])"),
        "expected `HashMap::from([])` for empty map in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Multi-entry map literal
// ---------------------------------------------------------------------------

#[test]
fn collection_literals_map_multi_entry() {
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

// ---------------------------------------------------------------------------
// 3. T82 — Map indexing syntax `m[key]` (read + write).
// ---------------------------------------------------------------------------

/// Build an `Expr::Ident` reference.
fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

/// Build a `let <name>: <ty> = <init>` statement with an explicit type
/// annotation (so the codegen's TypeInferencer can resolve map types
/// when deciding whether `m[k]` should lower via the T82 Map path or
/// the regular Vector path).
fn typed_let(name: &str, ty: &str, init: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value: init,
        mutable: false,
        ty: Some(buff_lang_ast::TypeRef::Named {
            name: ident(ty),
            span: span(),
        }),
        span: span(),
    }
}

/// Build an `Expr::Index` `base[idx]`.
fn index_expr(base: Expr, idx: Expr) -> Expr {
    Expr::Index {
        base: Box::new(base),
        indices: vec![idx],
        span: span(),
    }
}

/// Build an `m[key] = value` assignment statement.
fn index_assign(map_name: &str, key: Expr, value: Expr) -> Stmt {
    Stmt::Assignment {
        target: index_expr(ident_expr(map_name), key),
        op: buff_lang_ast::op::BinaryOp::Assign,
        value,
        span: span(),
    }
}

/// T82: READ — `let m: Map<String, Int> = {...}; m["k"]` must lower to
/// `m.get(&"k").cloned().unwrap_or_default()` so a missing key returns
/// the default (Buff's "no panic on missing keys" convention).
#[test]
fn t82_map_index_read_lowers_to_get_cloned_unwrap_or_default() {
    // Build: `let m: Map<String, Int> = { "k": 1 }; m["k"]`.
    let m_init = map_lit(vec![(string_expr("k"), int_expr(1))]);
    let m_decl = typed_let("m", "Map", m_init);
    let read = index_expr(ident_expr("m"), string_expr("k"));
    let src = codegen_stmts(vec![m_decl, Stmt::ExprStmt(read, span())]);
    assert!(
        src.contains(".get(&\"k\")"),
        "T82: expected `m.get(&\"k\")` in: {src}"
    );
    assert!(
        src.contains(".cloned()"),
        "T82: expected `.cloned()` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "T82: expected `.unwrap_or_default()` in: {src}"
    );
    // Must NOT emit the panic-on-missing `m["k"]` form.
    assert!(
        !src.contains("m[\"k\" as"),
        "T82: must NOT use raw `m[key as usize]` for Map: {src}"
    );
    must_reparse(&src);
}

/// T82: WRITE — `let m: Map<String, Int> = {...}; m["k"] = 42` must
/// lower to `m.insert("k", 42)` (Buff's "no panic on missing keys"
/// convention applies to writes too: insert creates-or-replaces).
#[test]
fn t82_map_index_write_lowers_to_insert() {
    let m_init = map_lit(vec![(string_expr("k"), int_expr(1))]);
    let m_decl = typed_let("m", "Map", m_init);
    let assign = index_assign("m", string_expr("k"), int_expr(42));
    let src = codegen_stmts(vec![m_decl, assign]);
    assert!(
        src.contains("m.insert(\"k\", 42)"),
        "T82: expected `m.insert(\"k\", 42)` in: {src}"
    );
    // Must NOT emit the unsupported `m["k"] = 42` form.
    assert!(
        !src.contains("m[\"k\"] ="),
        "T82: must NOT use raw `m[k] = v` for Map: {src}"
    );
    must_reparse(&src);
}

/// T82: Vector indexing is UNCHANGED — `v[i]` keeps lowering to
/// `v[i as usize]` (Rust's native Vec Index impl panics on out-of-
/// bounds, which is the existing Vector contract; T82 only relaxes the
/// Map contract). This guards against regressions where T82's Map path
/// accidentally catches Vector bases.
#[test]
fn t82_vector_indexing_is_unchanged() {
    // Build: `let v = [10, 20, 30]; v[1]`. No explicit type, so the
    // TypeInferencer infers `Vector<Int>` from the literal.
    let v_init = array_lit(vec![int_expr(10), int_expr(20), int_expr(30)]);
    let v_decl = Stmt::LetDecl {
        name: ident("v"),
        value: v_init,
        mutable: false,
        ty: None,
        span: span(),
    };
    let read = index_expr(ident_expr("v"), int_expr(1));
    let src = codegen_stmts(vec![v_decl, Stmt::ExprStmt(read, span())]);
    assert!(
        src.contains("v[1 as usize]"),
        "T82: Vector index must keep `v[i as usize]` shape: {src}"
    );
    // Must NOT lower via the Map path.
    assert!(
        !src.contains(".get(&"),
        "T82: Vector index must NOT use the Map `.get(&)` path: {src}"
    );
    must_reparse(&src);
}

/// T82: missing-key semantics — the READ lowering's `.unwrap_or_default()`
/// means a missing key returns the value type's default (0 for Int,
/// "" for String, false for Bool, etc.) rather than panicking. This
/// test verifies the codegen emits that chain shape exactly.
#[test]
fn t82_map_index_missing_key_returns_default_not_panic() {
    // `let m: Map<String, Int> = {:}; m["missing"]`.
    // An empty map indexed by a missing key — the codegen must produce
    // `.unwrap_or_default()` (no panic path).
    let m_decl = typed_let(
        "m",
        "Map",
        Expr::MapLit {
            entries: Vec::new(),
            span: span(),
        },
    );
    let read = index_expr(ident_expr("m"), string_expr("missing"));
    let src = codegen_stmts(vec![m_decl, Stmt::ExprStmt(read, span())]);
    assert!(
        src.contains("unwrap_or_default"),
        "T82: missing-key access must use `unwrap_or_default` (no panic): {src}"
    );
    must_reparse(&src);
}
