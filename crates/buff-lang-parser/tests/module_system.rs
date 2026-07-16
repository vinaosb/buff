//! T29 integration tests — module system: import / export / re-export
//! parsing, visibility, path resolution, and circular-import detection.
//!
//! Coverage:
//!
//! Parser (top-level decl shape):
//! - `import { a, b } from "./path"` parses to `Decl::ImportDecl` with
//!   `from_path = Some("./path")`, `imports = ["a","b"]`, `wildcard = false`.
//! - `import * from "./path"` parses to `wildcard = true`, empty imports.
//! - `import name from "./path"` parses to default-import shape.
//! - `export func name() { }` wraps a FuncDecl in `Decl::ExportDecl`.
//! - `export enum Name { ... }` wraps an EnumDecl in `Decl::ExportDecl`.
//! - `export * from "./path"` produces `Decl::ReexportDecl` (wildcard).
//! - `export { a, b } from "./path"` produces `Decl::ReexportDecl` (named).
//! - Error paths: missing `from`, missing path string, missing `}`.
//!
//! Module graph (buff-lang-types::modules):
//! - Multi-file: `main` imports `./hello`; graph resolves the file, parses
//!   it, and exposes the imported `greet` symbol.
//! - Circular: A imports B, B imports A → graph build fails with
//!   `"circular import detected"`.
//! - Visibility: importing a non-exported symbol → error.
//! - `export * from` re-exports flatten target's exports into re-exporter.
//! - Path resolution: `./utils/math.buff` resolves relative to importing
//!   file's directory.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser --test module_system
//! cargo test -p buff-lang-types --test module_system
//! ```

#![allow(clippy::approx_constant)]

use buff_lang_ast::{Decl, ImportDecl, ReexportDecl};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse `src` as a top-level program.
fn parse_program(src: &str) -> Vec<Decl> {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect("parser must succeed")
}

/// Tokenize + parse `src` as a top-level program, expecting FAILURE.
fn parse_program_err(src: &str) -> buff_lang_error::ParseError {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect_err("parser must fail")
}

// ---------------------------------------------------------------------------
// Parser: import declarations.
// ---------------------------------------------------------------------------

#[test]
fn module_system_import_named_from_path_parses() {
    let decls = parse_program(r#"import { greet, farewell } from "./hello.buff""#);
    assert_eq!(decls.len(), 1, "expected one decl");
    match &decls[0] {
        Decl::ImportDecl(ImportDecl {
            from_path,
            imports,
            wildcard,
            path,
            alias,
            ..
        }) => {
            assert_eq!(from_path.as_deref(), Some("./hello.buff"));
            assert!(!*wildcard);
            assert!(path.is_empty(), "ES6 form leaves path empty");
            assert!(alias.is_none());
            let names: Vec<&str> = imports.iter().map(|i| i.name.as_str()).collect();
            assert_eq!(names, vec!["greet", "farewell"]);
        }
        other => panic!("expected ImportDecl, got {other:?}"),
    }
}

#[test]
fn module_system_import_wildcard_parses() {
    let decls = parse_program(r#"import * from "./utils.buff""#);
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::ImportDecl(ImportDecl {
            wildcard,
            from_path,
            imports,
            ..
        }) => {
            assert!(*wildcard);
            assert_eq!(from_path.as_deref(), Some("./utils.buff"));
            assert!(imports.is_empty(), "wildcard leaves imports empty");
        }
        other => panic!("expected ImportDecl, got {other:?}"),
    }
}

#[test]
fn module_system_import_default_name_parses() {
    let decls = parse_program(r#"import greet from "./hello.buff""#);
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::ImportDecl(ImportDecl {
            imports,
            from_path,
            wildcard,
            ..
        }) => {
            assert_eq!(from_path.as_deref(), Some("./hello.buff"));
            assert!(!*wildcard);
            assert_eq!(imports.len(), 1);
            assert_eq!(imports[0].name, "greet");
        }
        other => panic!("expected ImportDecl, got {other:?}"),
    }
}

#[test]
fn module_system_import_single_named_in_braces() {
    let decls = parse_program(r#"import { greet } from "./hello.buff""#);
    assert_eq!(decls.len(), 1);
    if let Decl::ImportDecl(imp) = &decls[0] {
        assert_eq!(imp.imports.len(), 1);
        assert_eq!(imp.imports[0].name, "greet");
    } else {
        panic!("expected ImportDecl");
    }
}

#[test]
fn module_system_import_empty_braces_parses() {
    // `{}` is allowed (rare but valid syntactically).
    let decls = parse_program(r#"import { } from "./hello.buff""#);
    assert_eq!(decls.len(), 1);
    if let Decl::ImportDecl(imp) = &decls[0] {
        assert!(imp.imports.is_empty());
        assert!(!imp.wildcard);
    }
}

#[test]
fn module_system_import_path_no_extension_parses() {
    let decls = parse_program(r#"import { greet } from "./hello""#);
    assert_eq!(decls.len(), 1);
    if let Decl::ImportDecl(imp) = &decls[0] {
        assert_eq!(imp.from_path.as_deref(), Some("./hello"));
    }
}

#[test]
fn module_system_import_missing_from_errors() {
    let err = parse_program_err(r#"import { greet } "./hello.buff""#);
    assert!(
        err.diagnostic.message.contains("from"),
        "error should mention `from`: {}",
        err.diagnostic.message
    );
}

#[test]
fn module_system_import_missing_path_string_errors() {
    let err = parse_program_err(r#"import { greet } from"#);
    assert!(
        err.diagnostic.message.contains("path") || err.diagnostic.message.contains("string"),
        "error should mention path string: {}",
        err.diagnostic.message
    );
}

#[test]
fn module_system_import_missing_close_brace_errors() {
    let err = parse_program_err(r#"import { greet, farewell from "./h.buff""#);
    assert!(
        err.diagnostic.message.contains("}") || err.diagnostic.message.contains("brace"),
        "error should mention closing brace: {}",
        err.diagnostic.message
    );
}

#[test]
fn module_system_import_trailing_comma_in_braces() {
    let decls = parse_program(r#"import { greet, } from "./hello.buff""#);
    assert_eq!(decls.len(), 1);
    if let Decl::ImportDecl(imp) = &decls[0] {
        assert_eq!(imp.imports.len(), 1);
    }
}

// ---------------------------------------------------------------------------
// Parser: export declarations.
// ---------------------------------------------------------------------------

#[test]
fn module_system_export_func_wraps_in_export_decl() {
    let decls = parse_program("export func public() { return 42 }");
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::ExportDecl(e) => match &*e.inner {
            Decl::FuncDecl(f) => {
                assert_eq!(f.name.name, "public");
            }
            other => panic!("expected inner FuncDecl, got {other:?}"),
        },
        other => panic!("expected ExportDecl, got {other:?}"),
    }
}

#[test]
fn module_system_export_enum_wraps_in_export_decl() {
    let decls = parse_program("export enum Color { Red, Green, Blue }");
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::ExportDecl(e) => match &*e.inner {
            Decl::EnumDecl(en) => {
                assert_eq!(en.name.name, "Color");
                assert_eq!(en.variants.len(), 3);
            }
            other => panic!("expected inner EnumDecl, got {other:?}"),
        },
        other => panic!("expected ExportDecl, got {other:?}"),
    }
}

#[test]
fn module_system_export_wildcard_reexport_parses() {
    let decls = parse_program(r#"export * from "./other.buff""#);
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::ReexportDecl(ReexportDecl {
            wildcard,
            from,
            names,
            ..
        }) => {
            assert!(*wildcard);
            assert_eq!(from, "./other.buff");
            assert!(names.is_empty());
        }
        other => panic!("expected ReexportDecl, got {other:?}"),
    }
}

#[test]
fn module_system_export_named_reexport_parses() {
    let decls = parse_program(r#"export { greet, farewell } from "./other.buff""#);
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::ReexportDecl(ReexportDecl {
            wildcard,
            from,
            names,
            ..
        }) => {
            assert!(!*wildcard);
            assert_eq!(from, "./other.buff");
            let collected: Vec<&str> = names.iter().map(|n| n.name.as_str()).collect();
            assert_eq!(collected, vec!["greet", "farewell"]);
        }
        other => panic!("expected ReexportDecl, got {other:?}"),
    }
}

#[test]
fn module_system_export_local_only_no_from() {
    // `export { name }` without `from` — re-exports a local symbol.
    let decls = parse_program("export { helper }");
    assert_eq!(decls.len(), 1);
    if let Decl::ReexportDecl(r) = &decls[0] {
        assert!(r.from.is_empty());
        assert_eq!(r.names.len(), 1);
        assert_eq!(r.names[0].name, "helper");
    } else {
        panic!("expected ReexportDecl");
    }
}

#[test]
fn module_system_export_invalid_form_errors() {
    // `export import ...` is not allowed.
    let err = parse_program_err(r#"export import { x } from "./y.buff""#);
    assert!(
        err.diagnostic.message.contains("export"),
        "error should explain allowed export forms: {}",
        err.diagnostic.message
    );
}

// ---------------------------------------------------------------------------
// Parser: mixed top-level programs.
// ---------------------------------------------------------------------------

#[test]
fn module_system_mixed_imports_and_funcs() {
    let decls = parse_program(
        r#"
        import { greet } from "./hello.buff"
        import { helper } from "./utils.buff"
        export func main() { return 0 }
        func private_helper() { return 1 }
    "#,
    );
    assert_eq!(decls.len(), 4);
    assert!(matches!(decls[0], Decl::ImportDecl(_)));
    assert!(matches!(decls[1], Decl::ImportDecl(_)));
    assert!(matches!(decls[2], Decl::ExportDecl(_)));
    assert!(matches!(decls[3], Decl::FuncDecl(_)));
}

#[test]
fn module_system_reexport_followed_by_func() {
    let decls = parse_program(
        r#"
        export * from "./common.buff"
        export func main() { return 0 }
    "#,
    );
    assert_eq!(decls.len(), 2);
    assert!(matches!(decls[0], Decl::ReexportDecl(_)));
    assert!(matches!(decls[1], Decl::ExportDecl(_)));
}

// ---------------------------------------------------------------------------
// Parser: regression — pre-T29 syntax still parses.
// ---------------------------------------------------------------------------

#[test]
fn module_system_legacy_func_still_parses() {
    let decls = parse_program("func foo() { return 1 }");
    assert_eq!(decls.len(), 1);
    assert!(matches!(decls[0], Decl::FuncDecl(_)));
}

#[test]
fn module_system_legacy_enum_still_parses() {
    let decls = parse_program("enum Color { Red, Green, Blue }");
    assert_eq!(decls.len(), 1);
    assert!(matches!(decls[0], Decl::EnumDecl(_)));
}

// ---------------------------------------------------------------------------
// Module-graph tests (these exercise buff-lang-types::modules via the
// parser-anchored in-memory loader). Kept in this file per T29 acceptance.
// ---------------------------------------------------------------------------

use buff_lang_types::{build_graph, MemoryLoader};
use std::path::PathBuf;

fn loader_with(files: &[(&str, &str)]) -> MemoryLoader {
    let mut l = MemoryLoader::new();
    for (p, src) in files {
        l.insert(PathBuf::from(p), src);
    }
    l
}

#[test]
fn module_system_graph_resolves_simple_import() {
    // main imports hello; hello exports greet.
    let loader = loader_with(&[
        (
            "/main.buff",
            r#"import { greet } from "./hello.buff"

               func main() { return 0 }"#,
        ),
        ("/hello.buff", "export func greet() { return 1 }"),
    ]);
    let graph = build_graph(&PathBuf::from("/main.buff"), &loader).expect("graph builds");
    let hello = graph
        .get(&PathBuf::from("/hello.buff"))
        .expect("hello present");
    assert!(
        hello.exports.contains("greet"),
        "greet should be exported: {:?}",
        hello.exports
    );
    assert_eq!(graph.modules.len(), 2);
}

#[test]
fn module_system_graph_detects_circular_import() {
    let loader = loader_with(&[
        (
            "/a.buff",
            r#"import { b } from "./b.buff"

               export func a() { return 0 }"#,
        ),
        (
            "/b.buff",
            r#"import { a } from "./a.buff"

               export func b() { return 1 }"#,
        ),
    ]);
    let err = build_graph(&PathBuf::from("/a.buff"), &loader).expect_err("cycle must error");
    assert!(
        err.diagnostic.message.contains("circular import detected"),
        "error must mention circular import: {}",
        err.diagnostic.message
    );
    assert!(
        err.diagnostic.message.contains("a.buff") && err.diagnostic.message.contains("b.buff"),
        "chain should mention both files: {}",
        err.diagnostic.message
    );
}

#[test]
fn module_system_graph_rejects_private_symbol_import() {
    let loader = loader_with(&[
        (
            "/main.buff",
            r#"import { helper } from "./lib.buff"

               func main() { return 0 }"#,
        ),
        // lib has `helper` but it's NOT exported (module-private).
        ("/lib.buff", "func helper() { return 1 }"),
    ]);
    let err = build_graph(&PathBuf::from("/main.buff"), &loader).expect_err("private import");
    assert!(
        err.diagnostic.message.contains("not exported"),
        "error must mention visibility: {}",
        err.diagnostic.message
    );
    assert!(
        err.diagnostic.message.contains("helper"),
        "error must name the symbol: {}",
        err.diagnostic.message
    );
}

#[test]
fn module_system_graph_export_star_flattens() {
    let loader = loader_with(&[
        (
            "/main.buff",
            r#"import { greet } from "./reexporter.buff"

               func main() { return 0 }"#,
        ),
        ("/reexporter.buff", r#"export * from "./orig.buff""#),
        ("/orig.buff", "export func greet() { return 1 }"),
    ]);
    let graph = build_graph(&PathBuf::from("/main.buff"), &loader).expect("graph builds");
    let rex = graph
        .get(&PathBuf::from("/reexporter.buff"))
        .expect("reexporter present");
    assert!(
        rex.exports.contains("greet"),
        "export * should flatten greet into reexporter: {:?}",
        rex.exports
    );
}

#[test]
fn module_system_graph_path_resolution_subdir() {
    let loader = loader_with(&[
        (
            "/src/main.buff",
            r#"import { helper } from "./utils/math.buff"

               func main() { return 0 }"#,
        ),
        ("/src/utils/math.buff", "export func helper() { return 1 }"),
    ]);
    let graph = build_graph(&PathBuf::from("/src/main.buff"), &loader).expect("graph builds");
    assert!(
        graph.get(&PathBuf::from("/src/utils/math.buff")).is_some(),
        "utils/math.buff should be in graph"
    );
}

#[test]
fn module_system_graph_missing_file_errors() {
    let loader = loader_with(&[(
        "/main.buff",
        r#"import { x } from "./missing.buff"

           func main() { return 0 }"#,
    )]);
    let err = build_graph(&PathBuf::from("/main.buff"), &loader).expect_err("missing file");
    assert!(
        err.diagnostic.message.contains("not found") || err.diagnostic.message.contains("missing"),
        "error must mention file not found: {}",
        err.diagnostic.message
    );
}

#[test]
fn module_system_graph_topo_order_deps_first() {
    // Topological order should list dependencies before importers so a
    // single-pass codegen walk sees each symbol defined before use.
    let mut loader = MemoryLoader::new();
    loader.insert(
        PathBuf::from("/main.buff"),
        "import { greet } from \"./hello.buff\"\n\nfunc main() { return 0 }",
    );
    loader.insert(
        PathBuf::from("/hello.buff"),
        "export func greet() { return 1 }",
    );
    let graph = build_graph(&PathBuf::from("/main.buff"), &loader).expect("graph builds");
    let pos_main = graph
        .topo_order
        .iter()
        .position(|p| p == &PathBuf::from("/main.buff"))
        .expect("main in topo order");
    let pos_hello = graph
        .topo_order
        .iter()
        .position(|p| p == &PathBuf::from("/hello.buff"))
        .expect("hello in topo order");
    assert!(
        pos_hello < pos_main,
        "hello should come before main in topo order: {:?}",
        graph.topo_order
    );
}
