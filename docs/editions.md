# Buff Editions

Editions are Buff's mechanism for evolving the language without breaking existing
code. Each edition is a named snapshot of syntax and semantics rules. A project
declares its edition in `buff.toml`, and the compiler enforces that edition's
rules. Old code on an old edition keeps compiling on every future compiler,
forever.

This model is borrowed directly from [Rust editions], which demonstrated that a
language can evolve past early design mistakes without forcing a one-time
flag-day migration.

[Rust editions]: https://doc.rust-lang.org/edition-guide/

---

## How editions work

A `buff.toml` declares the edition:

```toml
[package]
name = "my-app"
edition = "2026"
```

When the compiler reads a source file, it resolves syntax and semantics against
the declared edition. Two projects with different editions can depend on each
other. Editions affect compilation of a project's own source, not its public API
surface, so cross-edition dependencies work without friction.

The compiler understands every edition it has ever shipped. A Buff 1.30
compiler can compile edition `2026` code and edition `2028` code (and any
future edition) side by side in the same build.

---

## Current edition

**Edition 2026** is the default. It is the edition you get from `buff new` and
`buff init` without specifying anything. This matches the `buff.toml` v2 schema
introduced in the T0 SDK 2.0 work (v1.13).

The shipped editions as of today:

| Edition | Introduced | Notes |
|---|---|---|
| `2026` | v1.13 (SDK 2.0) | Default edition. Standard syntax and semantics. |
| `scientific` | v1.19 (T57) | Opt-in. Julia-inspired numeric syntax: `2x` juxtaposition, matrix literals. |

The default edition only changes at a major version bump (v2.0+). Within v1.x,
the default stays `2026`.

---

## When a new edition is created

A new edition ships when accumulated breaking changes to syntax or semantics
warrant bundling them into a coherent opt-in step. The expected cadence is every
2-3 years, not every release.

Things that can trigger a new edition:

- **Removing deprecated syntax.** A construct that spent a full minor cycle as
  `@deprecated` (per the deprecation policy) is removed in the new edition.
- **Introducing a new reserved keyword.** If a new keyword would conflict with
  existing identifiers, it goes behind an edition gate rather than breaking
  default-edition code.
- **Changing a default.** Switching the default integer type, tightening a
  parsing rule, or changing overload resolution semantics. Anything where the
  old behaviour is still valid but no longer preferred.
- **Tightening semantics that were intentionally loose.** An experimental
  feature that shipped with relaxed rules can tighten those rules in a new
  edition.

Things that do **not** require a new edition:

- **Additive features.** New syntax that doesn't conflict with existing code, new
  prelude types, new CLI flags, new attributes. These ship in any minor release
  and are available on all editions.
- **Bug fixes.** Correctness fixes that change observable behaviour are
  permitted within an edition if they fix a genuine bug (not a deliberate
  semantic change).
- **Performance improvements.** Better codegen, faster compilation, changed
  runtime scheduling. Behaviour is preserved; speed is not promised stable.

---

## Migration

When a new edition ships, the compiler provides an automated migration path:

```bash
buff fix --edition 2028
```

This mirrors `cargo fix --edition` from Rust. The tool rewrites syntax that
changed between editions (removing deprecated constructs, applying new keyword
rules, updating formatting). Mechanical changes are handled automatically. Cases
that require human judgment (e.g., a renamed keyword that creates an ambiguity)
are left as warnings with a suggested fix.

Migration is always optional. Code on edition `2026` compiles on every future
compiler in edition `2026` mode. You migrate when you want the new edition's
benefits, not because the old edition stopped working.

---

## Stability promise

This is the core contract, aligned with the [stability promise]:

> Code written for edition 2026 compiles on ALL future Buff versions in edition
> 2026 mode. Breaking changes ONLY happen across edition boundaries, and only
> when you opt in.

Concretely:

- A `.buff` file that declares `edition = "2026"` and compiles on Buff 1.20
  compiles on Buff 1.30, 1.50, and 2.0, in edition `2026` mode.
- The compiler never silently changes what edition `2026` means. Once shipped,
  the rules for an edition are frozen for that major version.
- ErrorCodes (`E10xx`/`E11xx`/`E12xx`/`E13xx`) are stable forever regardless
  of edition. They are never renumbered, reused, or back-filled.
- New editions can add new syntax and tighten semantics. They cannot remove
  something that was valid in the previous edition without the deprecation
  cycle completing first.

[stability promise]: ../.sisyphus/decisions/stability-promise.md

---

## Cross-references

- **Stability promise** ([`.sisyphus/decisions/stability-promise.md`][stability-promise]):
  the canonical document governing what is stable, what may change, and how
  editions interact with SemVer. This editions doc is a focused companion to
  stability-promise section 3.
- **SDK conventions** ([`.sisyphus/decisions/sdk-conventions-v1x.md`][sdk-conventions]):
  defines the `edition` field in `buff.toml`, `stability` badges, and the
  `@deprecated` attribute syntax.
- **Rust edition guide** ([`doc.rust-lang.org/edition-guide`][rust-editions]):
  the precedent Buff follows. Buff's model is simpler (no edition-related
  lints, no `edition2021` crate-level macro gates) because the language is
  younger and has less historical baggage.
- **COMPATIBILITY.md** (T54, Wave 1): planned cross-version compatibility
  reference, not yet written.

[stability-promise]: ../.sisyphus/decisions/stability-promise.md
[sdk-conventions]: ../.sisyphus/decisions/sdk-conventions-v1x.md
[rust-editions]: https://doc.rust-lang.org/edition-guide/
