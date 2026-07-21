# buff-ui-dioxus

In-tree wrapper around Dioxus 0.7 for Buff UI applications. T130.

## OVERVIEW

T121b (decision record `.sisyphus/decisions/dioxus-feasibility.md`) proved that Buff's `syn`/`quote`/`prettyplease` codegen stack can emit valid Rust containing `#[component]` + `rsx!{}` macro invocations that compile to `wasm32-unknown-unknown` and render in real browsers. This crate is the thin re-export + Buff-idiomatic API surface that generated code calls into.

Not a component library. Re-exports Dioxus primitives at stable import paths so T133 codegen changes survive dioxus minor bumps (one-line edit here, no codegen change needed).

## STRUCTURE

| File | Lines | Role |
|------|-------|------|
| `src/lib.rs` | 587 | Everything: re-exports, helpers, inline tests. |
| `tests/codegen_regression.rs` | T121b regression: proves syn + prettyplease produce valid `#[component]` + `rsx!{}` that survives proc-macro expansion. |
| `tests/lifecycle_hooks.rs` | T134: asserts `on_init`/`on_destroy` token lowering matches codegen expectations. |

## WHERE TO LOOK

| Task | File |
|------|------|
| Change component API surface | `lib.rs` (re-export block) |
| Add lifecycle hook | `lib.rs` + `buff-lang-codegen-buffhtml` lowering arm |
| Fix dioxus path after minor bump | `lib.rs` (one-line `pub use` edit) |
| Verify codegen regression | `tests/codegen_regression.rs` |

## CONVENTIONS

- **Dioxus is a VENDORED upstream dependency, never a fork.** `dioxus = "0.7"` caret pin at workspace level. Re-test on every minor bump because `dioxus-rsx` proc-macro internals are NOT covered by semver guarantees.
- **Re-exports, not wrappers.** Every public name is a direct `pub use dioxus::...` passthrough. The one owned helper is `on_signal_mut` (the T121b counter pattern `onclick: move |_| count += 1`).
- **No opaque builder types.** What you see is what T133 emits: `use_signal(|| 0)`, `rsx!{ button { onclick: ... } }`, `dioxus::launch(App)`.
- **No lifetime parameters on any public API.** T133 emits `'static`-friendly code. Nothing here exposes borrow-checker pain upward.
- T135 added `render_to_string` via `dioxus-ssr = "0.7"` (workspace pin). Renders a component to HTML on the host (no browser, no wasm). Event handlers ignored during SSR; initial signal values rendered.

## PUBLIC SURFACE

### Component definition
`component` (attribute macro), `rsx!` (macro), `Element` (return type), `launch` (entry point).

### State
`use_signal`, `use_memo`, `use_effect`, `use_drop`, `Signal<T>`, `WritableExt`.

### Events
`MouseEvent`, `KeyboardEvent`, `FormEvent`, `on_signal_mut<T, F>` (inline helper).

### Lifecycle
`on_init(callback)` (runs once after mount), `on_destroy(callback)` (runs on unmount).

### SSR (T135)
`VirtualDom`, `dioxus_ssr`, `render_to_string(root) -> String`.

## PIPELINE

```
.buffhtml source
    |
    v  buff-lang-codegen-buffhtml (parse .buffhtml)
    v  buff-lang-codegen-rust (emit #[component] fn + rsx!{} invocation)
    v  prettyplease::unparse
Rust source string
    |
    v  rustc --target wasm32-unknown-unknown  (or host for SSR)
wasm bundle  (or native binary for SSR)
    |
    v  dioxus-rsx proc-macro expansion
rendered, reactive DOM
```

## DEPS

`dioxus` 0.7 (umbrella, workspace pin), `dioxus-ssr` 0.7 (workspace pin, T135). Dev-deps: `syn`, `quote`, `proc-macro2`, `prettyplease` (all workspace, for regression test).
