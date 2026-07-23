+++
title = "Onboarding by Background"
weight = 45
sort_by = "weight"
+++

# Onboarding by Background

> Coming to Buff from another language? Pick your background below. Each
> guide is a 30-minute ramp-up: a syntax mapping table, a tooling cheat
> sheet, an ecosystem map, a side-by-side "Hello World", the common
> pitfalls migrants hit, and pointers into the rest of the docs.

Buff is a high-level language that transpiles to Rust. The fastest way to
ramp up is to **map what you already know onto Buff's surface**, then
internalize the handful of mental shifts where Buff is intentionally
different from your home language. These guides do exactly that — they
assume you can already write working code in Python, Rust, Go, or
JavaScript/TypeScript and want to be productive in Buff today.

## Pick your background

| Coming from... | Read this guide | What you'll gain |
|---|---|---|
| **Python** | [Buff for Python developers](./python-developers/) | Async without `await`, type inference vs `typing`, `DataFrame` vs `pandas`, list comprehensions to `par_map`, decorators to `@State`/`@test` |
| **Rust** | [Buff for Rust developers](./rust-developers/) | Borrow-checker-free code, `extern` FFI, trait system, `match`/`Result`/`Option` (familiar), `async` transparent, `@attribute` + comptime |
| **Go** | [Buff for Go developers](./go-developers/) | `spawn` vs goroutines, `Channel<T>`, interfaces vs traits, `tokio` vs scheduler, error handling without `if err != nil` |
| **JavaScript / TypeScript** | [Buff for JS developers](./javascript-developers/) | async-transparent vs `Promise`/`await`, callbacks to closures, `buff-web` vs Express, npm to `buff add`, React hooks to `@State` |

## Why a per-background guide?

The four languages above are the dominant backgrounds of developers
evaluating Buff today. Each has a distinct set of habits that map cleanly
onto Buff and a handful that **don't** — Buff deliberately omits some
features (inheritance, `await`, `try`/`catch`, `null`/`nil`) for
soundness or ergonomics reasons that aren't obvious until you read the
why. The guides focus on the deltas, not the overlap.

If your background isn't listed (e.g. you're coming from C#, Java, Ruby,
or Elixir), the closest guide is still useful — most modern languages
share the same idioms these guides call out. The [Migration
guide](../migration/_index/) also has a feature-by-feature table across
the same four backgrounds plus TypeScript.

## How to read a guide

Every guide has the same six sections, so you can skim by jumping to the
section you need:

1. **Why Buff?** — one-paragraph pitch tailored to that background.
2. **Syntax mapping table** — the high-density reference: every common
   construct in your home language on the left, the Buff equivalent on
   the right.
3. **Tooling migration** — your package manager, formatter, linter, and
   test runner mapped to their Buff equivalents.
4. **Hello World, side by side** — a tiny program in your language, then
   the same program in Buff, with a line-by-line walkthrough.
5. **Common pitfalls** — the things that surprise migrants. Read this
   once before you start writing real code; it'll save you an hour.
6. **Where to go next** — links into the cookbook, language reference,
   and the `examples/` directory in the repo.

## What all four guides share

Regardless of your background, three things are true about Buff that
every migrant internalizes fast:

- **Indentation is the syntax.** Buff is offside-rule, like Python and
  Haskell. Four spaces per level. Tabs are a lexer error.
- **No `await`, no `async`/`await` color split.** A function declared
  `async func` runs async; every caller is propagated async-ness
  automatically. There is no `await` keyword.
- **No `null` / `nil` / `undefined`.** Absence is `Option<T>`; failure
  is `Result<T, E>`. Both are matched exhaustively. There is no
  `try`/`catch` and no `?.`-chain across `null` — Buff's `?.` is
  syntactic sugar for `Option.and_then`, not a null-coalescer.

Internalize those three and the rest is just translation.

## Installation reminder

If you haven't installed Buff yet, the [Getting Started →
Installation](../getting-started/installation/) page covers building
the CLI from source via `cargo install`. The short version:

```bash
git clone https://github.com/buff-lang/buff.git
cd buff
cargo install --path crates/buff-lang-cli --locked
```

Buff pins Rust 1.95.0 in `rust-toolchain.toml`; no external runtime, no
C library, no Docker needed.

## Read the guides

Ready? Pick one:

- [Buff for Python developers →](./python-developers/)
- [Buff for Rust developers →](./rust-developers/)
- [Buff for Go developers →](./go-developers/)
- [Buff for JavaScript developers →](./javascript-developers/)

After your guide, the [Cookbook](../cookbook/_index/) is the next stop —
55 copy-pasteable recipes for HTTP, files, JSON, database, parallel,
async, errors, testing, strings, and DataFrame work.

## Feedback

These guides evolve with the language. If you got stuck on something
while migrating, please file an issue in the [buff] repo — the guides
are tracked by T69 and updated whenever the corresponding language
feature or framework crate lands.

[buff]: https://github.com/buff-lang/buff
