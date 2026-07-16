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

## T99 — Process environment access: args/env/exit codegen shape only

**Date:** T99 (v0.5).  **Scope:** additive, non-breaking.

**Change:** Added three prelude functions (`args`, `env`, `exit`) to the T96
prelude infrastructure, plus two new generic type variants (`Type::Vector<T>`,
`Type::Option<T>`) to represent their return types.

### Design choices

**1. `Type::Vector<T>` and `Type::Option<T>` are new enum variants.**
These are needed so the type system can represent `args() -> Vector<String>`
and `env("NAME") -> Option<String>`. They are additive (no existing variant
changed). Full collection support (indexing, iteration, methods) is deferred
to T23.

**2. `quote!` + `syn::parse2` for codegen.** The `args()`/`env()`/`exit()`
codegen arms use the same pattern as `lower_read_line`: build a token stream
via `quote!`, then parse it via `syn::parse2` (which returns `Result`, not
panic). This avoids raw-string codegen while keeping the expressions readable.

**3. `PreludeCategory::System`.** The env functions are grouped under a new
category separate from I/O (print/read_line) and Math/Convert.

### DEFERRAL: `args()[0]` indexing

The end-to-end scenario `func main(): let a = args(); print(a[0])` uses
Vector indexing (`a[0]`), which requires the array/index expression AST node
(T23 — not yet done). T99 verifies the codegen SHAPE of each prelude call
individually; the indexing integration is deferred to T23.

### Rollback
Removing the `Type::Vector`/`Type::Option` variants + `PreludeFn::Args`/`Env`/`Exit`
+ `PreludeCategory::System` + the codegen arms + `tests/env_access.rs` reverts
T99 cleanly (additive-only change set).

## T23 — Additive AST changes: Expr::ArrayLit + Expr::Index (+ minimal closures)

**Date:** T23 (v0.5).  **Scope:** additive, non-breaking.

### A. Two new `Expr` variants

Added to `buff_lang_ast::Expr` (`crates/buff-lang-ast/src/expr.rs`), mirroring
the T20/T21 additive pattern (migration doc-comment on the enum + per-variant
doc-comment):

- `Expr::ArrayLit { elements: Vec<Expr>, span: Span }` — collection literal
  `[e1, e2, ...]` (empty `[]` and trailing comma allowed).
- `Expr::Index { base: Box<Expr>, index: Box<Expr>, span: Span }` — indexing
  `base[index]`.

**Why additive is safe:** no existing variant was renamed, reordered, or had
its payload altered. `Expr` derives only `Debug, Clone, PartialEq` (not `Eq`),
so the new `Vec<Expr>`/`Box<Expr>` payloads are consistent. Every internal
`match` on `Expr` was extended with arms: `span()`, `Display` (expr.rs),
`collect_uses` (ir.rs), parser, type inferencer, Rust codegen. `cargo check`
surfaces any missed match as a non-exhaustive error (it caught ir.rs).

These two nodes unblock the deferred items: T99 `args()[0]`, T21 typed-String
index rejection, and T22 collection auto-width end-to-end.

### B. Closure decision: implemented MINIMAL (not deferred)

`.map({x => x * 2})` needs closure parsing. Closures are officially T34, but
T34 is not done. **Decision: implement a minimal closure form now** rather
than defer, because it is small, additive, and unblocks the .map/.filter/
.reduce acceptance immediately.

The AST already had `Expr::Lambda { params, body, return_type, span }`
(defined in T2) but it was never parsed. T23 adds:
- **Parser:** `{ ident (, ident)* => expr }` -> `Expr::Lambda` with the body
  wrapped in a one-statement `Block` (`Stmt::ExprStmt`). 1 or 2 params
  (enough for map/filter/reduce).
- **Codegen:** `Expr::Lambda` -> Rust closure `|p1, p2| body`. Param types
  are NOT annotated (Rust infers) — a placeholder `TypeRef::Named{name:"_"}`
  is stored on the `Param` and ignored by codegen.

T34 will extend this to multi-statement bodies, typed params, and capture
analysis. The minimal form is a strict subset, so T34 only ADDS capability.

### C. Vector method codegen forms

| Buff                       | Rust                                                          |
|----------------------------|--------------------------------------------------------------|
| `v.push(x)`                | `v.push(x)` (default passthrough arm)                        |
| `v.pop()`                  | `v.pop()` (default passthrough arm)                          |
| `v.len()`                  | `v.len()` (default passthrough arm)                          |
| `v.map({x => e})`          | `v.into_iter().map(\|x\| e).collect::<Vec<_>>()`             |
| `v.filter({x => e})`       | `v.into_iter().filter(\|x\| e).collect::<Vec<_>>()`          |
| `v.reduce({a, b => e})`    | `v.into_iter().reduce(\|a, b\| e)` (returns `Option<T>`)     |

**Why `.into_iter()` (not `.iter()`):** closure params must be OWNED values
to match Buff's "hide references from the user" philosophy. `.iter()` yields
references, which would force closure bodies to deref (`*x`) — leaking Rust
borrow ergonomics into Buff. `.into_iter()` consumes the receiver, which is
correct under Buff's move-by-default semantics. `.collect::<Vec<_>>()` rebuilds
a `Vec` so the result is indexable/chainable.

**Why `.reduce` returns `Option<T>`:** Rust parity. A non-Option fold-style
reduce (with a required initial value) is deferred — the 2-arg `reduce`
closure maps directly to `Iterator::reduce`.

### D. Index codegen: dedicated `cast_to_usize`

`base[index]` -> `base[index as usize]` (Buff's `Int` is `i64`, which can't
index a Rust `Vec` directly). The shared `cast_to()` helper wraps EVERY
operand in parens (`(0) as usize`), which is ugly for the common literal/ident
index case. A dedicated `cast_to_usize()` wraps only non-atomic indices
(`Binary`/`Unary`/`Cast`/`Range`), yielding clean `v[0 as usize]` /
`v[i as usize]` while still protecting `(a + b) as usize` precedence.

### E. Parser: string-literal-only index rejection (T21 preserve)

The OLD `parse_postfix` `LBracket` arm rejected ALL `expr[...]`. T23 narrows
the rejection to **string-LITERAL receivers only** (`"abc"[0]`), preserving
the T21 helpful error ("for strings use .chars() or .first()"). All other
receivers (ident, call result, nested index) build `Expr::Index`; a future
type-check pass can reject typed-String indexing (e.g. `s[0]` where `s: String`).
This is the documented trade-off: parse-time rejection stays for the
unambiguous literal case; typed rejection is deferred to type checking.

### F. Auto-width via T22 range analysis

`TypeInferencer::infer_collection_element` recognises integer-literal
collections and calls `range_analysis::collection_int_width` to pick the
element width: `[1,2,3]` -> `Vector<Int<8>>`, `[300]` -> `Vector<Int<16>>`.
A `const_int_value()` helper recognises BOTH `Literal::Int(v)` and
`UnaryOp(Neg, Literal::Int(v))` (the parser-realistic form for negatives, since
`-200` lexes as unary-minus-on-`200`), so `[-200, 5]` auto-widens to `i16`.

### Rollback

Removing the two `Expr` variants + their arms in (span/Display/ir/parser/
infer/codegen) + `parse_array_literal`/`parse_closure`/postfix-index +
`lower_array_lit`/`lower_lambda`/`lower_into_iter_*`/`cast_to_usize` +
`infer_collection_element`/`const_int_value` + the two test files reverts T23
cleanly (additive-only change set).



## T24 — Matrix<T> type + codegen decisions

### A. Index-2D representation: EXTEND Expr::Index to indices: Vec<Expr>

**Decision**: Generalize T23's Expr::Index { base, index, span } to
{ base, indices: Vec<Expr>, span }. **Rejected alternatives**:
  - (b) A separate Expr::Index2D { base, row, col, span } variant — truly
    additive but doesn't generalize to N-D, duplicates codegen arms.
  - (c) A Matrix-specific desugar at parse time — leaks Matrix knowledge into
    the parser, breaks AST generality.

**Rationale**: the task explicitly preferred Vec<Expr> "if the ripple is
manageable". The ripple was exactly 8 match sites (grep-enumerated). ONE node
shape serves 1-D and 2-D today and N-D tensors (v1.0+) tomorrow. The
generalization is forward-compatible — additional indices just lengthen the
vec. This is a **migration** (not purely additive): the field was renamed +
retyped, so every construction/match site was updated and documented in the
T24 migration note (expr.rs). Single-index call sites now pass ec![index];
the Display wraps the list in [...] (Index(base, [idx])).

**Codegen dispatch** is on indices.len(): 1 -> Vector path
(ase[idx as usize]), 2 -> Matrix path
(ase.data[(row * base.cols + col) as usize]), other -> unsupported error.

### B. Matrix builtin-struct injection: EMIT-ON-DEMAND

**Decision**: Emit the Matrix<T> struct + 
ew impl into generated Rust
ONLY when a program references Matrix (detected via Matrix.new(...) scan),
prepending the 2 items before any function item. **Rejected alternatives**:
  - Always emit (like an implicit prelude) — bloats every program's output
    even when Matrix is unused.
  - Require an import — violates Buff's prelude-implicit philosophy.

**Detection**: program_uses_matrix(decls) walks all FuncDecl bodies looking
for any Expr::MethodCall { receiver: Ident("Matrix"), method: "new", .. }.
Conservative but sufficient: every well-formed Matrix program must construct
one first. A type-annotation-only Matrix<T> (no constructor) is a rare edge
case deferred to a later task.

**Build method**: the struct + impl are built via syn::parse_str::<File> on
a fixed Rust template. This plays the SAME role as the quote! token-stream
templates used elsewhere (lower_read_line, lower_into_iter_collect) — a
compile-time-fixed scaffold re-parsed into syn Items. It is NOT raw-string
Rust codegen: the single string producer remains prettyplease::unparse.

### C. Type::Matrix(Box<Type>) — additive, mirrors Type::Vector

**Decision**: Add Type::Matrix(Box<Type>) to the resolved-type enum (ty.rs),
mirroring the T99 Type::Vector(Box<Type>) pattern. Constructor
Type::matrix(elem), Display Matrix<{elem}>, codegen mapping
Matrix<T> -> Matrix<i64> path (Unknown elem falls back to i64 so the
annotation compiles). Inference: Matrix.new(...) -> Type::Matrix(Unknown)
(element deferred without evidence); m[r, c] on Matrix<T> -> T.

### D. 2-D index codegen: base lowered ONCE, spliced via clone

**Decision**: In lower_matrix_index, lower the base expression ONCE and
clone the resulting SynExpr into the two field-access positions (m.data,
m.cols). This preserves whatever clone decision the move analyzer baked
into the lowered base. The flat formula ow * cols + col is built as a
binary tree (Mul(row, Field(m, cols)) then Add(.., col)) and wrapped in
ONE s usize cast via cast_to (parenthesises operand). Output:
m.data[(row * m.cols + col) as usize] — exactly the T24 spec string.

**Note**: for a non-Ident base, the clone would double-evaluate in Rust, but
well-formed Matrix programs index through an Ident binding, so this is the
common path. A borrow-aware treatment is a future refinement.

### E. Single-index regression guard

The T24 Index generalization must not break T23's 1-D Vector indexing.
matrix_codegen_single_index_unchanged asserts [0] still lowers to
[0 as usize] (NOT the Matrix .data path). The codegen dispatch
(indices.len() == 1 -> Vector path, == 2 -> Matrix path) keeps the two
concerns cleanly separated in one node.

### Rollback

Reverting T24 means: restore Expr::Index { base, index: Box<Expr>, span }
(update the 8 match sites back), remove Type::Matrix + its arms, remove the
Matrix.new detection in lower_method_call, remove lower_matrix_index/
lower_matrix_new/field_access/program_uses_matrix/matrix_struct_items,
remove the Matrix injection in generate(), restore the 2 parser Display
assertions, restore the vector_codegen index_expr helper, delete
matrix_codegen.rs. Clean reversal (the change set is self-contained).
