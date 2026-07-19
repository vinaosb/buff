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

pub mod context;
pub mod format;
pub mod move_analysis;
pub mod race_analysis;
pub mod rust_codegen;

// T35: `generate_test_rust` (below) uses `syn::{Item, ItemFn, Ident}` and
// `quote!` to synthesise the test-runner `main` fn. Re-exported here so the
// function can build the runner without reaching into `rust_codegen`'s
// private imports.
use syn::{Ident, Item, ItemFn};

pub use context::CodegenContext;
pub use format::format;
pub use move_analysis::MoveAnalyzer;
pub use race_analysis::{
    analyze as analyze_parallel_races, is_assignment_op, ParallelMutabilityError,
    PARALLEL_COMBINATORS,
};
pub use rust_codegen::{buff_primitive_to_rust_name, RustCodegen};

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
