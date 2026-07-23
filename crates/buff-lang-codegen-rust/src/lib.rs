//! Buff Rust codegen crate — converts Buff AST to Rust source via syn/quote/prettyplease.
//!
//! ## Pipeline
//!
//! ```text
//!   &[buff_lang_ast::Decl]
//!        │
//!        ▼  RustCodegen::generate
//!   syn::File
//!        │
//!        ▼  format::format  (prettyplease::unparse)
//!   String  (valid Rust source)
//! ```
//!
//! Every Rust construct is built through `syn` types — we never hand-format
//! Rust strings. The single string producer is `prettyplease`, whose output
//! is equivalent to a `rustfmt` pass.
//!
//! # Example
//!
//! ```
//! use buff_lang_ast::{Decl, common::{Block, Ident}, decl::FuncDecl};
//! use buff_lang_error::Span;
//!
//! let func = FuncDecl {
//!     name: Ident::new("empty", Span::dummy()),
//!     params: Vec::new(),
//!     return_type: None,
//!     body: Block::empty(Span::dummy()),
//!     is_async: false,
//!     is_unsafe: false,
//!     is_extern: false, attributes: Vec::new(),
//!     span: Span::dummy(),
//! };
//! let src = buff_lang_codegen_rust::generate_rust(&[Decl::FuncDecl(func)]).unwrap();
//! assert!(src.contains("fn empty()"));
//! ```

pub mod atomic_analysis;
// T53: comptime-facts → Rust `const` items lowering. Consumes
// buff_lang_types::ComptimeFacts and emits one Item::Const per
// evaluated value.
pub mod comptime;
pub mod context;
pub mod format;
pub mod gpu_alignment;
pub mod move_analysis;
pub mod race_analysis;
pub mod rust_codegen;

// T35: `generate_test_rust` (below) uses `syn::{Item, ItemFn, Ident}` and
// `quote!` to synthesise the test-runner `main` fn. Re-exported here so the
// function can build the runner without reaching into `rust_codegen`'s
// private imports.
use syn::{Ident, Item, ItemFn};

pub use atomic_analysis::{analyze as analyze_atomic_promotions, AtomicPromotions, AtomicSet};
pub use context::CodegenContext;
pub use format::format;
pub use gpu_alignment::gpu_bound_structs as analyze_gpu_alignment;
pub use move_analysis::MoveAnalyzer;
pub use race_analysis::{
    analyze as analyze_parallel_races,
    analyze_with_exemptions as analyze_parallel_races_with_exemptions, is_assignment_op,
    ParallelMutabilityError, PARALLEL_COMBINATORS,
};
pub use rust_codegen::{buff_primitive_to_rust_name, collect_rust_deps, RustCodegen};

/// Convenience alias for [`format`] so external callers (tests, the CLI)
/// can refer to it without importing the module. T26 introduced the alias
/// so the `struct_codegen` integration tests can format a `syn::File` they
/// obtained from the lower-level [`RustCodegen::generate`] entry point
/// (needed for the `#[repr(C)]` hook test which bypasses the convenience
/// [`generate_rust`] wrapper).
pub fn format_file(file: &syn::File) -> String {
    format(file)
}

/// Convenience: lower a slice of Buff declarations to formatted Rust source.
///
/// Equivalent to building a [`RustCodegen`], calling [`RustCodegen::generate`],
/// then [`format`] on the result.
pub fn generate_rust(
    decls: &[buff_lang_ast::Decl],
) -> Result<String, buff_lang_error::CodegenError> {
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(decls)?;
    Ok(format(&file))
}

/// T24: lower a slice of Buff declarations to formatted Rust source +
/// capture the Rust-line ↔ Buff-span source map for `.buffmap` sidecar
/// emission.
///
/// Equivalent to [`generate_rust`] followed by
/// [`buff_lang_debug_info::build_source_map`]. The CLI calls this when
/// `buff build` / `buff run` want to emit a `.buffmap` alongside the
/// compiled binary so the runtime panic hook can remap Rust backtraces
/// to Buff source locations.
///
/// The source map is built from the FORMATTED Rust source (post-
/// `prettyplease`) so the recorded Rust line numbers match what `rustc`
/// actually sees — and what shows up in panic `Location` rows. The
/// `buff_source` slice is consumed for byte-offset → `(line, col)`
/// lookup; it must be the same source that produced `decls`.
pub fn generate_rust_with_source_map(
    decls: &[buff_lang_ast::Decl],
    buff_path: &std::path::Path,
    buff_source: &str,
) -> Result<(String, buff_lang_debug_info::SourceMap), buff_lang_error::CodegenError> {
    let rust_source = generate_rust(decls)?;
    let source_map =
        buff_lang_debug_info::build_source_map(decls, &rust_source, buff_path, buff_source);
    Ok((rust_source, source_map))
}

/// T0-B4: lower a slice of Buff declarations to formatted Rust source,
/// gated by the resolved feature set.
///
/// Decls carrying `@feature(name)` are emitted only when `name` appears
/// in `features`. Decls without `@feature` are always emitted. Mirrors
/// Rust's `#[cfg(feature = "...")]` + Go build tags.
///
/// Equivalent to pre-filtering `decls` via [`filter_by_features`] then
/// delegating to [`generate_rust`]. The CLI resolves features from
/// `buff.toml [features].default` + the `--features` CLI flag, then
/// calls this entry point.
///
/// When `features` is empty, all `@feature(name)` decls are dropped —
/// this matches Cargo's `--no-default-features` behaviour. To get the
/// "everything on" behaviour (useful for `buff check` that just wants
/// to type-check all source), pass the full feature list.
pub fn generate_rust_with_features(
    decls: &[buff_lang_ast::Decl],
    features: &[String],
) -> Result<String, buff_lang_error::CodegenError> {
    let filtered = filter_by_features(decls, features);
    generate_rust(&filtered)
}

/// T0-B4: filter a slice of declarations by `@feature(name)` gating.
///
/// Returns a new `Vec<Decl>` containing only:
/// - Decls without any `@feature(...)` attribute (always emitted).
/// - Decls whose `@feature(name)` has `name` in `features`.
///
/// Applied to top-level `Decl`s only (the parser does not currently
/// allow `@feature` on nested items; that's a v1.18+ concern).
///
/// Public so `buff check` can re-use the filter to type-check only
/// the active code paths (mirrors Rust's `cargo check --features ...`).
pub fn filter_by_features(
    decls: &[buff_lang_ast::Decl],
    features: &[String],
) -> Vec<buff_lang_ast::Decl> {
    decls
        .iter()
        .filter(|decl| decl_feature_satisfied(decl, features))
        .cloned()
        .collect()
}

/// `true` when `decl` carries no `@feature(...)` attribute OR carries
/// `@feature(name)` with `name` in `features`.
fn decl_feature_satisfied(decl: &buff_lang_ast::Decl, features: &[String]) -> bool {
    let attrs = match decl {
        buff_lang_ast::Decl::FuncDecl(f) => &f.attributes,
        // Only FuncDecls carry attributes today (T35). When structs/enums
        // gain attribute support, extend this match.
        _ => return true,
    };
    for attr in attrs {
        if attr.name.name == "feature" {
            // @feature(name) — first arg is the feature name. If the
            // attribute has zero args, treat as a parse error and drop
            // (defensive — parser should have rejected this earlier).
            if let Some(name) = attr.args.first() {
                if !features.iter().any(|f| f == name) {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    true
}

/// Generate a **test harness** Rust source for `buff test` (T35).
///
/// Produces a self-contained Rust file that, when compiled with `rustc`
/// (normal mode, NOT `--test`) and executed, runs each named test function
/// inside a `std::panic::catch_unwind` guard, prints per-test results, and
/// exits with code `0` if all pass or `1` if any fail.
///
/// The harness is assembled by:
///
/// 1. Running the normal codegen on ALL decls (so helper functions, structs,
///    etc. are emitted as usual — tests can call them).
/// 2. **Removing** the user's `fn main()` (if present) — a program can only
///    have one `main`, and the test harness provides its own.
/// 3. **Stripping `#[test]` attributes** from the generated items. The
///    codegen emits `#[test]` on `@test` fns (so `buff build` produces
///    idiomatic Rust); in the test-harness build we DON'T want `#[test]`
///    (we call the fns directly from our custom `main`), and stripping the
///    attribute avoids any ambiguity about whether `#[test]` fns are
///    callable in non-`--test` builds.
/// 4. **Appending a synthetic `fn main()`** that loops over `test_names`,
///    calls each inside `catch_unwind`, prints `test <name> ... ok|FAILED`,
///    then prints `\n<passed> passed, <failed> failed` and exits.
///
/// `test_names` must be a subset of the `@test` function names in `decls`
/// (the CLI discovers + filters them before calling this). Unknown names
/// produce a [`CodegenError`] (defensive — the synthetic `main` would fail
/// to compile if it referenced a non-existent fn).
///
/// # Why a custom runner (not `rustc --test`)?
///
/// The QA requires output in the `<n> passed, <m> failed` format; Rust's
/// built-in `--test` harness prints `1 passed; 0 failed` (semicolon +
/// different wording). A custom runner gives us full control of the output
/// format AND lets us avoid the `#[test]`-fn-vs-user-`main` conflict that
/// `--test` introduces.
///
/// # Errors
///
/// Propagates [`CodegenError`] from the normal codegen pass (unsupported
/// AST nodes, etc.).
pub fn generate_test_rust(
    decls: &[buff_lang_ast::Decl],
    test_names: &[String],
) -> Result<String, buff_lang_error::CodegenError> {
    let mut codegen = RustCodegen::new();
    let mut file = codegen.generate(decls)?;

    // 2. Remove the user's `fn main()` — only one `main` is allowed and we
    //    provide our own test-runner main below.
    file.items.retain(|item| match item {
        Item::Fn(f) => f.sig.ident != "main",
        _ => true,
    });

    // 3. Strip `#[test]` attributes from all items — the harness calls the
    //    fns directly; `#[test]` is unnecessary and could (in principle)
    //    affect non-`--test` compilation on future toolchains.
    for item in &mut file.items {
        if let Item::Fn(f) = item {
            f.attrs.retain(|attr| !attr.path().is_ident("test"));
        }
    }

    // 4. Build the synthetic test-runner main via `quote!` + `syn::parse2`
    //    (the Result-returning variant of parse_quote, so we never panic).
    //    Each test fn is called inside `catch_unwind`; panics count as
    //    failures (not crashes).
    let test_calls: Vec<proc_macro2::TokenStream> = test_names
        .iter()
        .map(|name| {
            let ident = Ident::new(name, proc_macro2::Span::call_site());
            let name_str = name.to_string();
            quote::quote! {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { #ident(); })) {
                    Ok(()) => {
                        passed += 1;
                        println!("test {} ... ok", #name_str);
                    }
                    Err(_) => {
                        failed += 1;
                        failures.push(#name_str);
                        println!("test {} ... FAILED", #name_str);
                    }
                }
            }
        })
        .collect();

    let runner_main: proc_macro2::TokenStream = quote::quote! {
        fn main() {
            let mut passed: usize = 0;
            let mut failed: usize = 0;
            let mut failures: Vec<&str> = Vec::new();
            #( #test_calls )*
            println!();
            println!("{} passed, {} failed", passed, failed);
            if failed > 0 {
                std::process::exit(1);
            }
        }
    };
    let main_item: ItemFn = syn::parse2(runner_main).map_err(|e| {
        buff_lang_error::CodegenError::new(buff_lang_error::Diagnostic::error(
            format!("test-runner main synthesis failed: {e}"),
            buff_lang_error::Span::dummy(),
        ))
    })?;
    file.items.push(Item::Fn(main_item));

    Ok(format(&file))
}
