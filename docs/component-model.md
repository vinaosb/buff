# Buff Component Model (v1.9 / T134)

This guide covers Buff's `.buffhtml` component model as shipped by T133
(floor grammar + 6 stretch features) and T134 (lifecycle hooks + typed
props + prop pre-checker + this guide). For the syntax decision record,
see [`.sisyphus/decisions/rsx-syntax-feasibility.md`](../.sisyphus/decisions/rsx-syntax-feasibility.md).

**Prerequisites:** familiarity with the T133 floor grammar (elements,
attributes, `{}` interpolation, `{#if}` / `{#each}` / fragments /
slots). See [`examples/counter.buffhtml`](../examples/counter.buffhtml)
and [`examples/todo_list.buffhtml`](../examples/todo_list.buffhtml) for
the floor in action.

---

## 1. Declaring a component

A `.buffhtml` file is itself a component. The file's basename is the
component's tag name (Svelte/Vue convention — capitalized tags are
components, lowercase tags are host elements). The minimal component
has no props and no script:

```html
<!-- Hello.buffhtml -->
<div>Hello, world!</div>
```

A parent invokes it as `<Hello />`. Buff codegen lowers this to:

```rust,ignore
use dioxus::prelude::*;
#[component]
fn Hello() -> Element {
    rsx! { div { "Hello, world!" } }
}
```

---

## 2. The props interface (T134)

Components that accept props declare their interface via the
`props="..."` attribute on `<script>`:

```html
<!-- Greeting.buffhtml -->
<script lang="buff" props="Props">
    #[derive(Clone, PartialEq)]
    struct Props {
        name: String,
        count: i32,
    }

    let greeting = format!("Hello, {name}! You have {count} messages.");
</script>

<div>{greeting}</div>
```

### What codegen does

When `props="Props"` is declared, `buff-lang-codegen-buffhtml`:

1. **Parses the script body** as a Rust block (statements list).
2. **Hoists all top-level items** (the `struct Props { ... }` and any
   `use` imports) to module scope.
3. **Auto-generates the destructure** `let Props { name, count, .. } = props;`
   as the FIRST body statement — the script body's own statements
   can then reference `name` and `count` directly.
4. **Switches the signature** to `fn Greeting(props: Props) -> Element`.

The full generated Rust for the example above:

```rust,ignore
use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
struct Props {
    name: String,
    count: i32,
}

#[component]
fn Greeting(props: Props) -> Element {
    let Props { name, count, .. } = props;
    let greeting = format!("Hello, {name}! You have {count} messages.");
    rsx! { div { {greeting} } }
}
```

### Why option (a)?

T134 considered three syntaxes for declaring the props interface:

| Option | Syntax | Verdict |
|---|---|---|
| **(a) `props="..."` attribute** ✅ | `<script lang="buff" props="Props">` | **CHOSEN** — reuses Rust struct syntax, self-documenting via attribute name, backward-compatible (no `props` → T133 floor behavior). |
| (b) `{#props count: Int, name: String}` directive | `{#props count: i32, name: String}` | Rejected — invents a new directive for what Rust already expresses via `struct`. |
| (c) convention: auto-derive from `struct Props` | (no attribute, scan script body for `struct Props`) | Rejected — magic; the explicit attribute makes the contract visible. |

The chosen syntax makes "this component has a typed interface" an
obvious, searchable property of the file (one grep finds every
typed-prop component) while keeping the struct declaration in plain
Rust (no new parser surface).

---

## 3. Lifecycle hooks (T134)

`buff-ui-dioxus` exposes two lifecycle helpers, available to any
`.buffhtml` script block:

| Helper | Fires | Dioxus 0.7 backing API |
|---|---|---|
| `on_init(\|\| { ... })` | Once after the component mounts | `use_effect` (wrapped: drains an `Option<F>` so it fires exactly once even if the effect re-runs) |
| `on_destroy(\|\| { ... })` | Once when the component unmounts | `use_drop` (Dioxus's scope-cleanup hook) |

### Example

```html
<!-- Timer.buffhtml -->
<script lang="buff">
    on_init(|| {
        // Start a timer, fetch data, log "mounted", ...
    });

    on_destroy(|| {
        // Cancel the timer, close the socket, release GPU buffers, ...
    });
</script>

<div>{elapsed}</div>
```

### Why these names?

`on_init` / `on_destroy` were chosen for **symmetry** (one conceptual
pair rather than `use_effect` + `use_drop`, which Dioxus exposes under
unrelated names). The wrappers are thin: `on_init` is 4 lines of code
on top of `use_effect`, `on_destroy` is a one-line alias for
`use_drop`. The full surface stays small — see
[`crates/buff-ui-dioxus/src/lib.rs`](../crates/buff-ui-dioxus/src/lib.rs)
§"Lifecycle-hook helpers".

---

## 4. Composition

Components compose by passing children and named slots. T133 shipped
both default slots (`<slot />`) and named slots (`<slot name="x" />`)
as stretch features; T134 builds on that with typed props:

```html
<!-- Card.buffhtml -->
<script lang="buff" props="Props">
    #[derive(Clone, PartialEq)]
    struct Props {
        title: String,
    }

    on_init(|| { /* "Card mounted" */ });
</script>

<div class="card">
    <div class="card-header">
        <h3>{title}</h3>
        <slot name="header" />
    </div>
    <div class="card-body">
        <slot />
    </div>
    <div class="card-footer">
        <slot name="footer" />
    </div>
</div>
```

A parent composes it as:

```html
<Card title: "Dashboard">
    <template slot="header"><small>Admin view</small></template>
    <p>Welcome to your dashboard.</p>
    <template slot="footer"><small>© 2026</small></template>
</Card>
```

See [`examples/composition_demo.buffhtml`](../examples/composition_demo.buffhtml)
for the full working example.

---

## 5. Prop type pre-checker (T134)

`buff-lang-codegen-buffhtml` ships a new pass — `prop_check` — that
runs **after parse + codegen, before rustc**. For every
`<Component prop: value />` invocation in a parent template, the
checker validates against the child component's declared interface:

| Check | Diagnostic kind | Example |
|---|---|---|
| Required prop missing | `MissingRequired` | `<Greeting />` when `Greeting` requires `name` |
| Unknown prop provided | `UnknownProp` | `<Greeting foo: 1 />` when `Greeting` has no `foo` field |
| (Stretch) literal-type mismatch | `TypeMismatch` | `<Greeting name: 42 />` when `name` is declared `String` |

### Architecture

1. The CLI walks every `.buffhtml` file in scope and calls
   [`extract_interface`](../crates/buff-lang-codegen-buffhtml/src/prop_check.rs)
   to build a `PropInterfaceRegistry`.
2. For each parent template, the CLI calls
   [`check_props`](../crates/buff-lang-codegen-buffhtml/src/prop_check.rs)
   with the parent's AST + the registry.
3. The returned `Vec<PropCheckDiagnostic>` carries `.buffhtml` spans
   directly on each diagnostic. The existing `SpanMap` infrastructure
   (T133 span-mapping spike, verdict PASS — see
   `.sisyphus/evidence/task-133-span-mapping-spike.txt`) lets the
   error mapper render them as file:line:col diagnostics.

### Backward compatibility

If a child component has **no** declared interface (no `props="..."`
attribute, or the named struct is missing from its script body), the
checker **skips** that component — T133 floor components continue to
work without modification. Spread props (`{...rest}`) also bypass the
checker (their contents are unknowable statically).

### Stretch gaps

T134 ships literal-type matching for the obvious cases
(string/integer/boolean literals against primitive Rust type names).
It does **not** validate:
- Identifiers or call expressions as prop values (deferred — these
  require real type inference, future work).
- `Option<T>` opt-out (every declared field is treated as required;
  `Option<T>` opt-out is T135+).

---

## 6. Examples

The four new `.buffhtml` examples shipped by T134:

| Example | Exercises |
|---|---|
| [`examples/lifecycle_demo.buffhtml`](../examples/lifecycle_demo.buffhtml) | `on_init` + `on_destroy` + `use_signal` interplay |
| [`examples/typed_props.buffhtml`](../examples/typed_props.buffhtml) | `props="Props"` declaration + parent invocation |
| [`examples/composition_demo.buffhtml`](../examples/composition_demo.buffhtml) | Typed props + named slots + child composition |
| [`examples/todo_app.buffhtml`](../examples/todo_app.buffhtml) | Typed props + lifecycle + `{#each}` + `{#if}` + state |

Each example is parseable + codegens cleanly under the T134 contract
(verified by `cargo test -p buff-lang-codegen-buffhtml`). Live wasm32
render is a **USER ACTION** — see the T130 render recipe at
[`.sisyphus/evidence/task-130-dioxus-render-USER-ACTION.txt`](../.sisyphus/evidence/task-130-dioxus-render-USER-ACTION.txt).

---

## 7. Cross-references

- **Decision record** (the why): [`.sisyphus/decisions/rsx-syntax-feasibility.md`](../.sisyphus/decisions/rsx-syntax-feasibility.md)
  — §6 explicitly defers the prop pre-checker to "T134+"; this guide
  documents the delivered shape.
- **T133 syntax examples** (the floor grammar): §3 of the decision
  record — 14 examples covering the floor + reserved-but-deferred
  constructs. The T133 stretch (named slots, keyed each, spread props,
  two-way binding, await, `{@html}`) all shipped — see
  [`crates/buff-lang-ast-rsx/src/lib.rs`](../crates/buff-lang-ast-rsx/src/lib.rs)
  for the full AST inventory.
- **Runtime wrapper**: [`crates/buff-ui-dioxus/src/lib.rs`](../crates/buff-ui-dioxus/src/lib.rs)
  — surfaces `component`, `rsx!`, `Element`, `use_signal`, `use_memo`,
  `use_effect`, `use_drop`, `Signal`, the event types, and the
  T134 `on_init` / `on_destroy` helpers.
- **Codegen + pre-checker**: [`crates/buff-lang-codegen-buffhtml/src/`](../crates/buff-lang-codegen-buffhtml/src/)
  — `lib.rs` for AST → Rust lowering, `span_map.rs` for the
  post-format span side-table, `prop_check.rs` for the T134
  pre-checker.
