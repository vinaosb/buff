# Buff v0.5 Decisions

## T20 — Additive AST change: Literal::Decimal(String) (migration plan)

**Date:** T20 (v0.5).  **Scope:** additive, non-breaking.

**Change:** Added a new variant `Literal::Decimal(String)` to the
`buff_lang_ast::Literal` enum (`crates/buff-lang-ast/src/expr.rs`). The variant
stores the **raw source digit text** of a `99.90m` literal (the digits incl.
decimal point, EXCLUDING the `m`/`M` suffix).

**Rationale:** Decimal was the only primitive with a `Type` representation
(`Type::Decimal` existed since T10) and a codegen mapping (`rust_decimal::Decimal`,
T11/T12) but NO literal flow — the lexer hard-errored on the `m` suffix. T20
closes that gap. Storing raw text (not f64) is mandatory: Decimal's whole point
is exactness, so the value must never round through an IEEE-754 float. The raw
text flows lexer -> AST -> parser -> codegen and is emitted verbatim inside
`rust_decimal_macros::dec!(<text>)`, preserving trailing zeros like the 0 in
`99.90`.

**Why additive is safe (no migration needed):**
- No existing variant was renamed, reordered, or had its payload type altered.
- The enum derives only `Debug, Clone, PartialEq` (NOT `Eq`), so the new
  `String` payload is consistent with the existing `String` variant.
- All internal `match`es on `Literal` were extended with a `Decimal` arm:
  `infer_literal` (types), `lower_literal` (codegen), `Display` (ast).
- Exhaustive external matches (if any) would get a non-exhaustive warning; none
  exist outside the workspace's own crates, and `cargo check --workspace` is green.

**Companion `Type` change:** `Type::Decimal` already existed; T20 only added the
two dispatch-metadata predicates `is_gpu_eligible()` / `must_run_on_cpu()` to
`Type` (additive methods, no enum change) so Decimal is flagged CPU-only.

**Token change:** `TokenKind::DecimalLit(String)` added (additive) to the lexer.
The old `m`-suffix LexerError branch in `scan_number` was replaced with the
emit path; the obsolete `test_decimal_m_suffix_unsupported` integration test was
replaced by `test_decimal_m_suffix_now_supported`.

**Rollback:** removing the `Decimal` arms + variant + token + the two `Type`
methods reverts T20 cleanly (additive-only change set).

## T21 — Additive AST/type changes for Char + String interpolation

**Date:** T21 (v0.5).  **Scope:** additive, non-breaking.

### A. `Literal::Char(char)` (mirrors T20's additive pattern)

Added `Literal::Char(char)` to `buff_lang_ast::Literal`
(`crates/buff-lang-ast/src/expr.rs`). Stores a single Rust `char` (one Unicode
scalar value). The lexer's new `TokenKind::CharLit(char)` feeds it; the parser
maps `CharLit` -> `Literal::Char`; the codegen emits `syn::Lit::Char`.

**Why `char` (not `String` or a custom struct):** Buff's Char is exactly Rust's
`char` — a Unicode scalar value, not a grapheme cluster. Reusing the primitive
keeps the FFI boundary zero-cost and matches user intuition from other languages
with a `Char` type.

**Companion type:** `Type::Char` added to `buff_lang_types::Type`
(`crates/buff-lang-types/src/ty.rs`). Maps to Rust `char` in
`buff_type_to_syn` and `ast_typeref_to_syn`. Not GPU-eligible (no WGSL scalar)
but T21 does NOT add a `must_run_on_cpu` predicate — Char is unlikely to appear
in numeric kernels, so the predicate is deferred to when collections arrive.

### B. `Expr::StringInterp { parts: Vec<InterpPart>, span }` + `InterpPart`

Added a new `Expr` variant for interpolated strings, plus a small enum
`InterpPart { Literal(String) | Expr(Box<Expr>) }` living next to `Literal` in
`crates/buff-lang-ast/src/expr.rs`. The `ir::collect_uses` free-variable
analysis was extended with an arm for `StringInterp` (it visits each
`InterpPart::Expr` and ignores literal text).

**Why a new variant (not reusing existing expr nodes):** The pre-T21 parser
collapsed simple strings to `Literal::String`. A brand-new variant was the
cleanest representation of "alternating literal text and embedded expressions"
without polluting `BinaryOp`/`MethodCall`. The fast path (no `{...}` in the
string) STILL produces `Literal::String`, so existing tests and behaviour are
preserved bit-for-bit — `StringInterp` only appears when interpolation actually
occurs.

**Codegen shape:** `format!` macro built via `quote!`-spliced tokens. The
format string escapes literal `{`/`}` as `{{`/`}}`; each `InterpPart::Expr`
contributes one `{}` slot and one positional argument.

### C. `s[0]` rejection at parser layer (NOT a new AST node)

The task asked for a helpful error on direct string indexing. T19 confirmed
there is no `Expr::Index` variant. Rather than add one solely to reject it,
T21 adds an `LBracket` arm in `parse_postfix` that returns a `ParseError`
whose message contains the required substring
"direct indexing `expr[...]` is not supported; for strings use .chars() or .first() instead"
plus a `with_note` explaining the UTF-8 soundness rationale.

**Layer that emits the error:** PARSER (parse_postfix). Documented in the test
`test_string_indexing_rejected` and in the error message itself.

### D. String-method codegen mapping (no AST change)

`Expr::MethodCall` was wired into `lower_expr` (it previously fell through to
the `_` unsupported arm). String methods are recognised by name in
`lower_method_call`:

| Buff              | Rust                                                       |
|-------------------|------------------------------------------------------------|
| `.char_count()`   | `.chars().count()`                                         |
| `.byte_len()`     | `.len()`                                                   |
| `.chars()`        | `.chars()`                                                 |
| `.bytes()`        | `.bytes()`                                                 |
| `.graphemes()`    | `unicode_segmentation::UnicodeSegmentation::graphemes(&s, true).collect::<String>()` |
| `.first()`        | `.chars().next()`                                          |
| `.last()`         | `.chars().last()`                                          |
| `.slice(a, b)`    | `.chars().skip(a).take(b - a).collect::<String>()`         |
| (anything else)   | plain `recv.method(args)` (passthrough)                    |

Unknown methods pass through unchanged so user-defined methods still work.

### E. New workspace dependency: `unicode-segmentation`

Added `unicode-segmentation = "1.12"` to `[workspace.dependencies]` and to
`buff-lang-codegen-rust`'s `[dependencies]`. Required by the `.graphemes()`
mapping (REFACTOR step of the task).

**Deferral:** wiring `unicode-segmentation` into the *generated* program's
Cargo.toml is NOT done in T21. The scaffold template
(`BUFF_TOML_TEMPLATE` in `buff-lang-cli/src/scaffold.rs`) emits a Buff
project file, not a Rust Cargo.toml; the Rust Cargo.toml is built later in
the transpile pipeline. Adding the dep there requires touching the build
pipeline (`buff build` / `buff run`), which is out of scope for T21. Any
generated program that calls `.graphemes()` will fail to compile until that
wiring lands; tracked as deferred work.

**Rollback:** removing the new AST variants + `Type::Char` + the
`TokenKind::CharLit` token + the `LBracket` parser arm + the `lower_method_call`
arm + the `unicode-segmentation` dep reverts T21 cleanly.

