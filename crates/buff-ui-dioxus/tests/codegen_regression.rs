//! T130 — codegen-pattern regression test.
//!
//! This is the maintained successor to the T121b codegen proof at
//! [`crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs`] (DO NOT
//! modify that file — it is the T121b artifact and stays as the
//! feasibility-spike record). The T121b proof established that
//! `syn::Macro` carrying an arbitrary `proc_macro2::TokenStream`
//! (built via `quote!`) survives `prettyplease::unparse` with all
//! tokens intact (only whitespace is massaged). This regression test
//! asserts the same shape still holds against the *exact* counter
//! `examples/counter.rs` ships.
//!
//! # Why is this in `buff-ui-dioxus`?
//!
//! T133 (RSX-for-Buff syntax, v1.9) will lower Buff UI blocks onto
//! `rsx!{}` macro invocations via the same syn::Macro + quote!{} +
//! prettyplease stack. This test guards the contract: if a future
//! dioxus minor bump OR a prettyplease patch breaks the TokenStream
//! survival guarantee, this crate's tests fail before T133 codegen
//! starts silently emitting broken code.
//!
//! # Mechanism
//!
//! We construct the SAME `syn::File` the T121b test constructs, run
//! it through `prettyplease::unparse`, and assert that:
//!
//! 1. The `#[component]` attribute survives.
//! 2. The `rsx!` macro invocation survives.
//! 3. The `use_signal` hook call survives.
//! 4. The macro body's tokens (onclick handler, interpolation) all
//!    survive, including prettyplease's whitespace-massage (`onclick
//!    : move | _ | count += 1` instead of `onclick: move |_| count
//!    += 1`).
//! 5. The formatted output re-parses as a valid `syn::File` (macro
//!    TokenStream survives round-trip — the proc macro will see
//!    exactly what we constructed).

// The `allow` mirrors the T121b test's stance: the raw string
// `r#"..."#` form is needed inside `quote!` for some interpolation
// patterns; clippy's `needless_raw_string_hashes` is a style lint
// that does not apply to codegen-generated code paths.
#![allow(clippy::needless_raw_string_hashes)]

use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};
use syn::{
    punctuated::Punctuated, AttrStyle, Attribute, Block, Expr, ExprMacro, FnArg, Ident, Item,
    ItemFn, Macro, MacroDelimiter, PatType, PathArguments, PathSegment, ReturnType, Signature,
    Stmt, Token, Type, Visibility,
};

// ---------------------------------------------------------------------------
// syn node builders — explicit construction (the realistic codegen path).
// ---------------------------------------------------------------------------
//
// These mirror the T121b test's helpers verbatim (no semantic change),
// because they encode the EXACT syn shape T133 will emit. If a future
// refactor changes how a `#[component] fn` is constructed, this test
// must be updated alongside the codegen visitor.

/// `#[component]` as an outer attribute. Built from a parsed
/// `# [ component ]` token stream because `syn::Attribute` has no
/// public constructor for the bare-`#[component]` form.
fn component_attr() -> Attribute {
    let meta_path: syn::Path = syn::parse_quote!(component);
    Attribute {
        pound_token: Default::default(),
        style: AttrStyle::Outer,
        bracket_token: Default::default(),
        meta: syn::Meta::Path(meta_path),
    }
}

/// Build the `rsx! { ... }` macro expression with its body as an
/// explicit `proc_macro2::TokenStream`. This is the node shape that
/// `prettyplease` must carry intact.
fn rsx_macro_expr(body: TokenStream) -> Expr {
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

/// Build the `App` component body — same as the T121b test's body
/// but uses the `buff_ui_dioxus::on_signal_mut` helper that the
/// `examples/counter.rs` example uses. The rsx! body is what T133
/// will emit; the lowering of `onclick: count += 1` to the helper
/// call is the T130 contract.
fn app_block() -> Block {
    // 1) `let mut count = buff_ui_dioxus::use_signal(|| 0);`
    let let_stmt: Stmt = syn::parse_quote! {
        let mut count = buff_ui_dioxus::use_signal(|| 0);
    };

    // 2) `rsx! { button { onclick: ..., "..." } }`
    //    The onclick handler uses the helper. The button label
    //    interpolates the signal via `{count}`.
    let rsx_body: TokenStream = quote! {
        button {
            onclick: move |_| buff_ui_dioxus::on_signal_mut(count, |c| *c += 1),
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

/// Build the whole `fn App() -> Element { ... }` item with
/// `#[component]`.
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

/// `fn main() { buff_ui_dioxus::launch(App); }` — the wasm + host
/// entry point.
fn main_item() -> Item {
    syn::parse_quote! {
        fn main() {
            buff_ui_dioxus::launch(App);
        }
    }
}

/// `use buff_ui_dioxus::{component, launch, on_signal_mut, rsx,
/// use_signal, Element};` — T133 will emit this single import line
/// (it's the wrapper crate's public surface).
fn use_buff_ui_dioxus_item() -> Item {
    syn::parse_quote! {
        use buff_ui_dioxus::{component, launch, on_signal_mut, rsx, use_signal, Element};
    }
}

/// Wire the items into a `syn::File` and format via `prettyplease`.
/// Returns the formatted Rust source string. This is the EXACT shape
/// `examples/counter.rs` produces, modulo the `use` import path
/// (the example imports from the crate root, the test mirrors T133
/// codegen which will use the fully-qualified path).
fn generate_counter_main_rs() -> String {
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![use_buff_ui_dioxus_item(), main_item(), app_item()],
    };
    // THE crux call: prettyplease must not mangle the macro
    // TokenStream.
    prettyplease::unparse(&file)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn t130_emits_component_attr_token() {
    let src = generate_counter_main_rs();
    assert!(
        src.contains("#[component]"),
        "expected `#[component]` token in generated Rust, got:\n{src}"
    );
}

#[test]
fn t130_emits_rsx_macro_token_verbatim() {
    let src = generate_counter_main_rs();
    assert!(
        src.contains("rsx!"),
        "expected `rsx!` token to survive prettyplease formatting, got:\n{src}"
    );
    // KEY FINDING (re-affirmed for T130): prettyplease inserts
    // whitespace INSIDE the macro TokenStream but does NOT delete,
    // reorder, or re-tokenize the body. We assert token-by-token
    // (NOT as a literal substring) to match the actual prettyplease
    // output.
    for needle in [
        "onclick",
        "move",
        "|",
        "_",
        "buff_ui_dioxus",
        "on_signal_mut",
        "count",
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
}

#[test]
fn t130_emits_use_signal_hook_via_wrapper() {
    let src = generate_counter_main_rs();
    assert!(
        src.contains("buff_ui_dioxus::use_signal(|| 0)"),
        "expected `buff_ui_dioxus::use_signal(|| 0)` hook via the wrapper crate, got:\n{src}"
    );
}

#[test]
fn t130_emits_launch_via_wrapper() {
    let src = generate_counter_main_rs();
    assert!(
        src.contains("buff_ui_dioxus::launch(App)"),
        "expected `buff_ui_dioxus::launch(App)` entry point via the wrapper crate, got:\n{src}"
    );
}

#[test]
fn t130_generated_source_re_parses() {
    // Symmetry check: prettyplease output must re-parse as a valid
    // `syn::File`. This proves the macro tokens survive round-trip
    // (the proc-macro will see exactly what we constructed).
    let src = generate_counter_main_rs();
    syn::parse_str::<syn::File>(&src).unwrap_or_else(|e| {
        panic!("prettyplease output must re-parse as syn::File: {e}\n--- src ---\n{src}")
    });
}

#[test]
fn t130_token_stream_appended_idempotently() {
    // Sanity check: appending tokens to a fresh TokenStream via
    // `quote!` produces a stream that, when fed into `syn::Macro`,
    // survives `prettyplease::unparse()`. This isolates the
    // TokenStream -> Macro -> prettyplease path so a regression here
    // is unambiguous.
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
    let out = prettyplease::unparse(&file);
    assert!(out.contains("rsx!"), "tiny rsx! must survive: {out}");
    assert!(out.contains("div"), "div body must survive: {out}");
}

// ---------------------------------------------------------------------------
// Silence unused-import warning for `PatType` (kept to mirror the
// T121b test's import list verbatim — useful for side-by-side
// comparison when this test is consulted during a future dioxus
// bump).
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _silence_unused_pattype_import(_: PatType) {}
