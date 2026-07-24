# Chapter 8 — Error Code Handbook

Every diagnostic the Buff compiler emits MAY carry a stable error code of the
form `E1xxx`. This chapter is the handbook for all of them. Codes are grouped
by compiler phase so that reading a code alone tells you which part of the
pipeline produced it:

| Range | Phase | Source crate |
|---|---|---|
| `E10xx` | Lexing | `buff-lang-lexer` |
| `E11xx` | Parsing | `buff-lang-parser` |
| `E12xx` | Type-checking | `buff-lang-types` |
| `E13xx` | Codegen | `buff-lang-codegen-rust` |
| `E14xx` | Runtime | `buff-lang-runtime` |
| `E15xx` | Warnings (lint) | various |

## 8.1 The stability guarantee (read this first)

Error codes are **stable forever**. Once a code ships in a release, six rules
apply (conventions doc §19):

1. **Never renumber.** `E1001` is `E1001` forever.
2. **Never reuse.** A code's meaning never changes.
3. **Never silently remove.** A tombstoned code stays in the enum and on the
   site with a note ("no longer emitted as of vX.Y").
4. **New codes are appended** at the end of their phase block. Gaps left by
   tombstones are NOT back-filled.
5. **The `ErrorCode` enum is the source of truth** in
   [`crates/buff-lang-error/src/code.rs`](https://github.com/buff-lang/buff/blob/v1x-frameworks/crates/buff-lang-error/src/code.rs).
   The static site at [`docs/errors/`](../../docs/errors/) is generated from
   it; the two must never drift (enforced by a test).
6. **Codes are append-only across releases.** A release may add new codes and
   tombstone existing ones; it may not renumber, reuse, or delete.

This mirrors `rustc`'s `E0xxx` policy. You can cite a Buff error code in a bug
report, a Stack Overflow answer, or a CI lint rule, and trust that the
citation stays meaningful across releases.

The canonical, always-up-to-date catalog is the generated HTML site at
[`docs/errors/index.html`](../../docs/errors/index.html). This chapter is a
convenient in-book summary; when in doubt, the site wins.

## 8.2 Lexing — `E1001`–`E1014`

Errors from the byte-scanner that turns `.buff` source text into tokens.

### E1001 — unexpected character

The lexer hit a byte it cannot start any token with: a stray `@` outside
attribute position, a non-breakable space, a CJK punctuation mark copied from
a blog post.

**Fix:** delete the character or replace it with its ASCII equivalent. If
pasting from a PDF, retype the offending line by hand.

### E1002 — unterminated string literal

A `"` opened a string but no closing `"` was found before end-of-line / EOF.

**Fix:** add the closing `"`. Buff strings cannot span lines without an
explicit continuation; use string interpolation or concatenation for
multi-line text.

### E1003 — invalid numeric literal

A token started like a number (`0x`, `0b`, digits) but is malformed:
`0xGH`, `1.2.3`, `0b2`.

**Fix:** correct the literal. Hex digits are `0-9a-fA-F`, binary `0-1`, and a
float has exactly one `.`.

### E1004 — mixed tabs and spaces in indentation

The single most common Buff error for users coming from Python or editors that
default to tabs. Buff mandates **4 spaces**; tabs are forbidden.

**Fix:** convert tabs to spaces in your editor. Set `"insertSpaces": true,
"tabSize": 4` in VSCode; the Buff extension enforces this automatically.

### E1005 — inconsistent indentation level

Two consecutive lines at the "same" block level use different space counts
(e.g. 4 spaces then 6 spaces in the same block).

**Fix:** pick one indentation width (4) and apply it consistently.

### E1006 — unterminated block comment

A `/*` opened a block comment but no `*/` closed it.

**Fix:** add the closing `*/`. Block comments are NOT nestable in Buff.

### E1007 — unterminated regex literal

A `re"..."` literal is missing its closing `"`.

### E1008 — empty regex literal

`re""` is empty. A regex needs at least one character.

### E1009 — unterminated char literal

A `'a'` char literal is missing its closing `'`.

### E1010 — empty char literal

`''` is empty. A char literal needs exactly one character (or one escape).

### E1011 — invalid character escape

`\q` is not a valid escape. Valid escapes: `\n \r \t \\ \" \' \0` and
`\u{HEX}`.

### E1012 — invalid unicode escape

`\u{GGGG}` has non-hex digits, or the codepoint is invalid. Must be 1–6 hex
digits in `{}`, value ≤ `0x10FFFF`.

### E1013 — unexpected closing brace in string literal

A `}` inside a string literal closed an interpolation that wasn't open, or
stray `}` outside `{...}`.

### E1014 — unterminated interpolation in string literal

A `"hello {name"` opened an interpolation with `{` but never closed it with
`}`.

**Fix:** add the closing `}`. Or escape a literal `{` with `{{`.

## 8.3 Parsing — `E1101`–`E1109`

Errors from the recursive-descent + Pratt parser that turns tokens into an
AST.

### E1101 — expected a different token

The parser expected one token (e.g. `:` after a parameter list) but found
another. The message tells you both.

**Fix:** add the expected token at the position indicated.

### E1102 — unexpected token in this position

The token is valid in Buff but not where it appeared (e.g. a `return` at the
top level outside a function).

### E1103 — expected newline after `:` for layout block

A `:` opened a block but the same line had more tokens. Buff blocks are
`header:` then a newline then an indented body.

**Fix:** put the body on the next line, indented 4 spaces.

### E1104 — expected indented block after `:`

A `:` + newline was followed by a dedented (or equally-indented) line. The
block body must be indented deeper than the `:` line.

**Fix:** indent the body by 4 spaces.

### E1105 — function declarations must be top-level

A `func` appeared nested inside another block. Buff functions are top-level
only (use lambdas `{ x => ... }` for nested callables).

### E1106 — expected an identifier

A position that requires a name (variable, function, parameter) has a keyword
or punctuation instead.

### E1107 — unterminated delimited list

A `(`, `[`, or `{` opened a group but the matching closer was never found.

### E1108 — unsupported ABI in `extern` declaration

`extern "C"` and `extern "Rust"` are the only supported ABIs.

### E1109 — generics are not supported on `extern` functions

`extern func foo<T>(...)` is rejected. Extern functions cannot be generic.

## 8.4 Type-checking — `E1201`–`E1209`

Errors from type inference, exhaustiveness, and module resolution.

### E1201 — undefined variable

The name is not in scope. Check the spelling, or add an `import`.

### E1202 — binary operator applied to incompatible types

`"hello" + 5` — string + int. The message tells you the expected and found
types.

**Fix:** convert one side: `String(5)` gives `"5"`, so `"hello" + String(5)`
works. Or use interpolation: `"hello{5}"`.

### E1203 — assignment type mismatch

`let x: Int = "not an int"` — the declared type doesn't match the value.

**Fix:** either change the declaration or convert the value. `Int(x)`
converts with `Option<Int>` return; use `.or(default: ...)` to handle parse
failure.

### E1204 — unary operator applied to invalid operand type

`-true` — negation on a Bool. Negation is for numbers; `!` is for Bool.

### E1205 — `if` condition must be `Bool`

`if 5:` — the condition isn't a boolean.

**Fix:** `if 5 > 0:` or `if Bool(5):` (the latter is rarely what you want).

### E1206 — `if` and `else` branches have different types

```buff
let x = if cond: 5 else: "hello"
```

The two branches produce `Int` and `String` — incompatible. Both branches of
an `if` used in value position must produce the same type.

**Fix:** make both branches the same type, or restructure to assign in each
arm.

### E1207 — non-exhaustive `match`

A `match` doesn't cover every possible value. The compiler tells you which
pattern(s) are missing.

**Fix:** add the missing arm(s), or add a `_` catch-all as the last arm.

### E1208 — `@prefer(gpu)` is not allowed on recursive functions

GPU dispatch requires the function to be lowered to a single WGSL shader;
recursion can't be expressed in WGSL. The runtime detects recursion via DFS
cycle detection (T48) and rejects the annotation.

**Fix:** remove `@prefer(gpu)` from the recursive function, or split the
recursive part into a separate non-annotated helper.

### E1209 — module / import resolution error

An `import` path couldn't be resolved: the file doesn't exist, the imported
name isn't exported, or there's a circular import.

**Fix:** check the path (relative to the current file), check that the name
is `export`ed, and break any import cycles.

## 8.5 Code generation — `E1301`–`E1304`

Errors (and one warning) emitted while lowering the AST to Rust.

### E1301 — unsupported language feature in code generation

The AST is valid but the codegen can't lower it yet. Usually a "this feature
is codegen-verified but not end-to-end" case — the message names the specific
node.

**Fix:** usually none until the feature ships; check the example status
table in the root README for workarounds.

### E1302 — codegen produced invalid rust (internal compiler error)

The generated Rust doesn't parse. This is a compiler bug — please report it
with the `.buff` source and the generated `.rs` if possible.

### E1303 — `block()` inside an async function can deadlock

Calling a sync `block()`-style helper from within an async function can
deadlock the tokio runtime. The lint warns you.

**Fix:** restructure to avoid the blocking call, or mark the calling function
as non-async.

### E1304 — codegen cannot lower a comptime value to Rust

A `comptime` expression evaluated to something the codegen can't embed (e.g.
a closure value). Comptime is limited to literal-embeddable values.

## 8.6 Runtime — `E1401`–`E1410`

Errors surfaced by the heterogeneous compute runtime.

### E1401 — GPU dispatch failed and no CPU fallback was available

The GPU path errored and the function was `@force(gpu)` (no fallback). The
message includes the underlying GPU error.

**Fix:** either remove `@force(gpu)` to allow CPU fallback, or fix the
underlying GPU issue (often E1403 / E1404 / E1406).

### E1402 — GPU shader execution fault

The compiled WGSL shader ran but faulted. This usually indicates a bug in the
generated shader — report it.

### E1403 — GPU adapter or device initialization failed

The GPU was present at startup but failed to initialize (driver issue,
adapter vanished).

**Fix:** update GPU drivers; fall back to CPU with `@prefer(cpu)`.

### E1404 — no GPU adapter is available on this host

No GPU at all. Common on headless servers or CI runners.

**Fix:** use `@prefer(gpu)` (auto-fallback) instead of `@force(gpu)`, or run
on a host with a GPU.

### E1405 — input exceeds the VRAM tiling budget

The dataset is too large for the GPU's memory.

**Fix:** chunk the input into smaller batches and dispatch each separately.

### E1406 — WGSL shader rejected by the GPU pipeline compiler

The GPU's driver rejected the WGSL the Buff compiler generated. Rare; usually
a driver/GPU mismatch or a bug in shader generation.

### E1407 — `Channel.send` failed — all receivers dropped

You called `.send()` on a `Channel<T>` after all receivers were dropped. The
message has nowhere to go.

**Fix:** keep a receiver alive, or handle the `Err` returned by `.send()`.

### E1408 — `Channel.receive` returned none — all senders dropped

`.receive()` returned `None` because every sender was dropped. This is the
"stream ended" signal.

**Fix:** treat `None` as end-of-stream in your receive loop.

### E1409 — spawned async task panicked before completing

A `spawn task()` panicked. The panic was caught (the runtime doesn't crash)
and surfaced as this error.

**Fix:** inspect the panic message; fix the underlying bug in the task.

### E1410 — runtime operation exceeded its deadline

A GPU dispatch or async operation took longer than its deadline. The runtime
cancelled it.

**Fix:** raise the deadline, or reduce the input size.

## 8.7 Warnings — `E1501`–`E1510`

Lint warnings surfaced by `buff check` and codegen. These do **not** fail
compilation; they advise you of suspicious or dead code.

### E1501 — use of a deprecated API

You called a function marked `@deprecated("msg", since = "1.0")`.

**Fix:** switch to the replacement named in the deprecation message.

### E1502 — unused variable

A `let` binding is never read.

**Fix:** either use the variable, prefix it with `_` (`let _x = ...`) to
signal intent, or remove it.

### E1503 — unused function parameter

A function parameter is never used in the body.

**Fix:** prefix with `_` (`func f(_x: Int)`) or remove it.

### E1504 — unreachable code after a terminator

Code after a `return`, `break`, `continue`, or infinite loop can never run.

**Fix:** remove the dead code, or fix the terminator that shouldn't be there.

### E1505 — unused import

An `import { name }` is never referenced.

**Fix:** remove the import.

### E1506 — branch can never execute (dead code)

An `if False:` branch, or an `else` after a branch that always returns.

**Fix:** remove the dead branch.

### E1507 — `let` binding shadows an outer binding

```buff
let x = 5
if cond:
    let x = 10   // shadows the outer x
```

Shadowing is legal but often a bug.

**Fix:** rename the inner binding unless shadowing is intentional.

### E1508 — enum variant is never constructed

An enum variant is declared but no code ever constructs it.

**Fix:** remove the variant, or construct it somewhere.

### E1509 — `if` / `else` branches produce identical values

Both branches evaluate to the same thing — the `if` is pointless.

**Fix:** remove the conditional.

### E1510 — `_` wildcard appears before the last explicit arm

```buff
match x:
    _: print("anything")     // _ catches everything
    5: print("five")         // unreachable!
```

The `_` must be the **last** arm; arms after it are dead.

**Fix:** move `_` to the end, or replace it with the specific patterns you
meant to match.

## 8.8 How errors render

A diagnostic renders in one of two forms:

- **With a code:** `[Error] error[E1001]: unexpected character: '@'`
- **Without a code:** `[Error] message`

Both are public API and byte-stable. The `E1xxx` tag is `Option<ErrorCode>` —
ad-hoc diagnostics (internal assertions, etc.) render without one.

The diagnostic includes a span (file + line + column) and, where helpful, a
suggestion:

```
[Error] error[E1203]: assignment type mismatch
   --> broken.buff:2:18
    |
  2 |     let x: Int = "not an int"
    |                   ^^^^^^^^^^^^ expected `Int`, found `String`
```

The `--->` and `|` gutter mirrors `rustc`'s diagnostic style deliberately —
if you can read a Rust error, you can read a Buff error.

## 8.9 The error suggestion engine (T63)

Starting in v1.20, the compiler includes a suggestion engine that proposes
fixes for common errors. When you misspell a name, the suggestion engine
proposes the closest in-scope identifier:

```
[Error] error[E1201]: undefined variable `prnit`
   --> hello.buff:2:5
    |
  2 |     prnit("hello")
    |     ^^^^^
    |     did you mean `print`?
```

Suggestions are powered by edit-distance over the names in scope. They appear
on E1201 (undefined variable), E1209 (module resolution), and a growing list
of other codes. The suggestion engine also maps `rustc` errors back to `.buff`
source positions via the SpanMap ([Chapter 5 §5.12](./chapter-5.md)) when a
post-codegen Rust error surfaces.

## 8.10 Disabling warnings

You cannot disable a warning globally (Buff has no `#[allow(...)]` attribute
equivalent today). The intentional choice is: fix the warning, or prefix the
binding with `_`. This keeps codebases clean — a warning you can silence is a
warning you'll ignore until it hides a real bug.

The exception is `@deprecated`, which you can suppress on a per-call basis
with `@allow_deprecated` (planned; not yet shipped). For now, migrating off
the deprecated API is the only path.

## 8.11 Where to read more

- **The generated catalog:** [`docs/errors/index.html`](../../docs/errors/index.html)
  — one page per code, with a longer explanation and a fix recipe. Open it in
  any browser; no server needed.
- **The source of truth:** [`crates/buff-lang-error/src/code.rs`](https://github.com/buff-lang/buff/blob/v1x-frameworks/crates/buff-lang-error/src/code.rs)
  — the `ErrorCode` enum. If a code is here, it has a page on the site
  (enforced by test).
- **The stability policy:** conventions doc §19, mirrored above in §8.1.
- **Regenerating the site:** `cargo run -p buff-lang-error --example gen_error_docs`
  writes the HTML files from the enum.

---

*Next: [Chapter 9 — Migration Guides](./chapter-9.md)*
