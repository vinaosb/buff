//! T29 integration tests for `buff_lang_types::modules`.
//!
//! These tests exercise the module-graph builder from the types crate's
//! surface — they complement the parser-level tests in
//! `crates/buff-lang-parser/tests/module_system.rs` (which run against
//! the parser directly).
//!
//! Coverage:
//!
//! - Two-module import chain (main → hello).
//! - Diamond dependency (main → {a, b} → common).
//! - Topological order is deps-first.
//! - Circular import (A → B → A) is rejected with chain message.
//! - Visibility: importing a private symbol is rejected.
//! - `export * from "..."` flattens target exports.
//! - `export { name } from "..."` requires name to be exported in target.
//! - Path resolution: relative, parent (`../`), subdir (`./x/y`).
//! - Missing file surfaces a clear error.
//! - `std/...` paths are rejected with a clear "not yet supported" message.
//! - Re-export chain (A re-exports from B re-exports from C).
//! - Module-decl-order: a forward-referenced module (declared later in the
//!   source) still resolves because graph building is order-independent.

use buff_lang_types::{build_graph, resolve_path, MemoryLoader, ModuleLoader};
use std::path::{Path, PathBuf};

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn loader_with(files: &[(&str, &str)]) -> MemoryLoader {
    let mut l = MemoryLoader::new();
    for (path, src) in files {
        l.insert(PathBuf::from(*path), src);
    }
    l
}

// ---------------------------------------------------------------------------
// Basic graph shape.
// ---------------------------------------------------------------------------

#[test]
fn types_modules_simple_two_module_chain() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { greet } from \"./hello.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/hello.buff", "export func greet() { return 1 }"),
    ]);
    let graph = build_graph(&p("/main.buff"), &loader).expect("graph builds");
    assert_eq!(graph.modules.len(), 2);
    let hello = graph.get(&p("/hello.buff")).expect("hello present");
    assert!(hello.exports.contains("greet"));
    let main = graph.get(&p("/main.buff")).expect("main present");
    assert_eq!(main.imports.len(), 1);
    assert!(main.exports.is_empty(), "main has no exports");
}

#[test]
fn types_modules_diamond_dependency() {
    // main imports from a and b; both a and b import from common.
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { fa } from \"./a.buff\"\nimport { fb } from \"./b.buff\"\n\nfunc main() { return 0 }",
        ),
        (
            "/a.buff",
            "import { shared } from \"./common.buff\"\n\nexport func fa() { return shared() }",
        ),
        (
            "/b.buff",
            "import { shared } from \"./common.buff\"\n\nexport func fb() { return shared() }",
        ),
        ("/common.buff", "export func shared() { return 1 }"),
    ]);
    let graph = build_graph(&p("/main.buff"), &loader).expect("graph builds");
    // 4 modules total; common is deduped (not loaded twice).
    assert_eq!(graph.modules.len(), 4);
    let common = graph.get(&p("/common.buff")).expect("common present");
    assert!(common.exports.contains("shared"));
}

// ---------------------------------------------------------------------------
// Topological order.
// ---------------------------------------------------------------------------

#[test]
fn types_modules_topo_order_is_deps_first() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { a } from \"./a.buff\"\nimport { b } from \"./b.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/a.buff", "export func a() { return 1 }"),
        ("/b.buff", "export func b() { return 1 }"),
    ]);
    let graph = build_graph(&p("/main.buff"), &loader).expect("graph builds");
    let pos = |name: &str| -> usize {
        graph
            .topo_order
            .iter()
            .position(|pth| pth == &PathBuf::from(name))
            .unwrap_or_else(|| panic!("{name} should be in topo order: {:?}", graph.topo_order))
    };
    let main = pos("/main.buff");
    let a = pos("/a.buff");
    let b = pos("/b.buff");
    assert!(a < main, "a before main");
    assert!(b < main, "b before main");
}

// ---------------------------------------------------------------------------
// Cycle detection.
// ---------------------------------------------------------------------------

#[test]
fn types_modules_two_module_cycle_rejected() {
    let loader = loader_with(&[
        (
            "/a.buff",
            "import { b } from \"./b.buff\"\n\nexport func a() { return b() }",
        ),
        (
            "/b.buff",
            "import { a } from \"./a.buff\"\n\nexport func b() { return a() }",
        ),
    ]);
    let err = build_graph(&p("/a.buff"), &loader).expect_err("cycle must error");
    assert!(
        err.diagnostic.message.contains("circular import detected"),
        "message was: {}",
        err.diagnostic.message
    );
    // Chain should mention both files.
    assert!(
        err.diagnostic.message.contains("a.buff") && err.diagnostic.message.contains("b.buff"),
        "chain should list both: {}",
        err.diagnostic.message
    );
}

#[test]
fn types_modules_self_cycle_rejected() {
    let loader = loader_with(&[(
        "/loopy.buff",
        "import { x } from \"./loopy.buff\"\n\nexport func x() { return 1 }",
    )]);
    let err = build_graph(&p("/loopy.buff"), &loader).expect_err("self-cycle must error");
    assert!(err.diagnostic.message.contains("circular import detected"));
}

#[test]
fn types_modules_three_module_cycle_rejected() {
    // a → b → c → a
    let loader = loader_with(&[
        (
            "/a.buff",
            "import { b } from \"./b.buff\"\n\nexport func a() { return 0 }",
        ),
        (
            "/b.buff",
            "import { c } from \"./c.buff\"\n\nexport func b() { return 0 }",
        ),
        (
            "/c.buff",
            "import { a } from \"./a.buff\"\n\nexport func c() { return 0 }",
        ),
    ]);
    let err = build_graph(&p("/a.buff"), &loader).expect_err("3-cycle must error");
    assert!(err.diagnostic.message.contains("circular import detected"));
    assert!(err.diagnostic.message.contains("a.buff"));
    assert!(err.diagnostic.message.contains("b.buff"));
    assert!(err.diagnostic.message.contains("c.buff"));
}

// ---------------------------------------------------------------------------
// Visibility.
// ---------------------------------------------------------------------------

#[test]
fn types_modules_importing_private_func_rejected() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { helper } from \"./lib.buff\"\n\nfunc main() { return 0 }",
        ),
        // lib has `helper` but it's NOT exported.
        ("/lib.buff", "func helper() { return 1 }"),
    ]);
    let err = build_graph(&p("/main.buff"), &loader).expect_err("private import rejected");
    assert!(err.diagnostic.message.contains("not exported"));
    assert!(err.diagnostic.message.contains("helper"));
}

#[test]
fn types_modules_importing_exported_func_ok() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { helper } from \"./lib.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/lib.buff", "export func helper() { return 1 }"),
    ]);
    build_graph(&p("/main.buff"), &loader).expect("public import ok");
}

#[test]
fn types_modules_importing_exported_enum_ok() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { Color } from \"./types.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/types.buff", "export enum Color { Red, Green, Blue }"),
    ]);
    let g = build_graph(&p("/main.buff"), &loader).expect("enum import ok");
    let types_mod = g.get(&p("/types.buff")).expect("types present");
    assert!(types_mod.exports.contains("Color"));
}

// ---------------------------------------------------------------------------
// Re-exports.
// ---------------------------------------------------------------------------

#[test]
fn types_modules_export_star_flattens_exports() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { greet } from \"./mid.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/mid.buff", "export * from \"./orig.buff\""),
        ("/orig.buff", "export func greet() { return 1 }"),
    ]);
    let graph = build_graph(&p("/main.buff"), &loader).expect("graph builds");
    let mid = graph.get(&p("/mid.buff")).expect("mid present");
    assert!(
        mid.exports.contains("greet"),
        "export * should flatten greet into mid: {:?}",
        mid.exports
    );
}

#[test]
fn types_modules_export_star_chain() {
    // a re-exports from b re-exports from c.
    let loader = loader_with(&[
        ("/a.buff", "export * from \"./b.buff\""),
        ("/b.buff", "export * from \"./c.buff\""),
        ("/c.buff", "export func deep() { return 1 }"),
    ]);
    let graph = build_graph(&p("/a.buff"), &loader).expect("graph builds");
    let a = graph.get(&p("/a.buff")).expect("a present");
    assert!(
        a.exports.contains("deep"),
        "chained re-export of deep: {:?}",
        a.exports
    );
}

#[test]
fn types_modules_export_star_chain_is_deterministic_across_runs() {
    // Regression guard (T29 flakiness fix): build the same a→b→c chain many
    // times and assert each iteration propagates `deep` to `a`. Before the
    // fix, resolve_reexports iterated ctx.modules.values() (HashMap order)
    // and would miss the chain whenever iteration put `a` before `b`/`c`.
    // The probability of failure per iteration was ~1/3; 50 iterations
    // makes the test extremely sensitive to the original bug.
    for i in 0..50 {
        let loader = loader_with(&[
            ("/a.buff", "export * from \"./b.buff\""),
            ("/b.buff", "export * from \"./c.buff\""),
            ("/c.buff", "export func deep() { return 1 }"),
        ]);
        let graph = build_graph(&p("/a.buff"), &loader).unwrap_or_else(|e| {
            panic!("iter {i}: graph build failed (determinism regression?): {e:?}")
        });
        let a = graph
            .get(&p("/a.buff"))
            .unwrap_or_else(|| panic!("iter {i}: a missing from graph (determinism regression?)"));
        assert!(
            a.exports.contains("deep"),
            "iter {i}: deep missing from a.exports — flaky regression: {:?}",
            a.exports
        );
    }
}

#[test]
fn types_modules_export_star_chain_length_5() {
    // Longer chain (a→b→c→d→e→export deep) — every hop must propagate.
    // This stress-tests the topo-order discipline of resolve_reexports:
    // 5 distinct modules = 5! = 120 possible HashMap iteration orders,
    // only 1 of which is dep-first. Without the fix, this test would fail
    // ~99% of the time.
    let loader = loader_with(&[
        ("/a.buff", "export * from \"./b.buff\""),
        ("/b.buff", "export * from \"./c.buff\""),
        ("/c.buff", "export * from \"./d.buff\""),
        ("/d.buff", "export * from \"./e.buff\""),
        ("/e.buff", "export func deep() { return 1 }"),
    ]);
    let graph = build_graph(&p("/a.buff"), &loader).expect("graph builds");
    let a = graph.get(&p("/a.buff")).expect("a present");
    assert!(
        a.exports.contains("deep"),
        "5-deep chained re-export of deep must reach a: {:?}",
        a.exports
    );
}

#[test]
fn types_modules_named_reexport_validates_target_exports() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { helper } from \"./mid.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/mid.buff", "export { helper } from \"./orig.buff\""),
        ("/orig.buff", "export func helper() { return 1 }"),
    ]);
    let graph = build_graph(&p("/main.buff"), &loader).expect("graph builds");
    let mid = graph.get(&p("/mid.buff")).expect("mid present");
    assert!(mid.exports.contains("helper"));
}

#[test]
fn types_modules_named_reexport_missing_target_symbol_rejected() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { helper } from \"./mid.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/mid.buff", "export { helper } from \"./orig.buff\""),
        // orig has OTHER but NOT helper.
        ("/orig.buff", "export func other() { return 1 }"),
    ]);
    let err = build_graph(&p("/main.buff"), &loader).expect_err("named reexport of missing");
    assert!(err.diagnostic.message.contains("not exported"));
    assert!(err.diagnostic.message.contains("helper"));
}

// ---------------------------------------------------------------------------
// Path resolution.
// ---------------------------------------------------------------------------

#[test]
fn types_modules_resolve_relative_subdir() {
    let loader = loader_with(&[
        (
            "/src/main.buff",
            "import { helper } from \"./utils/math.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/src/utils/math.buff", "export func helper() { return 1 }"),
    ]);
    let graph = build_graph(&p("/src/main.buff"), &loader).expect("graph builds");
    assert!(
        graph.get(&p("/src/utils/math.buff")).is_some(),
        "subdir module should be in graph"
    );
}

#[test]
fn types_modules_resolve_parent_dir() {
    let loader = loader_with(&[
        (
            "/proj/src/main.buff",
            "import { helper } from \"../lib.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/proj/lib.buff", "export func helper() { return 1 }"),
    ]);
    let graph = build_graph(&p("/proj/src/main.buff"), &loader).expect("graph builds");
    assert!(
        graph.get(&p("/proj/lib.buff")).is_some(),
        "parent-dir module should be in graph: {:?}",
        graph.modules.keys().collect::<Vec<_>>()
    );
}

#[test]
fn types_modules_resolve_auto_extension() {
    let loader = loader_with(&[
        (
            "/main.buff",
            "import { helper } from \"./lib\"\n\nfunc main() { return 0 }",
        ),
        // key has .buff; the spec omitted it.
        ("/lib.buff", "export func helper() { return 1 }"),
    ]);
    let graph = build_graph(&p("/main.buff"), &loader).expect("graph builds");
    assert!(graph.get(&p("/lib.buff")).is_some());
}

#[test]
fn types_modules_missing_file_errors() {
    let loader = loader_with(&[(
        "/main.buff",
        "import { x } from \"./missing.buff\"\n\nfunc main() { return 0 }",
    )]);
    let err = build_graph(&p("/main.buff"), &loader).expect_err("missing file");
    assert!(
        err.diagnostic.message.contains("not found") || err.diagnostic.message.contains("missing"),
        "message: {}",
        err.diagnostic.message
    );
}

#[test]
fn types_modules_std_imports_rejected() {
    let loader = loader_with(&[(
        "/main.buff",
        "import { println } from \"std/io.buff\"\n\nfunc main() { return 0 }",
    )]);
    let err = build_graph(&p("/main.buff"), &loader).expect_err("std must error");
    assert!(
        err.diagnostic.message.contains("std"),
        "message: {}",
        err.diagnostic.message
    );
    assert!(err.diagnostic.message.contains("not yet supported"));
}

// ---------------------------------------------------------------------------
// Pure resolution unit tests (no graph build needed).
// ---------------------------------------------------------------------------

#[test]
fn types_modules_resolve_path_relative_with_ext() {
    let r = resolve_path(&p("/proj/main.buff"), "./hello.buff").unwrap();
    assert!(r.ends_with("hello.buff"));
}

#[test]
fn types_modules_resolve_path_auto_append_ext() {
    let r = resolve_path(&p("/proj/main.buff"), "./hello").unwrap();
    assert!(r.ends_with("hello.buff"));
}

#[test]
fn types_modules_resolve_path_subdir_no_ext() {
    let r = resolve_path(&p("/proj/main.buff"), "./utils/math").unwrap();
    assert!(r.ends_with("math.buff"));
}

#[test]
fn types_modules_resolve_path_std_errors() {
    let err = resolve_path(&p("/proj/main.buff"), "std/io").unwrap_err();
    assert!(err.diagnostic.message.contains("not yet supported"));
}

#[test]
fn types_modules_resolve_path_empty_errors() {
    let err = resolve_path(&p("/proj/main.buff"), "  ").unwrap_err();
    assert!(err.diagnostic.message.contains("empty"));
}

// ---------------------------------------------------------------------------
// ModuleLoader implementations.
// ---------------------------------------------------------------------------

#[test]
fn types_modules_fs_loader_returns_none_for_missing() {
    let loader = buff_lang_types::FsLoader;
    let r = loader.load(Path::new("/this/path/should/not/exist/.buff"));
    assert!(r.is_none());
}

#[test]
fn types_modules_memory_loader_round_trip() {
    let mut l = MemoryLoader::new();
    l.insert(p("/a.buff"), "func a() { return 0 }");
    let loader: &dyn ModuleLoader = &l;
    assert_eq!(
        loader.load(&p("/a.buff")).as_deref(),
        Some("func a() { return 0 }")
    );
    assert!(loader.load(&p("/missing.buff")).is_none());
}

// ---------------------------------------------------------------------------
// Edge cases.
// ---------------------------------------------------------------------------

#[test]
fn types_modules_wildcard_import_skips_visibility_check() {
    // `import * from "..."` is always "visible" at graph build time —
    // individual symbols resolve at use-site (deferred). The graph builder
    // should accept this even if main never names a symbol.
    let loader = loader_with(&[
        (
            "/main.buff",
            "import * from \"./lib.buff\"\n\nfunc main() { return 0 }",
        ),
        ("/lib.buff", "export func helper() { return 1 }"),
    ]);
    build_graph(&p("/main.buff"), &loader).expect("wildcard import ok");
}

#[test]
fn types_modules_module_with_no_imports_builds() {
    let loader = loader_with(&[(
        "/solo.buff",
        "export func foo() { return 1 }\n\nfunc bar() { return 2 }",
    )]);
    let graph = build_graph(&p("/solo.buff"), &loader).expect("solo builds");
    let solo = graph.get(&p("/solo.buff")).expect("solo present");
    assert!(solo.exports.contains("foo"));
    assert!(!solo.exports.contains("bar"), "private bar is not exported");
    assert_eq!(graph.topo_order.len(), 1);
}
