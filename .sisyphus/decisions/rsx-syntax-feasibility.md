# Decision Record: RSX-for-Buff Syntax (T133)

**Status:** PROPOSED
**Decision ID:** rsx-syntax-feasibility
**Date:** 2026-07-20
**Author:** Oracle (read-only architecture consultant), synthesized by Atlas
**Plan reference:** `.sisyphus/plans/buff-post-v10-tooling.md` L2092-2101 (v1.9 ⚠️ CRITICAL language-change exception); T121b verdict PASS (commit 6b2235f, `.sisyphus/decisions/dioxus-feasibility.md`)
**Spike agents:** `bg_97317cdc` (librarian — prior-art survey), `bg_00cbabfb` (explore — Buff internals survey), `bg_cafaa555` (Oracle — synthesis)

---

## 1. VERDICT

**C — Separate `.buffhtml` template files (Svelte/Vue-style SFC).**

Not hybrid. Not a spike-more. C is the call.

## 2. Rationale

The decision turns on three load-bearing constraints that A and B violate:

**(a) Lexer/parser integrity.** The brief forbids a lexer or parser rewrite. Option A cannot satisfy this. Buff's `<` and `>` are unambiguous single tokens today (`Lt`/`Gt`); JSX-style embedding requires either context-tracking in the lexer (5+ active contexts for `<`, vs. the 2-context precedent in `regex_context`) or a parser-level `canStartJSXElement` flag like Babel/SWC — both are rewrites in everything but name. SWC's speed advantage over Babel comes from *token-scanning optimizations*, not from avoiding the fundamental context-tracking problem; the `<T,>` generic-arrow workaround (TS issue #47355) is evidence that this ambiguity is structural, not incidental. Option B is less obviously broken but adds a 6th meaning to `:` (already overloaded 5 ways: named args, type annotations, struct fields, struct patterns, layout blocks) and reuses `{}` for JSX children on top of the 4 existing brace contexts (blocks, struct literals, map literals, closures). Option C touches neither token: it introduces a new file format with its own grammar. **The Buff lexer/parser is unchanged.** This is the only option that satisfies the hard constraint.

**(b) Tooling cost scales with language complexity, not file count.** Option A requires a context-sensitive tree-sitter grammar for Buff itself — and there is no prior art for a non-Rust source-to-source Dioxus transpiler. The closest analogs (`tree-sitter-razor`, `svelte2tsx`) are 5-50K LoC efforts, and `svelte2tsx` exists precisely *because* Svelte's tooling couldn't ride on TypeScript's LSP directly. Option A also requires major TextMate scope additions and dual-mode string/JSX interpolation (Buff strings already use `{expr}`; JSX `{count}` collides). Option B has the same tooling cost (context-sensitive grammar for Buff itself). Option C's tooling cost is a *new, simpler* grammar for `.buffhtml` — directly modelable on `tree-sitter-svelte`, which already solves HTML+embedded-language. The LSP is file-extension-routed (`.buffhtml` opens the new language server; `.buff` files are unaffected). Crucially, **C's tooling work is additive, not invasive** — if the `.buffhtml` LSP slips, Buff itself still works.

**(c) Pioneer-risk minimization.** Buff is pioneering regardless: no non-Rust source-to-source Dioxus transpiler exists. The question is *which dimension* Buff pioneers on. Option A pioneers a new JSX variant inside a hand-rolled Pratt parser where `<`/`>` already sit at two precedence levels (L8 comparison, L12 shift) — untested waters with high risk of subtle ambiguity bugs (generics, comparison chains, arrow-returning-tag). Option B pioneers a colon-DSL with weak precedent (Haml is dead-end; Python's indentation is a different problem). Option C pioneers a new SFC format, but the SFC *pattern* is battle-tested (Vue since 2014, Svelte since 2016, millions of production components). The novelty is bounded: HTML + `{}` interpolation + control-flow markers, all with direct precedent. **C confines the pioneer-risk to the format, not the language.**

**Strongest counter-argument and response.** The case against C is developer experience: "component logic and template split across files (or one large SFC) is more friction than co-located `render: <div>...`." This is real but mitigated by three facts. (1) Svelte and Vue developers have accepted SFCs at scale — file-splitting is not a measurable adoption barrier and Svelte's developer-satisfaction rankings are industry-leading. (2) Dioxus components are *already* functions returning `rsx!` — the SFC model maps directly onto Dioxus's existing architecture; we are not fighting the target framework's grain. (3) LSP goto-definition, find-references, and rename work *better* across file boundaries than inside a mixed-syntax file (no scope confusion between markup and code). The DX tax is paid once at authoring; the tooling tax for A/B is paid forever in maintenance and ambiguity bugs.

## 3. Concrete Syntax Proposal

File extension: `.buffhtml`. Format: single-file component (Svelte-style). Optional companion `.buff` file for complex logic (auto-bound by basename).

The `.buffhtml` lexer has exactly three modes: `TEXT`, `BUFF_CODE` (inside `{}`), `BUFF_DIRECTIVE` (inside `{#...}`, `{:...}`, `{/...}`). The HTML structure is parsed by a recursive-descent parser that delegates to Buff's existing expression parser whenever it hits `{...}`.

**Counter component (script + markup):**
```html
<!-- Counter.buffhtml -->
<script lang="buff">
component Counter = fn(props: { initial: Int = 0 }) -> Element:
    count = state(props.initial)
    increment = fn(): count.set(count.get() + 1)
</script>

<div class="counter">
    <span>{count}</span>
    <button on:click={increment}>+1</button>
</div>
```

**Props (named, consistent with Buff §11 named-args rule):**
```html
<Greeting name: "Alice" age: 30 />
```

**Events (Svelte-style directive, with modifiers):**
```html
<button on:click={handleClick}>Click</button>
<form on:submit_prevent={handleSubmit}>{...}</form>
```

**Lists (Svelte-style `{#each}`, keyed for reconciliation):**
```html
<ul>
    {#each items as item, i}
        <li>{i}: {item.name}</li>
    {/each}
</ul>

{#each todos as todo (todo.id)}
    <Todo item: {todo} />
{/each}
```

**Conditionals:**
```html
{#if user.is_admin}
    <AdminPanel />
{:else if user.is_member}
    <MemberPanel />
{:else}
    <GuestPanel />
{/if}
```

**Nesting + fragments (`<>` for fragment roots):**
```html
<>
    <h1>{title}</h1>
    <p>{body}</p>
</>
```

**Interpolation (reuses Buff expression parser — string interpolation model already exists):**
```html
<span>Hello {user.name}, you have {messages.len()} new messages</span>
```

**Component composition + children:**
```html
<Layout>
    <Header />
    <main>{children}</main>
    <Footer />
</Layout>
```

**Slots (named + default):**
```html
<!-- Card.buffhtml -->
<div class="card">
    <slot name="header" />
    <slot />
    <slot name="footer" />
</div>

<!-- Usage -->
<Card>
    <template slot="header"><h1>{title}</h1></template>
    <p>Default slot content</p>
    <template slot="footer"><small>© 2026</small></template>
</Card>
```

**Spread props:**
```html
<Button {...rest} label: "Override" />
```

**Static class concat (interpolation inside attribute):**
```html
<div class="card {active ? 'card-active' : ''} {extra_classes}">
    {body}
</div>
```

**Companion-file variant (Vue-style, for complex logic):**
```html
<!-- Counter.buffhtml (markup only; logic lives in Counter.buff) -->
<div class="counter">
    <span>{count}</span>
    <button on:click={increment}>+1</button>
</div>
```
```rust
// Counter.buff (auto-bound by basename match)
component Counter = fn(props: { initial: Int = 0 }) -> Element:
    count = state(props.initial)
    increment = fn(): count.set(count.get() + 1)
    render_template(__file__, { count, increment })
```

**Comment syntax:**
```html
<!-- HTML comment, NOT rendered -->
{# This is a Buff directive comment, NOT rendered #}
```

These 14 examples cover the v1.9 floor grammar. The implementer can write the grammar directly from them; reserved-but-deferred constructs (two-way `bind=`, `{#await}`, `{@html}`) are explicitly out of scope for T133.

## 4. Compiler Impact

| Component | Impact | Specific change |
|---|---|---|
| `buff-lang-lexer` | **NONE** | Zero changes. Buff tokens unchanged. |
| `buff-lang-parser` | **NONE** | Zero changes. Buff grammar unchanged. |
| `buff-lang-ast` | **MINOR** | Add `RsxTemplateFile` top-level node in a new `buff-lang-ast-rsx` module (or sibling crate). No change to existing `Expr`/`Decl` variants. |
| `buff-lang-codegen-rust` | **NONE** | Untouched. |
| New: `buff-lang-codegen-buffhtml` | **MODERATE** | New crate. Lowers template AST → `rsx!{}` TokenStream via `syn`/`quote`, formats with `prettyplease`. Reuses T121b-proven emission path verbatim. |
| New: `buff-lang-buffhtml-parser` | **MODERATE** | New crate. Hand-rolled recursive-descent + 3-mode lexer. Calls Buff's existing expression parser for `{...}` contents. |
| LSP | **MODERATE** | New language server for `.buffhtml` (file-extension routed). Existing `.buff` LSP unchanged. Cross-file goto (`.buff` ↔ `.buffhtml`) is the only integration point — defer to T135. |
| Tree-sitter | **MODERATE** | New grammar for `.buffhtml`, modelable on `tree-sitter-svelte`. Existing Buff grammar untouched. |
| VSCode extension | **MINOR** | Register `.buffhtml` extension + TextMate scope. Existing Buff grammar unchanged. |
| `buff-lang-types` | **MINOR** | Optional: typecheck expressions inside `{}` using existing inference. No new inference rules. |
| `buff-lang-cli` | **MINOR** | `buff build` / `buff run` recognize `.buffhtml` in `src/`. Extend `buff check`. |

**Net:** zero changes to existing lexer/parser/AST/codegen-rust. All work is additive. The only MODERATE items are two new greenfield crates and the new tree-sitter grammar.

## 5. Risk Register

1. **`.buffhtml` grammar design sprawl.** *Likelihood: MEDIUM. Impact: MEDIUM.* Svelte/Vue grammars accreted features over years; v1.9 may try to match them all at once. **Mitigation:** ship a minimal subset (elements, props, events, `{#each}`, `{#if}`, interpolation, fragments, default slot). Named slots, await, `bind=`, `{@html}` defer to T134+.

2. **LSP cross-file features are genuinely hard.** *Likelihood: MEDIUM. Impact: MEDIUM.* Goto-definition from `.buffhtml` event handler to `.buff` function requires two language servers to coordinate. **Mitigation:** v1.9 ships single-file LSP within `.buffhtml` only. Cross-file features defer to T135. Document the gap.

3. **Error-message span mapping (`.rs` → `.buffhtml`) is unresolved.** *Likelihood: HIGH. Impact: MEDIUM.* T121b already flagged this for `.buff` → `.rs`. C adds one extra hop (`.buffhtml` → `.rs`). **Mitigation:** carry forward T121b's span-preservation approach. Dioxus `rsx!{}` supports `Location` callers — wire them through. Acceptable degradation: errors point to generated `.rs` with a `TODO(buffhtml-span)` marker until proper mapping lands in T134.

4. **Dioxus API drift breaks the SFC contract.** *Likelihood: LOW. Impact: HIGH.* If Dioxus changes `rsx!{}` macro syntax, all generated code breaks. **Mitigation:** pin Dioxus to a specific minor version in v1.9. CI compatibility test re-compiles all examples against the pin.

5. **Component prop type-checking is weak at v1.9.** *Likelihood: HIGH. Impact: LOW.* Without a prop typechecker, mismatches fail at Rust compile time with cryptic errors. **Mitigation:** acceptable for v1.9 (fail-loud at `buff build`). T134 adds a prop-type pre-checker.

6. **Svelte-syntax drift / divergent community conventions.** *Likelihood: MEDIUM. Impact: LOW.* Choosing `{#each}` over Vue's `v-for` is a style call. **Mitigation:** document the rationale (curly-brace directives compose with Buff's existing `{}` interpolation model; no new token types).

7. **Two-file vs single-file SFC ambiguity.** *Likelihood: MEDIUM. Impact: LOW.* Users will want both `Counter.buff` + `Counter.buffhtml` and single-file Svelte-style. **Mitigation:** support both. SFC's optional `<script lang="buff">` block; if absent, look for sibling `Counter.buff` by basename.

8. **Tree-sitter grammar lag blocks syntax highlighting in v1.9.** *Likelihood: MEDIUM. Impact: LOW.* A full grammar takes weeks. **Mitigation:** ship TextMate-only for v1.9 (basic highlighting). Tree-sitter lands in T134.

9. **`{@html}` (when added) and `{...spread}` introduce injection surface.** *Likelihood: LOW. Impact: HIGH.* These bypass Dioxus's auto-escaping. **Mitigation:** default-escape everything. `{@html}` requires explicit opt-in and emits a lint warning by default. Spread props emit a warning if source type is unknown.

## 6. Mitigation / v1.9 Scope Floor

**MUST ship in T133 (the floor — do not cut):**
- `.buffhtml` file format spec: frozen grammar covering elements, attributes, `{}` interpolation, `on:event` handlers, `{#each}`/`{:else}`/`{/each}`, `{#if}`/`{:else if}`/`{:else}`/`{/if}`, fragments `<>...</>`, child component composition, default `<slot />`, comments.
- New `buff-lang-buffhtml-parser` crate (3-mode lexer + recursive-descent).
- New `buff-lang-codegen-buffhtml` crate lowering to `rsx!{}` via T121b-proven path.
- `buff build` and `buff run` recognize `.buffhtml` in `src/`.
- TextMate grammar (basic highlighting; no LSP).
- Two example apps: `counter.buffhtml` and `todo_list.buffhtml`.
- End-to-end test: `.buffhtml` → wasm32 → renders in browser (reuse T121b harness).

**MAY ship in T133 (stretch):**
- Named slots (`<slot name="x" />`).
- Keyed each (`{#each xs as x (x.id)}`).
- Spread props (`{...rest}`).
- Companion-file auto-binding (`Counter.buff` + `Counter.buffhtml`).

**DEFERS to T134+:**
- Two-way binding (`bind={...}`).
- Await blocks (`{#await ...}`).
- `{@html}` escape hatch.
- Full LSP (goto-def, completion, diagnostics).
- Tree-sitter grammar.
- VSCode extension beyond TextMate.
- Prop type pre-checker.
- Cross-file LSP features.

**Hard limit:** if T133 cannot ship the floor in budget, cut stretch items, *not* the floor. If the floor itself is at risk, trigger §7.

## 7. Replan Triggers

Abort T133 mid-implementation and return to this decision if any of:

1. **`.buffhtml` → `rsx!{}` lowering requires `prettyplease` changes or a new codegen backend.** T121b proved `prettyplease` preserves `rsx!{}` TokenStream; if v1.9 discovers edge cases where it mangles templates, C's key advantage evaporates and the whole RSX strategy needs re-evaluation (not just C).

2. **Span mapping from `rsx!{}` errors back to `.buffhtml` proves impossible without compiler hacks.** If Dioxus's `Location` API cannot carry `.buffhtml` source positions, error messages will be unusable. This is the single highest-impact unknown — surface it early with a spike in week 1 of T133.

3. **The `.buffhtml` grammar cannot be parsed in a single forward pass.** If implementation reveals that `{#each}` nesting, component composition, or slot binding requires backtracking or arbitrary lookahead, the hand-rolled parser assumption is violated. Simplify the grammar before re-evaluating C.

4. **v1.9 budget consumed >60% before the floor (§6) is demonstrably working end-to-end.** At that threshold, remaining risk exceeds value of shipping in v1.9. Abort, write up lessons, replan for v1.10.

5. **Dioxus pins a `rsx!{}` breaking change during v1.9 development.** If upstream Dioxus ships `rsx!{}` v2 that invalidates T121b's emission pattern, the entire RSX-for-Buff strategy (not just C) needs re-evaluation — possibly pivoting to direct Dioxus-component emission without the macro.

## 8. Acceptance Criteria (6-month retrospective)

This decision was the right call if, six months after v1.9 ships:

1. The Buff lexer/parser has **not** been modified to support UI syntax. *(Confirms core C advantage held.)*
2. A user can write a non-trivial `.buffhtml` component (≥50 lines, with lists + conditionals + composition) and get a working wasm32 build on first try, with no manual Rust inspection. *(Confirms end-to-end UX.)*
3. Error messages from Rust compilation point to the correct `.buffhtml` source location ≥80% of the time. *(Confirms span mapping landed.)*
4. A tree-sitter grammar exists and powers syntax highlighting + structural editing in VSCode. *(Confirms tooling caught up.)*
5. No v1.9→v1.10 migration required users to rewrite `.buffhtml` files due to grammar changes. *(Confirms floor grammar was correctly chosen.)*
6. Fewer than 10 distinct users have retroactively requested JSX-style inline syntax (Option A). *(Confirms market acceptance.)*

If 5 of 6 hold, the decision was correct. If 4 or fewer hold, schedule a postmortem and reconsider A.

---

## Executive Summary

**Recommendation: Option C (`.buffhtml` separate template files, Svelte/Vue-style SFC).** C is the only option that satisfies the hard constraint of zero lexer/parser changes to Buff itself — A forces a parser-level rewrite for `<`/`>` disambiguation (the `<T,>` generic-arrow pain is structural, not incidental), and B overloads `:` a 6th time while reusing `{}` ambiguously on top of 4 existing brace contexts. C confines pioneer-risk to a new *file format* (with strong Vue/Svelte precedent) rather than the *language* (where Buff would be first-of-kind); all compiler work is additive (two new greenfield crates + new tree-sitter grammar), leaving existing lexer/parser/AST/codegen-rust untouched. Ship a minimal grammar floor in T133 (elements, props, events, `{#each}`, `{#if}`, interpolation, fragments, default slot) and defer LSP/tree-sitter/two-way-binding/await to T134+; the highest-impact unknown is error-span mapping from `rsx!{}` back to `.buffhtml`, which should be spiked in week 1 of T133.

---

## Spike Provenance

This decision record synthesizes three parallel spike agents (all completed 2026-07-20):

- **`bg_97317cdc`** (librarian, 4m54s, session `ses_07dee2274ffe77kZcxXOg8op3H`) — Prior-art survey covering JSX/Babel/SWC, Razor, Svelte, Vue, HTMX, Elm, Halogen, Yew, Dioxus. Key finding: "no transpilation precedent" verified — Buff would be the first non-Rust source-to-source Dioxus transpiler; Yew/Dioxus avoid lexer ambiguity via procedural macros (post-tokenization), which is NOT an option for a source-to-source transpiler.
- **`bg_00cbabfb`** (explore, 10m42s, session `ses_07dee1de3ffehvQ7b7l7pZ1kKN`) — Buff internals survey. Key findings: `<`/`>` are unambiguous single tokens (Lt/Gt); Pratt parser places them at two precedence levels (L8 comparison, L12 shift); colon `:` already has 5 meanings; braces `{}` already have 4 contexts; string interpolation uses `{}` (conflicts with JSX); T121b codegen proven; no tree-sitter grammar for Buff exists.
- **`bg_cafaa555`** (Oracle, 4m18s, session `ses_07de2bbc6ffe1n4D22jUkbSnlb`) — Synthesis + this decision record.

Precedent: this record mirrors the structure of `.sisyphus/decisions/dioxus-feasibility.md` (T121b PASS, commit 6b2235f) which authorized v1.8 T130 on top of the proven Dioxus feasibility spike.
