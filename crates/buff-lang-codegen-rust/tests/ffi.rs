//! T32 integration tests — FFI basics: `extern crate` + `extern func`.
//!
//! Coverage:
//!
//! - **`extern crate "<name>"`** → the codegen records the name in its
//!   extern-crate dep set (`RustCodegen::extern_crates()`) AND emits a
//!   `use <name>;` item at the top of the generated source. The recorded
//!   set is the codegen-level surrogate for "the generated Cargo.toml
//!   must contain `<name> = ...`" — full CLI→Cargo.toml wiring is deferred
//!   until the pipeline switches from single-file `rustc` invocation to a
//!   Cargo-project model (see decisions.md T32).
//! - **`extern func name(params) -> Ret`** → a foreign-function
//!   declaration (NO body) lowers to a Rust
//!   `extern "C" { fn name(params) -> Ret; }` foreign-mod item.
//! - **Type mapping** — `buff_primitive_to_rust_name` (the T32
//!   configurable table) covers all 9 primitive names (Int→i64, Byte→u8,
//!   Bits→u64, Float→f32, Double→f64, Bool→bool, String→String, Char→char,
//!   Decimal→rust_decimal::Decimal). The 4 generic containers
//!   (Vector→Vec, Option→Option, Matrix→Matrix, Map→HashMap, Result→Result)
//!   are tested via `ast_typeref_to_syn` since they carry type arguments.
//! - **Determinism** — multiple `extern crate` declarations record names
//!   in a `BTreeSet` so iteration order is stable across runs.
//! - **End-to-end** — `extern crate "serde"` + `extern func` round-trip
//!   through lexer → parser → codegen and re-parse as valid Rust.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test ffi
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{ExternCrateDecl, FuncDecl};
use buff_lang_ast::ty::TypeRef;
use buff_lang_ast::Decl;
use buff_lang_codegen_rust::{buff_primitive_to_rust_name, generate_rust, RustCodegen};
use buff_lang_error::Span;
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

fn empty_body_func(name: &str, params: Vec<Param>, ret: Option<TypeRef>, is_extern: bool) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params,
        return_type: ret,
        body: Block {
            stmts: Vec::new(),
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    })
}

fn extern_crate_decl(name: &str) -> Decl {
    Decl::ExternCrateDecl(ExternCrateDecl {
        name: name.to_string(),
        span: span(),
    })
}

fn int_param(name: &str) -> Param {
    Param {
        name: ident(name),
        ty: named_ty("Int"),
        default_value: None,
        is_comptime: false,
        span: span(),
    }
}

/// Assert the generated source re-parses as a valid Rust file.
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// extern crate "<name>" — dep-set recording + `use` emission
// ---------------------------------------------------------------------------

#[test]
fn extern_crate_records_name_in_extern_crates_set() {
    // `extern crate "serde"` → the codegen must record "serde" in its
    // extern-crates dep set. This is the codegen-level surrogate for
    // "the generated Cargo.toml contains `serde = ...`" (full CLI→Cargo.toml
    // wiring is deferred — see decisions.md T32).
    let mut codegen = RustCodegen::new();
    let file = codegen
        .generate(&[extern_crate_decl("serde")])
        .expect("codegen must succeed");
    let deps = codegen.extern_crates();
    assert!(
        deps.contains("serde"),
        "expected `serde` in extern_crates dep set, got {:?}",
        deps
    );
    // The generated File is non-empty (the `use serde;` item).
    assert!(
        !file.items.is_empty(),
        "expected at least one item (the `use` declaration)"
    );
}

#[test]
fn extern_crate_emits_use_item_in_generated_source() {
    // `extern crate "serde"` → generated Rust contains `use serde;`.
    let src = generate_rust(&[extern_crate_decl("serde")]).expect("codegen must succeed");
    assert!(
        src.contains("use serde;"),
        "expected `use serde;` in generated Rust: {src}"
    );
    must_reparse(&src);
}

#[test]
fn extern_crate_snapshot() {
    let src = generate_rust(&[extern_crate_decl("serde")]).expect("codegen must succeed");
    insta::assert_snapshot!(src, @r###"
    use serde;
    "###);
}

#[test]
fn multiple_extern_crates_recorded_in_deterministic_btree_order() {
    // Multiple `extern crate` declarations must record names in a BTreeSet
    // so iteration order is DETERMINISTIC across runs (the T29 flaky-test
    // lesson — never rely on HashSet iteration order for codegen output).
    // Deliberately feed them in NON-alphabetical order to verify the set
    // sorts them.
    let decls = vec![
        extern_crate_decl("zlib"),
        extern_crate_decl("serde"),
        extern_crate_decl("rayon"),
    ];
    let mut codegen = RustCodegen::new();
    let _file = codegen.generate(&decls).expect("codegen must succeed");
    let deps: Vec<&String> = codegen.extern_crates().iter().collect();
    assert_eq!(
        deps.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        vec!["rayon", "serde", "zlib"],
        "extern_crates must be sorted (BTreeSet) for deterministic output, got {:?}",
        deps
    );
}

#[test]
fn extern_crate_normalises_hyphen_to_underscore_in_use() {
    // Crate names may contain `-` (e.g. `rust-decimal`) which is NOT a
    // valid Rust identifier — crates.io normalises `-` to `_`. The
    // generated `use` must do the same.
    let src = generate_rust(&[extern_crate_decl("rust-decimal")]).expect("codegen must succeed");
    assert!(
        src.contains("use rust_decimal;"),
        "expected `use rust_decimal;` (hyphen→underscore) in generated Rust: {src}"
    );
    must_reparse(&src);
}

#[test]
fn duplicate_extern_crates_dedup_in_dep_set() {
    // Two `extern crate "serde"` declarations → the dep set contains
    // `serde` exactly once (BTreeSet dedup).
    let decls = vec![extern_crate_decl("serde"), extern_crate_decl("serde")];
    let mut codegen = RustCodegen::new();
    let _file = codegen.generate(&decls).expect("codegen must succeed");
    assert_eq!(
        codegen.extern_crates().len(),
        1,
        "duplicate extern crate must dedup in BTreeSet, got {:?}",
        codegen.extern_crates()
    );
}

// ---------------------------------------------------------------------------
// extern func — foreign-mod lowering
// ---------------------------------------------------------------------------

#[test]
fn extern_func_lowers_to_rust_foreign_mod() {
    // `extern func rust_fn(x: Int) -> Int` → Rust
    // `extern "C" { fn rust_fn(x: i64) -> i64; }`.
    let f = empty_body_func("rust_fn", vec![int_param("x")], Some(named_ty("Int")), true);
    let src = generate_rust(&[f]).expect("codegen must succeed");
    assert!(
        src.contains("extern \"C\""),
        "expected `extern \"C\"` ABI marker in generated Rust: {src}"
    );
    assert!(
        src.contains("fn rust_fn(x: i64) -> i64"),
        "expected `fn rust_fn(x: i64) -> i64` signature in generated Rust: {src}"
    );
    assert!(
        !src.contains('{') || src.matches('{').count() >= 1,
        "extern block should be braced"
    );
    must_reparse(&src);
}

#[test]
fn extern_func_has_no_body_in_generated_source() {
    // An extern-func declaration has NO body. The generated source must
    // contain the signature terminated by `;` (foreign-fn syntax), NOT a
    // brace-delimited body.
    let f = empty_body_func("no_body", vec![int_param("a")], None, true);
    let src = generate_rust(&[f]).expect("codegen must succeed");
    assert!(
        src.contains("fn no_body(a: i64);"),
        "expected bodyless foreign-fn signature `fn no_body(a: i64);` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn extern_func_void_return_uses_default_return_type() {
    // `extern func f(a: Int)` (no return type) → `fn f(a: i64);` (no `->`)
    let f = empty_body_func("void_extern", vec![int_param("a")], None, true);
    let src = generate_rust(&[f]).expect("codegen must succeed");
    assert!(
        src.contains("fn void_extern(a: i64);"),
        "expected `fn void_extern(a: i64);` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn extern_func_snapshot() {
    let f = empty_body_func("rust_fn", vec![int_param("x")], Some(named_ty("Int")), true);
    let src = generate_rust(&[f]).expect("codegen must succeed");
    insta::assert_snapshot!(src, @r###"
    extern "C" {
        fn rust_fn(x: i64) -> i64;
    }
    "###);
}

#[test]
fn extern_func_string_param_type_maps_correctly() {
    // Verify the type mapping applies inside extern-fn signatures:
    // `extern func takes_str(s: String) -> String`.
    let f = empty_body_func(
        "takes_str",
        vec![Param {
            name: ident("s"),
            ty: named_ty("String"),
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        Some(named_ty("String")),
        true,
    );
    let src = generate_rust(&[f]).expect("codegen must succeed");
    assert!(
        src.contains("fn takes_str(s: String) -> String"),
        "expected String→String mapping in extern fn signature: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Type mapping table (T32 REFACTOR) — all primitive names
// ---------------------------------------------------------------------------

#[test]
fn buff_primitive_to_rust_name_maps_all_nine_primitives() {
    // The T32 configurable type-mapping table covers all 9 Buff primitive
    // names. This is the single source of truth consulted by
    // `ast_typeref_to_syn`.
    assert_eq!(buff_primitive_to_rust_name("Int"), "i64");
    assert_eq!(buff_primitive_to_rust_name("Byte"), "u8");
    assert_eq!(buff_primitive_to_rust_name("Bits"), "u64");
    assert_eq!(buff_primitive_to_rust_name("Float"), "f32");
    assert_eq!(buff_primitive_to_rust_name("Double"), "f64");
    assert_eq!(buff_primitive_to_rust_name("Bool"), "bool");
    assert_eq!(buff_primitive_to_rust_name("String"), "String");
    assert_eq!(buff_primitive_to_rust_name("Char"), "char");
    assert_eq!(
        buff_primitive_to_rust_name("Decimal"),
        "rust_decimal::Decimal"
    );
}

#[test]
fn buff_primitive_to_rust_name_passes_through_unknown_names() {
    // Unknown names (user-defined types, generic type params like `T`)
    // pass through unchanged so struct/enum names keep their spelling.
    assert_eq!(buff_primitive_to_rust_name("MyStruct"), "MyStruct");
    assert_eq!(buff_primitive_to_rust_name("T"), "T");
    assert_eq!(buff_primitive_to_rust_name("Result"), "Result");
}

// ---------------------------------------------------------------------------
// Generic-container type mapping (the other 4 of the "13 types")
// ---------------------------------------------------------------------------

#[test]
fn ast_typeref_generic_containers_map_inner_types_correctly() {
    // Verify the generic-container mappings via a generated extern-fn
    // signature (which routes types through `ast_typeref_to_syn`).
    //
    // NOTE: the unresolved `TypeRef::Generic` path passes the BASE name
    // through verbatim (so user-written `Vector<T>` → `Vector<T>`), and
    // only the inner type ARGS go through the primitive mapping table.
    // The `Vector`→`Vec` / `Map`→`HashMap` spelling rewrite happens on
    // the RESOLVED `Type` path (`buff_type_to_syn`, exercised by the
    // `let`-binding inference tests in vector_codegen.rs / map_codegen.rs).
    // Here we verify the inner-arg mapping is applied inside generics.
    let vec_int = TypeRef::Generic {
        base: Box::new(named_ty("Vector")),
        args: vec![named_ty("Int")],
        span: span(),
    };
    let opt_int = TypeRef::Option(Box::new(named_ty("Int")), span());
    let map_str_int = TypeRef::Generic {
        base: Box::new(named_ty("Map")),
        args: vec![named_ty("String"), named_ty("Int")],
        span: span(),
    };
    // Each type goes through its own extern-fn. Base names pass through;
    // inner args are mapped (Int→i64, String→String).
    for (ty, expected_substr) in [
        (vec_int, "Vector<i64>"),
        (opt_int, "Option<i64>"),
        (map_str_int, "Map<String, i64>"),
    ] {
        let f = empty_body_func(
            "f",
            vec![Param {
                name: ident("x"),
                ty,
                default_value: None,
                is_comptime: false,
                span: span(),
            }],
            None,
            true,
        );
        let src = generate_rust(&[f]).expect("codegen must succeed");
        assert!(
            src.contains(expected_substr),
            "expected `{expected_substr}` in generated extern-fn signature: {src}"
        );
        must_reparse(&src);
    }
}

// ---------------------------------------------------------------------------
// End-to-end: lexer → parser → codegen
// ---------------------------------------------------------------------------

#[test]
fn end_to_end_extern_crate_and_func_round_trip() {
    // A small Buff program exercising both `extern crate` and `extern func`
    // round-trips through lexer → parser → codegen and produces valid Rust.
    let src = "extern crate \"serde\"\n\nextern func add(x: Int, y: Int) -> Int\n";
    let sid = buff_lang_error::SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");

    // Two top-level decls: one ExternCrateDecl + one (extern) FuncDecl.
    assert_eq!(decls.len(), 2, "expected 2 decls, got {:#?}", decls);
    assert!(
        matches!(decls[0], Decl::ExternCrateDecl(ref d) if d.name == "serde"),
        "first decl must be ExternCrateDecl(serde), got {:?}",
        decls[0]
    );
    match &decls[1] {
        Decl::FuncDecl(f) => {
            assert_eq!(f.name.name, "add");
            assert!(f.is_extern, "extern func must set is_extern");
            assert_eq!(f.params.len(), 2);
            // Extern funcs have an empty placeholder body (no real body).
            assert!(f.body.stmts.is_empty());
        }
        other => panic!("expected FuncDecl for second decl, got {other:?}"),
    }

    let rust = generate_rust(&decls).expect("codegen must succeed");
    assert!(
        rust.contains("use serde;"),
        "expected `use serde;` in generated Rust: {rust}"
    );
    assert!(
        rust.contains("extern \"C\""),
        "expected `extern \"C\"` block in generated Rust: {rust}"
    );
    assert!(
        rust.contains("fn add(x: i64, y: i64) -> i64;"),
        "expected extern fn signature in generated Rust: {rust}"
    );
    must_reparse(&rust);
}

#[test]
fn parser_rejects_extern_without_crate_or_func() {
    // `extern <anything-else>` is a parse error.
    let src = "extern let x = 5\n";
    let sid = buff_lang_error::SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let err = parse(&tokens, sid).expect_err("must error on `extern let`");
    assert!(
        err.diagnostic.message.contains("expected `extern crate"),
        "expected helpful error mentioning `extern crate` or `extern func`, got: {}",
        err.diagnostic.message
    );
}

#[test]
fn parser_rejects_extern_crate_without_string() {
    // `extern crate` with no string literal is a parse error.
    let src = "extern crate\n";
    let sid = buff_lang_error::SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let err = parse(&tokens, sid).expect_err("must error on missing crate name");
    assert!(
        err.diagnostic.message.contains("crate-name string"),
        "expected error about missing crate-name string, got: {}",
        err.diagnostic.message
    );
}
