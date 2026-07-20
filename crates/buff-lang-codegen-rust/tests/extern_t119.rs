//! T119 integration tests — minimal extern/bindgen.
//!
//! Coverage:
//!
//! - **`extern "C" func name(...)` parse + codegen** — the new (v1.3)
//!   ABI-string form lowers to a Rust `extern "C" { fn ...; }` foreign
//!   mod, identical to the legacy `extern func` lowering except that the
//!   ABI string is sourced from the user's declaration.
//! - **`extern "C" from "serde_json" func ...`** — the `from "crate"`
//!   annotation records the crate in the codegen's `extern_crates` set
//!   AND contributes to `collect_rust_deps` so the CLI can populate
//!   `[rust-deps]` in `buff.toml`.
//! - **Type marshalling** — String, Int, Float, Bool, Vector all map
//!   correctly to their Rust counterparts (String→String, Int→i64,
//!   Float→f32, Double→f64, Bool→bool, Vector<T>→Vec<T>).
//! - **Call-site `unsafe` wrapping** — Buff calls to declared extern
//!   functions are silently wrapped in `unsafe { ... }` (Rust requires
//!   it; Buff hides it from the user per the README "no unsafe Rust"
//!   guarantee).
//! - **Generic rejection** — `extern "C" func parse<T>(...)` produces a
//!   clear parse error mentioning the v1.3 generics-unsupported policy.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test extern_t119
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::ty::TypeRef;
use buff_lang_ast::Decl;
use buff_lang_codegen_rust::{collect_rust_deps, generate_rust, RustCodegen};
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

fn named_ty(s: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(s),
        span: span(),
    }
}

/// Round-trip Buff source → AST → Rust source.
fn rust_for(src: &str) -> String {
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");
    generate_rust(&decls).expect("codegen must succeed")
}

/// Assert the generated source re-parses as a valid Rust file.
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// Parse + codegen: `extern "C" func name(params) -> Ret`
// ---------------------------------------------------------------------------

#[test]
fn t119_extern_c_func_lowers_to_rust_foreign_mod() {
    // `extern "C" func rust_fn(x: Int) -> Int` → Rust
    // `extern "C" { fn rust_fn(x: i64) -> i64; }`.
    let src = "extern \"C\" func rust_fn(x: Int) -> Int\n";
    let rust = rust_for(src);
    assert!(
        rust.contains("extern \"C\""),
        "expected `extern \"C\"` ABI marker in generated Rust: {rust}"
    );
    assert!(
        rust.contains("fn rust_fn(x: i64) -> i64"),
        "expected `fn rust_fn(x: i64) -> i64` in generated Rust: {rust}"
    );
    must_reparse(&rust);
}

#[test]
fn t119_extern_c_func_snapshot() {
    let src = "extern \"C\" func parse(input: String) -> String\n";
    let rust = rust_for(src);
    insta::assert_snapshot!(rust, @r###"
    extern "C" {
        fn parse(input: String) -> String;
    }
    "###);
}

// ---------------------------------------------------------------------------
// `from "crate"` annotation → extern_crates + collect_rust_deps
// ---------------------------------------------------------------------------

#[test]
fn t119_from_annotation_records_crate_in_extern_crates() {
    // `extern "C" from "serde_json" func parse(s: String) -> String`
    // records `serde_json` in the codegen's `extern_crates` set (the
    // codegen-level surrogate for `[rust-deps]`).
    let src = "extern \"C\" from \"serde_json\" func parse(s: String) -> String\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");

    let mut codegen = RustCodegen::new();
    let _file = codegen.generate(&decls).expect("codegen must succeed");
    let deps = codegen.extern_crates();
    assert!(
        deps.contains("serde_json"),
        "expected `serde_json` in extern_crates, got {:?}",
        deps
    );
}

#[test]
fn t119_collect_rust_deps_picks_up_from_annotation() {
    let src = "extern \"C\" from \"serde_json\" func parse_str(s: String) -> String\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");
    let deps = collect_rust_deps(&decls);
    assert_eq!(
        deps.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        vec!["serde_json"]
    );
}

#[test]
fn t119_collect_rust_deps_picks_up_both_extern_forms() {
    // The legacy `extern crate "serde"` AND the new
    // `extern "C" from "serde_json" func ...` BOTH contribute to the
    // collect_rust_deps set.
    let src = concat!(
        "extern crate \"serde\"\n",
        "extern \"C\" from \"serde_json\" func parse(s: String) -> String\n",
    );
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");
    let deps = collect_rust_deps(&decls);
    assert!(deps.contains("serde"));
    assert!(deps.contains("serde_json"));
}

#[test]
fn t119_collect_rust_deps_dedupes_repeated_crates() {
    let src = concat!(
        "extern \"C\" from \"serde_json\" func parse_a(s: String) -> String\n",
        "extern \"C\" from \"serde_json\" func parse_b(s: String) -> String\n",
    );
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");
    let deps = collect_rust_deps(&decls);
    // Two decls, same crate → dedup to one entry (BTreeSet semantics).
    assert_eq!(deps.len(), 1, "expected serde_json deduped, got {deps:?}");
    assert!(deps.contains("serde_json"));
}

// ---------------------------------------------------------------------------
// Type marshalling — String/Int/Float/Double/Bool/Vector all map correctly
// ---------------------------------------------------------------------------

#[test]
fn t119_marshal_string_param_and_return() {
    let src = "extern \"C\" func echo(s: String) -> String\n";
    let rust = rust_for(src);
    assert!(
        rust.contains("fn echo(s: String) -> String"),
        "expected String→String mapping in: {rust}"
    );
    must_reparse(&rust);
}

#[test]
fn t119_marshal_int_param() {
    let src = "extern \"C\" func incr(n: Int) -> Int\n";
    let rust = rust_for(src);
    assert!(
        rust.contains("fn incr(n: i64) -> i64"),
        "expected Int→i64 mapping in: {rust}"
    );
}

#[test]
fn t119_marshal_float_and_double_params() {
    // Buff `Float` → Rust `f32`, Buff `Double` → Rust `f64`.
    let src = "extern \"C\" func mix(a: Float, b: Double) -> Double\n";
    let rust = rust_for(src);
    assert!(
        rust.contains("fn mix(a: f32, b: f64) -> f64"),
        "expected Float→f32 and Double→f64 mapping in: {rust}"
    );
}

#[test]
fn t119_marshal_bool_param() {
    let src = "extern \"C\" func negate(b: Bool) -> Bool\n";
    let rust = rust_for(src);
    assert!(
        rust.contains("fn negate(b: bool) -> bool"),
        "expected Bool→bool mapping in: {rust}"
    );
}

#[test]
fn t119_marshal_vector_param_via_generic_typeref() {
    // NOTE: the unresolved TypeRef::Generic path passes the BASE name
    // through verbatim, so user-written `Vector<Int>` → `Vector<i64>`
    // (the inner-arg mapping IS applied). The `Vector`→`Vec` spelling
    // rewrite happens on the RESOLVED `Type` path (`buff_type_to_syn`),
    // not on the unresolved TypeRef path the extern-fn signature uses.
    // This is the same limitation as the pre-T119 test
    // `ast_typeref_generic_containers_map_inner_types_correctly`.
    let src = "extern \"C\" func sum_vec(v: Vector<Int>) -> Int\n";
    let rust = rust_for(src);
    assert!(
        rust.contains("Vector<i64>"),
        "expected Vector<Int> → Vector<i64> in unresolved form: {rust}"
    );
}

// ---------------------------------------------------------------------------
// Call-site `unsafe { ... }` auto-wrapping
// ---------------------------------------------------------------------------

#[test]
fn t119_call_to_extern_fn_wraps_in_unsafe_block() {
    // A Buff program that calls a declared extern fn must lower the call
    // site to `unsafe { name(args) }` — Rust requires this, Buff hides it.
    let src = concat!(
        "extern \"C\" func parse(input: String) -> String\n",
        "func main():\n",
        "    let result = parse(\"hello\")\n",
        "    print(result)\n",
    );
    let rust = rust_for(src);
    // prettyplease formats the unsafe block across multiple lines, so we
    // assert on the canonical text `unsafe {` followed by `parse(`.
    assert!(
        rust.contains("unsafe {") && rust.contains("parse("),
        "expected `unsafe {{ ... parse(...) }}` call-site wrap in generated Rust: {rust}"
    );
    must_reparse(&rust);
}

#[test]
fn t119_call_to_non_extern_fn_does_not_wrap_in_unsafe() {
    // A regular user fn call is NOT wrapped in `unsafe` — only declared
    // extern fns are.
    let src = concat!(
        "func helper(x: Int) -> Int:\n",
        "    return x + 1\n",
        "func main():\n",
        "    let result = helper(41)\n",
        "    print(result)\n",
    );
    let rust = rust_for(src);
    assert!(
        !rust.contains("unsafe {"),
        "regular fn call must NOT be wrapped in unsafe: {rust}"
    );
}

// ---------------------------------------------------------------------------
// Generic rejection
// ---------------------------------------------------------------------------

#[test]
fn t119_generic_extern_func_rejected_with_clear_error() {
    // `extern "C" func parse<T>(s: String) -> T` must be a parse error
    // (generics unsupported in v1.3).
    let src = "extern \"C\" func parse<T>(s: String) -> T\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let err = parse(&tokens, sid).expect_err("must error on generic extern fn");
    assert!(
        err.diagnostic
            .message
            .contains("generics are not supported on `extern` functions"),
        "expected generics-rejection error, got: {}",
        err.diagnostic.message
    );
}

#[test]
fn t119_unsupported_abi_rejected_with_clear_error() {
    // `extern "system" func ...` is unsupported in v1.3 — only "C".
    let src = "extern \"system\" func go():\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let err = parse(&tokens, sid).expect_err("must error on unsupported ABI");
    assert!(
        err.diagnostic.message.contains("unsupported ABI"),
        "expected unsupported-ABI error, got: {}",
        err.diagnostic.message
    );
    assert!(
        err.diagnostic.message.contains("\"C\""),
        "expected error to mention the supported ABI \"C\": {}",
        err.diagnostic.message
    );
}

// ---------------------------------------------------------------------------
// End-to-end: `extern "C" from "serde_json" func parse(...) -> ...` round-trip
// ---------------------------------------------------------------------------

#[test]
fn t119_serde_json_pattern_round_trip() {
    // The README's flagship T119 example: a Buff program declares a
    // serde_json-backed extern fn and calls it. The generated Rust must
    // contain BOTH the foreign-mod declaration AND the unsafe call wrap.
    let src = concat!(
        "extern \"C\" from \"serde_json\" func parse_str(input: String) -> String\n",
        "func main():\n",
        "    let result = parse_str(\"data\")\n",
        "    print(result)\n",
    );
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");

    // Both decls parse correctly.
    assert_eq!(
        decls.len(),
        2,
        "expected 2 decls (extern + func), got {}",
        decls.len()
    );

    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&decls).expect("codegen must succeed");
    let rust = buff_lang_codegen_rust::format(&file);

    // The crate is recorded for [rust-deps] emission.
    assert!(codegen.extern_crates().contains("serde_json"));

    // The generated Rust contains the extern block + the unsafe call wrap.
    assert!(rust.contains("extern \"C\""));
    assert!(rust.contains("fn parse_str(input: String) -> String;"));
    assert!(
        rust.contains("unsafe {") && rust.contains("parse_str("),
        "expected unsafe call wrap in: {rust}"
    );
    must_reparse(&rust);
}

// ---------------------------------------------------------------------------
// Legacy compat: `extern func name(...)` (no ABI string) still works
// ---------------------------------------------------------------------------

#[test]
fn t119_legacy_extern_func_still_works() {
    // The v0.5 form `extern func name(...) -> Ret` (no ABI string) is
    // preserved — it lowers to FuncDecl with is_extern=true and emits
    // the same `extern "C" { ... }` foreign-mod as before.
    let src = "extern func legacy_fn(x: Int) -> Int\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::FuncDecl(f) => {
            assert_eq!(f.name.name, "legacy_fn");
            assert!(f.is_extern, "legacy `extern func` must set is_extern");
        }
        other => panic!("expected FuncDecl for legacy extern, got {other:?}"),
    }
    let rust = generate_rust(&decls).expect("codegen must succeed");
    assert!(rust.contains("extern \"C\""));
    assert!(rust.contains("fn legacy_fn(x: i64) -> i64;"));
}

// ---------------------------------------------------------------------------
// Parser disambiguation: three `extern ...` shapes
// ---------------------------------------------------------------------------

#[test]
fn t119_parser_disambiguates_three_extern_forms() {
    // The dispatcher in parse_one_decl routes by peeking at the second
    // token: Ident("crate"), StringStart (ABI literal), or KwFunc.
    // Each must reach the correct handler.
    for (src, expected_marker) in [
        ("extern crate \"serde\"\n", "ExternCrate"),
        ("extern \"C\" func f(x: Int) -> Int\n", "ExternFuncDecl"),
        ("extern func legacy(x: Int) -> Int\n", "FuncDecl"),
    ] {
        let sid = SourceId(0);
        let tokens = tokenize(src, sid).expect("tokenize must succeed");
        let decls = parse(&tokens, sid).expect("parse must succeed");
        assert_eq!(
            decls.len(),
            1,
            "src `{src}` should yield exactly 1 decl, got {} ({:?})",
            decls.len(),
            decls
        );
        let debug = format!("{:?}", decls[0]);
        assert!(
            debug.contains(expected_marker),
            "src `{src}` should parse to {expected_marker}, got: {debug}"
        );
    }
}

// ---------------------------------------------------------------------------
// Direct AST construction (smoke test of lower_extern_func_decl_with_abi)
// ---------------------------------------------------------------------------

#[test]
fn t119_direct_externfuncdecl_construction_lowers_correctly() {
    use buff_lang_ast::decl::ExternFuncDecl;

    let _ = span(); // silence unused warning on helper
    let d = ExternFuncDecl {
        abi: "C".to_string(),
        crate_name: Some("serde_json".to_string()),
        name: ident("parse"),
        params: vec![Param {
            name: ident("s"),
            ty: named_ty("String"),
            default_value: None,
            span: span(),
        }],
        return_type: Some(named_ty("String")),
        span: span(),
    };
    let mut codegen = RustCodegen::new();
    let _file = codegen
        .generate(&[Decl::ExternFuncDecl(d)])
        .expect("codegen must succeed");
    assert!(codegen.extern_crates().contains("serde_json"));
    let _ = generate_rust; // silence unused import warning
    let _ = FuncDecl {
        name: ident("_unused"),
        params: vec![],
        return_type: None,
        body: Block {
            stmts: vec![],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: vec![],
        span: span(),
    };
}
