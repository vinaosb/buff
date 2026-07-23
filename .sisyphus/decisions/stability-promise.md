# Decision: Buff Stability Promise (T71)

**Status:** Accepted (v1.21.0 / T71)
**Date:** 2026-07-23
**Task:** [T71 — Stability Promise Document](../../.sisyphus/plans/buff-v1x-frameworks.md)
**Author:** Buff Lang Team
**Inspired by:** the [Rust 1.0 stability promise][rust-stability] and the
[Rust edition model][rust-editions].
**Scope:** the Buff **language** (syntax, type system, prelude), the
**compiler crates** (`buff-lang-*`), the **tooling crates** (`buff-*`), and the
**framework crates** published via the v1.13+ SDK (`buff-web`, `buff-ml`,
`buff-game`, …). This document is the *canonical* reference; the docs-site
mirror at `docs.buff-lang.org/stability/` is generated from it.

[rust-stability]: https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field
[rust-editions]: https://doc.rust-lang.org/edition-guide/

---

## 0. TL;DR

Buff follows a **Rust-style** stability contract. In one paragraph:

> Code that compiles on a released Buff version continues to compile on all
> future **minor** and **patch** releases of the same **major** version, with
> three narrow exceptions: (1) a new **edition** may opt into breaking changes
> the user explicitly requested; (2) a **security fix** may break an API when
> keeping it would ship a vulnerability; (3) an API marked **`@deprecated`** in
> this major cycle may be removed in the **next** major cycle. ErrorCodes
> (`E10xx`/`E11xx`/`E12xx`/`E13xx`) are stable **forever** — they are never
> renumbered, reused, silently removed, or back-filled.

The rest of this document expands that paragraph into seven sections plus two
appendices. If you only read one section, read **§1** (what's guaranteed) and
**§3** (the edition escape hatch).

---

## 1. What's Guaranteed Stable

This section lists the surfaces that, once shipped on a released version, are
guaranteed **not to break** on any subsequent minor or patch release of the
same major version. "Break" means: a program that compiled and ran on version
`X.Y.Z` will compile and run — with the same observable behaviour — on version
`X.Y'.Z'` for any `Y' >= Y` and any `Z'`.

### 1.1 Public Buff language syntax

The **grammar** of the Buff language is stable. Specifically:

- The **25 reserved keywords** are fixed: `func`, `let`, `mut`, `struct`,
  `enum`, `trait`, `type`, `if`, `else`, `for`, `return`, `break`,
  `continue`, `in`, `match`, `async`, `spawn`, `import`, `export`, `from`,
  `as`, `true`, `false`, `extern`, `unsafe`. A keyword shipped in v1.x stays
  reserved through all of v1.x and only becomes *un*-reserved at a major bump
  (v2.0+). New keywords may be **added** only as **contextual keywords**
  (parsed by position, not reserved globally) or **edition-gated** (see §3);
  they must not change the meaning of existing programs.
- The **layout-sensitive grammar** (offside rule, 4-space indentation,
  brace-for-data convention) is stable. The rules "indentation defines blocks",
  "braces `{ }` are reserved for data (struct literals, maps, lambdas,
  interpolation)", and "tabs are rejected" are invariant across v1.x.
- The **three parse-time desugars** — pipeline `|>`, null-conditional `?.`,
  null-coalesce `??` — lower to the same AST nodes they lower to today and will
  continue to for all of v1.x. If a future version changes *how* they lower
  (e.g. a faster codegen path), the *observable behaviour* is preserved.
- The **operators** (arithmetic, comparison, logical, bitwise, the `?`
  propagation operator) keep their current precedence, associativity, and
  overloadability surface.
- The **attribute syntax** (`@name`, `@name(args)`) is stable. New attributes
  may be **added** (e.g. `@deprecated` in §4, `@prefer(gpu)`) but existing
  attributes keep their semantics. An attribute that is *removed* is treated
  per the deprecation policy in §4.

A program that is valid Buff on `v1.20.0` is valid Buff on `v1.21.0`,
`v1.21.5`, etc., unless the user **opts in** to a new edition (§3) that
intentionally changes parsing.

### 1.2 Type system — `Type` and `PreludeType` variants

Every **type variant** shipped through v1.20.0 is stable:

- The primitive `Type` enum variants (`Int`, `Float`, `Bool`, `String`,
  `Char`, `Void`, `Option<T>`, `Result<T,E>`, `Vector<T>`, `Map<K,V>`,
  `Tuple`, function types, …) keep their names, arity, and representation
  contract. New primitive variants may be **added**; existing ones are not
  removed or renumbered in v1.x.
- Every **`PreludeType` variant** shipped in the extensible stdlib registry
  (`crates/buff-lang-types/src/prelude_types.rs`) is stable: `DateTime`,
  `Date`, `Time`, `Duration`, `Instant`, `Log`, `Regex`, `Toml`, `Math`,
  `Random`, `Strings`, `Args`, `Env`, and all variants added by later T124
  sub-tasks through v1.20.0. The **constructor surface**
  (`Type.new(...)` / `Type.from(...)`) is stable; new constructors may be added.
- The **free-function prelude** (`print`, `println`, `input`, etc. in
  `prelude.rs`) is stable. A free function shipped on v1.x keeps its name,
  parameter names, and return-type contract for all of v1.x.
- The **associated-function** and **instance-method** registries
  (`PreludeAssocFn`, `PreludeInstanceFn`, `PreludeAssocConst`) are stable once
  shipped: a method `DateTime.format(fmt)` or `Regex.match(text)` keeps its
  name, named-parameter contract, and return type.

The same "additive only" rule applies: a new variant or method may be **added**,
but an existing one is **never** removed or have its signature changed within
v1.x. Removal requires the deprecation cycle in §4 and a major bump.

> **Note on prelude shadowing.** Prelude types are *not* keywords — like
> `Option` and `Result`, they resolve by name lookup and can be shadowed by a
> user-defined type of the same name. Shadowing is the user's responsibility
> (a documented footgun). The *guarantee* here is about the prelude-provided
> binding, not about a user's redefinition.

### 1.3 ErrorCodes — `E10xx` / `E11xx` / `E12xx` / `E13xx` — FOREVER stable

This is the strongest guarantee in the whole document. Per AGENTS.md §19,
**ErrorCodes are stable forever** — not just for v1.x, but for the lifetime of
the language. Concretely:

- An `ErrorCode` shipped on any version is **never** renumbered. `E1001` is
  "unexpected character" forever; it does not become `E1099` later.
- An `ErrorCode` is **never** reused for a different condition. If the
  original condition becomes unreachable (e.g. a syntax is removed in v2.0),
  the code is **retired** (reserved, never emitted, never reassigned).
- An `ErrorCode` is **never** silently removed. A diagnostic that stops being
  emitted is documented in the CHANGELOG and the code remains reserved.
- New `ErrorCode`s are **never back-filled** into gaps. Once `E1001`-`E1014`
  exist, `E1015` is the next lexer code even if `E1014` is later retired.

The four bands are:

| Band | Crate | Meaning |
|---|---|---|
| `E10xx` | `buff-lang-lexer` | Lexing errors (unexpected char, unterminated literal, …) |
| `E11xx` | `buff-lang-parser` | Parsing errors (expected token, layout, extern ABI, …) |
| `E12xx` | `buff-lang-types` | Type-checking errors (undefined name, type mismatch, …) |
| `E13xx` | `buff-lang-codegen-rust` | Codegen errors (code-emission failures, …) |

The **human-readable message text** attached to a code MAY change (see §2.3);
the **code itself** does not. Tooling (LSP, IDE extensions, the docs-site
error catalog at `docs.buff-lang.org/E1xxx`) relies on this stability to build
stable lookup tables and on-click "explain this error" links.

### 1.4 CLI semantics — `buff check`, `buff build`, `buff run`

The **exit-code contract** and **flag surface** of the primary CLI commands are
stable within v1.x:

- `buff check <file>` exits `0` if the program type-checks, non-zero otherwise.
  It runs the standalone typechecker (T55: lex → parse → `TypeInferencer` →
  `naming_lint`) with **no codegen**. The `--json` output shape (when shipped)
  is stable.
- `buff build <file>` produces a native executable. `--release` and
  `--minimal` flags keep their documented five-knob meaning
  (`opt-level=z` + `panic=abort` + `strip=symbols` + `lto=true` +
  `codegen-units=1`).
- `buff run <file>` is `buff build` + execute. The argument-passing and
  stdout/stderr contracts are stable.
- `buff new <name>` / `buff init` scaffold a runnable project from the
  documented templates. The *file layout* of the output is stable; new files
  may be added, existing files are not silently renamed/removed.

New **subcommands** and new **flags** may be added (additive). An existing
flag's meaning is not changed within v1.x. A flag that is *removed* goes
through the deprecation cycle (§4).

The 21-subcommand surface (as of v1.20.0) is the baseline: `add`, `build`,
`check`, `clean`, `deps`, `fmt`, `init`, `install`, `jupyter`, `login`, `new`,
`outdated`, `publish`, `registry`, `repl`, `run`, `ssr`, `test`, `ui_build`,
`ui_dev`, `ui_new`, `update`.

### 1.5 Workspace crate versioning tiers

The workspace ships two **version tiers**, and both follow SemVer:

| Tier | Version | Crates |
|---|---|---|
| **Core compiler** | `1.2.0` (bumps on release) | `ast`, `lexer`, `parser`, `types`, `codegen-rust`, `codegen-wgsl`, `runtime`, `error`, `cli`, `playground-wasm`, `lsp` |
| **Tooling** | `1.0.0` (bumps on release) | `eval`, `repl`, `jupyter`, `registry`, `ui-dioxus`, `ast-rsx`, `buffhtml-parser`, `codegen-buffhtml` |

Within a major version, every crate's **public Rust API** follows SemVer:
minor and patch releases are additive (new items, new trait impls, new
non-breaking changes); removal or signature change requires a major bump of
*that crate* (which, for the core compiler tier, coincides with a Buff
language major bump).

The **WGSL binding contract** between `buff-lang-codegen-wgsl` and
`buff-lang-runtime` — `@group(0) @binding(0)` input storage (read),
`@group(0) @binding(1)` output storage (read_write), workgroup size 64 default
— is stable within v1.x. Both crates MUST stay in sync; a change is a
coordinated major bump.

The **`compile_to_rust` / `compile_rust_to_exe` / `compile_buffhtml_to_rust`**
split in `pipeline.rs` is a stable public surface for the lib-target consumers
(LSP, REPL, Jupyter, playground-wasm). The *internals* of these functions may
change (§2.1); the *signature and return contract* do not within v1.x.

---

## 2. What May Change

This section is the honest counterpart to §1: the surfaces that are **not**
covered by the stability guarantee and may change between minor releases
without notice. Reading both sections together is the point of the document —
the guarantee is precise because the *exclusions* are precise.

### 2.1 Internal compiler implementation

The **internals** of the compiler are not a public API:

- The **hand-rolled lexer** byte-scanner internals, the **offside-rule**
  indent tracker's data structures, and the **parser's** recursive-descent +
  Pratt table layout may be refactored freely. Only the *accepted language*
  (§1.1) is stable, not the code that accepts it.
- The **`TypeInferencer`** algorithm, the **ownership analysis** (T33
  Copy/Arc/CoW classification), the **async fixpoint propagation** (T31), the
  **recursion cycle detection** (T48), and the **exhaustiveness checker**
  (T27) may be reimplemented. Only the *type judgements they produce* are
  stable (§1.2), not the order in which they produce them or the internal
  IR.
- The **codegen internals** — how `syn`/`quote`/`prettyplease` nodes are
  constructed, the race/atomic/gpu_alignment pre-passes, the
  `lower_prelude_call` / `lower_prelude_type_assoc_fn` helpers — may change.
  The *generated Rust* is covered by §2.5.
- The **`boulder.json`** session state, the `.sisyphus/evidence/` directory,
  and other orchestration artifacts are **not** public and may change format
  at any time.

The dividing line: **if it's not documented in §1, it's an implementation
detail.** Importing an internal module (`buff_lang_parser::stream`, …) from
outside the workspace is unsupported.

### 2.2 Experimental and unstable features

Features explicitly marked **experimental** or **unstable** are **not** covered
by the stability guarantee, even when they ship in a released version. They
may be refined, restructured, or removed in a minor release. The current list
(as of v1.20.0) includes:

- **`comptime`** (T53) — compile-time metaprogramming. The `comptime` block
  and parameter syntax is shipped but the *evaluation model* may refine as the
  plugin architecture (T72) lands.
- **Multiple dispatch** (T58) — the dispatch semantics for overloaded
  functions are edition-gated and may refine.
- **The `buff-*` framework crates** (T13-T52) carry a **`stability` badge**
  in `buff.toml` (see sdk-conventions §7.2): `experimental`, `beta`,
  `stable`, `locked`. Only `stable` and `locked` are covered by the SemVer
  guarantee in §6; `experimental` and `beta` may break between minor versions
  *with a CHANGELOG note*.
- **The Tauri desktop scaffold** (`buff ui new --desktop`) depends on
  upstream Tauri; its surface tracks Tauri's stability, not Buff's.

A feature graduates from experimental to stable via a documented announcement
in the CHANGELOG and an update to its `stability` badge. Once `stable`, §1
applies retroactively from the version that flipped the badge.

### 2.3 Error message text

The **human-readable message** attached to an `ErrorCode` may change between
releases. The code is stable (§1.3); the prose is not. Concretely:

- `E1001` is always "an unexpected character was hit during lexing". The
  *exact wording* — "unexpected character: '@'" vs "lexer cannot start a
  token with '@'" — may be rephrased for clarity, may gain a suggestion, may
  change its colour/style output.
- Suggestions (`help:` blocks, "did you mean ...") may be added, reworded, or
  reordered.
- The **span** a diagnostic points at may tighten (a more precise column
  range) but will not *widen* in a way that hides the previously-reported
  location.

Tooling that pattern-matches on **message text** is unsupported; tooling that
pattern-matches on **`ErrorCode`** is supported forever.

### 2.4 Performance characteristics

Buff does **not** promise stable performance. The compiler may produce
**faster** code over time (better codegen, smarter ownership analysis, tighter
Rust). It may also produce **slightly slower** code in one release if a
correctness fix requires it, then recover. Specifically not guaranteed:

- Compiled binary size (except the documented `--minimal` *budget*, which is a
  target, not a contract).
- Compile time.
- Runtime speed of generated code.
- GPU dispatch thresholds (the arithmetic-intensity cutoff that decides CPU
  vs GPU may move between releases).
- The exact set of CPU features the runtime targets.

What **is** guaranteed: a program's **observable behaviour** (output, side
effects, termination) is stable. If `fib(10)` prints `55` on v1.20.0, it
prints `55` on v1.21.0 — even if the binary is twice as fast or half the size.

### 2.5 Generated Rust source

The **Rust source** emitted by `buff-lang-codegen-rust` is **not** a public
API. Inspecting it (via `buff build --emit-rust` or the `compile_to_rust`
return value) is supported for *human reading and debugging*; depending on its
exact text from release to release is not. Concretely:

- The `extern_crates` `BTreeSet` populated during codegen may gain or lose
  entries as lowering improves (e.g. a future release might emit a more
  efficient `Arc` path that drops a `clone` crate).
- The generated identifier names (locals, struct fields) may change.
- `prettyplease`'s formatting may change as the formatter is upgraded.
- The `// buff-line:N` comments and the post-format `SpanMap` (for `.buffhtml`)
  are internal diagnostics aids and may change format.

The **stable** surface is the **Buff source** → **observable behaviour**
mapping (§1). The Rust in the middle is an implementation detail.

---

## 3. Edition Contract

Editions are Buff's **opt-in escape hatch** for changes that would otherwise
be breaking. They are modelled directly on [Rust editions][rust-editions]:
each edition is a coherent set of language changes; a project opts in by
declaring the edition in `buff.toml`; code written for an older edition keeps
compiling unchanged on newer compilers (the compiler understands every
historical edition).

### 3.1 Default edition

The **default edition** is the one a project gets when no `edition` field is
present in `buff.toml` (or when the field is omitted by `buff new` / `buff
init`). The default edition is **backward compatible** across minor versions:
a program that compiled on the default edition of v1.20.0 compiles on the
default edition of v1.21.0, v1.21.5, etc.

The default edition only changes at a **major** version bump (v2.0+). When it
does, the new default is announced in the CHANGELOG with a migration guide,
and the compiler continues to accept the previous default edition indefinitely
(see §3.3).

### 3.2 Opt-in editions

A project may declare an **opt-in edition** in `buff.toml`:

```toml
[package]
name = "my-science-app"
edition = "scientific"   # T57: opt-in mathematical syntax
```

Opt-in editions may **add new syntax** that would **not parse** in the default
edition — e.g. `edition = "scientific"` (T57) enables Julia-inspired matrix
literals, `2x` juxtaposition multiplication, and other numerical conveniences.
They may also **refine** the semantics of an experimental feature (e.g.
`edition = "2026"` per sdk-conventions §4 may tighten a rule that was loose in
the default edition).

The contract for opt-in editions:

- **Opt-in only.** Declaring an edition never breaks code that didn't declare
  it. A program on the default edition is unaffected by the existence of
  `edition = "scientific"`.
- **Additive within the edition.** Once an edition is shipped, its syntax and
  semantics are stable for the rest of the major version (same rule as §1).
  A new edition (`edition = "scientific-2"`) is a separate opt-in, not a
  mutation of the existing one.
- **Edition-gated features stay gated.** A feature that requires an edition on
  v1.20.0 still requires that edition on v1.21.0. It does not silently become
  default (that would be a breaking change to default-edition programs). It
  may graduate to default only at a major bump.
- **Migration is mechanical.** The compiler provides an `buff update edition`
  path (planned) that rewrites a project to the next edition where the rewrite
  is mechanical, leaving any genuinely-different semantics for the human.

The set of **shipped editions** as of v1.20.0: the default edition,
`edition = "scientific"` (T57, v1.19), and `edition = "2026"` (T0/SDK 2.0,
v1.13). New editions may be added in minor releases; existing editions are
stable for the major version.

### 3.3 Major version bumps (v2.0+)

A **major** version bump is the one place where the stability guarantee
**does not hold** for the default edition. At v2.0:

- APIs marked `@deprecated` during v1.x **may be removed** (see §4).
- The default edition **may change** to a newer one (the old default remains
  accepted, but is no longer the implicit choice for new projects).
- Language features that were **experimental** throughout v1.x and never
  graduated to stable **may be removed** without a deprecation cycle (they
  were never covered by §1 to begin with — see §2.2).
- ErrorCodes are **still** stable forever (§1.3) — even at v2.0, an `ErrorCode`
  is retired, not reused.

Even at a major bump, the bar is **minimise breakage**: every removal is
documented in the CHANGELOG with a migration path, and the compiler keeps
accepting the previous default edition so that unmaintained code keeps
compiling. The Rust precedent (Rust 2015 → 2018 → 2021) is the model: editions
let the language evolve without a forced rewrite.

---

## 4. Deprecation Policy

Deprecation is the **gradual** path from "shipped" to "removed". It applies to
any stable surface in §1 that the team decides to retire: a prelude function,
a `PreludeType` method, a CLI flag, an attribute.

### 4.1 The `@deprecated` attribute

A stable API that is being retired is first marked with the `@deprecated`
attribute (defined in sdk-conventions §7.3 / G3):

```buff
@deprecated(since = "1.21", replacement = "Strings.split")
export func split_text(text: String, sep: String) -> Vector<String>:
    return Strings.split(text, sep)
```

The attribute takes two named arguments:

- **`since`** — the version (typically a minor version) at which the
  deprecation begins. This is the "T0" mark in the deprecation timeline.
- **`replacement`** — the name of the API that should be used instead. The
  replacement MUST exist and be stable at the moment the `@deprecated`
  attribute lands; you cannot deprecate "in the direction of" something that
  does not yet exist.

### 4.2 Removal timeline

The deprecation cycle is:

1. **T0** — the `@deprecated` attribute lands in a **minor** release
   (e.g. v1.21.0). From this release, `buff check` emits a **warning** at
   every call site:

   ```
   warning: call to deprecated function 'split_text'
      --> src/main.buff:5:5
       |
     5 |     split_text(line, ",")
       |     ^^^^^^^^^^^ since 1.21, use 'Strings.split'
   ```

   The warning is **non-fatal**: the program still compiles and runs.

2. **One minor version of warning** — the deprecation warning ships in at
   least one full minor cycle (e.g. v1.21.x) before removal is considered.
   This gives users a release window to migrate.

3. **Removal at next major** — the deprecated API is **removed** at the next
   **major** version (v2.0+). After removal, call sites become hard errors
   (`E12xx` undefined name, or a dedicated `E13xx` for removed-with-notice
   APIs). The ErrorCode for "removed" is **new** (never reused from the
   deprecated API's old errors — see §1.3).

An API is **never** removed in a patch release, and **never** removed in a
minor release without having spent at least one minor cycle as `@deprecated`.
The only exception is the security carve-out in §5.

### 4.3 Migration guide requirement

Every `@deprecated` attribute MUST be accompanied by a migration note. The
note:

- Lives in the **CHANGELOG** entry for the release that introduces the
  deprecation.
- Names the **replacement** (matching the `replacement = "..."` argument).
- Shows a **before/after** snippet for the common use case.
- Is linked from the **docs-site page** for the deprecated API (the page is
  not deleted at deprecation time; it gains a "Deprecated since vX.Y, use
  ... instead" banner and is removed only when the API is removed at the next
  major).

A deprecation without a migration guide is a documentation bug and is treated
as a release blocker.

---

## 5. Security Exception

Security is the **one** reason the stability guarantee may be broken **outside**
the deprecation cycle. If keeping an API would ship a vulnerability, the API
may be **changed or removed** in a **patch** release, without the one-minor
warning window of §4.2.

### 5.1 What qualifies

A "security fix" is a change that:

- Removes or changes an API whose continued existence would let a Buff program
  compromise its own or its host's security, AND
- Cannot be fixed by an additive change (e.g. a new, safe overload that
  shadows the unsafe one).

Typical examples:

- A prelude function that wraps a vulnerable crypto primitive (e.g. a hashing
  function with a collision attack) is **removed** and replaced with a
  safe default. The old name may return a hard error pointing at the
  replacement, or may be re-typed to refuse the vulnerable input.
- A `buff.toml` field that defaults to an insecure transport (`http://`) is
  flipped to require an explicit opt-in. Programs that relied on the insecure
  default get a hard error with a one-line fix.
- A codegen path that emitted memory-unsafe Rust (a bug in the ownership
  analysis) is fixed even if the fix changes the generated code for existing
  programs.

### 5.2 What does NOT qualify

Security is **not** a blanket excuse for breakage:

- A **performance** regression is not a security fix.
- A **style** change (renaming for consistency) is not a security fix — it
  goes through §4.
- A **deprecation the team finds convenient** is not a security fix — it goes
  through §4.
- Removing an **experimental** feature (§2.2) is not a security fix —
  experimental features may be removed at any time without this exception.

### 5.3 Documentation requirement

Every security-breaking change is:

- Tracked in the CHANGELOG under a **`security-breaking`** heading, distinct
  from the normal `breaking` heading.
- Given a **migration path** (the same requirement as §4.3). The migration may
  be "delete the call, here is the safe replacement" — but it must exist.
- **Yanked** from the registry (see §7) if the vulnerable version is already
  published. The fixed version is published as a patch bump (`X.Y.Z+1`).

The bar for invoking §5 is **high**: the team prefers an additive safe default
over a removal whenever possible. The exception exists so that a "we cannot
ship this even one more release" situation has a documented, honest path.

---

## 6. Versioning Scheme

Buff follows **Semantic Versioning 2.0** ([semver.org][semver]) for both the
language releases and the published crates.

[semver]: https://semver.org/

A version is `MAJOR.MINOR.PATCH`:

| Component | Bumped when | Stability impact |
|---|---|---|
| **MAJOR** | A breaking change to a §1 surface (outside §3 editions and §5 security) | The guarantee in §1 may not hold for code that relied on the removed/changed surface. ErrorCode stability (§1.3) still holds. |
| **MINOR** | A new feature (additive, backward compatible) | §1 holds fully. New syntax/variants/methods are added; nothing stable is removed. An experimental feature (§2.2) may ship here. |
| **PATCH** | A bug fix only | §1 holds fully. No new features. A security fix (§5) may land here despite being technically breaking. |

### 6.1 What counts as "breaking"

For SemVer purposes, "breaking" means: a program that compiled and ran on
`X.Y.Z` fails to compile, or changes observable behaviour, on `X.(Y+1).0`.
The surfaces covered by §1 are the ones that count; surfaces in §2 (internals,
experimental, error text, performance, generated Rust) do **not** count.

This means, for example, that a release which rephrases every error message
(§2.3) is a **patch**, not a major — the ErrorCodes are unchanged, the
behaviour is unchanged, only the prose moved. Conversely, removing a prelude
function without a deprecation cycle is a **major** (and a process violation
of §4).

### 6.2 Pre-release and build metadata

Buff follows the SemVer 2.0 pre-release (`-alpha`, `-beta.1`, `-rc.2`) and
build metadata (`+sha.abc123`, `+20260723`) syntax. Pre-release versions are
**not** covered by the stability guarantee — they are explicitly "do not
depend on this yet". Build metadata is ignored for stability purposes.

### 6.3 Crate tier coordination

The two version tiers (§1.5) bump **independently** within a release, but a
**language** major bump (v2.0+) bumps both tiers' majors together. Within v1.x,
the core compiler tier may go `1.2.0 → 1.3.0` (new prelude type) while the
tooling tier stays at `1.0.x` (no tooling API change). The CHANGELOG records
which tier moved and why.

---

## 7. Yanked Versions

A published version may be **yanked** from the package registry
(`buff-registry`, T126) when it has a severe bug or a security vulnerability
that cannot wait for the next patch. Yanking is a **distribution** action, not
a source-control action.

### 7.1 What yanking means

- The yanked version **remains in git history**. It is not rewritten, deleted,
  or hidden. Anyone who pins to the exact commit can still build it.
- The yanked version is **discouraged for new projects**: `buff install
  <pkg>` and `buff add <pkg>` refuse to resolve to a yanked version unless the
  user explicitly pins to it (with a `=X.Y.Z` constraint and a
  `--allow-yanked` flag).
- Existing `buff.lock` files that already pin the yanked version **keep
  working** — yanking does not break builds that already resolved. It only
  prevents *new* resolutions from landing on the yanked version.
- The registry's web UI and `buff search` results mark the version as
  `[yanked]` with a link to the yanking announcement.

### 7.2 When a version is yanked

A version is yanked when:

- It contains a **security vulnerability** covered by §5, and a fixed version
  is already published. The vulnerable version is yanked so that new
  resolutions pick the fix.
- It contains a **severe correctness bug** (e.g. the compiler miscompiles a
  common pattern) and a fixed version is already published.
- It was published by **mistake** (e.g. a leaked pre-release that escaped the
  pre-release channel) and the team decides to retract it.

Yanking is **not** a substitute for the deprecation cycle (§4). An API you
want to retire is `@deprecated`, not yanked. Yanking is for **versions**,
deprecation is for **APIs**.

### 7.3 Un-yanking

A yanked version may be **un-yanked** if the yank was made in error (e.g. the
"severe bug" turned out to be a user misconfiguration). Un-yanking restores
normal resolution. The yanking announcement is updated with a retraction
note; the git history of the registry's yank flag is the audit trail.

---

## Appendix A — Stability checklist for contributors

Before shipping a change, run through this checklist. If any answer is "yes"
where it shouldn't be, the change is a release blocker.

1. **Does the change alter the accepted language?** (A program that parsed
   before no longer parses, or parses differently.)
   - If yes and the change is **additive** (new keyword as contextual, new
     syntax only under an edition) → **minor** release, OK.
   - If yes and the change is **subtractive** (a program stops parsing) →
     **major** release OR edition-gate it (§3) OR deprecate first (§4).

2. **Does the change remove or renumber an `ErrorCode`?**
   - If yes → **STOP**. §1.3 forbids this absolutely. Retire the code
     (reserve it, stop emitting it) instead.

3. **Does the change remove or re-signature a `PreludeType` variant, a
   prelude free function, or a prelude method?**
   - If yes → mark `@deprecated` (§4.1) NOW, plan removal for the next major.

4. **Does the change alter a CLI flag's meaning or remove a subcommand?**
   - If yes → deprecate first (§4). New flags/subcommands are additive (minor).

5. **Does the change alter generated Rust text?**
   - If yes and the **observable behaviour** is unchanged → OK (§2.5), minor
     or patch.
   - If yes and the **observable behaviour** changes → treat as a language
     change (item 1).

6. **Does the change alter an experimental feature (§2.2)?**
   - If yes → OK, minor release, note in CHANGELOG. Experimental features are
     not covered by §1.

7. **Is the change a security fix that cannot be additive?**
   - If yes → invoke §5, document as `security-breaking`, yank the vulnerable
     version (§7).

8. **Does the change rename an internal module or restructure a `src/` file?**
   - If yes → OK, the internals are not public (§2.1). Update the crate's
     `AGENTS.md`.

---

## Appendix B — Relationship to existing documents

This document is the **canonical** stability reference. It composes with — and
does not override — these existing documents:

| Document | Overlap | Relationship |
|---|---|---|
| **AGENTS.md §19** (root) | ErrorCode stability | §1.3 reproduces and strengthens the AGENTS.md rule. AGENTS.md is the *source of truth* for "ErrorCodes are stable forever"; this document is the *user-facing* expression of it. |
| **sdk-conventions-v1x.md §7** (Versioning & Compatibility) | SemVer, `stability` badges, `@deprecated` | §4 and §6 cite §7.2 (badges) and §7.3 (`@deprecated`) verbatim. sdk-conventions defines the *mechanism* (attribute syntax, badge values); this document defines the *policy* (when to use them, how long the cycle is). |
| **sdk-conventions-v1x.md §4** (Configuration) | `edition` field | §3.2 cites the `edition = "2026"` and `edition = "scientific"` opt-ins. sdk-conventions defines the *field*; this document defines the *contract*. |
| **buff-v1x-frameworks.md T71** (this task) | The spec for this document | T71 is the *task*; this document is the *deliverable*. The task's acceptance criteria (document published, covers all dimensions, referenced from README, 3 tests) are met by this file + the docs-site mirror + the README link + `tests/stability_doc.rs`. |
| **Rust stability promise** | Inspiration | Buff's model is directly inspired by Rust's. The differences: Buff has no `#[stable]`/`#[unstable]` attribute gates in the compiler source (stability is governed by this document + the `stability` badge in `buff.toml`); Buff's ErrorCodes are *stricter* than Rust's (forever, not just per-edition). |

When this document and one of the above disagree, **this document wins** for
user-facing stability questions; the others win for their own internal
definitions (e.g. sdk-conventions is still the source of truth for the exact
TOML schema of `buff.toml`).

---

## References

- [Rust stability promise][rust-stability] — Cargo manifest `rust-version`
  field.
- [Rust edition guide][rust-editions] — the model for §3.
- [Semantic Versioning 2.0.0][semver] — the model for §6.
- AGENTS.md §19 — ErrorCode "stable forever" rule (root repo file).
- `.sisyphus/decisions/sdk-conventions-v1x.md` §7 — Versioning &
  Compatibility (SemVer, `stability` badges, `@deprecated`).
- `.sisyphus/decisions/sdk-conventions-v1x.md` §4 — Configuration (`edition`
  field).
- `.sisyphus/plans/buff-v1x-frameworks.md` T71 — task spec for this document.
- `.sisyphus/plans/buff-v1x-frameworks.md` T57 — `edition = "scientific"` opt-in.
- `crates/buff-lang-error/src/code.rs` — the `ErrorCode` enum (E10xx/E11xx/
  E12xx/E13xx).
- `crates/buff-lang-types/src/prelude_types.rs` — the `PreludeType` enum.
- `crates/buff-lang-cli/tests/stability_doc.rs` — the 3 doc-validation tests
  that enforce this document's structure.

---

**End of stability promise.** For the rendered docs-site version, see
`docs.buff-lang.org/stability/` (generated from
`docs-site/content/stability/_index.md`).
