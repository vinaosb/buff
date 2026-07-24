# Chapter 5 — Build a UI App

Buff compiles not just to native binaries but also to **WebAssembly** for the
browser. The UI story is `.buffhtml` — a single-file component format (SFC)
that pairs a Buff `<script>` block with an RSX `<template>` and an optional
`<style>`, then lowers the whole thing to a [Dioxus 0.7][dioxus] component
that runs in `wasm32-unknown-unknown`.

[dioxus]: https://dioxuslabs.com/

In this chapter you'll learn:

- the three-section `.buffhtml` SFC shape (`<script>` + `<template>` +
  `<style>`),
- RSX syntax (the JSX-like template language),
- reactive state with `use_signal`,
- the `{#each}` and `{#if}` control-flow directives,
- typed props and component composition,
- lifecycle hooks (`on_init`, `on_destroy`),
- the `buff ui dev` hot-reload server and `buff ssr` for server-side rendering.

By the end you'll have a working todo-list component and know how the
`.buffhtml` pipeline fits alongside the `.buff` pipeline you've been using.

## 5.1 Why a separate format?

The `.buff` language is layout-sensitive (indentation defines blocks) and
optimized for imperative logic. HTML templates are the opposite — they're
tree-structured, tag-delimited, and interleaved with text. Trying to shove
both into one grammar produces awkward hybrids (PHP, JSX-in-Python, etc.).

Buff's answer is **Option C** (decided in
[`rsx-syntax-feasibility.md`](https://github.com/buff-lang/buff/blob/v1x-frameworks/.sisyphus/decisions/rsx-syntax-feasibility.md)):
a *parallel* file format (`.buffhtml`) with its own lexer and parser, lowered
to the same Dioxus target. This keeps the `.buff` grammar clean and the
`.buffhtml` grammar HTML-native. The two pipelines share the back-end
(`buff-lang-codegen-rust` → `rustc`) but not the front-end.

```
.buff source              .buffhtml SFC
    │                          │
    ▼                          ▼
buff-lang-parser         buff-lang-buffhtml-parser
    │                  (3-mode lexer + recursive-descent)
    ▼                          │
buff-lang-ast             buff-lang-ast-rsx
    │                          │
    ├──── types analyses ──────┤
    │                          │
    ▼                          ▼
buff-lang-codegen-rust    buff-lang-codegen-buffhtml
    │                  (+ post-format SpanMap side-table)
    ▼                          │
syn::File → prettyplease        ▼
    │                      syn::File → prettyplease
    ▼                          │
native binary             wasm32-unknown-unknown (via Dioxus)
```

The key insight: `.buffhtml` is a *parallel pipeline*, not a modification to
the `.buff` compiler. The two never share a parser; they share only the
back-end and the Dioxus target.

## 5.2 The SFC shape

A `.buffhtml` file has up to three sections, in this order:

```
<script lang="buff">
    // imperative setup — state, effects, handlers
</script>

<div>
    <!-- RSX template — the rendered output -->
</div>

<style>
    /* optional scoped CSS */
</style>
```

- `<script lang="buff">` is required. Its body is spliced into the generated
  component function before the `rsx!{}` expression.
- The RSX template (no wrapper tag — the file *is* the template) is required.
- `<style>` is optional.

Let's build up from the simplest example.

## 5.3 A counter — `use_signal` and event handlers 🔶

From [`examples/counter.buffhtml`](../../examples/counter.buffhtml):

```html
<script lang="buff">
    let mut count = use_signal(|| 0);
    let increment = move |_| {
        count += 1;
    };
</script>

<div class="counter">
    <span>{count}</span>
    <button on:click={increment}>+1</button>
</div>
```

> 🔶 The T133 "floor" of `.buffhtml` ships *Rust-in-script-block pass-through*:
  the script body is spliced verbatim into the generated
  `fn Counter() -> Element { ... }` ahead of the `rsx!{}` expression. Full
  Buff-syntax script-block transpilation (T134+) is in flight. The examples
  here use Rust-compatible script syntax — which is also valid Buff syntax
  for the subset shown.

New ideas:

- **`use_signal(|| 0)`** — Dioxus's reactive state primitive. Returns a
  signal you read with `count` and write with `count += 1`. Reading inside
  the RSX automatically subscribes the template to changes.
- **`move |_| { ... }`** — an event handler closure. `move` captures `count`
  by value (it's a signal handle, cheaply cloneable); `|_|` ignores the event
  payload.
- **`{count}`** — interpolation. Curly braces inside RSX splice a Rust
  expression into the rendered text.
- **`on:click={increment}`** — the `on:` prefix binds a DOM event. The value
  is a Rust closure.

This compiles to a Dioxus component named `Counter`, runs in the browser via
WebAssembly, and re-renders the `<span>` text every time you click the button.

## 5.4 Lists with `{#each}` and conditionals with `{#if}` 🔶

From [`examples/todo_list.buffhtml`](../../examples/todo_list.buffhtml):

```html
<script lang="buff">
    let items: Vec<(String, bool)> = vec![
        ("Learn Buff".to_string(), false),
        ("Write .buffhtml".to_string(), false),
        ("Ship v1.9".to_string(), true),
    ];

    let remaining: usize = items.iter().filter(|t| !t.1).count();
</script>

<div class="todo-list">
    <h2>Todo List ({remaining} remaining)</h2>

    <ul>
        {#each items as todo}
            <li>
                {#if todo.1}
                    <span class="done">{todo.0}</span>
                {:else}
                    <span>{todo.0}</span>
                {/if}
            </li>
        {/each}
    </ul>

    {#if remaining == 0}
        <p>All done!</p>
    {:else}
        <p>You still have work to do.</p>
    {/if}
</div>
```

The control-flow directives:

| Directive | Purpose |
|---|---|
| `{#each items as item}` ... `{/each}` | iterate `items`, binding each to `item` |
| `{#if cond}` ... `{:else if cond}` ... `{:else}` ... `{/if}` | conditional rendering |

> **The `(` rule in `{#each}`.** The `.buffhtml` lexer rejects any `(` inside
> the `{#each}` directive body — that's how it detects the deferred keyed-each
> form `{#each items as item (key)}`. Pre-bind your iterable to a plain local
> (no method-call parens) and reference it bare: `{#each items as todo}`.
> Field access via `todo.0` / `todo.1` works; tuple destructuring in the
> binding position (`as (a, b)`) would trip the `(` check, so use field access
> instead.

## 5.5 Typed props 🔶

A component that takes typed props declares a `struct Props` and references
the `props="Props"` attribute on the `<script>` tag. From
[`examples/typed_props.buffhtml`](../../examples/typed_props.buffhtml):

```html
<script lang="buff" props="Props">
    #[derive(Clone, PartialEq)]
    struct Props {
        name: String,
        count: i32,
    }

    let greeting = format!("Hello, {}! You have {} messages.", name, count);
</script>

<div class="greeting">
    <h2>{greeting}</h2>
</div>
```

The codegen does three things for you:

1. **Hoists** the `struct Props` to module scope.
2. **Switches** the component signature to `fn Greeting(props: Props)`.
3. **Splices** `let Props { name, count, .. } = props;` at function entry, so
   the template body can reference `name` and `count` directly.

The parent renders this component as:

```html
<Greeting name="World" count: 42 />
```

- `name="World"` — a String prop (HTML attribute syntax).
- `count: 42` — a non-string prop (the `:` colon syntax evaluates a Rust
  expression).

The T134 prop pre-checker verifies that every required prop is provided and
the literal's type matches the declared field type.

## 5.6 Composition with slots 🔶

For layout wrappers (cards, panels, dialogs), `.buffhtml` supports named slots.
From [`examples/composition_demo.buffhtml`](../../examples/composition_demo.buffhtml):

```html
<script lang="buff" props="Props">
    #[derive(Clone, PartialEq)]
    struct Props {
        title: String,
    }

    on_init(|| {
        // runs after mount, after the Props destructure
    });
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

- `<slot />` — the default slot (unnamed children).
- `<slot name="header" />` — a named slot. Parents pass slotted content via
  `<template slot="header">...</template>` children.

A parent uses `Card` like:

```html
<Card title="Settings">
    <template slot="header">
        <button>close</button>
    </template>
    <p>Main body content goes here.</p>
    <template slot="footer">
        <button>save</button>
    </template>
</Card>
```

## 5.7 Lifecycle hooks 🔶

Two helpers wrap Dioxus effects:

- **`on_init(closure)`** — runs once after mount. Wraps `use_effect`. Use for
  logging, starting timers, fetching initial data.
- **`on_destroy(closure)`** — runs once on unmount. Wraps `use_drop`. Use for
  cancelling intervals, closing websockets, releasing GPU buffers.

From [`examples/lifecycle_demo.buffhtml`](../../examples/lifecycle_demo.buffhtml):

```html
<script lang="buff">
    let mut mount_count = use_signal(|| 0u32);

    on_init(|| {
        // In a real app: log "mounted", start a timer, fetch data...
    });

    on_destroy(|| {
        // In a real app: cancel intervals, close websockets...
    });

    let increment = move |_| {
        mount_count += 1;
    };
</script>

<div class="lifecycle-demo">
    <h2>Lifecycle Demo</h2>
    <p>This component was remounted {mount_count} times via clicks.</p>
    <button on:click={increment}>Bump mount counter</button>
</div>
```

`on_init` runs only once per mount; `on_destroy` runs only once per unmount.
If a parent toggles the component in and out of the tree, both fire on each
cycle.

## 5.8 The `buff ui dev` hot-reload server

During development, `buff ui dev` starts a WebSocket-equipped dev server that
watches your `.buffhtml` files and live-reloads the browser on every save:

```bash
buff ui dev examples/counter.buffhtml
```

This opens a browser at `http://localhost:XXXX` showing your component. Edit
the `.buffhtml`, save, and the page refreshes automatically — no manual rebuild.

Under the hood:

1. The file watcher detects a change to `counter.buffhtml`.
2. The Buff compiler re-runs the `.buffhtml` pipeline (parse → codegen →
   `wasm32-unknown-unknown`).
3. A WebSocket message tells the browser to reload the new WASM module.

The dev server is implemented in
[`crates/buff-lang-cli/src/ui_dev/`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-lang-cli/src/ui_dev).
It's both a module root *and* the `buff ui dev` command handler.

## 5.9 Server-side rendering with `buff ssr`

For SEO-friendly HTML or fast first paint, `buff ssr` renders a `.buffhtml`
component to an HTML string on the server via [`dioxus-ssr`][dioxus-ssr]:

[dioxus-ssr]: https://crates.io/crates/dioxus-ssr

```bash
buff ssr examples/counter.buffhtml
```

This produces the static HTML the component *would* render with `count = 0`
(the initial signal value). Pair it with client-side hydration (the `buff ui
dev` output) for a full isomorphic app: the server ships meaningful HTML
immediately, the WASM bundle takes over and adds interactivity once loaded.

## 5.10 Desktop apps with Tauri

For a native desktop binary (not a browser tab), `buff ui new --desktop`
scaffolds a [Tauri](https://tauri.app/) project that wraps your `.buffhtml`
components in a native window. The component pipeline is identical; only the
shell differs (a Rust/Tauri main binary instead of a browser). This shipped
in v1.8 (T131).

```bash
buff ui new my_app --desktop
cd my_app
buff ui dev src/main.buffhtml   # iterate in a native window
buff build --release            # ship a native .app / .exe / .deb
```

## 5.11 The full todo app 🔶

Putting it all together — typed props, slots, `{#each}`, `{#if}`, and a
lifecycle hook. This is a parent `App` component that renders a `TodoList`
child:

```html
<!-- App.buffhtml -->
<script lang="buff">
    let mut items: Vec<(String, bool)> = vec![
        ("Write chapter 5".to_string(), false),
        ("Review PRs".to_string(), false),
        ("Ship the book".to_string(), true),
    ];

    let mut draft = use_signal(|| "".to_string());
    let remaining = items.iter().filter(|t| !t.1).count();

    let add = move |_| {
        let text = draft.clone();
        if !text.is_empty() {
            items.push((text, false));
            draft.set("".to_string());
        }
    };

    let toggle = move |i: usize| {
        items[i].1 = !items[i].1;
    };

    on_init(|| {
        // Could log "App mounted" or fetch initial state from an API.
    });
</script>

<div class="app">
    <h1>Todos ({remaining} remaining)</h1>

    <form on:submit={move |e| { e.prevent_default(); add(()); }}>
        <input value="{draft}" on:input={move |e| draft.set(e.value())} />
        <button type="submit">Add</button>
    </form>

    <ul>
        {#each items.enumerate() as (i, todo)}
            <li>
                <input
                    type="checkbox"
                    checked={todo.1}
                    on:change={move |_| toggle(i)}
                />
                {#if todo.1}
                    <span class="done">{todo.0}</span>
                {:else}
                    <span>{todo.0}</span>
                {/if}
            </li>
        {/each}
    </ul>

    {#if remaining == 0}
        <p>All done!</p>
    {:else}
        <p>Keep going.</p>
    {/if}
</div>

<style>
    .app { max-width: 480px; margin: 2rem auto; font-family: sans-serif; }
    .done { text-decoration: line-through; color: #888; }
    li { list-style: none; padding: 0.25rem 0; }
</style>
```

This is the shape of every `.buffhtml` app: a `<script>` block that sets up
state and handlers, an RSX template that reads state and binds events, and an
optional `<style>` for presentation.

## 5.12 The SpanMap — reverse error mapping

One subtle but important detail: when the generated Rust has a compile error,
`rustc` reports the error against the *generated* `.rs` file, not your
`.buffhtml`. That's useless to you. To fix it, the `.buffhtml` codegen emits a
**SpanMap side-table** (post-format) that maps generated-Rust byte offsets back
to `.buffhtml` source positions.

So when `rustc` says "error at generated.rs:42:10", the Buff CLI consults the
SpanMap, translates that to `App.buffhtml:17:5`, and surfaces the diagnostic
at the position you actually wrote. This is the same reverse-mapping trick
every transpiler (TypeScript, Svelte, Sass) needs; Buff's implementation lives
in `buff-lang-codegen-buffhtml`.

## 5.13 What's deferred

The `.buffhtml` surface is growing. As of v1.24:

- ✅ Shipped: 3-mode lexer, recursive-descent parser, RSX template lowering,
  `use_signal` state, `{#each}` / `{#if}`, typed props, named slots,
  `on_init` / `on_destroy`, `buff ui dev` hot-reload, `buff ssr`, Tauri
  desktop scaffolding.
- 🔶 In flight: full Buff-syntax script-block transpilation (today the script
  uses Rust-compatible syntax), keyed-each (`{#each xs as x (key)}`), slot
  fallthrough.
- 📋 Deferred: transition animations, suspense/async boundaries, routing,
  state-management stores (Redux/Zustand equivalents).

The shipped subset is enough to build real interactive UIs. The deferred items
are tracked in the per-crate
[`AGENTS.md`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-ui-dioxus).

## 5.14 Recap

- `.buffhtml` is a **parallel format** to `.buff` — separate lexer + parser,
  shared Dioxus 0.7 + WebAssembly back-end.
- Three sections: `<script lang="buff">` (setup), RSX template (rendered
  output), optional `<style>` (scoped CSS).
- `use_signal(|| initial)` for reactive state. Read bare, write with `+=` /
  `.set(...)`.
- `{#each xs as x}` for iteration, `{#if}` / `{:else if}` / `{:else}` for
  conditionals.
- Typed props via `props="Props"` + a `struct Props` declaration.
- Named slots (`<slot name="...">`) + default slot (`<slot />`) for
  composition.
- `on_init` / `on_destroy` lifecycle hooks wrap `use_effect` / `use_drop`.
- `buff ui dev` for hot-reload, `buff ssr` for server-side rendering,
  `buff ui new --desktop` for native Tauri apps.
- A SpanMap side-table maps generated-Rust errors back to `.buffhtml` source
  positions.

---

*Next: [Chapter 6 — Language Reference](./chapter-6.md)*
