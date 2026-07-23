+++
title = "Codegen errors (E13xx)"
weight = 54
+++

# Codegen errors (`E13xx`)

Codegen (`buff-lang-codegen-rust`) lowers the typed AST to Rust via
`syn` / `quote` / `prettyplease`, then invokes `rustc`. `E13xx` codes
cover the failures *inside* codegen itself (not rustc's errors on the
generated code — those are mapped to `E12xx` via the error mapper).

## Codes

| Code   | Variant                  | Trigger                                          |
|--------|--------------------------|--------------------------------------------------|
| `E1301`| `UnsupportedCodegen`     | AST node has no Rust lowering yet                |
| `E1302`| `CodegenParseError`      | codegen emitted a token stream `syn` rejected    |
| `E1303`| `AsyncBlockDeadlock`     | `block()` inside `async func` (warning)          |
| `E1304`| `ComptimeLoweringFailed` | comptime value codegen cannot splice             |

## Internal vs user errors

`E1302` (`CodegenParseError`) is always an **internal compiler error**:
the user's Buff is well-formed, the bug is in codegen. The diagnostic
includes the `syn` parse error for triage. If you hit one, file a bug
with a minimal reproducer.

`E1301` (`UnsupportedCodegen`) is a *feature gate*: the front-end accepted
your program but codegen does not yet lower the construct. The message
names the construct. Rewrite it using a supported equivalent, or wait for
the feature.

`E1303` (`AsyncBlockDeadlock`) is a **warning**, not an error. Codegen
still emits the `block()` call, but warns that it can deadlock the
single-threaded async runtime. Remove `block()` and `return` the future
directly, or move the blocking work to a non-async function.

## Rustc errors on generated code

When `rustc` rejects the generated `.rs` (ownership, lifetime, trait
bounds), the error is a *rustc* code (`E0xxx`), not a Buff code. The
error mapper (T24 + T63):

1. Rewrites the `.rs` filename → `.buff` filename.
2. Uses the `SourceMap` (Buff span ↔ Rust line) to translate the line
   number back to the `.buff` source line.
3. Classifies the rustc message into the closest Buff `E12xx` code and
   appends a `help: Buff code: E1xxx — <title>` line.

So you should rarely see a raw rustc code in `buff build` output — the
translation layer surfaces the Buff equivalent and points at your source.

## Example

```text
[Error] error[E1303]: `block()` inside an async function can deadlock
  |
2 | func fetch():
3 |     block(other_async())
  |     ^^^^^
  |
  note: remove `block()` and `return` the future directly
```
