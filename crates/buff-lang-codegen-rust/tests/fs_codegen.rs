//! T124j integration tests - filesystem prelude modules codegen.
//!
//! Verifies that the Rust codegen lowers the three T124j filesystem
//! modules:
//!
//! - **Path** value type (`Path.join(a, b, ...) -> Path`, instance
//!   methods `.parent() -> Option<Path>`, `.extension() ->
//!   Option<String>`, `.basename() -> String`, `.exists() -> Bool`)
//!   - wraps `std::path::PathBuf` + the std `Path` methods (NO
//!     extern crate needed - std-only).
//! - **Dir** namespace (`Dir.list(p) -> Vector<String>`,
//!   `Dir.create(p)`, `Dir.remove(p)`, `Dir.walk(p) -> Vector<Path>`)
//!   - `list`/`create`/`remove` wrap `std::fs::*` (std-only - NO
//!     extern crate); `walk` wraps the `walkdir` Rust crate.
//! - **Tempfile** namespace (`Tempfile.create() -> Path`,
//!   `Tempfile.dir() -> Path`)
//!   - `create` wraps `tempfile::NamedTempFile::new()`; `dir`
//!     wraps `std::env::temp_dir()` (but the narrow walker still
//!     records `tempfile` for symmetry).
//!
//! Acceptance snapshots for the canonical criteria (per the task
//! spec):
//!
//! ```text
//! Path.join("a", "b", "c")      -> std::path::PathBuf::from("a").join("b").join("c")
//! path.parent()                 -> recv.parent().map(|p| p.to_path_buf())
//! path.extension()              -> recv.extension().map(|e| e.to_string())
//! path.basename()               -> recv.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string()
//! path.exists()                 -> recv.exists()
//! Dir.list(p)                   -> std::fs::read_dir(p).map(...).unwrap_or_default()
//! Dir.create(p)                 -> std::fs::create_dir_all(p).ok()
//! Dir.remove(p)                 -> std::fs::remove_dir_all(p).ok()
//! Dir.walk(p)                   -> walkdir::WalkDir::new(p).into_iter()...collect()
//! Tempfile.create()             -> tempfile::NamedTempFile::new().map(...).unwrap_or_default()
//! Tempfile.dir()                -> std::env::temp_dir()
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test fs_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All three modules are prelude namespaces (or a runtime-value
//! type constructed via a prelude assoc fn), so source parsing
//! requires no new keyword / AST node - the existing `MethodCall`
//! shape handles them. We construct ASTs by hand here for the
//! same reasons `format_codegen.rs` (T124i), `web_codegen.rs`
//! (T124h), `system_codegen.rs` (T124g), `regex_codegen.rs`
//! (T124d), `toml_codegen.rs` (T124e), and `utility_codegen.rs`
//! (T124f) do: direct AST construction decouples the codegen-
//! pinning snapshots from any future parser-restructuring work,
//! and lets us test specific edge cases (e.g. wrong arity, ident
//! vs literal arg, receiver inference for instance methods)
//! without writing Buff source that the parser may reject for
//! orthogonal reasons.

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

/// `<namespace>.<method>(args...)` AST node (associated-function call
/// shape). The receiver is the bare namespace Ident (e.g. `Path`,
/// `Dir`, `Tempfile`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
fn instance_call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(recv),
        method: ident(method),
        args,
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
// 1. Path module - associated function (join) + instance accessors.
// ===========================================================================

#[test]
fn path_codegen_join_two_args_chains_pathbuf() {
    // Path.join("a", "b") -> std::path::PathBuf::from("a").join("b").
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Path", "join", vec![str_expr("a"), str_expr("b")]),
    );
    assert!(
        src.contains("std::path::PathBuf::from("),
        "expected `std::path::PathBuf::from(` in: {src}"
    );
    assert!(
        src.contains(".join("),
        "expected `.join(` (chained join) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Path.join output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn path_codegen_join_three_args_chains_two_joins() {
    // Path.join("a", "b", "c") -> PathBuf::from("a").join("b").join("c").
    // Verify that the chain has TWO `.join(` calls (one per extra arg).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Path",
            "join",
            vec![str_expr("a"), str_expr("b"), str_expr("c")],
        ),
    );
    let join_count = src.matches(".join(").count();
    assert_eq!(
        join_count, 2,
        "expected 2 `.join(` calls for 3-arg Path.join, got {join_count} in: {src}"
    );
    assert!(
        src.contains("std::path::PathBuf::from("),
        "expected `std::path::PathBuf::from(` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn path_codegen_join_single_arg_returns_pathbuf_of_arg() {
    // Path.join("a") -> PathBuf::from("a") (no-op join, no .join call).
    let src = codegen_one_expr_in("f", ns_assoc_call("Path", "join", vec![str_expr("a")]));
    assert!(
        src.contains("std::path::PathBuf::from("),
        "expected `std::path::PathBuf::from(` in: {src}"
    );
    // Single-arg join has NO `.join(` call - just the PathBuf::from.
    assert!(
        !src.contains(".join("),
        "expected NO `.join(` for single-arg Path.join in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn path_codegen_join_via_ident_args() {
    // Path.join(a, b) where a and b are ident vars - both should be
    // spliced in by value (PathBuf::from accepts any AsRef<Path>).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Path", "join", vec![ident_expr("a"), ident_expr("b")]),
    );
    assert!(
        src.contains("std::path::PathBuf::from(a)"),
        "expected `std::path::PathBuf::from(a)` in: {src}"
    );
    assert!(src.contains(".join(b)"), "expected `.join(b)` in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Path instance accessors. The receiver must be a value of type `Path`
// (constructed via `Path.join(...)` and bound via `let p = ...`) so the
// type inferencer can resolve it to `Type::Path` and the codegen's
// `instance_fn_lookup` arm dispatches to `lower_prelude_type_instance_fn`.
// ---------------------------------------------------------------------------

/// Build a typical Path-using function body: `let p = Path.join(...)` then
/// one extra expr_stmt the test slots in. Returns the stmts vec.
fn path_body_with_extra(extra: Expr) -> Vec<Stmt> {
    vec![
        let_stmt(
            "p",
            ns_assoc_call("Path", "join", vec![str_expr("a"), str_expr("b")]),
        ),
        expr_stmt(extra),
    ]
}

#[test]
fn path_codegen_parent_accessor() {
    // path.parent() -> recv.parent().map(|p| p.to_path_buf()).
    let src = codegen_stmts_in(
        "f",
        path_body_with_extra(instance_call(ident_expr("p"), "parent", vec![])),
    );
    assert!(
        src.contains(".parent()"),
        "expected `.parent()` (Rust accessor returning Option<&Path>) in: {src}"
    );
    assert!(
        src.contains(".map(|p| p.to_path_buf())"),
        "expected `.map(|p| p.to_path_buf())` (lift &Path -> PathBuf) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn path_codegen_extension_accessor() {
    // path.extension() -> recv.extension().map(|e| e.to_string()).
    let src = codegen_stmts_in(
        "f",
        path_body_with_extra(instance_call(ident_expr("p"), "extension", vec![])),
    );
    assert!(
        src.contains(".extension()"),
        "expected `.extension()` (Rust accessor returning Option<&OsStr>) in: {src}"
    );
    assert!(
        src.contains(".map(|e| e.to_string())"),
        "expected `.map(|e| e.to_string())` (lift &OsStr -> String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn path_codegen_basename_accessor() {
    // path.basename() -> recv.file_name().and_then(|n| n.to_str())
    //                          .unwrap_or_default().to_string().
    let src = codegen_stmts_in(
        "f",
        path_body_with_extra(instance_call(ident_expr("p"), "basename", vec![])),
    );
    // `file_name` is the underlying Rust method (Buff surfaces it as
    // `basename` per POSIX convention).
    assert!(
        src.contains(".file_name("),
        "expected `.file_name(` (Rust accessor for basename) in: {src}"
    );
    assert!(
        src.contains(".and_then(|n| n.to_str())"),
        "expected `.and_then(|n| n.to_str())` (lossy non-UTF-8 handling) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (empty String when no basename - NEVER panics) in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (lift &str -> String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn path_codegen_exists_accessor() {
    // path.exists() -> recv.exists().
    let src = codegen_stmts_in(
        "f",
        path_body_with_extra(instance_call(ident_expr("p"), "exists", vec![])),
    );
    assert!(
        src.contains(".exists()"),
        "expected `.exists()` (Rust std::path::Path::exists) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. Dir module - associated functions (list / create / remove / walk).
// ===========================================================================

#[test]
fn dir_codegen_list_uses_read_dir_unwrap_or_default() {
    // Dir.list(p) -> std::fs::read_dir(p).map(...).unwrap_or_default().
    // Acceptance criterion: NEVER panics on inaccessible directories
    // (empty Vec fallback), matching Buff's "no panicking generated
    // code" rule.
    let src = codegen_one_expr_in("f", ns_assoc_call("Dir", "list", vec![str_expr("/tmp")]));
    assert!(
        src.contains("std::fs::read_dir("),
        "expected `std::fs::read_dir(` in: {src}"
    );
    // The .map(...) on the Result yields the Vec; .unwrap_or_default
    // yields empty Vec on the Err path (inaccessible directory).
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (empty Vec on inaccessible dir - NEVER panics) in: {src}"
    );
    // Entries are skipped via .filter_map(|e| e.ok()) - panic-free.
    assert!(
        src.contains(".filter_map(|e| e.ok())"),
        "expected `.filter_map(|e| e.ok())` (skip inaccessible entries) in: {src}"
    );
    // Entry names via file_name().to_string_lossy().into_owned().
    assert!(
        src.contains(".file_name()"),
        "expected `.file_name()` (entry name accessor) in: {src}"
    );
    assert!(
        src.contains(".to_string_lossy()"),
        "expected `.to_string_lossy()` (lossy non-UTF-8 name handling) in: {src}"
    );
    // Final collect to Vec<String>.
    assert!(
        src.contains(".collect::<Vec<String>>()"),
        "expected `.collect::<Vec<String>>()` turbofish in: {src}"
    );
    // No bare `.unwrap()` (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Dir.list output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn dir_codegen_create_uses_create_dir_all_with_ok() {
    // Dir.create(p) -> std::fs::create_dir_all(p).ok().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Dir", "create", vec![str_expr("/tmp/a")]),
    );
    assert!(
        src.contains("std::fs::create_dir_all("),
        "expected `std::fs::create_dir_all(` (mkdir -p semantics) in: {src}"
    );
    // `.ok()` discards the Result error (panic-free).
    assert!(
        src.contains(".ok()"),
        "expected `.ok()` (panic-free error discard) in: {src}"
    );
    // No bare `.unwrap()` (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Dir.create output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn dir_codegen_remove_uses_remove_dir_all_with_ok() {
    // Dir.remove(p) -> std::fs::remove_dir_all(p).ok().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Dir", "remove", vec![str_expr("/tmp/a")]),
    );
    assert!(
        src.contains("std::fs::remove_dir_all("),
        "expected `std::fs::remove_dir_all(` (recursive remove) in: {src}"
    );
    // `.ok()` discards the Result error (panic-free).
    assert!(
        src.contains(".ok()"),
        "expected `.ok()` (panic-free error discard) in: {src}"
    );
    // No bare `.unwrap()` (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Dir.remove output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn dir_codegen_walk_uses_walkdir_with_filter_map_ok() {
    // Dir.walk(p) -> walkdir::WalkDir::new(p).into_iter()
    //                  .filter_map(|e| e.ok())
    //                  .map(|e| e.path().to_path_buf())
    //                  .collect::<Vec<std::path::PathBuf>>().
    let src = codegen_one_expr_in("f", ns_assoc_call("Dir", "walk", vec![str_expr("/tmp")]));
    assert!(
        src.contains("walkdir::WalkDir::new("),
        "expected `walkdir::WalkDir::new(` in: {src}"
    );
    assert!(
        src.contains(".into_iter()"),
        "expected `.into_iter()` (walkdir entry iterator) in: {src}"
    );
    // Malformed entries skipped via .filter_map(|e| e.ok()) - panic-free.
    assert!(
        src.contains(".filter_map(|e| e.ok())"),
        "expected `.filter_map(|e| e.ok())` (skip inaccessible entries) in: {src}"
    );
    assert!(
        src.contains(".map(|e| e.path().to_path_buf())"),
        "expected `.map(|e| e.path().to_path_buf())` (DirEntry -> PathBuf) in: {src}"
    );
    // Final collect to Vec<PathBuf>.
    assert!(
        src.contains(".collect::<Vec<std::path::PathBuf>>()"),
        "expected `.collect::<Vec<std::path::PathBuf>>()` turbofish in: {src}"
    );
    // No bare `.unwrap()` (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Dir.walk output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Tempfile module - associated functions (create / dir).
// ===========================================================================

#[test]
fn tempfile_codegen_create_uses_named_temp_file_with_keep() {
    // Tempfile.create() -> tempfile::NamedTempFile::new()
    //   .map(|f| f.into_temp_path().keep().unwrap_or_default())
    //   .unwrap_or_default().
    let src = codegen_one_expr_in("f", ns_assoc_call("Tempfile", "create", vec![]));
    assert!(
        src.contains("tempfile::NamedTempFile::new("),
        "expected `tempfile::NamedTempFile::new(` in: {src}"
    );
    assert!(
        src.contains(".into_temp_path()"),
        "expected `.into_temp_path()` (NamedTempFile -> TempPath) in: {src}"
    );
    assert!(
        src.contains(".keep()"),
        "expected `.keep()` (TempPath -> PathBuf, persists the file) in: {src}"
    );
    // Two .unwrap_or_default() calls (one for the keep() Result, one
    // for the outer map() Result).
    let unwrap_or_default_count = src.matches(".unwrap_or_default()").count();
    assert_eq!(
        unwrap_or_default_count, 2,
        "expected 2 `.unwrap_or_default()` calls (panic-free on both inner + outer Result), got {unwrap_or_default_count} in: {src}"
    );
    // No bare `.unwrap()` (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Tempfile.create output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn tempfile_codegen_dir_uses_std_env_temp_dir() {
    // Tempfile.dir() -> std::env::temp_dir().
    // The tempfile::env::temp_dir() is a re-export of the std fn;
    // we splice the std path directly so this call alone needs NO
    // extern crate.
    let src = codegen_one_expr_in("f", ns_assoc_call("Tempfile", "dir", vec![]));
    assert!(
        src.contains("std::env::temp_dir()"),
        "expected `std::env::temp_dir()` in: {src}"
    );
    // The tempfile crate name MUST NOT appear in the generated source
    // for `Tempfile.dir` alone (the walker still records `tempfile` in
    // extern_crates for symmetry, but the codegen uses the std path).
    assert!(
        !src.contains("tempfile::"),
        "expected NO `tempfile::` path in Tempfile.dir output (uses std::env): {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. extern_crates registration.
// ===========================================================================

#[test]
fn dir_codegen_does_not_register_walkdir_for_list_only() {
    // A program with only Dir.list (NO Dir.walk) should NOT register
    // walkdir (Dir.list uses std::fs - no extern crate needed).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Dir",
            "list",
            vec![str_expr("/tmp")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("walkdir"),
        "extern_crates should NOT contain `walkdir` when only Dir.list is used, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("tempfile"),
        "extern_crates should NOT contain `tempfile` when Tempfile is unused, got: {:?}",
        extern_crates
    );
}

#[test]
fn dir_codegen_registers_walkdir_for_walk() {
    // A program with Dir.walk(...) registers the walkdir crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Dir",
            "walk",
            vec![str_expr("/tmp")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("walkdir"),
        "extern_crates should contain `walkdir`, got: {:?}",
        extern_crates
    );
}

#[test]
fn dir_codegen_does_not_register_walkdir_for_create_or_remove() {
    // A program with Dir.create + Dir.remove (NO Dir.walk) should
    // NOT register walkdir (those calls use std::fs only).
    let main = func_decl(
        "main",
        &[],
        vec![
            expr_stmt(ns_assoc_call("Dir", "create", vec![str_expr("/tmp/a")])),
            expr_stmt(ns_assoc_call("Dir", "remove", vec![str_expr("/tmp/a")])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("walkdir"),
        "extern_crates should NOT contain `walkdir` when only Dir.create/remove are used, got: {:?}",
        extern_crates
    );
}

#[test]
fn tempfile_codegen_registers_tempfile_for_create() {
    // A program with Tempfile.create() registers the tempfile crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call("Tempfile", "create", vec![]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("tempfile"),
        "extern_crates should contain `tempfile`, got: {:?}",
        extern_crates
    );
}

#[test]
fn tempfile_codegen_registers_tempfile_for_dir() {
    // A program with Tempfile.dir() (NO Tempfile.create) should
    // STILL register tempfile (the walker flags any Tempfile.* call
    // for symmetry - over-registration is benign).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call("Tempfile", "dir", vec![]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("tempfile"),
        "extern_crates should contain `tempfile` (Tempfile.dir walker), got: {:?}",
        extern_crates
    );
}

#[test]
fn path_codegen_registers_no_extern_crate() {
    // A program using Path.* (join + instance methods) should NOT
    // register any extern crate (std::path is in std - NO extern
    // crate needed, mirrors Math/Strings/Args/Env stance).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "p",
                ns_assoc_call("Path", "join", vec![str_expr("a"), str_expr("b")]),
            ),
            expr_stmt(instance_call(ident_expr("p"), "parent", vec![])),
            expr_stmt(instance_call(ident_expr("p"), "extension", vec![])),
            expr_stmt(instance_call(ident_expr("p"), "basename", vec![])),
            expr_stmt(instance_call(ident_expr("p"), "exists", vec![])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("walkdir"),
        "extern_crates should NOT contain `walkdir` when only Path is used, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("tempfile"),
        "extern_crates should NOT contain `tempfile` when only Path is used, got: {:?}",
        extern_crates
    );
}

#[test]
fn fs_codegen_no_extern_crate_when_unused() {
    // A program with no Path/Dir/Tempfile calls should not register
    // walkdir or tempfile.
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
        !extern_crates.contains("walkdir"),
        "extern_crates should NOT contain `walkdir` when Dir.walk is unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("tempfile"),
        "extern_crates should NOT contain `tempfile` when Tempfile is unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 5. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn path_codegen_rejects_join_with_zero_args() {
    // Path.join() with no args - should error (needs >= 1 arg).
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Path", "join", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Path.join()` (no path head)"
    );
}

#[test]
fn dir_codegen_rejects_list_with_wrong_arity() {
    // Dir.list() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Dir", "list", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Dir.list()` (no path arg)"
    );
}

#[test]
fn dir_codegen_rejects_walk_with_wrong_arity() {
    // Dir.walk() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Dir", "walk", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Dir.walk()` (no path arg)"
    );
}

#[test]
fn tempfile_codegen_rejects_create_with_args() {
    // Tempfile.create(extra) with args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call("Tempfile", "create", vec![str_expr("x")]),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Tempfile.create(\"x\")` (expected 0 args)"
    );
}

#[test]
fn tempfile_codegen_rejects_dir_with_args() {
    // Tempfile.dir(extra) with args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Tempfile", "dir", vec![str_expr("x")]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Tempfile.dir(\"x\")` (expected 0 args)"
    );
}

// ===========================================================================
// 6. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn path_codegen_join_two_args_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Path", "join", vec![str_expr("a"), str_expr("b")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn path_codegen_join_three_args_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Path",
            "join",
            vec![str_expr("a"), str_expr("b"), str_expr("c")],
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn path_codegen_parent_snapshot() {
    let src = codegen_stmts_in(
        "f",
        path_body_with_extra(instance_call(ident_expr("p"), "parent", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn path_codegen_extension_snapshot() {
    let src = codegen_stmts_in(
        "f",
        path_body_with_extra(instance_call(ident_expr("p"), "extension", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn path_codegen_basename_snapshot() {
    let src = codegen_stmts_in(
        "f",
        path_body_with_extra(instance_call(ident_expr("p"), "basename", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn path_codegen_exists_snapshot() {
    let src = codegen_stmts_in(
        "f",
        path_body_with_extra(instance_call(ident_expr("p"), "exists", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn dir_codegen_list_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Dir", "list", vec![str_expr("/tmp")]));
    insta::assert_snapshot!(src);
}

#[test]
fn dir_codegen_create_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Dir", "create", vec![str_expr("/tmp/a")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn dir_codegen_remove_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Dir", "remove", vec![str_expr("/tmp/a")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn dir_codegen_walk_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Dir", "walk", vec![str_expr("/tmp")]));
    insta::assert_snapshot!(src);
}

#[test]
fn tempfile_codegen_create_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Tempfile", "create", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn tempfile_codegen_dir_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Tempfile", "dir", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn fs_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises one call from each
    // of the three filesystem modules. Pins the full shape of the
    // generated Rust for a typical fs-using program (the acceptance
    // criterion from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            // Path value type + all 4 instance methods.
            let_stmt(
                "p",
                ns_assoc_call(
                    "Path",
                    "join",
                    vec![str_expr("a"), str_expr("b"), str_expr("c.txt")],
                ),
            ),
            let_stmt("parent", instance_call(ident_expr("p"), "parent", vec![])),
            let_stmt("ext", instance_call(ident_expr("p"), "extension", vec![])),
            let_stmt("base", instance_call(ident_expr("p"), "basename", vec![])),
            let_stmt("exists", instance_call(ident_expr("p"), "exists", vec![])),
            // Dir namespace - all 4 associated fns.
            let_stmt(
                "entries",
                ns_assoc_call("Dir", "list", vec![str_expr("/tmp")]),
            ),
            expr_stmt(ns_assoc_call("Dir", "create", vec![str_expr("/tmp/a")])),
            expr_stmt(ns_assoc_call("Dir", "remove", vec![str_expr("/tmp/a")])),
            let_stmt(
                "walked",
                ns_assoc_call("Dir", "walk", vec![str_expr("/tmp")]),
            ),
            // Tempfile namespace - both associated fns.
            let_stmt("tmp", ns_assoc_call("Tempfile", "create", vec![])),
            let_stmt("tmpdir", ns_assoc_call("Tempfile", "dir", vec![])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}
