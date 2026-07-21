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

/// Re-export of `use_effect` — the canonical side-effect hook.
///
/// Runs the closure on mount AND whenever any signal read inside it
/// changes. Used internally by [`on_init`] for the "run once after
/// mount" pattern. Exposed at the wrapper surface so T134+ codegen
/// (and hand-written Rust inside `.buffhtml` script blocks) can reach
/// it without depending on a private dioxus path.
#[doc(inline)]
pub use dioxus::prelude::use_effect;

/// Re-export of `use_drop` — the canonical scope-cleanup hook.
///
/// Schedules a `Drop`-style callback to run when the current
/// component scope is destroyed (unmounted). Used internally by
/// [`on_destroy`]. Re-exported for parity with [`use_effect`].
#[doc(inline)]
pub use dioxus::prelude::use_drop;

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
// (3b) Lifecycle-hook helpers — T134.
// ---------------------------------------------------------------------------

/// `on_init(callback)` — run a closure **once** after the component
/// mounts. Lowers onto Dioxus 0.7's [`use_effect`] internally.
///
/// `.buffhtml` script blocks invoke this directly:
///
/// ```ignore
/// // Greeting.buffhtml
/// <script lang="buff">
///     on_init(|| { /* log "mounted" */ });
/// </script>
/// <div>{name}</div>
/// ```
///
/// # Why wrap `use_effect`?
///
/// `use_effect` accepts `FnMut` and reruns whenever a tracked signal
/// changes. The "init" semantic is "run once after mount," so the
/// wrapper drains an `Option<F>` on the first invocation and becomes
/// a no-op thereafter. This guarantees the user's closure sees its
/// effects fire exactly once even if the component re-renders before
/// unmount.
///
/// # Panics (host unit tests)
///
/// Like [`use_signal`], `use_effect` queries a thread-local scope at
/// call time and panics with `no current scope` outside a Dioxus
/// component body. Behavioral coverage lives in
/// `tests/lifecycle_hooks.rs` (which builds the syn tree the codegen
/// emits, runs it through `prettyplease`, and asserts the lowered
/// tokens), NOT in a runtime `#[test]`.
pub fn on_init<F>(callback: F)
where
    F: FnOnce() + 'static,
{
    let mut slot = Some(callback);
    use_effect(move || {
        if let Some(f) = slot.take() {
            f();
        }
    });
}

// ---------------------------------------------------------------------------
// (3c) Server-side rendering surface — T135.
// ---------------------------------------------------------------------------

/// Re-export of `dioxus::prelude::VirtualDom` — the reactive root that
/// drives both client and server rendering.
///
/// T135's [`render_to_string`] constructs a `VirtualDom` from a component
/// fn pointer, runs [`VirtualDom::rebuild_in_place`] to materialise the
/// initial render, then hands the dom to `dioxus_ssr::render`. Exposed
/// at the wrapper surface so the `buff ssr` CLI subcommand (and any
/// user-authored Rust code) can drive the dom manually for advanced SSR
/// patterns (e.g. streaming, suspense boundaries).
#[doc(inline)]
pub use dioxus::prelude::VirtualDom;

/// Re-export of the `dioxus-ssr` crate.
///
/// Mirrors the [`dioxus`] umbrella re-export: gives T133+ generated code +
/// the `buff ssr` CLI a single import path that survives `dioxus-ssr`
/// minor bumps. The one function [`render_to_string`] calls into is
/// [`dioxus_ssr::render`] — exposed here so advanced users can reach
/// [`dioxus_ssr::Renderer`] (configurable HTML output: pretty-printing,
/// extra indentation, custom element renderers) without adding a second
/// dependency.
#[doc(inline)]
pub use dioxus_ssr;

/// `render_to_string(root)` — render a Dioxus component to an HTML string
/// on the host (no browser, no wasm).
///
/// T135 lowers `buff ssr <file.buffhtml>` onto this helper. The pipeline:
///
/// 1. Parse + codegen the `.buffhtml` file via the existing T133 path
///    (`pipeline::compile_buffhtml_to_rust`) → `syn::File` with a
///    `#[component] fn Counter() -> Element { ... }` item.
/// 2. Splice in an SSR driver `fn main()` that calls
///    `buff_ui_dioxus::render_to_string(Counter)` and prints the result.
/// 3. Compile via `rustc` (host target — no `wasm32-unknown-unknown`).
/// 4. Run the binary; capture stdout; that is the rendered HTML.
///
/// This helper is the *minimal* host-side surface: it constructs a
/// [`VirtualDom`], runs [`VirtualDom::rebuild_in_place`] to materialise
/// the initial render tree (including any `use_signal` / `use_memo`
/// initial values), and hands the dom to [`dioxus_ssr::render`].
///
/// # Event handlers
///
/// `onclick` / `oninput` / ... handlers are **ignored** during SSR — they
/// do not fire (there is no user to click) and do not appear in the
/// output HTML. The initial state of any `use_signal` is rendered as it
/// would appear on first mount. For the T130/T133 counter, this means
/// `Increment (count: 0)` (initial value `0`).
///
/// # Hydration
///
/// To re-attach interactivity in the browser, ship the same component
/// compiled to `wasm32-unknown-unknown` and call `dioxus::launch` (or
/// the lower-level `dioxus-web::hydrate`) against the SSR-rendered DOM.
/// See `.sisyphus/evidence/task-135-hydration-USER-ACTION.txt` for the
/// full hydration recipe (browser build + HTML shell + hydration entry).
///
/// # Example
///
/// ```rust,ignore
/// use buff_ui_dioxus::*;
///
/// #[component]
/// fn App() -> Element {
///     rsx! { div { "Hello, SSR!" } }
/// }
///
/// fn main() {
///     let html = render_to_string(App);
///     assert!(html.contains("Hello, SSR!"));
///     println!("{html}");
/// }
/// ```
///
/// # Panics
///
/// Internally constructs a `VirtualDom` and runs `rebuild_in_place`. If
/// the component body itself panics during initial render (e.g. calls
/// `use_signal(|| panic!("boom"))`), the panic propagates through
/// `rebuild_in_place` and out of this function. Normal Dioxus components
/// (`use_signal(|| 0)`, `use_effect`, `rsx!{}` bodies) do not panic on
/// the initial render path.
pub fn render_to_string(root: fn() -> Element) -> String {
    let mut vdom = VirtualDom::new(root);
    // Materialise the initial render tree. `rebuild_in_place` runs every
    // effect + memo once so signals settle at their initial values; the
    // resulting `VirtualDom` is then safe to hand to `dioxus_ssr::render`.
    vdom.rebuild_in_place();
    dioxus_ssr::render(&vdom)
}

/// `on_destroy(callback)` — run a closure **once** when the component
/// unmounts. Lowers onto Dioxus 0.7's [`use_drop`] (the canonical
/// scope-cleanup hook — Dioxus attaches a `Drop`-implementing guard to
/// the current scope that fires the closure on teardown).
///
/// `.buffhtml` script blocks invoke this directly:
///
/// ```ignore
/// // Timer.buffhtml
/// <script lang="buff">
///     on_destroy(|| { /* cancel interval */ });
/// </script>
/// <div>{elapsed}</div>
/// ```
///
/// # Why a helper at all?
///
/// `use_drop` is already trivially callable as `buff_ui_dioxus::use_drop(f)`.
/// The `on_destroy` alias exists for **symmetry with [`on_init`]** and to
/// give `.buffhtml` script-block authors a single conceptual pair
/// ("init" + "destroy") rather than two unrelated entry points
/// ("init" via helper + "destroy" via a differently-named hook).
///
/// # Panics (host unit tests)
///
/// Same caveat as [`on_init`] — `use_drop` panics outside a Dioxus
/// scope. Coverage is structural (see `tests/lifecycle_hooks.rs`).
pub fn on_destroy<F>(callback: F)
where
    F: FnOnce() + 'static,
{
    use_drop(callback);
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
    // `rsx!` macro expansion needs the dioxus-internal crate aliases
    // (`dioxus_signals`, `dioxus_elements`, ...) in scope — mirroring
    // the dual `use` line in `examples/counter.rs` (without it the
    // macro expansion fails with `use of unresolved module or
    // unlinked crate dioxus_signals`).
    use dioxus::prelude::*;

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

        // T134 lifecycle helpers — referenced as path values so the
        // compiler resolves their monomorphized `fn` item. We do NOT
        // call them (they would panic outside a Dioxus scope — see
        // `on_init` / `on_destroy` docs).
        let _ = on_init::<fn()>;
        let _ = on_destroy::<fn()>;

        // T135 SSR helper — referenced as a path value so a future
        // rename / signature change fails this test at COMPILE time.
        let _ = render_to_string as fn(fn() -> Element) -> String;

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

    /// T135 vertical-slice: render a trivial static component to HTML
    /// via `render_to_string`. Asserts the dioxus-ssr integration works
    /// end-to-end on the host (no wasm, no browser) AND that the
    /// rendered HTML contains the literal text the component's rsx! body
    /// produces.
    ///
    /// This is the *only* host-side runtime behavioral test in this
    /// crate — every other test asserts codegen / re-export shape
    /// because `use_signal` / `use_effect` / `on_init` / `on_destroy`
    /// panic outside a Dioxus scope. SSR's `VirtualDom::new` +
    /// `rebuild_in_place` constructs an internal scope, so the hooks
    /// resolve normally.
    #[test]
    fn t135_render_to_string_emits_static_html() {
        // Local component — defines a fn item that matches the
        // `fn() -> Element` signature `VirtualDom::new` expects. We do
        // NOT use the `#[component]` attribute here because (a) it is
        // not needed for SSR (`VirtualDom::new` accepts a plain fn
        // pointer) and (b) the attribute macro expands differently
        // under `#[cfg(test)]` and would add compilation surface
        // without exercising any additional SSR behavior. The body is
        // the simplest thing that proves end-to-end rendering works.
        fn static_component() -> Element {
            rsx! {
                div { "Hello, SSR!" }
            }
        }

        let html = render_to_string(static_component);
        assert!(
            html.contains("Hello, SSR!"),
            "expected rendered HTML to contain the literal `Hello, SSR!`, got: {html}"
        );
        assert!(
            html.contains("<div"),
            "expected rendered HTML to contain a `<div` open tag, got: {html}"
        );
    }

    /// T135 vertical-slice part 2: render the canonical counter
    /// pattern (`use_signal(|| 0)` + `onclick` handler + interpolation)
    /// via SSR. The event handler is **ignored** during SSR (no user to
    /// click) but the initial signal value IS rendered. This is the
    /// shape `buff ssr <file.buffhtml>` produces against
    /// `examples/counter.buffhtml`.
    ///
    /// Asserts:
    /// 1. The initial count `0` appears in the output (signal resolved).
    /// 2. The button label renders verbatim (text content survives).
    /// 3. The HTML is non-empty (the VirtualDom was built + rebuilt).
    #[test]
    fn t135_render_counter_pattern_to_html() {
        fn counter_component() -> Element {
            let mut count = use_signal(|| 0);
            rsx! {
                button {
                    onclick: move |_| count += 1,
                    "Increment (count: {count})"
                }
            }
        }

        let html = render_to_string(counter_component);
        assert!(
            !html.is_empty(),
            "rendered HTML should be non-empty; got empty string"
        );
        assert!(
            html.contains("Increment"),
            "expected button label to survive SSR; got: {html}"
        );
        assert!(
            html.contains("count: 0"),
            "expected initial signal value (0) in SSR output; got: {html}"
        );
    }
}
