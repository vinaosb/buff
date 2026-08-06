//! T56 integration tests — codegen for property wrappers
//! (`@State` / `@Published` / `@Cached`).
//!
//! Property wrappers are a PURE parse-time desugar (no new AST nodes —
//! mirrors `|>`/`?.`/`??`). The parser rewrites:
//!
//! - `@State let x = init`      → `let x = ReactiveSignal.new(init)`
//! - `@Published let x = init`  → `let x = ReactiveSignal.new(init)`
//! - `@Cached(fn) let x = init` → `let x = ReactiveComputed.new({ || fn() })`
//!
//! The generated Rust uses the `ReactiveSignal` / `ReactiveComputed` surface
//! directly; the `program_uses_namespace` walker records `buff-reactive` in
//! `extern_crates` automatically.
//!
//! These tests verify BOTH ends of the pipeline:
//! - source-parsing tests exercise the full lex → parse → codegen path
//! - AST-driven tests pin the codegen shape against hand-build trees
//! - extern_crates tests confirm the walker records `buff-reactive`
//! - negative tests confirm plain `let` produces NO Signal/Computed
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test property_wrappers_codegen
//! cargo test -p buff-lang-codegen-rust property_wrappers
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_codegen_rust::{generate_rust, RustCodegen};
use buff_lang_error::{SourceId, Span};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

/// Wrap a list of statements in a zero-arg `fn f() { ... }` helper.
fn func_with_stmts(stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
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
    })
}

/// Run a Buff source snippet through lex → parse → codegen and return
/// the generated Rust source string.
fn codegen_src(src: &str) -> String {
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");
    generate_rust(&decls).expect("codegen must succeed")
}

/// Like [`codegen_src`] but also exposes the codegen's `extern_crates`
/// set so tests can verify `buff-reactive` registration.
fn codegen_src_with_deps(src: &str) -> (String, std::collections::BTreeSet<String>) {
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&decls).expect("codegen must succeed");
    let deps = codegen.extern_crates().clone();
    // `RustCodegen::generate` returns a `syn::File`; the canonical
    // String producer is `prettyplease::unparse` (see crate::format).
    // We re-run the same source through `generate_rust` to obtain the
    // formatted String (it constructs a fresh codegen internally, but
    // the output is byte-identical because codegen is deterministic).
    let formatted = generate_rust(&decls).expect("generate_rust must succeed");
    let _ = file;
    (formatted, deps)
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ===========================================================================
// 1. @State — desugars to ReactiveSignal.new(init).
// ===========================================================================

#[test]
fn state_source_desugars_to_buff_reactive_signal_new() {
    // @State let count = 0  -->  let count = ReactiveSignal.new(0)
    let src = "func f():\n    @State let count = 0\n";
    let out = codegen_src(src);
    assert!(
        out.contains("ReactiveSignal.new(0)"),
        "expected `ReactiveSignal.new(0)` (the @State desugar target) in: {out}"
    );
    assert!(
        !out.contains("@State"),
        "@State must NOT leak into generated Rust (it's a parse-time desugar): {out}"
    );
    must_reparse(&out);
}

#[test]
fn state_ast_driven_signal_new_with_initializer() {
    // Hand-build the desugared AST and verify the codegen output matches
    // the source-parsing path exactly (defends against parser regressions
    // masking codegen bugs).
    let desugared = Expr::MethodCall {
        receiver: Box::new(ident_expr("ReactiveSignal")),
        method: ident("new"),
        args: vec![int_expr(0)],
        span: span(),
    };
    let stmt = Stmt::LetDecl {
        name: ident("count"),
        value: desugared,
        mutable: false,
        ty: None,
        span: span(),
    };
    let out = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        out.contains("ReactiveSignal.new(0)"),
        "AST-driven path must produce the same codegen as source-parsing: {out}"
    );
}

// ===========================================================================
// 2. @Published — desugars to ReactiveSignal.new(init) (same as @State).
// ===========================================================================

#[test]
fn published_source_desugars_to_buff_reactive_signal_new() {
    // @Published is semantically distinct from @State (it's the
    // "observable" variant meant for cross-component sharing), but the
    // MVP lowering is identical: a Signal cell.
    let src = "func f():\n    @Published let score = 100\n";
    let out = codegen_src(src);
    assert!(
        out.contains("ReactiveSignal.new(100)"),
        "expected `ReactiveSignal.new(100)` (the @Published desugar target) in: {out}"
    );
    assert!(
        !out.contains("@Published"),
        "@Published must NOT leak into generated Rust: {out}"
    );
    must_reparse(&out);
}

// ===========================================================================
// 3. @Cached(compute_fn) — desugars to ReactiveComputed.new({ || fn() }).
// ===========================================================================

#[test]
fn cached_source_desugars_to_buff_reactive_computed_new() {
    // @Cached(expensive_fn) let cached = expensive_fn()
    //   --> let cached = ReactiveComputed.new({ || expensive_fn() })
    let src = "func f():\n    @Cached(expensive_fn) let cached = expensive_fn()\n";
    let out = codegen_src(src);
    assert!(
        out.contains("ReactiveComputed.new"),
        "expected `ReactiveComputed.new` (the @Cached desugar target) in: {out}"
    );
    assert!(
        out.contains("expensive_fn"),
        "expected the compute-fn name `expensive_fn` to appear in the closure body: {out}"
    );
    assert!(
        !out.contains("@Cached"),
        "@Cached must NOT leak into generated Rust: {out}"
    );
    must_reparse(&out);
}

#[test]
fn cached_ast_driven_computed_new_with_closure() {
    // Hand-build the desugared AST: a no-arg lambda whose body is a
    // FuncCall to the compute-fn name.
    let fn_call = Expr::FuncCall {
        callee: Box::new(ident_expr("expensive_fn")),
        args: Vec::new(),
        span: span(),
    };
    let lambda = Expr::Lambda {
        params: Vec::new(),
        body: Block {
            stmts: vec![Stmt::ExprStmt(fn_call, span())],
            span: span(),
        },
        return_type: None,
        span: span(),
    };
    let desugared = Expr::MethodCall {
        receiver: Box::new(ident_expr("ReactiveComputed")),
        method: ident("new"),
        args: vec![lambda],
        span: span(),
    };
    let stmt = Stmt::LetDecl {
        name: ident("cached"),
        value: desugared,
        mutable: false,
        ty: None,
        span: span(),
    };
    let out = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        out.contains("ReactiveComputed.new"),
        "AST-driven @Cached path must produce ReactiveComputed.new: {out}"
    );
}

// ===========================================================================
// 4. extern_crates registration.
// ===========================================================================

#[test]
fn state_registers_buff_reactive_in_extern_crates() {
    let src = "func f():\n    @State let count = 0\n";
    let (_out, deps) = codegen_src_with_deps(src);
    assert!(
        deps.contains("buff-reactive"),
        "expected `buff-reactive` in extern_crates for @State, got {:?}",
        deps
    );
}

#[test]
fn published_registers_buff_reactive_in_extern_crates() {
    let src = "func f():\n    @Published let score = 100\n";
    let (_out, deps) = codegen_src_with_deps(src);
    assert!(
        deps.contains("buff-reactive"),
        "expected `buff-reactive` in extern_crates for @Published, got {:?}",
        deps
    );
}

#[test]
fn cached_registers_buff_reactive_in_extern_crates() {
    // @Cached uses ReactiveComputed (buff-reactive), so the walker
    // records `buff-reactive`. (The T56 spec mentioned once_cell, but
    // the MVP uses buff-reactive's Computed for the same memoized-lazy
    // semantics — see the implementation note in the parser.)
    let src = "func f():\n    @Cached(compute) let cached = compute()\n";
    let (_out, deps) = codegen_src_with_deps(src);
    assert!(
        deps.contains("buff-reactive"),
        "expected `buff-reactive` in extern_crates for @Cached, got {:?}",
        deps
    );
}

// ===========================================================================
// 5. Negative: plain `let` produces NO Signal/Computed.
// ===========================================================================

#[test]
fn plain_let_produces_no_reactive_calls() {
    // A `let` WITHOUT a property-wrapper attribute must not pick up
    // any Signal/Computed wrapping by accident. This guards the
    // desugar against accidentally firing on the wrong AST shape.
    let src = "func f():\n    let count = 0\n";
    let (out, deps) = codegen_src_with_deps(src);
    assert!(
        !out.contains("buff_reactive::Signal::new"),
        "plain `let` must NOT produce Signal::new: {out}"
    );
    assert!(
        !out.contains("buff_reactive::Computed::new"),
        "plain `let` must NOT produce Computed::new: {out}"
    );
    assert!(
        !deps.contains("buff-reactive"),
        "plain `let` must NOT register buff-reactive in extern_crates, got {:?}",
        deps
    );
}

// ===========================================================================
// 6. Composability: multiple wrappers in one function.
// ===========================================================================

#[test]
fn multiple_wrappers_in_one_function_all_desugar() {
    // All three wrappers in one function body — each must desugar
    // independently to its target constructor.
    let src = "func f():\n    @State let a = 1\n    @Published let b = 2\n    @Cached(compute) let c = compute()\n";
    let out = codegen_src(src);
    assert!(
        out.contains("ReactiveSignal.new(1)"),
        "expected `ReactiveSignal.new(1)` for @State a: {out}"
    );
    assert!(
        out.contains("new(2)"),
        "expected `new(2)` for @Published b: {out}"
    );
    assert!(
        out.contains("ReactiveComputed.new"),
        "expected `ReactiveComputed.new` for @Cached c: {out}"
    );
    must_reparse(&out);
}

// ===========================================================================
// 7. Round-trip: existing user-written Signal.new() API still works.
// ===========================================================================

#[test]
fn existing_reactive_signal_new_api_still_compiles() {
    // The T56 spec requires additive-only behaviour: existing code that
    // uses Signal.new() / .set() / .get() directly must keep working.
    let src = "func f():\n    let count = ReactiveSignal.new(0)\n    count.set(1)\n    print(count.get())\n";
    let out = codegen_src(src);
    assert!(
        out.contains("ReactiveSignal.new(0)"),
        "existing ReactiveSignal.new() surface must still lower: {out}"
    );
    assert!(
        out.contains(".set(1)"),
        "existing Signal.set() instance method must still lower: {out}"
    );
    assert!(
        out.contains(".get()"),
        "existing Signal.get() instance method must still lower: {out}"
    );
    must_reparse(&out);
}

// ===========================================================================
// 8. @State drops the `mut` modifier on the binding.
// ===========================================================================

#[test]
fn state_drops_mut_modifier_on_binding() {
    // The Signal cell itself is immutable (mutation goes through
    // .set()/.update()), so `@State let mut x = init` must produce
    // `let x = Signal::new(init)` (no `mut`). Emitting `let mut x =
    // Signal::new(..)` would compile but is misleading and triggers
    // a clippy needless_mut warning.
    let src = "func f():\n    @State let mut count = 0\n";
    let out = codegen_src(src);
    assert!(
        out.contains("let count = ReactiveSignal.new(0)"),
        "expected `let count = ReactiveSignal.new(0)` (no `mut`), got: {out}"
    );
    assert!(
        !out.contains("let mut count"),
        "@State binding must NOT carry `mut`: {out}"
    );
}

// ===========================================================================
// 9. Parser error: unknown wrapper attribute.
// ===========================================================================

#[test]
fn parser_rejects_unknown_property_wrapper_attribute() {
    let src = "func f():\n    @Observed let x = 0\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let err = parse(&tokens, sid).expect_err("unknown wrapper must be a parse error");
    let msg = err.diagnostic.message;
    assert!(
        msg.contains("unknown property wrapper") && msg.contains("@Observed"),
        "expected error mentioning `unknown property wrapper` and `@Observed`, got: {msg}"
    );
}

// ===========================================================================
// 10. Parser error: @Cached missing its required arg.
// ===========================================================================

#[test]
fn parser_rejects_cached_without_compute_fn_arg() {
    let src = "func f():\n    @Cached let x = 0\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let err = parse(&tokens, sid).expect_err("@Cached without arg must be a parse error");
    let msg = err.diagnostic.message;
    assert!(
        msg.contains("@Cached") && msg.contains("compute_fn"),
        "expected error mentioning `@Cached` and `compute_fn`, got: {msg}"
    );
}

// ===========================================================================
// 11. Parser error: @State/@Published with spurious args.
// ===========================================================================

#[test]
fn parser_rejects_state_with_args() {
    let src = "func f():\n    @State(42) let x = 0\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let err = parse(&tokens, sid).expect_err("@State(42) must be a parse error");
    let msg = err.diagnostic.message;
    assert!(
        msg.contains("@State") && msg.contains("no arguments"),
        "expected error mentioning `@State` and `no arguments`, got: {msg}"
    );
}

// ===========================================================================
// 12. Parser error: stacked wrappers (more than one per `let`).
// ===========================================================================

#[test]
fn parser_rejects_stacked_property_wrappers() {
    let src = "func f():\n    @State @Published let x = 0\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let err = parse(&tokens, sid).expect_err("stacked wrappers must be a parse error");
    let msg = err.diagnostic.message;
    assert!(
        msg.contains("only one property wrapper"),
        "expected error mentioning `only one property wrapper`, got: {msg}"
    );
}
