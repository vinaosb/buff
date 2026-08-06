//! T124f integration tests - utility prelude modules codegen.
//!
//! Verifies that the Rust codegen lowers the four T124f utility modules:
//!
//! - **Math** namespace (`Math.sqrt(x)`, `Math.sin(x)`, ..., `Math.PI`,
//!   `Math.E`) - wraps Rust's `std::f64` methods + `std::f64::consts`.
//!   Uses only Rust `std` (NO extern crate).
//! - **Random** namespace (`Random.int(lo, hi)`, `Random.float()`,
//!   `Random.choice(v)`, `Random.shuffle(v)`) - wraps the `rand` crate
//!   (0.9 API: `rng().random_range`, `random::<f64>`,
//!   `IndexedRandom::choose`/`shuffle`). Records `rand` in extern_crates.
//! - **Sort** instance methods on Buff's existing Vector type
//!   (`vec.sort()`, `vec.sort_by(cmp)`) - lowers to a `{ let mut __v =
//!   recv; __v.sort[_by](...); __v }` block so the surface stays
//!   functional (`[3,1,2].sort() -> [1,2,3]`). Uses only Rust `std`.
//! - **Strings** namespace (`Strings.split(t, s)`, `Strings.join(v, s)`,
//!   ..., `Strings.to_uppercase(t)`, `Strings.to_lowercase(t)`) - wraps
//!   Rust's `str` / `String` methods. Uses only Rust `std`.
//!
//! Acceptance snapshots for the canonical criteria:
//!
//! ```text
//! Math.sqrt(16)        ->  (16 as f64).sqrt()       (= 4.0)
//! Math.PI              ->  std::f64::consts::PI
//! Math.floor(3.7)      ->  (3.7 as f64).floor()     (= 3.0)
//! Random.int(1, 100)   ->  rand::rng().random_range(1..=100)
//! Random.choice(v)     ->  rand::seq::IndexedRandom::choose(&v, &mut rng).cloned()
//! [3, 1, 2].sort()     ->  { let mut __v = vec![3, 1, 2]; __v.sort(); __v }
//! Strings.split(s, ",") ->  s.split(",").map(|s| s.to_string()).collect::<Vec<String>>()
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test utility_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All four modules are prelude namespaces (or instance methods on the
//! existing Vector type), so source parsing requires no new keyword /
//! AST node - the existing `MethodCall` shape handles them. We
//! construct ASTs by hand here for the same reasons `regex_codegen.rs`
//! (T124d) and `toml_codegen.rs` (T124e) do: direct AST construction
//! decouples the codegen-pinning snapshots from any future parser-
//! restructuring work, and lets us test specific edge cases (e.g.
//! wrong arity, ident vs literal arg) without writing Buff source
//! that the parser may reject for orthogonal reasons.

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

fn str_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn float_expr(n: f64) -> Expr {
    Expr::Literal(Literal::Float(n as f32), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
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

/// `<Namespace>.<method>(args...)` AST node (associated-function call shape).
/// The receiver is the bare namespace Ident.
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `<Namespace>.<CONST>` AST node (associated-constant access shape).
/// Buff's parser produces a zero-arg MethodCall for `obj.field`, so we
/// mirror that exactly: args == [].
fn ns_const_access(namespace: &str, name: &str) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(name),
        args: vec![],
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
fn method_call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(recv),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `[a, b, c, ...]` AST node (Vector literal).
fn vec_lit(elements: Vec<Expr>) -> Expr {
    Expr::ArrayLit {
        elements,
        span: span(),
    }
}

/// `{ params... => body }` AST node (lambda).
fn lambda(params: &[&str], body: Expr) -> Expr {
    Expr::Lambda {
        params: params
            .iter()
            .map(|n| buff_lang_ast::common::Param {
                name: ident(n),
                ty: named_type("Int"),
                default_value: None,
                is_comptime: false,
                span: span(),
            })
            .collect(),
        body: Block {
            stmts: vec![expr_stmt(body)],
            span: span(),
        },
        return_type: None,
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
// 1. Math module - associated functions (sqrt / sin / cos / tan / abs /
//    floor / ceil / round / pow / min / max).
// ===========================================================================

#[test]
fn math_codegen_sqrt_int_arg_casts_to_f64() {
    // Math.sqrt(16) -> (16 as f64).sqrt()
    // Acceptance criterion: cast makes Int arg work like Float arg.
    let src = codegen_one_expr_in("f", ns_assoc_call("Math", "sqrt", vec![int_expr(16)]));
    assert!(
        src.contains("(16 as f64).sqrt()"),
        "expected `(16 as f64).sqrt()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn math_codegen_sqrt_float_arg_no_double_cast() {
    // Math.sqrt(2.0) -> (2.0 as f64).sqrt()
    // The cast is always emitted (uniform codegen for all arg types).
    let src = codegen_one_expr_in("f", ns_assoc_call("Math", "sqrt", vec![float_expr(2.0)]));
    // Spot-check that sqrt appears at all (prettyplease may render the
    // literal differently, e.g. `2.0f64`).
    assert!(src.contains(".sqrt()"), "expected `.sqrt()` in: {src}");
    assert!(
        src.contains("2.0") || src.contains("2f64"),
        "expected the float literal `2.0` (or prettyplease rendering) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn math_codegen_sin_cos_tan_abs_floor_ceil_round() {
    // Each unary Math method lowers to `(<arg> as f64).<method>()`.
    for (method, _) in [
        ("sin", ()),
        ("cos", ()),
        ("tan", ()),
        ("abs", ()),
        ("floor", ()),
        ("ceil", ()),
        ("round", ()),
    ] {
        let src = codegen_one_expr_in("f", ns_assoc_call("Math", method, vec![ident_expr("x")]));
        assert!(
            src.contains(&format!("(x as f64).{method}()")),
            "expected `(x as f64).{method}()` in: {src}"
        );
        must_reparse(&src);
    }
}

#[test]
fn math_codegen_pow_two_args() {
    // Math.pow(base, exp) -> ((base as f64).powf(exp as f64))
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Math", "pow", vec![ident_expr("base"), ident_expr("exp")]),
    );
    assert!(
        src.contains("(base as f64).powf(exp as f64)"),
        "expected `(base as f64).powf(exp as f64)` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn math_codegen_min_max_two_args() {
    // Math.min(a, b) -> (a as f64).min(b as f64)
    // Math.max(a, b) -> (a as f64).max(b as f64)
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Math", "min", vec![ident_expr("a"), ident_expr("b")]),
    );
    assert!(
        src.contains("(a as f64).min(b as f64)"),
        "expected `(a as f64).min(b as f64)` in: {src}"
    );
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Math", "max", vec![ident_expr("a"), ident_expr("b")]),
    );
    assert!(
        src.contains("(a as f64).max(b as f64)"),
        "expected `(a as f64).max(b as f64)` in: {src}"
    );
}

#[test]
fn math_codegen_no_extern_crate_registered() {
    // Math uses only Rust `std` - NO extern crate should be registered.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call("Math", "sqrt", vec![int_expr(16)]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("rand"),
        "Math should NOT register `rand` extern crate, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("chrono"),
        "Math should NOT register `chrono` extern crate, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 2. Math module - associated CONSTANTS (Math.PI / Math.E).
// ===========================================================================

#[test]
fn math_codegen_pi_const_lowers_to_std_path() {
    // Math.PI -> std::f64::consts::PI
    let src = codegen_one_expr_in("f", ns_const_access("Math", "PI"));
    assert!(
        src.contains("std::f64::consts::PI"),
        "expected `std::f64::consts::PI` in: {src}"
    );
    // Must NOT be lowered as a Rust field access `Math.PI` (Math is a
    // namespace, not a Rust type with a PI field - that would not
    // compile).
    assert!(
        !src.contains("Math.PI"),
        "expected NO bare `Math.PI` field access in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn math_codegen_e_const_lowers_to_std_path() {
    // Math.E -> std::f64::consts::E
    let src = codegen_one_expr_in("f", ns_const_access("Math", "E"));
    assert!(
        src.contains("std::f64::consts::E"),
        "expected `std::f64::consts::E` in: {src}"
    );
    assert!(
        !src.contains("Math.E"),
        "expected NO bare `Math.E` field access in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Random module - associated functions (int / float / choice / shuffle).
// ===========================================================================

#[test]
fn random_codegen_int_inclusive_range() {
    // Random.int(1, 100) -> rand::rng().random_range(1..=100)
    // The `..=` (inclusive) matches the spec acceptance `Random.int(1, 10)`
    // returns int in [1, 10] (NOT [1, 11)).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Random", "int", vec![int_expr(1), int_expr(100)]),
    );
    assert!(
        src.contains("rand::rng().random_range(1..=100)"),
        "expected `rand::rng().random_range(1..=100)` in: {src}"
    );
    // Must use `..=` (inclusive), NOT `..` (exclusive).
    assert!(
        !src.contains("1..100)"),
        "expected inclusive `..=` range, NOT exclusive `..`, in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn random_codegen_float_zero_args() {
    // Random.float() -> rand::rng().random::<f64>()
    let src = codegen_one_expr_in("f", ns_assoc_call("Random", "float", vec![]));
    assert!(
        src.contains("rand::rng().random::<f64>()"),
        "expected `rand::rng().random::<f64>()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn random_codegen_choice_uses_slicerandom_cloned() {
    // Random.choice(vec) ->
    //   rand::seq::IndexedRandom::choose(&vec, &mut rand::rng()).cloned()
    //
    // The `.cloned()` lifts `Option<&T>` to `Option<T>` (Buff hides
    // references). The fully-qualified `IndexedRandom::choose` path
    // avoids needing a `use` import.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Random", "choice", vec![ident_expr("vec")]),
    );
    assert!(
        src.contains("rand::seq::IndexedRandom::choose"),
        "expected `rand::seq::IndexedRandom::choose` in: {src}"
    );
    assert!(
        src.contains("&vec"),
        "expected `&vec` (IndexedRandom::choose takes &self) in: {src}"
    );
    assert!(
        src.contains("&mut rand::rng()"),
        "expected `&mut rand::rng()` in: {src}"
    );
    assert!(
        src.contains(".cloned()"),
        "expected `.cloned()` (lift Option<&T> -> Option<T>) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn random_codegen_shuffle_uses_block_returning_vec() {
    // Random.shuffle(vec) -> { let mut __v = vec;
    //   rand::seq::IndexedRandom::shuffle(&mut __v, &mut rng); __v }
    //
    // The block evaluates to the owned shuffled Vec (Buff's surface
    // treats sort/shuffle as functional - returns a NEW Vec).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Random", "shuffle", vec![ident_expr("vec")]),
    );
    assert!(
        src.contains("rand::seq::IndexedRandom::shuffle"),
        "expected `rand::seq::IndexedRandom::shuffle` in: {src}"
    );
    assert!(
        src.contains("let mut __v"),
        "expected `let mut __v` (internal mutation binding) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn random_codegen_registers_rand_extern_crate() {
    // Any Random.* call should register the `rand` crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Random",
            "int",
            vec![int_expr(1), int_expr(10)],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("rand"),
        "extern_crates should contain `rand`, got: {:?}",
        extern_crates
    );
}

#[test]
fn random_codegen_registers_rand_via_float() {
    // A program with Random.float() but no other Random call should
    // still register rand.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call("Random", "float", vec![]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("rand"),
        "extern_crates should contain `rand` (float walker), got: {:?}",
        extern_crates
    );
}

#[test]
fn random_codegen_registers_rand_via_choice_or_shuffle() {
    // Both Random.choice and Random.shuffle should register rand.
    for method in ["choice", "shuffle"] {
        let main = func_decl(
            "main",
            &[],
            vec![expr_stmt(ns_assoc_call(
                "Random",
                method,
                vec![ident_expr("v")],
            ))],
        );
        let mut codegen = RustCodegen::new();
        let _ = codegen.generate(&[main]).expect("codegen must succeed");
        let extern_crates = codegen.extern_crates();
        assert!(
            extern_crates.contains("rand"),
            "extern_crates should contain `rand` (via {method}), got: {:?}",
            extern_crates
        );
    }
}

#[test]
fn random_codegen_no_rand_extern_crate_when_unused() {
    // A program with no Random calls should not register rand.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![str_expr("hi")],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("rand"),
        "extern_crates should NOT contain `rand` when Random is unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 4. Sort - instance methods on Buff's existing Vector type.
// ===========================================================================

#[test]
fn sort_codegen_zero_arg_sort_returns_block() {
    // `[3, 1, 2].sort()` -> { let mut __v = vec![3, 1, 2]; __v.sort(); __v }
    //
    // The block evaluates to the owned sorted Vec (functional surface).
    let src = codegen_one_expr_in(
        "f",
        method_call(
            vec_lit(vec![int_expr(3), int_expr(1), int_expr(2)]),
            "sort",
            vec![],
        ),
    );
    assert!(
        src.contains("let mut __v"),
        "expected `let mut __v` (internal mutation binding) in: {src}"
    );
    assert!(
        src.contains("__v.sort()"),
        "expected `__v.sort()` in: {src}"
    );
    // Must NOT be lowered as a bare field access `vec.sort` (T26
    // field-access heuristic bypassed via KNOWN_ZERO_ARG_METHODS).
    assert!(
        !src.contains(".sort\n"),
        "expected NO bare field access `vec.sort` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn sort_codegen_sort_by_with_comparator() {
    // `[3, 1, 2].sort_by(cmp)` -> { let mut __v = vec![3, 1, 2];
    //   __v.sort_by(cmp); __v }
    let cmp = lambda(&["a", "b"], ident_expr("a"));
    let src = codegen_one_expr_in(
        "f",
        method_call(
            vec_lit(vec![int_expr(3), int_expr(1), int_expr(2)]),
            "sort_by",
            vec![cmp],
        ),
    );
    assert!(
        src.contains("let mut __v"),
        "expected `let mut __v` in: {src}"
    );
    assert!(
        src.contains("__v.sort_by("),
        "expected `__v.sort_by(` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn sort_codegen_no_extern_crate_registered() {
    // Sort uses only Rust `std` slice `.sort()` / `.sort_by()` - NO
    // extern crate should be registered.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(method_call(
            vec_lit(vec![int_expr(3), int_expr(1), int_expr(2)]),
            "sort",
            vec![],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("rand"),
        "Sort should NOT register `rand` extern crate, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 5. Strings module - associated functions (split / join / trim / replace
//    / contains / starts_with / to_uppercase / to_lowercase).
// ===========================================================================

#[test]
fn strings_codegen_split_collects_to_vec_string() {
    // Strings.split(text, sep) ->
    //   text.split(sep).map(|s| s.to_string()).collect::<Vec<String>>()
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Strings", "split", vec![str_expr("a,b,c"), str_expr(",")]),
    );
    assert!(
        src.contains(".split(\",\".to_string())"),
        "expected `.split(\",\".to_string())` in: {src}"
    );
    assert!(
        src.contains(".map(|s| s.to_string())"),
        "expected `.map(|s| s.to_string())` (lift &str to String) in: {src}"
    );
    assert!(
        src.contains(".collect::<Vec<String>>()"),
        "expected `.collect::<Vec<String>>()` turbofish in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_split_ident_arg_borrows() {
    // Strings.split(my_string_var, sep) - non-literal text borrows via &.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Strings",
            "split",
            vec![ident_expr("my_string_var"), str_expr(",")],
        ),
    );
    assert!(
        src.contains("&my_string_var"),
        "expected `&my_string_var` (borrow coercion for String -> &str) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_join_borrows_sep() {
    // Strings.join(vec, sep) -> vec.join(&sep.to_string())
    // The sep is borrowed via `&` to satisfy `&str` bound; the literal is
    // lifted via `.to_string()` (Buff hides `&str` from the user).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Strings", "join", vec![ident_expr("vec"), str_expr(",")]),
    );
    assert!(
        src.contains("vec.join(&"),
        "expected `vec.join(&...)` (sep borrowed) in: {src}"
    );
    assert!(
        src.contains("\",\".to_string()"),
        "expected the comma separator literal in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_trim_chains_to_string() {
    // Strings.trim(text) -> text.trim().to_string()
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Strings", "trim", vec![str_expr("  hi  ")]),
    );
    assert!(
        src.contains(".trim().to_string()"),
        "expected `.trim().to_string()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_replace_three_args() {
    // Strings.replace(text, from, to) -> text.replace(from.to_string(), to.to_string())
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Strings",
            "replace",
            vec![str_expr("a1b2"), str_expr("1"), str_expr("X")],
        ),
    );
    assert!(
        src.contains(".replace(\"1\".to_string(), \"X\".to_string())"),
        "expected `.replace(\"1\".to_string(), \"X\".to_string())` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_contains_returns_bool_via_method_call() {
    // Strings.contains(text, substr) -> text.contains(substr.to_string())
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Strings",
            "contains",
            vec![str_expr("hello"), str_expr("ell")],
        ),
    );
    assert!(
        src.contains(".contains(\"ell\".to_string())"),
        "expected `.contains(\"ell\".to_string())` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_starts_with_returns_bool() {
    // Strings.starts_with(text, prefix) -> text.starts_with(prefix.to_string())
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Strings",
            "starts_with",
            vec![str_expr("hello"), str_expr("he")],
        ),
    );
    assert!(
        src.contains(".starts_with(\"he\".to_string())"),
        "expected `.starts_with(\"he\".to_string())` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_to_uppercase_chains_to_string() {
    // Strings.to_uppercase(text) -> text.to_uppercase().to_string()
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Strings", "to_uppercase", vec![str_expr("hi")]),
    );
    assert!(
        src.contains(".to_uppercase().to_string()"),
        "expected `.to_uppercase().to_string()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_to_lowercase_chains_to_string() {
    // Strings.to_lowercase(text) -> text.to_lowercase().to_string()
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Strings", "to_lowercase", vec![str_expr("HI")]),
    );
    assert!(
        src.contains(".to_lowercase().to_string()"),
        "expected `.to_lowercase().to_string()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn strings_codegen_no_extern_crate_registered() {
    // Strings uses only Rust `std` - NO extern crate should be registered.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Strings",
            "trim",
            vec![str_expr("hi")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("rand"),
        "Strings should NOT register `rand` extern crate, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 6. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn math_codegen_rejects_sqrt_with_wrong_arity() {
    // Math.sqrt() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Math", "sqrt", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Math.sqrt()` (no arg)"
    );
}

#[test]
fn random_codegen_rejects_int_with_wrong_arity() {
    // Random.int(1) with one arg - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Random", "int", vec![int_expr(1)]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Random.int(1)` (expected 2 args)"
    );
}

#[test]
fn random_codegen_rejects_float_with_args() {
    // Random.float(42) with args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Random", "float", vec![int_expr(42)]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Random.float(42)` (expected 0 args)"
    );
}

#[test]
fn strings_codegen_rejects_replace_with_wrong_arity() {
    // Strings.replace("a", "b") with two args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call("Strings", "replace", vec![str_expr("a"), str_expr("b")]),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Strings.replace(a, b)` (expected 3 args)"
    );
}

// ===========================================================================
// 7. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn math_codegen_sqrt_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Math", "sqrt", vec![int_expr(16)]));
    insta::assert_snapshot!(src);
}

#[test]
fn math_codegen_pi_const_snapshot() {
    let src = codegen_one_expr_in("f", ns_const_access("Math", "PI"));
    insta::assert_snapshot!(src);
}

#[test]
fn math_codegen_pow_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Math", "pow", vec![int_expr(2), int_expr(10)]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn random_codegen_int_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Random", "int", vec![int_expr(1), int_expr(100)]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn random_codegen_float_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Random", "float", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn random_codegen_choice_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Random", "choice", vec![ident_expr("vec")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn random_codegen_shuffle_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Random", "shuffle", vec![ident_expr("vec")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn sort_codegen_literal_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        method_call(
            vec_lit(vec![int_expr(3), int_expr(1), int_expr(2)]),
            "sort",
            vec![],
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn sort_by_codegen_snapshot() {
    let cmp = lambda(&["a", "b"], ident_expr("a"));
    let src = codegen_one_expr_in(
        "f",
        method_call(
            vec_lit(vec![int_expr(3), int_expr(1), int_expr(2)]),
            "sort_by",
            vec![cmp],
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn strings_codegen_split_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Strings", "split", vec![str_expr("a,b,c"), str_expr(",")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn strings_codegen_join_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Strings", "join", vec![ident_expr("vec"), str_expr(",")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn strings_codegen_replace_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Strings",
            "replace",
            vec![str_expr("a1b2"), str_expr("1"), str_expr("X")],
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn utility_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises one call from each
    // of the four modules. Pins the full shape of the generated Rust
    // for a typical utility-using program (the acceptance criterion
    // from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt("r", ns_assoc_call("Math", "sqrt", vec![int_expr(16)])),
            let_stmt("pi", ns_const_access("Math", "PI")),
            let_stmt(
                "n",
                ns_assoc_call("Random", "int", vec![int_expr(1), int_expr(100)]),
            ),
            let_stmt(
                "s",
                method_call(
                    vec_lit(vec![int_expr(3), int_expr(1), int_expr(2)]),
                    "sort",
                    vec![],
                ),
            ),
            expr_stmt(ns_assoc_call(
                "Strings",
                "to_uppercase",
                vec![str_expr("hi")],
            )),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}
