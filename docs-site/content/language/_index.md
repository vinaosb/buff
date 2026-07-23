+++
title = "Language Reference"
weight = 20
sort_by = "weight"
+++

This section is the language reference for Buff — the parts that aren't
covered by a quick `buff run` of an example. It is intentionally compact;
Buff's syntax is small by design.

## Pages

- [Syntax](./syntax/) — layout rules, keywords, comments, literals.
- [Types](./types/) — primitives, collections, `Option`, `Result`, inference.
- [Async](./async/) — `async func`, `spawn`, the no-`await` model.
- [Error handling](./error-handling/) — `Result<T,E>`, `?`, `Error`, `match`.
- [Attributes](./attributes/) — `@prefer(gpu)`, `@ui`, comptime hints.

## What's intentionally absent

Buff omits a deliberate set of features that exist in Rust:

| Absent | Reason |
|---|---|
| `class`, inheritance | OOP via structs + traits + embedding |
| `null` / `nil` | Absence is `Option<T>` |
| `&` references | Owned data + intelligent clones |
| `'a` lifetimes | Hidden by the transpiler |
| `await` | Async propagates up the call graph automatically |
| `try` / `catch` | Errors are values (`Result<T, E>`) |
| braces `{ }` for blocks | Reserved for data (structs, maps, lambdas, match arms) |

The compiler emits only "easy" Rust on your behalf; you never see what's
hidden.

## Reserved keywords (25)

```
func let mut struct enum trait type
if else for return break continue in match
async spawn import export from as
true false extern unsafe
```

If you need one of these as an identifier, prefix it (Buff has no escape
syntax like Rust's `r#type`).

## Conventions

The full coding-conventions document lives at
[`.sisyphus/plans/buff-conventions.md`][conv] in the repo (18 conventions
covering naming, formatting, docs, errors, testing, and APIs). Highlights:

[conv]: https://github.com/buff-lang/buff/blob/master/.sisyphus/plans/buff-conventions.md

- **Indentation:** 4 spaces. Buff's lexer rejects tabs.
- **Trailing whitespace:** forbidden. Two consecutive blank lines: forbidden.
- **Async API naming:** no `_async` suffix. The compiler infers async-ness;
  you don't annotate it.
- **Constructors:** `Type.new()` / `Type.from()` only — never `Type.create()`,
  `Type.build()`, `new Type()`.
- **Boolean parameters:** always named (`fetch(url, cache: true)`), never
  positional.
- **Result/Option methods:** mirror Rust (`is_ok()`, `unwrap_or(...)`,
  `map(...)`) so muscle memory transfers.
