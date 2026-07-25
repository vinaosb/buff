//! Integration tests for the T8 multi-crate emission mode.
//!
//! These exercise the [`buff_lang_codegen_rust::generate_multi_crate`]
//! entry point with hand-built AST nodes (mirroring the
//! `tests/pipeline.rs` pattern). They assert the generated multi-file
//! Rust output has the expected `mod X;` + `use X::*;` wiring and that
//! each module file contains only its own decls (no leakage).
//!
//! End-to-end `buff run examples/modules/main.buff` execution is
//! verified separately via the CLI integration tests (which require
//! rustc + a working toolchain — these tests do NOT).

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::{FuncDecl, ImportDecl};
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_codegen_rust::{
    generate_multi_crate, module_ident_from_path, uses_multi_crate, ParsedModule,
};
use buff_lang_error::Span;

// ---------------------------------------------------------------------------
// Helpers — hand-built AST.
// ---------------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn call_expr(callee: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(callee)),
        args,
        span: span(),
    }
}

/// Build `func name(params) -> Ret { body }` as a [`Decl::FuncDecl`].
fn func_decl(name: &str, body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: Vec::new(),
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

/// Build `print(arg)` as a statement.
fn print_stmt(arg: &str) -> Stmt {
    Stmt::ExprStmt(call_expr("print", vec![string_expr(arg)]), span())
}

/// Build an ES6-form `import { names... } from "./path.buff"`.
fn es6_import(from: &str, names: &[&str]) -> Decl {
    Decl::ImportDecl(ImportDecl {
        path: Vec::new(),
        imports: names.iter().map(|n| ident(n)).collect(),
        alias: None,
        from_path: Some(from.to_string()),
        wildcard: false,
        span: span(),
    })
}

// ---------------------------------------------------------------------------
// Predicate tests.
// ---------------------------------------------------------------------------

#[test]
fn predicate_false_for_no_imports() {
    let decls: Vec<Decl> = vec![func_decl("main", vec![print_stmt("hi")])];
    assert!(!uses_multi_crate(&decls));
}

#[test]
fn predicate_true_for_es6_import() {
    let decls = vec![
        es6_import("./greet.buff", &["greet"]),
        func_decl("main", vec![]),
    ];
    assert!(uses_multi_crate(&decls));
}

// ---------------------------------------------------------------------------
// module_ident_from_path tests.
// ---------------------------------------------------------------------------

#[test]
fn ident_simple_path() {
    assert_eq!(module_ident_from_path("./greet.buff"), "greet");
}

#[test]
fn ident_strips_extra_dot_prefix() {
    assert_eq!(module_ident_from_path("../sibling.buff"), "sibling");
    assert_eq!(module_ident_from_path("./a/b/c.buff"), "c");
}

#[test]
fn ident_handles_special_chars() {
    assert_eq!(module_ident_from_path("./my-mod.buff"), "my_mod");
    assert_eq!(module_ident_from_path("./123.buff"), "m123");
    assert_eq!(module_ident_from_path("./greet.v2.buff"), "greet_v2");
}

// ---------------------------------------------------------------------------
// generate_multi_crate end-to-end tests.
// ---------------------------------------------------------------------------

#[test]
fn two_module_program_emits_mod_and_use() {
    // main.buff: import { greet } from "./greet.buff"; func main() { print(greet("Buff")) }
    // greet.buff: export func greet(name) { return name }
    let root_decls = vec![
        es6_import("./greet.buff", &["greet"]),
        func_decl("main", vec![print_stmt("Buff")]),
    ];
    let greet_decls = vec![func_decl("greet", vec![])];
    let modules = vec![ParsedModule {
        ident: "greet".to_string(),
        from_path: "./greet.buff".to_string(),
        decls: greet_decls,
    }];
    let out = generate_multi_crate(&root_decls, &modules).expect("multi-crate codegen ok");

    // Root contains `mod greet;` declaration.
    assert!(
        out.root_source.contains("mod greet"),
        "expected `mod greet;` in root; got:\n{}",
        out.root_source
    );
    // Root contains `use greet::greet;` bringing the symbol into scope.
    assert!(
        out.root_source.contains("use greet::greet"),
        "expected `use greet::greet;` in root; got:\n{}",
        out.root_source
    );
    // Root contains the user's main fn.
    assert!(
        out.root_source.contains("fn main"),
        "expected `fn main` in root; got:\n{}",
        out.root_source
    );
    // Root does NOT contain the greet fn body (it's in greet.rs).
    assert!(
        !out.root_source.contains("fn greet"),
        "greet fn body should NOT be in root; got:\n{}",
        out.root_source
    );

    // The greet module is emitted as a sibling .rs file containing only
    // its own decls.
    let greet_src = out.modules.get("greet").expect("greet module emitted");
    assert!(
        greet_src.contains("fn greet"),
        "expected `fn greet` in greet.rs; got:\n{}",
        greet_src
    );
    assert!(
        !greet_src.contains("mod greet"),
        "greet.rs should NOT contain `mod greet;` (no self-ref); got:\n{}",
        greet_src
    );
}

#[test]
fn wildcard_import_emits_glob_use() {
    let root_decls = vec![
        Decl::ImportDecl(ImportDecl {
            path: Vec::new(),
            imports: Vec::new(),
            alias: None,
            from_path: Some("./utils.buff".to_string()),
            wildcard: true,
            span: span(),
        }),
        func_decl("main", vec![]),
    ];
    let modules = vec![ParsedModule {
        ident: "utils".to_string(),
        from_path: "./utils.buff".to_string(),
        decls: vec![func_decl("helper", vec![])],
    }];
    let out = generate_multi_crate(&root_decls, &modules).expect("multi-crate codegen ok");
    assert!(
        out.root_source.contains("use utils::*"),
        "expected `use utils::*;` for wildcard import; got:\n{}",
        out.root_source
    );
}

#[test]
fn multiple_named_imports_emit_brace_group() {
    let root_decls = vec![
        es6_import("./utils.buff", &["foo", "bar", "baz"]),
        func_decl("main", vec![]),
    ];
    let modules = vec![ParsedModule {
        ident: "utils".to_string(),
        from_path: "./utils.buff".to_string(),
        decls: vec![func_decl("foo", vec![])],
    }];
    let out = generate_multi_crate(&root_decls, &modules).expect("multi-crate codegen ok");
    // The brace-group form `use utils::{foo, bar, baz};` should appear.
    assert!(
        out.root_source.contains("use utils::{") && out.root_source.contains("foo"),
        "expected `use utils::{{foo, bar, baz}};` brace-group; got:\n{}",
        out.root_source
    );
    assert!(
        out.root_source.contains("bar") && out.root_source.contains("baz"),
        "expected all three names in brace group; got:\n{}",
        out.root_source
    );
}

#[test]
fn missing_module_for_import_is_error() {
    let root_decls = vec![
        es6_import("./missing.buff", &["foo"]),
        func_decl("main", vec![]),
    ];
    let result = generate_multi_crate(&root_decls, &[]);
    assert!(
        result.is_err(),
        "expected error when import has no matching parsed module"
    );
    let err_msg = result.unwrap_err().diagnostic.message;
    assert!(
        err_msg.contains("missing.buff"),
        "error should mention the missing path; got: {err_msg}"
    );
}

#[test]
fn extern_crates_aggregate_across_modules() {
    // Root declares extern chrono; module declares extern rand.
    let root_decls = vec![
        Decl::ExternCrateDecl(buff_lang_ast::ExternCrateDecl {
            name: "chrono".to_string(),
            span: span(),
        }),
        func_decl("main", vec![]),
    ];
    let modules = vec![ParsedModule {
        ident: "m1".to_string(),
        from_path: "./m1.buff".to_string(),
        decls: vec![
            Decl::ExternCrateDecl(buff_lang_ast::ExternCrateDecl {
                name: "rand".to_string(),
                span: span(),
            }),
            func_decl("helper", vec![]),
        ],
    }];
    let out = generate_multi_crate(&root_decls, &modules).expect("multi-crate ok");
    assert!(
        out.extern_crates.contains("chrono"),
        "extern_crates should contain chrono from root: {:?}",
        out.extern_crates
    );
    assert!(
        out.extern_crates.contains("rand"),
        "extern_crates should contain rand from module: {:?}",
        out.extern_crates
    );
}

#[test]
fn three_module_program_emits_all_mods() {
    // Root imports from a, b, c. All three should appear as `mod X;`.
    let root_decls = vec![
        es6_import("./a.buff", &["a_fn"]),
        es6_import("./b.buff", &["b_fn"]),
        es6_import("./c.buff", &["c_fn"]),
        func_decl("main", vec![]),
    ];
    let modules = vec![
        ParsedModule {
            ident: "a".to_string(),
            from_path: "./a.buff".to_string(),
            decls: vec![func_decl("a_fn", vec![])],
        },
        ParsedModule {
            ident: "b".to_string(),
            from_path: "./b.buff".to_string(),
            decls: vec![func_decl("b_fn", vec![])],
        },
        ParsedModule {
            ident: "c".to_string(),
            from_path: "./c.buff".to_string(),
            decls: vec![func_decl("c_fn", vec![])],
        },
    ];
    let out = generate_multi_crate(&root_decls, &modules).expect("multi-crate ok");
    // All three modules are declared.
    for name in &["a", "b", "c"] {
        assert!(
            out.root_source.contains(&format!("mod {name}")),
            "expected `mod {name};` in root; got:\n{}",
            out.root_source
        );
        assert!(
            out.modules.contains_key(*name),
            "expected module {name} in modules map"
        );
    }
    // The use items bring each symbol into scope.
    for (mod_name, fn_name) in &[("a", "a_fn"), ("b", "b_fn"), ("c", "c_fn")] {
        assert!(
            out.root_source
                .contains(&format!("use {mod_name}::{fn_name}")),
            "expected `use {mod_name}::{fn_name};`; got:\n{}",
            out.root_source
        );
    }
}

#[test]
fn single_module_no_imports_returns_module_only() {
    // Even with no imports, generate_multi_crate should still work if
    // called directly — it just produces a root with no mod items.
    let root_decls = vec![func_decl("main", vec![print_stmt("hello")])];
    let out = generate_multi_crate(&root_decls, &[]).expect("multi-crate ok");
    assert!(out.modules.is_empty());
    assert!(
        out.root_source.contains("fn main"),
        "root should contain fn main; got:\n{}",
        out.root_source
    );
    // No `mod` declarations.
    assert!(
        !out.root_source.contains("\nmod "),
        "root with no imports should have no mod items; got:\n{}",
        out.root_source
    );
}
