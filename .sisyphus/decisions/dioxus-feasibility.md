# Decision: Dioxus codegen feasibility (T121b)

**Date:** 2026-07-19
**Task:** [T121b — Dioxus codegen feasibility spike (UI go/no-go gate)](../../.sisyphus/plans/buff-post-v10-tooling.md#L956)
**Author:** Sisyphus-Junior (spike executor)

---

## VERDICT: **PASS**

Buff's existing `syn`/`quote`/`prettyplease` codegen stack can emit valid
Rust source containing Dioxus macro invocations (`#[component]` + `rsx!{}`)
that rustc + the dioxus-rsx proc-macro expand, that compile to
`wasm32-unknown-unknown`, and that **render and react in a real browser**.
The counter component (signal + `onclick` + reactive re-render) rendered
`Increment (count: 0)` initially and updated to `Increment (count: 1)` after
a headless click — full signal→event→DOM update pipeline proven.

---

## Pinned versions

| Component | Version | Source |
|---|---|---|
| **Dioxus** (the umbrella crate) | **`=0.7.2`** | pinned in spike `Cargo.toml` |
| `dioxus-core`, `-hooks`, `-signals`, `-html`, `-web`, … | `0.7.9` (transitive, resolved by cargo) | `Cargo.lock` |
| `wasm-bindgen` | `0.2.126` | matches the host-installed `wasm-bindgen.exe` |
| rustc toolchain | `1.95.0` (buff's pin) | `rust-toolchain.toml` |
| wasm target | `wasm32-unknown-unknown` | `rustup target list --installed` |

Dioxus 0.7.2 (umbrella) internally depends on the 0.7.x sub-crates; cargo
resolved them to 0.7.9. This is the published-API surface this spike
validates. **v1.8 should pin `dioxus = "0.7"` (caret) at the workspace level
and re-test on every minor bump** — the dioxus-rsx proc-macro internals
are not covered by semver guarantees.

---

## What was proven

### 1. Codegen → macro token-stream survival (the core risk)

The crux question: can `syn`'s `Macro` node carry an arbitrary
`proc_macro2::TokenStream` (built via `quote!`) through
`prettyplease::unparse` without losing or mangling tokens?

**Answer: YES, with one cosmetic caveat.**

The codegen test
[`crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs`](../../crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs)
builds the counter's `rsx!{ button { onclick: move |_| count += 1, "..." } }`
macro by hand:

```rust
let rsx_body: TokenStream = quote! {
    button {
        onclick: move |_| count += 1,
        "Increment (count: {count})"
    }
};
let mac = Macro {
    path: parse_quote!(rsx),
    delimiter: MacroDelimiter::Brace(Default::default()),
    tokens: rsx_body,
    ..
};
```

…then formats it via `prettyplease::unparse(file)`. The output is:

```rust
#[component]
fn App() -> Element {
    let mut count = use_signal(|| 0);
    rsx! {
        button { onclick : move | _ | count += 1, "Increment (count: {count})" }
    }
}
```

**Key finding:** prettyplease inserts **whitespace inside the macro
TokenStream** — `onclick : move | _ | count += 1` (spaces around `:`, `|`,
`_`) instead of the idiomatic `onclick: move |_| count += 1`. **It does
NOT delete, reorder, or re-tokenize the body.** The proc macro receives an
equivalent TokenStream and parses it identically. The 7/7 codegen tests
confirm this; the wasm32 build and live render confirm the proc macro
accepts the prettyplease-massaged form.

This is the single most important de-risking result of the spike.

### 2. End-to-end wasm32 compile

`cargo check --target wasm32-unknown-unknown` and
`cargo build --release --target wasm32-unknown-unknown` both exit 0 on the
generated counter. The dioxus-rsx proc macro runs at compile time and
expands the prettyplease-formatted tokens into a working
`VirtualDom`-rendering function.

### 3. Browser render + reactivity (the QA bar for PASS)

The wasm artifact was post-processed with
`wasm-bindgen --target web` (no `dx`/`trunk`/`wasm-pack` needed — the
already-installed `wasm-bindgen.exe` 0.2.126 suffices). The
resulting JS + wasm bundle was served via Python's `python -m http.server`
and loaded in headless Chrome via Playwright.

| State | Visible button text | DOM |
|---|---|---|
| Initial render | `Increment (count: 0)` | `<div id="main" data-dioxus-id="0"><button data-dioxus-id="1">Increment (count: 0)</button></div>` |
| After `#main button` click | `Increment (count: 1)` | `<button data-dioxus-id="1">Increment (count: 1)</button>` |

This exercises the **full reactive pipeline**: `use_signal(|| 0)` →
`Signal<i32>` → `onclick: move |_| count += 1` mutates the signal →
dioxus's reactive runtime re-renders the button text. Not hello-world.

**Screenshot:** [`.sisyphus/evidence/task-121b-dioxus-counter.png`](../evidence/task-121b-dioxus-counter.png) (post-click state, button reads `Increment (count: 1)`).

### 4. Codegen integration is non-invasive

The spike added exactly ONE test file to the buff repo
([`crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs`](../../crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs))
and ZERO changes to:
- `[workspace.dependencies]` in root `Cargo.toml` (dioxus is NOT a buff
  workspace dep — it lives only in the throwaway spike's own Cargo.toml),
- any `src/` file in any of the 11 buff crates,
- any existing test,
- `rust-toolchain.toml`, `boulder.json`, the plan file.

`cargo build --workspace` continues to exit 0 (verified post-spike).

---

## Throwaway spike artifacts (locations)

Everything outside `crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs` is
disposable. The spike crate lives under `%TEMP%\opencode\dioxus-spike\`
(WINDOWS: `C:\Users\vsbb1\AppData\Local\Temp\opencode\dioxus-spike\`), NOT
inside the buff repo, so it cannot break the workspace.

```
%TEMP%\opencode\dioxus-spike\
├── Cargo.toml                    # [workspace] + [package], pins dioxus = "=0.7.2"
├── rust-toolchain.toml           # pins 1.95.0 to match buff (and reuse wasm32 target)
├── src\
│   ├── main.rs                   # GENERATED by dioxus_t121b.rs test (valid counter)
│   └── broken.rs                 # GENERATED, deliberately broken for error-mapping test
├── dist\                         # wasm-bindgen output (served to headless Chrome)
│   ├── index.html                # minimal harness with <div id="main">
│   ├── dioxus-spike.js           # wasm-bindgen JS glue (70 KB)
│   ├── dioxus-spike_bg.wasm      # the actual wasm binary (839 KB)
│   └── snippets\                 # dioxus-interpreter-js / dioxus-web JS snippets
├── target\wasm32-unknown-unknown\release\dioxus-spike.wasm   # cargo build output
└── httpd.out / httpd.err         # python http.server logs
```

**Inside the buff repo (committed):**
- [`crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs`](../../crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs) — the codegen proof + generator.
- `.sisyphus/decisions/dioxus-feasibility.md` — this file.
- `.sisyphus/evidence/task-121b-dioxus-counter.png` — screenshot.
- `.sisyphus/evidence/task-121b-decision.txt` — short verdict summary.

---

## Error-message quality assessment

**Setup:** the `t121b_writes_broken_variant_for_error_mapping_assessment`
test emits a second `src/broken.rs` containing a deliberately-invalid
attribute (`not_a_real_attr_xyz`) inside the `rsx!` macro body. Running
`cargo check` (host or `--target wasm32-unknown-unknown`) produces:

```text
error[E0425]: cannot find value `not_a_real_attr_xyz` in module `dioxus_elements::button`
 --> src\broken.rs:9:18
  |
9 |         button { not_a_real_attr_xyz : 42, onclick : move | _ | count += 1, "{count}" }
  |                  ^^^^^^^^^^^^^^^^^^^ not found in `dioxus_elements::button`
For more information about this error, try `rustc --explain E0425`.
```

### What works

- ✅ **Rustc/dioxus-rsx diagnostics point at exact line:column in the
  generated `.rs` file** — here `src\broken.rs:9:18`, which is the literal
  column of `not_a_real_attr_xyz` inside the macro body.
- ✅ **The message is readable** — "cannot find value X in module Y" with
  a caret underline. A buff user would see this and immediately understand
  the *kind* of error.
- ✅ **Filename translation already exists** in
  `crates/buff-lang-cli/src/error_mapper.rs` — the `.rs` path in the
  diagnostic can be swapped for the original `.buff` path that produced
  it. So the user sees `app.buff:?:?` not `<tmpdir>/main.rs:?:?`.

### What is hard / unresolved (the #1 real-world risk)

- 🚨 **The `rsx!{}` macro body is an opaque `proc_macro2::TokenStream`.**
  Individual tokens within it DO carry proc_macro2 `Span`s, but those
  spans point at the **generated Rust text** — not at the originating
  Buff AST node. So while rustc says "broken.rs:9:18", that line:col
  refers to a position in `prettyplease`'s *output*, which is downstream
  of any Buff AST span we recorded before formatting.

- 🚨 **The post-`prettyplease` line:col is not pre-computable.** As the
  codegen crate's own docs note
  ([`rust_codegen.rs:46-56`](../../crates/buff-lang-codegen-rust/src/rust_codegen.rs)):
  "prettyplease reformats the tree after construction, so line numbers
  computed pre-format would be wrong." A **post-format text scan** that
  reverse-maps generated-Rust line → Buff AST span is needed and does not
  yet exist.

- 🚨 **Granularity collapse inside macro bodies.** Even with a perfect
  post-format scanner, the rsx! macro body is emitted as a single opaque
  `TokenStream` from Buff's perspective. If a Buff UI block has 50
  elements, ALL rustc errors inside that block would map to the same
  Buff span (the start of the UI block). Users would see "error in your
  view block at line N" without per-element localization.

### Mitigations for v1.8 (concrete replan notes)

Recorded for the v1.8 UI foundation task (T130):

1. **Filename translation only (v1.0-era):** swap `.rs` paths for `.buff`
   in diagnostic output. Cheap, already half-built. Surfacing "error in
   app.buff, generated from line N region" without precise column.
   **Acceptable for an early preview; NOT for v1.8 stable release.**

2. **Post-format line scan:** scan the formatted Rust source for
   `// buff-line:N` comments emitted before each lowered item; build a
   Rust-line → Buff-line map on demand when a diagnostic arrives.
   Moderate complexity, decent UX. **Recommended baseline for v1.8.**

3. **Per-element span tagging:** emit one `rsx!{}` per Buff source line
   (very heavyweight, breaks component locality), OR embed Buff source
   positions as inert string attributes inside `rsx!{}` and post-process
   diagnostics to extract them (hacky, may confuse the proc macro).
   **Defer to v1.9+ unless v1.8 users report pain.**

4. **proc-macro2 Span preservation:** investigate whether the
   `proc_macro2::Span` data on each token in the `quote!`-built
   TokenStream can carry a Buff source-line *byte offset* via
   `Span::call_site()` replacement. This would be the cleanest fix but
   requires non-trivial span-juggling. **Research spike for v1.9.**

---

## "No transpilation precedent" risk (documented honestly)

**Buff is breaking ground.** No known language transpiles to Dioxus from
non-Rust source. The closest precedents are:

- **Kotlin Compose** → has its own compiler plugin, not a transpiler.
- **SwiftUI** → first-party, same story.
- **HTMX / Handlebars / JSX** → template-string-based, not source-to-source
  compiled from a high-level language.

This means there is NO community wisdom on:
- How to map rustc diagnostics back through a transpiler layer (above).
- How to handle Buff-side type checking of `rsx!` element trees (the
  macro does its own type checking post-expansion; Buff's `TypeInferencer`
  would need to learn the dioxus element/attribute type system).
- How to expose dioxus's component model (props, hooks rules) in Buff
  syntax without exposing Rust's `move |_: Event|` closures verbatim.
- How to handle Hot Module Reload (dioxus's `dx serve`) across a
  transpile step that the user doesn't see.

**Implication for v1.8 T130:** budget at least 30% of the task for
"discover-on-contact" problems that have no prior-art solution. The PASS
verdict here removes the *fundamental* feasibility doubt ("can it work at
all?") but does NOT eliminate the *integration* risk ("can we make it
ergonomic?").

---

## Replan notes (IF v1.8 surfaces blockers)

Since the verdict is PASS, no replan is triggered. However, the following
contingencies should be kept in the orchestrator's back pocket:

- **If error-message quality blocks adoption:** defer UI stable release
  to v1.9; ship v1.8 with the filename-translation-only baseline (mitigation
  #1 above) as a "preview" feature.
- **If dioxus upgrades break us:** pin tightly (`dioxus = "=0.7.2"`) and
  migrate only on minor dioxus releases, never on patch. The dioxus-rsx
  proc-macro internal API is the most breakage-prone surface.
- **If wasm bundle size is a problem (839 KB today):** add a
  `dioxus = { version = "0.7", default-features = false, features = ["web"] }`
  profile and investigate `wasm-opt` post-processing. Out of scope for T121b.

---

## Acceptance criteria check

| Criterion (plan L980-987) | Status | Evidence |
|---|---|---|
| Decision record written with PASS/PARTIAL/FAIL verdict | ✅ | this file, top of doc |
| Dioxus version pinned and recorded | ✅ | "Pinned versions" section |
| Counter renders AND reacts to click | ✅ | screenshot + DOM dumps above |
| Generated `.rs` contains `rsx!` token (grep) | ✅ | `Select-String -Pattern "rsx!"` returns 1 match in `src/main.rs` |
| Error-message quality assessed | ✅ | dedicated section above |
| FAIL/PARTIAL replan notes | N/A (PASS) | "Replan notes" section covers contingencies anyway |
| "No transpilation precedent" risk documented | ✅ | dedicated section above |

**QA scenario 1 (happy path): all 8 steps PASS.** Evidence:
`.sisyphus/evidence/task-121b-dioxus-counter.png`.

---

## Reproduction recipe (for v1.8 T130 / future spikes)

```powershell
# 1. Set up MSVC env (host build scripts / proc-macros need it)
$env:LIB="C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64;..."
$env:INCLUDE="C:\BuildTools\VC\Tools\MSVC\14.44.35207\include;..."
$env:PATH="C:\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64;$env:PATH"

# 2. Regenerate the counter main.rs from the buff repo
cd C:\Users\vsbb1\source\repos\buff
cargo test -p buff-lang-codegen-rust --test dioxus_t121b -- --nocapture

# 3. Build for wasm32
cd C:\Users\vsbb1\AppData\Local\Temp\opencode\dioxus-spike
cargo build --release --target wasm32-unknown-unknown --bin dioxus-spike

# 4. Post-process with wasm-bindgen (0.2.126)
wasm-bindgen --out-dir dist --target web `
  target\wasm32-unknown-unknown\release\dioxus-spike.wasm

# 5. Write a minimal dist\index.html with <div id="main"></div> +
#    a <script type="module"> that imports ./dioxus-spike.js and
#    awaits its default({ module_or_path: './dioxus-spike_bg.wasm' }).

# 6. Serve
cd dist
python -m http.server 8765

# 7. Open http://localhost:8765/index.html in any modern browser.
#    Click the button. Counter goes 0 → 1 → 2 …
```
