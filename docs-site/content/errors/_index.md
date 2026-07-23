+++
title = "Error codes"
weight = 50
+++

# Error codes

Every Buff diagnostic carries a **stable error code** of the form `E1xxx`,
grouped by compiler phase. The code appears in the diagnostic header —
for example `[Error] error[E1201]: undefined variable: pritn` — and points
at a longer explanation on this page.

Codes are a *stability contract* (convention §19): once a code ships, it
is **never renumbered, reused, or silently removed**. A code may be marked
deprecated if the underlying error becomes impossible to trigger, but it
keeps its number forever. This mirrors the guarantee `rustc` gives for its
`E0xxx` codes.

## Code ranges

| Range    | Phase       | Explained in                  |
|----------|-------------|-------------------------------|
| `E10xx`  | Lexing      | [Lexer errors](./E10xx-lexer/)   |
| `E11xx`  | Parsing     | [Parser errors](./E11xx-parser/) |
| `E12xx`  | Type-check  | [Type errors](./E12xx-type/)     |
| `E13xx`  | Codegen     | [Codegen errors](./E13xx-codegen/) |

## Diagnostic format

A diagnostic renders rustc-style: a header, the offending source line with
a caret underline, and optional `note:` / `help:` lines.

```text
[Error] error[E1201]: undefined variable: pritn
  |
1 | pritn("hello")
  | ^^^^^
  |
  help: did you mean `print`?
```

The `help:` line is produced by the suggestion engine (T63): when an
unknown identifier is within Levenshtein distance 2 of a prelude name,
the compiler offers the closest match.

## Suggestions ("did you mean?")

When the type-checker or linter sees a name that is *almost* a prelude
builtin, it attaches a `help:` note:

- `pritn` → `help: did you mean \`print\`?`
- `Print` → `help: function names are lowercase, did you mean \`print\`?`
- `dictionry` → `help: did you mean \`dictionary\`?`

The suggestion engine lives in `buff-lang-error::suggest` and uses
char-based Levenshtein distance with a threshold of 2 edits. Ties are
broken alphabetically so the suggested name is deterministic.

## Rustc → Buff span mapping

When `rustc` rejects the Rust code Buff generated, the error mapper
rewrites the `.rs` filename and line number back to the `.buff` source
location (using the `SourceMap` populated during codegen), and classifies
the rustc message into the closest Buff `E1xxx` code:

| rustc message                        | Buff code  |
|--------------------------------------|------------|
| `cannot find value/function/type`    | `E1201`    |
| `mismatched types`                   | `E1203`    |
| `cannot multiply/add ... types`      | `E1202`    |
| `expected bool`                      | `E1205`    |
| `if and else have incompatible types`| `E1206`    |
| `non-exhaustive patterns`            | `E1207`    |

So a `rustc` error like `error[E0308]: mismatched types` on the generated
`.rs` becomes `error[E1203]: ... help: Buff code: E1203 — assignment type
mismatch` pointing at your `.buff` source.

## Where the catalog lives

The full, machine-readable catalog is the `ErrorCode` enum in
[`crates/buff-lang-error/src/code.rs`](https://github.com/buff-lang/buff/tree/master/crates/buff-lang-error/src/code.rs).
Every variant exposes `.code_str()`, `.title()`, and `.explanation()`.
This site is the human-readable mirror of that enum.
