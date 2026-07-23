+++
title = "Stability Promise"
weight = 100
+++

# Buff Stability Promise

> Buff follows a **Rust-style** stability contract. Code that compiles on a
> released Buff version continues to compile on all future **minor** and
> **patch** releases of the same **major** version, with three narrow
> exceptions: a new **edition** may opt into breaking changes the user
> explicitly requested; a **security fix** may break an API when keeping it
> would ship a vulnerability; an API marked **`@deprecated`** in this major
> cycle may be removed in the **next** major cycle. **ErrorCodes are stable
> forever** — they are never renumbered, reused, silently removed, or
> back-filled.

This page is the rendered version of the canonical decision record at
[`.sisyphus/decisions/stability-promise.md`][src] (v1.21.0 / T71). It is kept
in sync with that file; if the two ever disagree, the decision record wins.

[src]: https://github.com/buff-lang/buff/blob/master/.sisyphus/decisions/stability-promise.md

---

## 1. What's Guaranteed Stable

The following surfaces, once shipped on a released version, are guaranteed
**not to break** on any subsequent minor or patch release of the same major
version.

### 1.1 Public language syntax

The **25 reserved keywords** (`func`, `let`, `mut`, `struct`, `enum`, `trait`,
`type`, `if`, `else`, `for`, `return`, `break`, `continue`, `in`, `match`,
`async`, `spawn`, `import`, `export`, `from`, `as`, `true`, `false`,
`extern`, `unsafe`) are fixed. The **layout-sensitive grammar** (offside rule,
4-space indentation, brace-for-data convention) is stable. The **three
parse-time desugars** — pipeline `|>`, null-conditional `?.`, null-coalesce
`??` — lower to the same AST nodes for all of v1.x. New keywords may be added
only as **contextual keywords** or **edition-gated** (§3); they never change
the meaning of existing programs.

### 1.2 Type system — `Type` and `PreludeType` variants

Every **primitive `Type` variant** (`Int`, `Float`, `Bool`, `String`, `Char`,
`Void`, `Option<T>`, `Result<T,E>`, `Vector<T>`, `Map<K,V>`, `Tuple`,
function types, …) keeps its name, arity, and representation. Every
**`PreludeType` variant** shipped through v1.20.0 (`DateTime`, `Date`, `Time`,
`Duration`, `Instant`, `Log`, `Regex`, `Toml`, `Math`, `Random`, `Strings`,
`Args`, `Env`, …) is stable, including its constructor surface
(`Type.new(...)` / `Type.from(...)`) and all associated/instance methods. The
free-function prelude (`print`, `println`, `input`, …) is stable. New variants
and methods may be **added**; existing ones are **never** removed or
re-signatured within v1.x.

### 1.3 ErrorCodes — `E10xx` / `E11xx` / `E12xx` / `E13xx` — FOREVER stable

The strongest guarantee in the whole document. An `ErrorCode` shipped on any
version is **never** renumbered, **never** reused, **never** silently removed,
and **never** back-filled into gaps. The four bands:

| Band | Crate | Meaning |
|---|---|---|
| `E10xx` | `buff-lang-lexer` | Lexing |
| `E11xx` | `buff-lang-parser` | Parsing |
| `E12xx` | `buff-lang-types` | Type-checking |
| `E13xx` | `buff-lang-codegen-rust` | Codegen |

The **human-readable message** may change (§2.3); the **code** does not.
Tooling that pattern-matches on `ErrorCode` is supported forever.

### 1.4 CLI semantics

`buff check`, `buff build`, `buff run` keep their exit-code contract and flag
surface within v1.x. `--release` and `--minimal` keep their documented
five-knob meaning. `buff new` / `buff init` produce a stable file layout. The
21-subcommand surface (as of v1.20.0) is the baseline. New subcommands and
flags may be **added** (additive); existing ones are not silently changed or
removed within v1.x.

### 1.5 Workspace crate versioning tiers

Two tiers, both SemVer:

| Tier | Base version | Crates |
|---|---|---|
| **Core compiler** | `1.2.0` | `ast`, `lexer`, `parser`, `types`, `codegen-rust`, `codegen-wgsl`, `runtime`, `error`, `cli`, `playground-wasm`, `lsp` |
| **Tooling** | `1.0.0` | `eval`, `repl`, `jupyter`, `registry`, `ui-dioxus`, `ast-rsx`, `buffhtml-parser`, `codegen-buffhtml` |

The **WGSL binding contract** (`@group(0) @binding(0)` input read,
`@group(0) @binding(1)` output read_write, workgroup 64) is stable within
v1.x. The `compile_to_rust` / `compile_rust_to_exe` / `compile_buffhtml_to_rust`
public signatures are stable.

---

## 2. What May Change

The surfaces **not** covered by the §1 guarantee. The guarantee is precise
because the exclusions are precise.

- **2.1 Internal compiler implementation** — lexer/parser/typechecker/codegen
  internals may be refactored freely. Only the *accepted language* (§1.1) and
  *type judgements* (§1.2) are stable, not the code that produces them.
- **2.2 Experimental and unstable features** — `comptime` (T53), multiple
  dispatch (T58), and any `buff-*` framework crate carrying a `stability`
  badge of `experimental` or `beta` are **not** covered by §1. They may be
  refined or removed in a minor release with a CHANGELOG note. A feature
  graduates to stable via a `stability` badge flip; once `stable`, §1 applies
  retroactively.
- **2.3 Error message text** — the prose attached to an `ErrorCode` may be
  rephrased, gain suggestions, or change colour/style. The **code** is stable;
  the **message** is not. Pattern-match on codes, not text.
- **2.4 Performance characteristics** — compiled binary size, compile time,
  runtime speed, GPU dispatch thresholds, and CPU-feature targeting are **not**
  guaranteed. Observable **behaviour** (output, side effects, termination)
  **is** guaranteed: `fib(10)` prints `55` forever.
- **2.5 Generated Rust source** — the Rust emitted by `buff-lang-codegen-rust`
  is not a public API. Identifier names, `extern_crates` set membership,
  `prettyplease` formatting, and `// buff-line:N` comments may change. The
  stable surface is **Buff source → observable behaviour**.

---

## 3. Edition Contract

Editions are Buff's **opt-in escape hatch** for changes that would otherwise
be breaking, modelled directly on [Rust editions][rust-editions].

[rust-editions]: https://doc.rust-lang.org/edition-guide/

- **3.1 Default edition** — the edition a project gets when no `edition` field
  is in `buff.toml`. Backward compatible across minor versions. Changes only
  at a major bump (v2.0+), at which point the old default remains accepted.
- **3.2 Opt-in editions** — declaring an edition (`edition = "scientific"` T57,
  `edition = "2026"` T0/SDK 2.0) may add new syntax that wouldn't parse in the
  default edition. **Opt-in only**: declaring an edition never breaks code
  that didn't declare it. **Additive within the edition**: once shipped, its
  syntax/semantics are stable for the major version. Edition-gated features
  stay gated (do not silently become default). Migration between editions is
  mechanical (`buff update edition`, planned).
- **3.3 Major version bumps (v2.0+)** — the one place the §1 guarantee does
  not hold for the default edition. `@deprecated` APIs may be removed,
  experimental features may be removed, the default edition may change.
  **ErrorCodes are still stable forever** (§1.3) — retired, never reused. The
  bar is minimise breakage; the compiler keeps accepting the previous default
  edition indefinitely.

---

## 4. Deprecation Policy

Deprecation is the **gradual** path from "shipped" to "removed".

### 4.1 The `@deprecated` attribute

```buff
@deprecated(since = "1.21", replacement = "Strings.split")
export func split_text(text: String, sep: String) -> Vector<String>:
    return Strings.split(text, sep)
```

Takes `since` (the version deprecation begins) and `replacement` (the API to
use instead — MUST exist and be stable at the moment the attribute lands).

### 4.2 Removal timeline

1. **T0** — the `@deprecated` attribute lands in a minor release. `buff check`
   emits a **non-fatal warning** at every call site: *"call to deprecated
   function 'split_text' ... since 1.21, use 'Strings.split'"*.
2. **One minor version of warning** — the warning ships for at least one full
   minor cycle (e.g. v1.21.x) before removal is considered.
3. **Removal at next major** — the API is removed at v2.0+. Call sites become
   hard errors. The "removed" `ErrorCode` is **new** (never reused — §1.3).

An API is **never** removed in a patch release, and **never** removed in a
minor release without one full minor cycle as `@deprecated`. The only
exception is the security carve-out (§5).

### 4.3 Migration guide requirement

Every `@deprecated` MUST be accompanied by: a CHANGELOG entry, the named
replacement (matching `replacement = "..."`), a before/after snippet, and a
docs-site banner ("Deprecated since vX.Y, use ... instead"). A deprecation
without a migration guide is a release blocker.

---

## 5. Security Exception

Security is the **one** reason the stability guarantee may be broken
**outside** the deprecation cycle. If keeping an API would ship a
vulnerability, it may be changed or removed in a **patch** release.

- **5.1 What qualifies** — an API whose continued existence compromises
  security AND cannot be fixed additively. E.g. removing a vulnerable crypto
  primitive; flipping an insecure default to require explicit opt-in; fixing
  a codegen path that emitted memory-unsafe Rust.
- **5.2 What does NOT qualify** — performance regressions, style renames,
  convenient deprecations, or removal of experimental features (§2.2 covers
  those without this exception).
- **5.3 Documentation requirement** — every security-breaking change is
  tracked in the CHANGELOG under a **`security-breaking`** heading (distinct
  from normal `breaking`), given a migration path, and the vulnerable version
  is **yanked** (§7) if already published. The bar is **high**: additive safe
  defaults are preferred over removal whenever possible.

---

## 6. Versioning Scheme

Buff follows [Semantic Versioning 2.0][semver] for both language releases and
published crates.

[semver]: https://semver.org/

| Component | Bumped when | Stability impact |
|---|---|---|
| **MAJOR** | A breaking change to a §1 surface (outside §3 editions and §5 security) | §1 may not hold for code that relied on the changed surface. §1.3 (ErrorCodes) still holds. |
| **MINOR** | A new feature (additive, backward compatible) | §1 holds fully. New syntax/variants/methods added; nothing stable removed. Experimental features may ship here. |
| **PATCH** | A bug fix only | §1 holds fully. No new features. A security fix (§5) may land here despite being technically breaking. |

"Breaking" means: a program that compiled and ran on `X.Y.Z` fails to compile,
or changes observable behaviour, on `X.(Y+1).0`. Only §1 surfaces count;
§2 surfaces (internals, experimental, error text, performance, generated Rust)
do not. Pre-release versions (`-alpha`, `-rc.1`) are not covered by the
guarantee. The two crate tiers (§1.5) bump independently within a release, but
a language major bump (v2.0+) bumps both tiers' majors together.

---

## 7. Yanked Versions

A published version may be **yanked** from the registry (`buff-registry`,
T126) for severe bugs or security vulnerabilities. Yanking is a
**distribution** action, not a source-control action.

- **7.1 What yanking means** — the version **remains in git history**
  (not rewritten or deleted); it is **discouraged for new projects**
  (`buff install` / `buff add` refuse to resolve to it unless explicitly
  pinned with `=X.Y.Z` + `--allow-yanked`); existing `buff.lock` pins
  **keep working**; the registry UI marks it `[yanked]`.
- **7.2 When a version is yanked** — it contains a §5 security vulnerability
  with a fix already published; it contains a severe correctness bug with a
  fix already published; it was published by mistake. **Yanking is for
  versions, deprecation (§4) is for APIs** — they are not substitutes.
- **7.3 Un-yanking** — a yanked version may be restored if the yank was made
  in error. The yanking announcement gets a retraction note; the registry's
  yank-flag history is the audit trail.

---

## Further reading

- **Canonical source:** [`stability-promise.md`][src] decision record (this
  page is a mirror; the decision record wins on disagreement).
- **Rust stability promise:** the [Cargo `rust-version` field][rust-stability].
- **Rust edition guide:** the [model for §3][rust-editions].
- **SemVer 2.0.0:** the [model for §6][semver].
- **sdk-conventions §7:** SemVer, `stability` badges, `@deprecated` mechanism.
- **sdk-conventions §4:** the `edition` field schema.
- **Error reference:** [`/errors/E10xx-lexer/`](../errors/E10xx-lexer/) et al.

[rust-stability]: https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field
