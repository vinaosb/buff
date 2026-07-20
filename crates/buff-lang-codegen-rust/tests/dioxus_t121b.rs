//! T121b — Dioxus codegen feasibility spike.
//!
//! **Mechanism under test: CODEGEN, NOT FFI.** Dioxus is macro-driven
//! (`rsx!{}` is a compile-time proc macro that expands during rustc). The
//! integration path is: Buff codegen-rust emits Rust *source text* that
//! contains `#[component]` and `rsx!{ ... }` macro invocations → rustc +
//! dioxus-rsx proc macro expand → compiles to wasm32 → renders in browser.
//!
//! This test exercises the EXACT same syn/quote/prettyplease stack that
//! `generate_rust()` uses (see `crates/buff-lang-codegen-rust/src/lib.rs:79`
//! and `format.rs:15`). It programmatically constructs a `syn::File` for the
//! Dioxus **counter** component (signals + `onclick` + reactive re-render —
//! NOT hello-world), then formats it via `prettyplease::unparse`. The crux
//! risk is that prettyplease mangles the `rsx!` macro's internal TokenStream;
//! we assert it survives intact.
//!
//! The generated source is written to disk so the standalone wasm spike crate
//! (under `%TEMP%\opencode\dioxus-spike\`) can compile it for wasm32 and
//! render in a headless browser.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test dioxus_t121b -- --nocapture
//! ```

#![allow(clippy::needless_raw_string_hashes)]

use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};
use syn::{
    punctuated::Punctuated, Attribute, AttrStyle, Block, Expr, ExprMacro, FnArg, Ident,
    Item, ItemFn, Macro, MacroDelimiter, PatType, PathArguments, PathSegment, ReturnType,
    Signature, Stmt, Token, Type, Visibility,
};

// ---------------------------------------------------------------------------
// Target path for the generated source — picked up by the standalone spike
// crate at `%TEMP%\opencode\dioxus-spike\src\main.rs`.
// ---------------------------------------------------------------------------

fn spike_dir() -> std::path::PathBuf {
    let base = std::env::var_os("TEMP")
        .or_else(|| std::env::var_os("TMP"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    base.join("opencode").join("dioxus-spike")
}

fn generated_main_rs_path() -> std::path::PathBuf {
    spike_dir().join("src").join("main.rs")
}

// ---------------------------------------------------------------------------
// syn node builders — explicit construction (the realistic codegen path).
// ---------------------------------------------------------------------------

/// `#[component]` as an outer attribute.
fn component_attr() -> Attribute {
    // `syn::Attribute` has no public `from_meta` constructor for the literal
    // `#[component]` form, so build it from a parsed `# [ component ]` token
    // stream. The path `component` has no arguments.
    let meta_path: syn::Path = syn::parse_quote!(component);
    Attribute {
        pound_token: Default::default(),
        style: AttrStyle::Outer,
        bracket_token: Default::default(),
        meta: syn::Meta::Path(meta_path),
    }
}

/// Build the `rsx! { ... }` macro expression with its body as an explicit
/// `proc_macro2::TokenStream` (this is what `quote!` produces). The syn node
/// type carrying macro invocations is `Expr::Macro(ExprMacro { mac: Macro })`,
/// where `Macro.tokens` is a raw `TokenStream` that prettyplease prints
/// verbatim.
fn rsx_macro_expr(body: TokenStream) -> Expr {
    // `rsx` path: a single segment, no generics, no arguments.
    let path_segments: Punctuated<PathSegment, Token![::]> = {
        let mut p = Punctuated::new();
        p.push(PathSegment {
            ident: Ident::new("rsx", proc_macro2::Span::call_site()),
            arguments: PathArguments::None,
        });
        p
    };
    let path = syn::Path {
        leading_colon: None,
        segments: path_segments,
    };
    let mac = Macro {
        path,
        bang_token: Default::default(),
        delimiter: MacroDelimiter::Brace(Default::default()),
        tokens: body,
    };
    Expr::Macro(ExprMacro {
        attrs: Vec::new(),
        mac,
    })
}

/// `Element` type reference (Dioxus's component return type).
fn element_return_type() -> Type {
    syn::parse_quote!(Element)
}

/// Build the `App` component body:
///
/// ```text
/// {
///     let mut count = use_signal(|| 0);
///     rsx! {
///         button {
///             onclick: move |_| count += 1,
///             "Increment (count: {count})"
///         }
///     }
/// }
/// ```
///
/// The `rsx!{}` is the trailing expression (no semicolon) so its `Element`
/// becomes the function's return value.
fn app_block() -> Block {
    // 1) `let mut count = use_signal(|| 0);`
    let let_stmt: Stmt = syn::parse_quote! {
        let mut count = use_signal(|| 0);
    };

    // 2) `rsx! { ... }` — built explicitly so we prove the TokenStream path.
    //    Body text mirrors the canonical Dioxus 0.7 counter example from
    //    context7 /dioxuslabs/dioxus v0.7.2. The `onclick` handler captures
    //    `count` by move and mutates the signal; the `{count}` interpolation
    //    re-renders on signal change. This exercises signal + event +
    //    reactivity (NOT hello-world).
    let rsx_body: TokenStream = quote! {
        button {
            onclick: move |_| count += 1,
            "Increment (count: {count})"
        }
    };
    let rsx_expr = rsx_macro_expr(rsx_body);
    let rsx_stmt = Stmt::Expr(rsx_expr, /* no semi */ None);

    Block {
        brace_token: Default::default(),
        stmts: vec![let_stmt, rsx_stmt],
    }
}

/// Build the whole `fn App() -> Element { ... }` item with `#[component]`.
fn app_item() -> Item {
    let sig = Signature {
        constness: None,
        asyncness: None,
        unsafety: None,
        abi: None,
        fn_token: Default::default(),
        ident: Ident::new("App", proc_macro2::Span::call_site()),
        generics: Default::default(),
        paren_token: Default::default(),
        inputs: Punctuated::<FnArg, Token![,]>::new(),
        variadic: None,
        output: ReturnType::Type(Default::default(), Box::new(element_return_type())),
    };
    Item::Fn(ItemFn {
        attrs: vec![component_attr()],
        vis: Visibility::Inherited,
        sig,
        block: Box::new(app_block()),
    })
}

/// `fn main() { dioxus::launch(App); }` — the wasm entry point.
fn main_item() -> Item {
    syn::parse_quote! {
        fn main() {
            dioxus::launch(App);
        }
    }
}

/// `use dioxus::prelude::*;` — needed for `Element`, `use_signal`, `rsx!`.
fn use_dioxus_item() -> Item {
    syn::parse_quote! {
        use dioxus::prelude::*;
    }
}

/// Wire the three items into a `syn::File` and format via prettyplease.
/// Returns the formatted Rust source string.
pub fn generate_counter_main_rs() -> String {
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![use_dioxus_item(), main_item(), app_item()],
    };
    // THE crux call: prettyplease must not mangle the macro TokenStream.
    buff_lang_codegen_rust::format(&file)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn t121b_emits_component_attr_token() {
    let src = generate_counter_main_rs();
    assert!(
        src.contains("#[component]"),
        "expected `#[component]` token in generated Rust, got:\n{src}"
    );
}

#[test]
fn t121b_emits_rsx_macro_token_verbatim() {
    let src = generate_counter_main_rs();
    assert!(
        src.contains("rsx!"),
        "expected `rsx!` token to survive prettyplease formatting, got:\n{src}"
    );
    // KEY FINDING: prettyplease inserts whitespace INSIDE the macro
    // TokenStream (`onclick : move | _ | count += 1` instead of
    // `onclick: move |_| count += 1`) but does NOT delete, reorder, or
    // re-tokenize the body. The proc macro receives an equivalent
    // TokenStream and will parse it identically. We assert token-by-token
    // (NOT as a literal substring) to match the actual prettyplease output.
    for needle in [
        "onclick",
        "move",
        "|",
        "_",
        "count",
        "+=",
        "1",
        "Increment",
        "{count}",
    ]
    .iter()
    {
        assert!(
            src.contains(needle),
            "expected token `{needle}` to survive in macro body, got:\n{src}"
        );
    }
    // Sanity: assert the broken-layout variant (with extra spaces) IS the
    // output — this documents the prettyplease whitespace-massage behavior.
    assert!(
        src.contains("onclick : move | _ | count += 1,"),
        "expected prettyplease-whitespace-massaged form, got:\n{src}"
    );
}

#[test]
fn t121b_emits_use_signal_hook() {
    let src = generate_counter_main_rs();
    assert!(
        src.contains("use_signal(|| 0)"),
        "expected `use_signal(|| 0)` hook in generated Rust, got:\n{src}"
    );
}

#[test]
fn t121b_generated_source_re_parses() {
    // Symmetry check: prettyplease output must re-parse as a valid
    // `syn::File`. This proves the macro tokens survive round-trip
    // (the proc-macro will see exactly what we constructed).
    let src = generate_counter_main_rs();
    syn::parse_str::<syn::File>(&src)
        .unwrap_or_else(|e| panic!("prettyplease output must re-parse as syn::File: {e}\n--- src ---\n{src}"));
}

#[test]
fn t121b_token_stream_appended_idempotently() {
    // Sanity check: appending tokens to a fresh TokenStream via quote!
    // produces a stream that, when fed into syn::Macro, survives format().
    // This isolates the TokenStream → Macro → prettyplease path so a
    // regression here is unambiguous.
    let mut ts: TokenStream = TokenStream::new();
    ts.append_all(vec![quote! { div { "hi" } }]);
    let expr = rsx_macro_expr(ts);
    let stmt = Stmt::Expr(expr, None);
    let block = Block {
        brace_token: Default::default(),
        stmts: vec![stmt],
    };
    let item = Item::Fn(ItemFn {
        attrs: vec![component_attr()],
        vis: Visibility::Inherited,
        sig: Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: Default::default(),
            ident: Ident::new("Tiny", proc_macro2::Span::call_site()),
            generics: Default::default(),
            paren_token: Default::default(),
            inputs: Punctuated::<FnArg, Token![,]>::new(),
            variadic: None,
            output: ReturnType::Default,
        },
        block: Box::new(block),
    });
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![item],
    };
    let out = buff_lang_codegen_rust::format(&file);
    assert!(out.contains("rsx!"), "tiny rsx! must survive: {out}");
    assert!(out.contains("div"), "div body must survive: {out}");
}

#[test]
fn t121b_writes_main_rs_to_spike_dir() {
    // Side-effectful: writes the generated counter main.rs to the
    // standalone spike crate's src/ dir so cargo can build it for wasm32.
    // The spike crate lives under %TEMP%\opencode\dioxus-spike\ — a
    // throwaway location, NOT inside the buff repo.
    let src = generate_counter_main_rs();
    let path = generated_main_rs_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("could not create spike dir {parent:?}: {e}"));
    }
    std::fs::write(&path, &src)
        .unwrap_or_else(|e| panic!("could not write generated main.rs to {path:?}: {e}"));
    // Verify the write actually landed.
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not re-read {path:?}: {e}"));
    assert_eq!(on_disk, src, "disk content must match in-memory source");
    assert!(on_disk.contains("rsx!"));
    assert!(on_disk.contains("#[component]"));
    eprintln!("[T121b] wrote generated counter main.rs to {}", path.display());
}

// ---------------------------------------------------------------------------
// Error-message quality assessment helper (invoked from the bash harness).
// ---------------------------------------------------------------------------

/// Generate a deliberately-broken variant (`rsx!` body with an unknown
/// attribute) so the rustc/dioxus-rsx error location can be inspected and
/// its mapping back to a Buff source line assessed. Written to
/// `<spike_dir>/src/broken.rs`.
#[test]
fn t121b_writes_broken_variant_for_error_mapping_assessment() {
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![
            syn::parse_quote! { use dioxus::prelude::*; },
            syn::parse_quote! { fn main() { dioxus::launch(App); } },
            Item::Fn(ItemFn {
                attrs: vec![component_attr()],
                vis: Visibility::Inherited,
                sig: Signature {
                    constness: None,
                    asyncness: None,
                    unsafety: None,
                    abi: None,
                    fn_token: Default::default(),
                    ident: Ident::new("App", proc_macro2::Span::call_site()),
                    generics: Default::default(),
                    paren_token: Default::default(),
                    inputs: Punctuated::<FnArg, Token![,]>::new(),
                    variadic: None,
                    output: ReturnType::Type(
                        Default::default(),
                        Box::new(element_return_type()),
                    ),
                },
                block: Box::new({
                    // Body uses a bogus attribute `not_a_real_attr_xyz` to
                    // force dioxus-rsx to emit a diagnostic. The line/column
                    // of that diagnostic in the .rs file is the input to the
                    // error-mapping-quality assessment in the decision doc.
                    let let_stmt: Stmt = syn::parse_quote! {
                        let mut count = use_signal(|| 0);
                    };
                    let rsx_body: TokenStream = quote! {
                        button {
                            not_a_real_attr_xyz: 42,
                            onclick: move |_| count += 1,
                            "{count}"
                        }
                    };
                    let rsx_expr = rsx_macro_expr(rsx_body);
                    let rsx_stmt = Stmt::Expr(rsx_expr, None);
                    Block {
                        brace_token: Default::default(),
                        stmts: vec![let_stmt, rsx_stmt],
                    }
                }),
            }),
        ],
    };
    let src = buff_lang_codegen_rust::format(&file);
    let path = spike_dir().join("src").join("broken.rs");
    std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))
        .ok();
    std::fs::write(&path, &src)
        .unwrap_or_else(|e| panic!("could not write broken.rs: {e}"));
    eprintln!("[T121b] wrote broken variant to {}", path.display());
}

// ---------------------------------------------------------------------------
// Surface the generator to other tools via a `main` shim (cargo test ignores
// `fn main`; but the binary entrypoint here is irrelevant — the test above
// already writes the file). Suppress dead_code warnings for the helpers.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _silence_unused_pattype_import(_: PatType) {}
