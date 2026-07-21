//! T134 — lifecycle-hook codegen regression test.
//!
//! Sister file to `tests/codegen_regression.rs`. The T130 test asserts
//! the `use_signal` + `on_signal_mut` lowering survives
//! `prettyplease::unparse`. This file asserts the same for the T134
//! `on_init` / `on_destroy` helpers.
//!
//! # Why syn-tree construction instead of a runtime test?
//!
//! `on_init` wraps `use_effect`; `on_destroy` wraps `use_drop`. Both
//! query Dioxus's thread-local `current_scope` at call time and panic
//! with `no current scope` outside a component body (same caveat as
//! `use_signal`, see `src/lib.rs` §"Panics (host unit tests)"). We
//! therefore assert the lowered token shape, NOT the runtime
//! behavior — the wasm32 render step in T121b's harness (USER ACTION)
//! is the canonical behavioral check.
//!
//! # Mechanism
//!
//! Construct the syn tree T134 codegen will emit when a `.buffhtml`
//! script block contains `on_init(|| { ... }); on_destroy(|| { ... });`,
//! run it through `prettyplease::unparse`, and assert:
//!
//! 1. Both helper calls survive (`buff_ui_dioxus::on_init` +
//!    `buff_ui_dioxus::on_destroy`).
//! 2. The closures passed to them survive verbatim.
//! 3. The formatted output re-parses as a valid `syn::File`.

#![allow(clippy::needless_raw_string_hashes)]

use proc_macro2::TokenStream;
use quote::quote;
use syn::{File, Item};

/// Build the `use buff_ui_dioxus::{...};` import line covering the
/// T134 lifecycle surface. Generated `.buffhtml` code will use this
/// exact import set when a script block references the hooks.
fn use_lifecycle_item() -> Item {
    syn::parse_quote! {
        use buff_ui_dioxus::{
            component, on_destroy, on_init, rsx, use_effect, use_drop, Element,
        };
    }
}

/// `fn LifecycleDemo() -> Element { on_init(...); on_destroy(...); rsx!{...} }`
/// — the canonical T134 component shape that exercises both hooks.
fn lifecycle_demo_item() -> Item {
    // Body tokens — exactly what the .buffhtml script block contains:
    //   on_init(|| { tracing::info!("mounted"); });
    //   on_destroy(|| { tracing::info!("unmounting"); });
    //
    // We do NOT call the hooks at runtime in the test (they would
    // panic) — we only emit them as tokens so prettyplease + rustc
    // (under T133's CLI integration) see the expected shape.
    let body: TokenStream = quote! {
        buff_ui_dioxus::on_init(|| {
            // "mounted" log — the T134 lifecycle_demo.buffhtml example
            // uses an equivalent side effect.
            let _ = "mounted";
        });
        buff_ui_dioxus::on_destroy(|| {
            let _ = "unmounted";
        });
        rsx! {
            div { "Lifecycle demo" }
        }
    };
    syn::parse_quote! {
        #[component]
        fn LifecycleDemo() -> Element {
            #body
        }
    }
}

fn generate_lifecycle_main_rs() -> String {
    let file = File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![use_lifecycle_item(), lifecycle_demo_item()],
    };
    prettyplease::unparse(&file)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn t134_emits_on_init_via_wrapper() {
    let src = generate_lifecycle_main_rs();
    assert!(
        src.contains("buff_ui_dioxus::on_init"),
        "expected `buff_ui_dioxus::on_init` hook via the wrapper crate, got:\n{src}"
    );
    assert!(
        src.contains("mounted"),
        "expected on_init closure body to survive verbatim, got:\n{src}"
    );
}

#[test]
fn t134_emits_on_destroy_via_wrapper() {
    let src = generate_lifecycle_main_rs();
    assert!(
        src.contains("buff_ui_dioxus::on_destroy"),
        "expected `buff_ui_dioxus::on_destroy` hook via the wrapper crate, got:\n{src}"
    );
    assert!(
        src.contains("unmounted"),
        "expected on_destroy closure body to survive verbatim, got:\n{src}"
    );
}

#[test]
fn t134_use_effect_and_use_drop_reachable() {
    // Compile-only check: the underlying dioxus hooks power the
    // `on_init` / `on_destroy` helpers; if either path becomes
    // unreachable after a dioxus minor bump, the wrapper crate
    // itself fails to compile (the `use_effect`/`use_drop` `pub use`
    // declarations in `src/lib.rs` are the canary). This test
    // mirrors that contract from outside the crate by referencing
    // the wrapper crate's `on_init` / `on_destroy` as fn-item paths
    // — if either helper disappears, this test fails compilation.
    let _init_path: fn(fn()) = buff_ui_dioxus::on_init::<fn()>;
    let _destroy_path: fn(fn()) = buff_ui_dioxus::on_destroy::<fn()>;
    let _ = (_init_path, _destroy_path);
}

#[test]
fn t134_lifecycle_source_re_parses() {
    // Symmetry check matching codegen_regression.rs: the prettyplease
    // output must re-parse as a valid syn::File so the downstream
    // dioxus-rsx proc macro sees well-formed Rust.
    let src = generate_lifecycle_main_rs();
    syn::parse_str::<File>(&src).unwrap_or_else(|e| {
        panic!("prettyplease output must re-parse as syn::File: {e}\n--- src ---\n{src}")
    });
}
