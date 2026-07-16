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

## T22 — Numeric coercion (flexible vs fixed Int modes): pure module, no AST change

**Date:** T22 (v0.5).  **Scope:** additive, non-breaking.

**Change:** Added a NEW module `crates/buff-lang-types/src/range_analysis.rs`
with a pure range-analysis API: `IntRange` (closed i128 interval with widening
arithmetic via `Add`/`Sub`/`Mul`/`Neg` operator traits + `union` for joins),
`smallest_int_width(min, max) -> IntWidth`, and `collection_int_width(values)
-> IntWidth`. The module is re-exported at the crate root.

**No AST or `Type` change** (unlike T20/T21). T22 lives entirely in the
types crate and is consumed by tests today; the inference pass will call
into it once flexible-mode literal inference is wired in (deferred to T23/T67
— see below).

### Design choices

**1. Range analysis is a PURE module.** The T22 plan called for "extract
range tracker into `buff-lang-types/src/range_analysis.rs`" (REFACTOR step).
I went one step further and made it pure from the start: the module knows
nothing about the AST, the `TypeInferencer`, or the parser. It exposes
`IntRange` + two free functions. Tests today call it directly; the
inference pass (and T23/T67 collection literals) will call it later.
Benefits: (a) trivially testable, (b) reusable from any call-site,
(c) no risk of circular deps with `infer.rs`.

**2. i128 internal width.** `IntRange::min`/`max` are `i128`. A single
interval type then soundly represents the bounds of every signed Rust
width up to and including `i128` itself. Saturating arithmetic guards
the (unreachable-in-practice) endpoints.

**3. Operator traits over inherent methods.** Initial draft used inherent
`add`/`sub`/`mul`/`neg` methods on `IntRange`. Clippy `-D warnings`
rejected them (`should_implement_trait`). Switched to `Add`/`Sub`/`Mul`/`Neg`
trait impls — more idiomatic Rust anyway (`x + y`, `-r`). The non-trait
helpers `sub_interval` / `mul_interval` are kept as the documented
implementation bodies the trait impls delegate to.

**4. Overflow behaviour is inherited from Rust FOR FREE — no `checked_*` emitted.**
The T22 spec said fixed-mode overflow must "panic in debug, wrap in
release". Codegen ALREADY maps each fixed `Int<W>` to the corresponding
native Rust integer (`i8`/`i16`/`i32`/`i64`/`i128`) via `buff_type_to_syn`,
and Rust's native `+`/`-`/`*` operators ALREADY have exactly this overflow
contract. So T22 does NOT emit explicit `checked_*` calls — the simplest
correct path. The T22 codegen tests (`t22_fixed_int_widths_map_to_native_rust_widths`,
`t22_fixed_int8_preserves_width_through_arithmetic`) pin the mapping contract
so a regression in `buff_type_to_syn` cannot silently widen every fixed-width
integer (which would change the overflow boundary).

**5. Plain `Int` annotation still means `Int<64>`.** Flexible narrowing
(value 5 → i8) is for UNANNOTATED `let x = 5`. An explicit `let x: Int = 5`
keeps the default width (Int<64>) — pinned by `let_decl_int_annotation_uses_int64_default`.
`typeref_to_type("Int")` returns `Type::int_default()` and T22 does not
change that. The parser does not yet produce `Int<8>` TypeRefs (T11
limitation), so `let x: Int<8>` is not parseable today; the fixed-width
mapping is exercised via direct `IntRange` / `Type::Int { width: W8 }`
construction in tests.

### CRITICAL DEFERRAL: collection-literal end-to-end inference

The T22 plan's "RED: `[20, 25, 18] → Vector<Int<8>>`" line cannot be
tested end-to-end because **the AST has no array/collection-literal
expression yet** — `[20,25,18]` does not parse until T23/T67 (Wave 6).
Per the task instructions, T22 instead:

1. Implements `collection_int_width(values: &[i128]) -> IntWidth` as a
   pure, reusable function.
2. Tests it directly with value slices: `collection_int_width(&[20,25,18]) == W8`,
   `&[100000,200000] == W32`, etc. (4 dedicated tests + 5 unit tests in
   the module).
3. Documents (here) that end-to-end `[...] → Vector<Int<8>>` inference
   is deferred to **T23/T67**, which will call `collection_int_width`
   when lowering collection literals.

This satisfies the auto-width acceptance intent without a fake end-to-end
test.

### DEFERRAL: flexible literal inference in `TypeInferencer`

Today `infer_literal` maps `Literal::Int(_) => Type::int_default()` (always
Int<64>); T22 does NOT change that mapping. Wiring range analysis into the
inferencer (so `let x = 5` infers `Int<8>` rather than `Int<64>`) is a
cross-cutting change that will land together with T23/T67's
collection-literal inference. The pure-function foundation (`IntRange`,
`smallest_int_width`, `collection_int_width`) is in place and tested; the
wiring is deferred to keep T22 atomic and non-breaking.

### Why no runtime overflow-panic test
A `#[should_panic]` test that asserts `i8::MAX + 1` panics in debug builds
would test *Rust's* overflow contract, not Buff's codegen. The
mapping-contract test is the correct hook: it pins the *width* that flows
into Rust's native operators, which is where the debug-panic/release-wrap
behaviour actually comes from. T23/T67 (or a future runtime-evaluation
task) can layer a runtime test on top.

**Rollback:** removing `range_analysis.rs`, the `pub mod range_analysis;`
line + 3 re-exports from `lib.rs`, the `tests/numeric_coercion.rs` file,
and the 2 `t22_*` tests in `rust_codegen.rs::tests` reverts T22 cleanly
(additive-only change set; no other crate's behaviour changes).

