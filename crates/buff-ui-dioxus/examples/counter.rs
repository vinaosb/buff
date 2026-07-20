//! T130 counter example — the T121b counter (`use_signal(|| 0)` +
//! `onclick` + reactive re-render) ported to use the
//! `buff_ui_dioxus` wrappers instead of importing `dioxus` directly.
//!
//! # What this proves
//!
//! This is the *maintained* successor to the throwaway T121b spike
//! crate (which lived under `%TEMP%\opencode\dioxus-spike\`). It:
//!
//! 1. **Builds on host** (`cargo build -p buff-ui-dioxus --example
//!    counter`) — proves the wrappers re-export to the right paths.
//! 2. **Builds for `wasm32-unknown-unknown`** (`cargo build -p
//!    buff-ui-dioxus --example counter --target
//!    wasm32-unknown-unknown`) — proves the wrappers survive the
//!    dioxus-rsx proc-macro expansion in the same way the T121b
//!    spike did.
//! 3. **Reactive pipeline**: `use_signal(|| 0)` initialises the
//!    signal; `onclick` mutates it via `on_signal_mut`; the
//!    `{count}` interpolation in the rsx! body re-renders on every
//!    signal write. The visible label goes `Increment (count: 0)` ->
//!    `Increment (count: 1)` -> ... on every click.
//!
//! # Live render recipe
//!
//! See `.sisyphus/evidence/task-130-dioxus-render-USER-ACTION.txt` for
//! the manual reproduction recipe (build wasm32, post-process with
//! `wasm-bindgen --target web`, serve via `python -m http.server`,
//! open in any modern browser, click the button).

// The wrapper crate's prelude — T133-generated code will emit
// `use buff_ui_dioxus::*;` for the same effect.
//
// NOTE: the second `use` line is NOT redundant. `buff_ui_dioxus::*`
// brings in our wrappers (`component`, `rsx`, `Element`, `use_signal`,
// `on_signal_mut`, `launch`); `buff_ui_dioxus::dioxus::prelude::*`
// brings in the rsx!-macro-INTERNAL crate aliases
// (`dioxus_signals::{self, *}`, `pub use dioxus_core`, `pub use
// dioxus_html as dioxus_elements`). The macro expands to absolute
// paths like `dioxus_elements::button::...` and needs those names
// in scope. This mirrors the T121b spike's `use dioxus::prelude::*;`
// line — the crux name-resolution behavior is unchanged.
use buff_ui_dioxus::dioxus::prelude::*;
use buff_ui_dioxus::*;

fn main() {
    launch(App);
}

/// The counter component.
///
/// Marked `#[buff_ui_dioxus::component]` (which re-exports
/// `#[dioxus::prelude::component]`). Returns the rsx!-built
/// `Element` — Dioxus mounts it at `<div id="main">` in the browser
/// (the dioxus-web default mount point).
#[component]
fn App() -> Element {
    // T121b-proven signal hook. NOTE: `count` itself is not declared
    // `mut` — mutation happens through `on_signal_mut(count, ...)`,
    // which calls `Signal::write()` internally (interior mutability
    // via the reactive runtime). The original T121b spike used
    // `let mut count = ...; count += 1` directly (which requires
    // `mut`); the helper version is the T130-recommended lowering
    // target so the rustc diagnostic for an invalid handler points
    // at `on_signal_mut` (one stable location) instead of inside
    // the rsx! macro body.
    let count = use_signal(|| 0);

    // T121b-proven rsx! macro invocation. `onclick` uses the
    // `on_signal_mut` helper from the wrapper crate (the ONE
    // event-handler helper the plan allows). The button label
    // interpolates the signal via `{count}` — Dioxus re-renders the
    // label on every signal write.
    rsx! {
        button {
            onclick: move |_| on_signal_mut(count, |c| *c += 1),
            "Increment (count: {count})"
        }
    }
}
