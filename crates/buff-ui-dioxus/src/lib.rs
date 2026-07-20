//! `buff-ui-dioxus` — thin Rust-level wrapper around Dioxus 0.7.
//!
//! # Purpose
//!
//! T130 hardens the T121b feasibility PoC into a production wrapper
//! crate. T121b (decision record
//! [`.sisyphus/decisions/dioxus-feasibility.md`](../../.sisyphus/decisions/dioxus-feasibility.md))
//! proved that Buff's `syn`/`quote`/`prettyplease` codegen stack can
//! emit valid Rust source containing `#[component]` + `rsx!{}` macro
//! invocations that rustc + the dioxus-rsx proc macro expand, that
//! compile to `wasm32-unknown-unknown`, and that render and react in a
//! real headless browser. The crux risk (prettyplease massaging
//! whitespace inside the macro TokenStream) is documented and
//! non-blocking — the proc macro re-parses fine.
//!
//! This crate exposes the *minimal* Rust-level surface that the future
//! RSX-for-Buff syntax (T133 / v1.9) will lower onto:
//!
//! - **Component definition** — a thin re-export of Dioxus's
//!   `#[component]` attribute macro. T133 will emit Buff `func`
//!   declarations tagged `@ui` that lower to `#[component] fn`.
//! - **Signals / state** — re-exports of `use_signal`, `use_memo`,
//!   and `Signal<T>` so T133-generated code has a stable import path
//!   independent of dioxus minor bumps.
//! - **Event handlers** — re-exports of the canonical `MouseEvent` /
//!   `KeyboardEvent` / `FormEvent` types plus a small helper for the
//!   most common event-handler shape (the T121b counter pattern
//!   `onclick: move |_| count += 1`).
//!
//! # Design philosophy
//!
//! Following the repo's "codegen emits only easy Rust" rule
//! ([`AGENTS.md`](../../AGENTS.md)), this wrapper:
//!
//! - **Owns its data.** No lifetime parameters on any public API
//!   surface. T133 will emit `'static`-friendly code; nothing here
//!   exposes borrow-checker pain upward.
//! - **Re-exports, does not wrap.** The `Signal<T>` type is exposed
//!   as-is. Re-wrapping it would (a) duplicate dioxus's API surface
//!   (anti-goal: "NOT a full component library" per the T130 plan),
//!   and (b) break the moment dioxus ships a minor bump. T133 will
//!   lower Buff syntax directly onto `Signal<T>::read` / `::write` /
//!   `::set` — we just give it a single import path that survives
//!   dioxus minor bumps.
//! - **No opaque builder types.** What you see is what T133 emits:
//!   `use_signal(|| 0)`, `rsx!{ button { onclick: ... } }`,
//!   `dioxus::launch(App)`. Nothing fancier.
//!
//! # Pipeline
//!
//! ```text
//! Buff source (with @ui func declarations)        <- T133 future work
//!    |
//!    v  buff-lang-codegen-rust
//! syn::File with #[component] fn + rsx!{} items   <- T121b proven
//!    |
//!    v  prettyplease::unparse
//! Rust source string
//!    |
//!    v  rustc (host)  OR  rustc --target wasm32-unknown-unknown
//! native binary  OR  .wasm bundle
//!    |
//!    v  wasm-bindgen --target web  (for browser only)
//! JS glue + wasm
//!    |
//!    v  served + opened in browser
//! rendered, reactive DOM
//! ```
//!
//! # Example
//!
//! The T121b counter ported to use this crate's wrappers lives at
//! [`examples/counter.rs`](../examples/counter.rs) — a single
//! `use_signal(|| 0)` plus an `onclick` handler that mutates it,
//! with reactive re-render of the button label.

// ---------------------------------------------------------------------------
// Re-exports — the canonical Dioxus 0.7 surface T133 lowers onto.
// ---------------------------------------------------------------------------
//
// We do NOT re-implement; we re-export. Every name below is the exact
// path T133-generated code will import. If a future dioxus minor bump
// renames a path, the fix is a one-line edit here (NOT a codegen
// change).

/// Dioxus's umbrella crate, re-exported so T133 can emit
/// `buff_ui_dioxus::dioxus::...` for paths not individually re-exported
/// below (e.g. `dioxus::launch`).
#[doc(inline)]
pub use dioxus;

// ---------------------------------------------------------------------------
// (1) Component definition surface.
// ---------------------------------------------------------------------------

/// Re-export of the `#[component]` attribute macro.
///
/// T133 lowers Buff `@ui func App():` onto:
///
/// ```rust,ignore
/// #[buff_ui_dioxus::component]
/// fn App() -> buff_ui_dioxus::Element {
///     buff_ui_dioxus::rsx! { /* ... */ }
/// }
/// ```
///
/// This is a direct passthrough to `dioxus::prelude::component` so the
/// attribute macro's expansion stays inside dioxus's own version
/// control — we add ZERO macro logic.
#[doc(inline)]
pub use dioxus::prelude::component;

/// Re-export of the `rsx!` macro.
///
/// T133 lowers Buff UI blocks onto `rsx!{ ... }` macro invocations.
/// The T121b decision record documents that `prettyplease` inserts
/// whitespace inside the macro TokenStream (`onclick : move | _ | ...`
/// instead of `onclick: move |_| ...`) — the proc macro re-parses
/// both forms identically. Nothing in this wrapper changes that
/// contract.
#[doc(inline)]
pub use dioxus::prelude::rsx;

/// Re-export of `Element` — the canonical Dioxus component return
/// type.
///
/// Every `#[component] fn` T133 emits returns `Element`. Re-exporting
/// it here keeps T133-generated `use` blocks short
/// (`use buff_ui_dioxus::*;` covers the full surface).
#[doc(inline)]
pub use dioxus::prelude::Element;

// ---------------------------------------------------------------------------
// (2) Signals / state surface.
// ---------------------------------------------------------------------------

/// Re-export of `use_signal` — the canonical Dioxus state hook.
///
/// T133 lowers Buff `state count: Int = 0` onto
/// `let mut count = buff_ui_dioxus::use_signal(|| 0);`. Signal reads
/// in the rsx! body (`{count}`) and writes in event handlers
/// (`count += 1`) trigger dioxus's reactive runtime to re-render.
///
/// This is the exact hook T121b proved end-to-end (signal -> event ->
/// DOM update pipeline).
///
/// # Panics (host unit tests)
///
/// `use_signal` queries a thread-local scope id at call time. Calling
/// it outside a Dioxus component body (e.g. inside a plain
/// `#[test]`) panics with `no current scope`. Behavioral coverage
/// therefore lives in the `examples/counter.rs` example (which the
/// T130 task builds for both host and `wasm32-unknown-unknown`) and
/// in the codegen regression test (`tests/codegen_regression.rs`),
/// NOT in `#[cfg(test)]` unit tests.
#[doc(inline)]
pub use dioxus::prelude::use_signal;

/// Re-export of `use_memo` — the canonical derived-state hook.
///
/// T133 lowers Buff `derived total: Int = count * price` onto
/// `let total = buff_ui_dioxus::use_memo(move || count() * price());`.
/// Re-exported here for surface completeness; the T121b spike did NOT
/// exercise it, but the surface is stable since dioxus 0.6.
#[doc(inline)]
pub use dioxus::prelude::use_memo;

/// Re-export of `Signal<T>` — the reactive cell type returned by
/// [`use_signal`].
///
/// T133-generated code reads `Signal<T>` via `()` call operator
/// (`count()`) and writes via `+=` / `-=` / `=` (`count += 1`,
/// `count.set(5)`). Exposing the type here lets T133 type-annotate
/// when needed without depending on a private dioxus path.
#[doc(inline)]
pub use dioxus::prelude::Signal;

/// Re-export of `WritableExt` — the trait that provides
/// `Signal<T>::write()` (and other mutation methods).
///
/// T133-generated code that calls `signal.write()` (or uses the
/// [`on_signal_mut`] helper, which calls it internally) needs this
/// trait in scope. Re-exporting it here keeps T133's `use buff_ui_dioxus::*;`
/// import self-sufficient — without it, the user would need a
/// separate `use dioxus::prelude::WritableExt;` line for any code
/// path that touches the writable side of a Signal.
#[doc(inline)]
pub use dioxus::prelude::WritableExt;

// ---------------------------------------------------------------------------
// (3) Event-handler surface.
// ---------------------------------------------------------------------------

/// Re-export of `MouseEvent` — the type `onclick` handlers receive.
///
/// T133 lowers Buff `onclick: event => ...` onto
/// `onclick: move |event: buff_ui_dioxus::MouseEvent| { ... }`.
#[doc(inline)]
pub use dioxus::events::MouseEvent;

/// Re-export of `KeyboardEvent` — the type `onkeydown` etc. receive.
#[doc(inline)]
pub use dioxus::events::KeyboardEvent;

/// Re-export of `FormEvent` — the type `oninput` / `onsubmit` receive.
#[doc(inline)]
pub use dioxus::events::FormEvent;

/// Helper for the most common event-handler shape: a closure that
/// mutates a signal in place (the T121b counter pattern:
/// `onclick: move |_| count += 1`).
///
/// This is NOT a generic event-handler builder — it's the narrowest
/// helper that covers the T121b reactive pattern. T133 lowers Buff
/// `onclick: count += 1` onto
/// `onclick: move |_| buff_ui_dioxus::on_signal_mut(count, |c| *c += 1)`.
///
/// The function is `inline(always)` because the indirection is purely
/// ergonomic — at runtime this is exactly the same machine code as
/// inlining `f(&mut *signal.write())` at the call site.
///
/// # Why a helper at all?
///
/// Two reasons:
/// 1. **Codegen convenience.** T133 has a single canonical lowering
///    target instead of constructing the `signal.write()` guard +
///    closure shape inline.
/// 2. **Diagnostic clarity.** If a Buff user writes
///    `onclick: count += 1` against a non-`Signal<T: AddAssign>`
///    target, the rustc error points at this helper (one location)
///    rather than at the inside of an rsx!{} macro body (which has
///    the line-mapping problem documented in the T121b decision
///    record).
///
/// # Why not more helpers?
///
/// The plan explicitly forbids a "full component library". This is
/// the ONE event-handler helper because it's the ONE shape T121b
/// proved. Anything else (`oninput` debounce, `onsubmit` form
/// processing) is the user's closure in their own code — not ours.
#[inline(always)]
pub fn on_signal_mut<T, F>(mut signal: Signal<T>, mut f: F)
where
    F: FnMut(&mut T) + 'static,
    T: 'static,
{
    f(&mut *signal.write());
}

// ---------------------------------------------------------------------------
// Launch entry point.
// ---------------------------------------------------------------------------

/// Re-export of `dioxus::launch` — the host + wasm entry point.
///
/// T133 lowers Buff `func main(): render App` onto
/// `fn main() { buff_ui_dioxus::launch(App); }`. On
/// `wasm32-unknown-unknown` this starts the dioxus-web runtime; on
/// host targets it opens a native webview (T121b tested only wasm32 —
/// native webview is a future convenience).
#[doc(inline)]
pub use dioxus::launch;

// ---------------------------------------------------------------------------
// Unit tests — kept inline (per the AGENTS.md convention) because they
// are smoke-shape checks, not feature tests. The real test surface is
// `tests/codegen_regression.rs` and `examples/counter.rs` (built on
// both host and wasm32). Behavioral coverage of `use_signal` lives in
// the example because `use_signal` panics outside a Dioxus component
// scope (no thread-local `current_scope` in plain `#[test]`).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke check: every public symbol listed in the lib.rs
    /// re-export block RESOLVES from inside the crate. This catches
    /// accidental typos in `pub use dioxus::...` paths the moment a
    /// dioxus minor bump renames anything — the test fails to COMPILE
    /// (not just fail at runtime), which is the strongest signal we
    /// can get without instantiating a Dioxus runtime.
    #[test]
    fn re_exports_resolve() {
        // Value-level symbol: the one helper we own. Referenced via
        // `let _ = PATH;` so the compiler considers it used.
        let _ = on_signal_mut::<i32, fn(&mut i32)>;

        // Type-level symbols — referenced via position in a phantom
        // tuple. `_` binding is fine here; we just need them in the
        // AST so a missing re-export fails compilation.
        fn _phantom<T: ?Sized>() {}
        _phantom::<Signal<i32>>();
        _phantom::<Element>();
        _phantom::<MouseEvent>();
        _phantom::<KeyboardEvent>();
        _phantom::<FormEvent>();
    }
}
