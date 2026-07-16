# Buff v0.5 Learnings

## T18 — Double (f64) Full Support

### Status: COMPLETE (no implementation changes needed)

The Double type was already fully implemented across all layers:

**Lexer** (`buff-lang-lexer/src/lexer.rs`):
- `d` and `D` suffixes on float literals produce `TokenKind::DoubleLit(f64)`
- Tests: `test_double_literal_d_suffix`, `test_double_literal_capital_d_suffix`

**AST** (`buff-lang-ast/src/expr.rs`):
- `Literal::Double(f64)` variant exists with Display impl

**Parser** (`buff-lang-parser/src/expr.rs`):
- `TokenKind::DoubleLit(v)` → `Expr::Literal(Literal::Double(*v), span)`
- Test: `test_double_literal` in expr_tests.rs

**Types** (`buff-lang-types/src/`):
- `ty.rs`: `Type::Double` variant, `Type::double()` constructor, `is_numeric()`, `is_float_like()`, Display as "Double"
- `infer.rs`: `Literal::Double(_)` → `Type::double()`, `typeref_to_type("Double")` → `Some(Type::double())`
- `promote.rs`: `Double` dominates all non-decimal numerics (`Double + Float → Double`, `Double + Int → Double`)
- `assignable_to`: `Int → Double`, `Float → Double` widening passes

**Codegen** (`buff-lang-codegen-rust/src/rust_codegen.rs`):
- `Literal::Double(d)` → `syn::LitFloat` with `f64` suffix
- `buff_type_to_syn(Type::Double)` → `"f64"`
- `ast_typeref_to_syn("Double")` → `"f64"`

**Tests added for T18 acceptance criteria**:
- `double_inference` in `buff-lang-types/tests/infer_tests.rs`: 3.14d → Double, 3.14 → Float, Double+Double → Double, Double+Float → Double, Float+Double → Double, Int+Double → Double
- `double_codegen` in `buff-lang-codegen-rust/tests/literal_tests.rs`: `let x = 3.14d` → `let x: f64 = 3.14f64;`, `let y = 1.0d + 2.0` → `let y: f64 = ...`

### Key insight
The `d` suffix is handled at the **lexer** level (logos tokenizer), not the parser. The lexer detects `d`/`D` after a decimal number and produces `DoubleLit`. This means the parser, type checker, and codegen never see the suffix — they just see a `Double` literal. This is a clean design that was already in place.

## T19 — Byte (Bits<8>) Support

### Status: COMPLETE (minimal changes — most plumbing already existed)

The Byte type was already fully implemented across all layers. Only tests needed to be added for the T19 acceptance criteria.

**What already existed (no changes needed):**

**Lexer** (`buff-lang-lexer/src/lexer.rs`):
- `0xFF`/`0b1010` → `TokenKind::ByteLit(u8)` via `scan_number` → `byte_lit()` helper
- Overflow check: values > 255 produce `LexerError::invalid_number`
- Tests: `byte_hex_and_binary` (unit), `test_byte_literal_hex_uppercase/lowercase/binary/zero` (integration)

**AST** (`buff-lang-ast/src/expr.rs`):
- `Literal::Byte(u8)` variant with Display as `Byte(0x{val:02X})`

**Parser** (`buff-lang-parser/src/expr.rs`):
- `TokenKind::ByteLit(v)` → `Expr::Literal(Literal::Byte(*v), span)`

**Types** (`buff-lang-types/src/`):
- `ty.rs`: `Type::Bits { width: IntWidth }`, `Type::byte()` → `Bits<8>`, `is_numeric()`, `is_integer_like()`
- `infer.rs`: `Literal::Byte(_)` → `Type::byte()`, `typeref_to_type("Byte")` → `Some(Type::byte())`
- `promote.rs`: `Bits + Bits → max width`, `Int + Bits → Int` (signed wins)

**Codegen** (`buff-lang-codegen-rust/src/rust_codegen.rs`):
- `Literal::Byte(b)` → unsuffixed `syn::LitInt` (e.g. `255`)
- `buff_type_to_syn(Bits<8>)` → `"u8"`
- `ast_typeref_to_syn("Byte")` → `"u8"`

**Tests added for T19 acceptance criteria:**
- `hex_binary_literals` in `buff-lang-lexer/tests/lexer_tests.rs`: 0xFF → ByteLit(255), 0b1010 → ByteLit(10), 0x100 overflow error
- `byte_type` in `buff-lang-types/tests/infer_tests.rs`: 0xFF → Bits<8>, 0b1010 → Bits<8>, `let b: Byte = 0xFF` type-checks, Byte+Byte → Bits<8>, Byte+Int → Int
- `byte_codegen` in `buff-lang-codegen-rust/tests/literal_tests.rs`: `let b = 0xFF` → `let b: u8 = 255`, `let b: Byte = 0xFF` → `let mut b: u8 = 255`, `let b = 0b1010` → `let b: u8 = 10`
- `byte_codegen_snapshot` in `buff-lang-codegen-rust/tests/literal_tests.rs`: inline insta snapshot

**Side fix:** Fixed pre-existing `clippy::approx_constant` warnings in T18's `double_inference` and `double_codegen` tests (changed `3.14` to `2.5` to avoid π approximation detection).

**Deferred:**
- **Buffer indexing (`buf[i]` → `buf[i as usize]`)**: The AST has NO `Index` expression variant. The AST is frozen (per plan convention). Buffer indexing codegen is deferred until a future task that adds an Index variant to the AST. The `.len()` method call codegen already works through the existing `MethodCall` AST node.

## T20 — Decimal (128-bit) rust_decimal integration

### Status: COMPLETE

Closed the only real gap (Decimal *literal* flow) end-to-end. Most type/codegen
plumbing already existed from T10/T11/T12 (Type::Decimal, typeref_to_type,
buff_type_to_syn -> rust_decimal::Decimal, ast_typeref_to_syn("Decimal"),
promote.rs Decimal-dominates, rust_decimal+rust_decimal_macros workspace deps).

### What was added (the literal pipeline)

**Lexer** (`buff-lang-lexer/src/lexer.rs` + `token.rs`):
- NEW `TokenKind::DecimalLit(String)` carrying the RAW digit text (incl. decimal
  point, EXCLUDING the `m`/`M` suffix), e.g. `"99.90"`. Never rounds through f64.
- `scan_number`: the `m`/`M` branch previously ERRORED ("decimal 'm' suffix is not
  supported"); now emits `DecimalLit`. Handled in BOTH the fractional branch
  (`99.90m`) AND a new integer-only branch (`100m` -> DecimalLit("100")).
- Display: `decimal("99.90")`. Module doc updated.
- Test module `decimal_literals` (7 unit tests) + integration test
  `test_decimal_m_suffix_now_supported` REPLACED the obsolete
  `test_decimal_m_suffix_unsupported` (which asserted the old error).

**AST** (`buff-lang-ast/src/expr.rs`) — ADDITIVE change (see decisions.md):
- NEW `Literal::Decimal(String)` storing raw text. Display: `Decimal("99.90")`.
  Enum stays `PartialEq`-only (consistent with f32/f64 siblings).

**Parser** (`buff-lang-parser/src/expr.rs`):
- `TokenKind::DecimalLit(s)` -> `Expr::Literal(Literal::Decimal(s.clone()), span)`.
  Mirrors the Double/Byte mapping. 3 parser tests added.

**Types** (`buff-lang-types/src/infer.rs` + `ty.rs`):
- `infer_literal`: NEW arm `Literal::Decimal(_) => Type::Decimal` (99.90m infers
  as Decimal, NOT Double/Float).
- NEW predicates on `Type`: `is_gpu_eligible()` (true only for WGSL-native
  Float<32>/Int<32>/Bits<32>/Bool) and `must_run_on_cpu()` (complement; true for
  Decimal, Double, wide ints). This is TYPE METADATA ONLY — no dispatch engine
  exists in v0.5 (arrives v1.0); tests assert the predicate directly.
- Test module `decimal_type` (13 tests): literal inference, Decimal + - * / %
  -> Decimal, Decimal dominates Int/Float/Double, comparison -> Bool, let-binding,
  let-annotation match, unary neg, CPU-only predicate, compound-assign.

**Codegen** (`buff-lang-codegen-rust/src/rust_codegen.rs`):
- `lower_literal` early-returns for Decimal via NEW `lower_decimal_literal(raw)`
  which builds a `syn::Expr::Macro`: path `rust_decimal_macros::dec` + tokens =
  `syn::parse_str::<proc_macro2::TokenStream>(raw)`. Parsing the raw text into a
  TokenStream PRESERVES trailing zeros (`dec!(99.90)` not `dec!(99.9)`) and never
  transits f64. prettyplease prints `rust_decimal_macros::dec!(99.90)`.
- NEW helper `rust_path(&str)` (sibling to `rust_path_type`) builds a `syn::Path`
  for `::`-separated names.
- `let` bindings infer `rust_decimal::Decimal` annotation (already mapped).
- 5 codegen tests incl. inline insta snapshot for `let price = 99.90m`.

### Exactness proof (0.1m + 0.2m == 0.3m)
Two-layer proof: (1) codegen test `decimal_exact_arithmetic_codegen` shows the
generated Rust is `dec!(0.1) + dec!(0.2) == dec!(0.3)`; (2) `decimal_exact_arithmetic_proof`
asserts `dec!(0.1)+dec!(0.2)==dec!(0.3)` is TRUE directly via rust_decimal (a dep of
the codegen crate), AND contrasts `0.1_f64 + 0.2_f64 != 0.3_f64`. Chose the "rust_decimal
unit assertion" path (task's allowed OR) over a fragile ignored rustc test.

### Key insights
- The raw-text-through-TokenStream approach is what preserves exactness AND the
  trailing zero. Using `proc_macro2::Literal::f64_unsuffixed` would ROUND (99.90 -> 99.9).
- `syn::parse_str` is endorsed by the task and is NOT "raw-string codegen" (forbidden)
  — it builds syn/proc-macro2 tokens, fully within the syn/quote/prettyplease stack.
- The old `m`-suffix ERROR was a v0.1 placeholder; flipping it to support was the
  core lexer change. Capital `M` and integer `100m` both work.
- CPU-only is a type flag only; NO GPU dispatch analyzer was built (correctly deferred
  to v1.0 Phase per plan).

### Side fix
- `cargo fmt` applied to all 5 touched crates; this also normalized one PRE-EXISTING
  fmt violation in `buff-lang-parser/src/parser.rs:28` (the `parse()` signature line
  wrap) — that file was NOT semantically edited by T20 (only `expr.rs` was).
- Replaced obsolete lexer integration test `test_decimal_m_suffix_unsupported`.

### Deferred
- Real heterogeneous CPU/GPU dispatch engine (v1.0). `is_gpu_eligible()` / `must_run_on_cpu()`
  are the type-metadata hooks it will consume.
- An ignored rustc compile-and-run variant of the exactness proof (like move-test #17)
  was not added: plain `rustc` cannot easily link `rust_decimal` (needs --extern/cargo);
  the direct rust_decimal unit assertion is the equivalent, stronger-in-practice proof.

### Process note (notepad encoding)
The T20 notepad entries were originally written via PowerShell here-strings, which
silently corrupted backtick sequences (PowerShell `` `0 `` escape => NUL byte). The
affected notepad files were deleted and recreated via the `write` tool (clean UTF-8,
no BOM). Future notepad appends should avoid PowerShell here-strings containing
backticks — prefer the `write`/`edit` tools or single-quoted PS strings.

## T22 — Numeric coercion rules (flexible vs fixed Int modes)

### Status: COMPLETE

Implemented range analysis as a **pure module** (`range_analysis.rs`) that the
inference pass (and future T23/T67 collection literals) will call. No changes
to the AST or to the existing `promote_binary` precedence chain were needed —
the widening rules `Int+Float→Float` and `Float+Double→Double` already worked
end-to-end from T10/T20; T22 just pinned them with regression tests.

### What was added

**NEW module** `crates/buff-lang-types/src/range_analysis.rs` (~250 lines incl. 13 unit tests):
- `struct IntRange { min: i128, max: i128 }` — closed signed interval, `Copy`.
- Methods: `new` (defensive ordering), `exact(v)` (singleton), `width()` (delegates to `smallest_int_width`), `union` (if/else join), `sub_interval`, `mul_interval`.
- Operator-trait impls (the idiomatic Rust path; also satisfies clippy `should_implement_trait`):
  - `Add<Self>` — interval addition `[a.min+b.min, a.max+b.max]`, saturating.
  - `Sub<Self>` — delegates to `sub_interval` (`[a.min-b.max, a.max-b.min]`).
  - `Mul<Self>` — delegates to `mul_interval` (4-corner sound rule).
  - `Neg` — `[-max, -min]`.
- Free fns:
  - `smallest_int_width(min: i128, max: i128) -> IntWidth` — cascade of
    `i8/i16/i32/i64/i128` range checks. `5→W8`, `300→W16`, `100000→W32`, etc.
  - `collection_int_width(values: &[i128]) -> IntWidth` — min/max across slice,
    forwards to `smallest_int_width`. Empty slice → `W64` (Buff default).

**lib.rs**: added `pub mod range_analysis;` + crate-root re-exports of
`IntRange`, `smallest_int_width`, `collection_int_width`.

**Integration tests** `crates/buff-lang-types/tests/numeric_coercion.rs` (~390 lines, 27 tests):
- 6 cross-category widening tests (Int↔Float↔Double, nested arithmetic).
- 8 flexible-mode auto-width tests (5→i8, 300→i16, 100000→i32, 127/128 boundary,
  `x=127; y=x+1→i16`, negative widening, negation, union).
- 4 collection-helper tests (`[20,25,18]→W8`, large pair, negative min, empty→W64).
- 7 fixed-mode preservation tests (Int<32>+Int<32>→Int<32>, Int<8> stays Int<8>
  across Add/Sub/Mul/Div/Mod, BitAnd/Or/Xor/Shl/Shr, negation, mixed-width max).
- 1 overflow-contract documentation test (names the Rust-inherited behaviour,
  asserts the width invariant the contract depends on).
- 2 let-decl integration tests (fixed annotation pins width; plain `Int`
  annotation resolves to default Int<64>).

**Codegen tests** in `rust_codegen.rs::tests` (2 new tests):
- `t22_fixed_int_widths_map_to_native_rust_widths` — pins every `Int<W>` →
  native Rust width mapping (i8/i16/i32/i64/i128).
- `t22_fixed_int8_preserves_width_through_arithmetic` — codegen end of the
  fixed-preservation contract (Int<8>→i8, Int<32>→i32; not silently widened).

### Key design insights

- **Operator traits over inherent methods.** Initial implementation used
  inherent `add`/`sub`/`mul`/`neg` methods on `IntRange`; clippy's
  `-D warnings` rejected them (`should_implement_trait`). Fix: implement
  `Add`/`Sub`/`Mul`/`Neg`. More idiomatic Rust anyway (`x + y`, `-r`).
  The non-trait helpers `sub_interval`/`mul_interval` are retained as the
  documented implementation bodies the trait impls delegate to.

- **Overflow behaviour is inherited from Rust FOR FREE.** T22 spec says
  fixed-mode overflow must "panic in debug, wrap in release". Codegen
  already maps `Int<W>` → native Rust integer; Rust's native `+`/`-`/`*`
  already has exactly this overflow contract. NO `checked_*` calls need
  to be emitted — the simplest correct path. The T22 codegen tests pin
  the *mapping contract* (the width that flows into Rust's operators),
  which is the actual hook where the behaviour lives.

- **No AST change.** Unlike T20/T21 which added new `Literal`/`Expr`
  variants, T22 is a pure-types-crate addition. The new module lives
  entirely inside `buff-lang-types` and is consumed by tests today; the
  inference pass will call it once flexible-mode literal inference is
  wired in (a follow-up — see Deferral below).

- **i128 is the right internal width.** Using `i128` for `IntRange::min`/`max`
  lets a single interval type soundly represent the bounds of every signed
  Rust width up to and including `i128` itself. Saturating arithmetic on
  i128 is unreachable for any practical Buff program.

- **Plain `Int` annotation still means Int<64>.** Flexible narrowing is for
  *unannotated* `let x = 5`; an explicit `let x: Int = 5` keeps the default
  width (Int<64>) — pinned by `let_decl_int_annotation_uses_int64_default`.
  `typeref_to_type("Int")` returns `Type::int_default()` and T22 does not
  change that.

### Deferred (to T23/T67 — Wave 6 collection literals)

- **End-to-end `[...] → Vector<Int<8>>` inference.** The AST has no
  array/collection-literal expression yet (T23/T67 adds it). T22
  implements `collection_int_width(&[i128])` as a pure, reusable function
  and tests it directly with value slices. End-to-end inference will call
  this helper once T23/T67 lands. Documented in decisions.md §T22.

- **Flexible literal inference in `TypeInferencer`.** Today `Literal::Int(_) → Type::int_default()`
  (always Int<64>); T22 does NOT change that mapping. Wiring the range
  analysis into the inferencer (so `let x = 5` infers `Int<8>` not
  `Int<64>`) is a cross-cutting change that should land together with
  T23/T67's collection-literal inference. The pure-function foundation is
  in place and tested; the wiring is deferred to keep T22 atomic and
  non-breaking.

- **Runtime overflow-panic test.** A `#[should_panic]` test that exercises
  Rust's native i8 overflow would test Rust, not Buff codegen. The
  mapping-contract test (`t22_fixed_int_widths_map_to_native_rust_widths`)
  is the correct hook: it pins the width that flows into Rust's operators,
  which is where the debug-panic/release-wrap behaviour comes from.

### Verification (all green)
- `cargo test -p buff-lang-types`           115 pass (31 lib + 56 infer_tests + 27 numeric_coercion + 1 doc)
- `cargo test -p buff-lang-codegen-rust`    108 pass (25 lib + 83 across 5 integration test files)
- `cargo check --workspace`                 PASS
- `cargo clippy -p buff-lang-types -p buff-lang-codegen-rust --all-targets -- -D warnings`  0 warnings
- `cargo fmt -p buff-lang-types -p buff-lang-codegen-rust -- --check`  PASS

### Side notes
- Pre-existing `cargo fmt --check` diff in `buff-lang-error/tests/span_test.rs`
  (import alphabetical ordering) was NOT touched (that crate is out of T22's
  scope). Following T20 precedent, only the two touched crates were fmt'd.
- The `unused manifest key: workspace.dev-dependencies` warning is pre-existing
  (workspace Cargo.toml); not introduced by T22 and out of scope.

## T99 — Process environment access (args/env/exit)

### Status: COMPLETE

Added three prelude functions for process environment access, extending the T96
prelude infrastructure. This is a small additive change — no new AST nodes, no
new parser/lexer changes.

### What was added

**Types** (`buff-lang-types/src/ty.rs`):
- `Type::Vector(Box<Type>)` — generic vector type (maps to Rust `Vec<T>`)
- `Type::Option(Box<Type>)` — generic option type (maps to Rust `Option<T>`)
- Constructors: `Type::vector(elem)`, `Type::option(inner)`

**Prelude** (`buff-lang-types/src/prelude.rs`):
- `PreludeCategory::System` — new category for process env functions
- `PreludeFn::Args` — `args()` → `Vector<String>`
- `PreludeFn::Env` — `env("NAME")` → `Option<String>`
- `PreludeFn::Exit` — `exit(code)` → `Void`

**Codegen** (`buff-lang-codegen-rust/src/rust_codegen.rs`):
- `buff_type_to_syn`: early-return for `Type::Vector` → `Vec<T>` and `Type::Option` → `Option<T>` via `make_generic_path_type`
- `lower_prelude_call` arms for `Args`/`Env`/`Exit` using `quote!` + `syn::parse2`

**Codegen mappings:**
| Buff | Rust |
|------|------|
| `args()` | `std::env::args().collect::<Vec<String>>()` |
| `env("PATH")` | `std::env::var("PATH").ok()` |
| `exit(0)` | `std::process::exit(0)` |

### Key design decisions

- **`quote!` + `syn::parse2`** for the codegen arms (same pattern as `lower_read_line`).
  This avoids raw-string codegen while keeping the expressions readable.
- **`Type::Vector` and `Type::Option`** are new generic type variants. They're
  needed so the type system can represent the return types of `args()` and `env()`.
  Full collection support (indexing, iteration) is deferred to T23.
- **`PreludeCategory::System`** keeps the env functions grouped separately from
  I/O (print/read_line) and Math/Convert.

### Deferred
- **`args()[0]` indexing** requires the array/index expression AST node (T23).
  The end-to-end scenario `func main(): let a = args(); print(a[0])` cannot work
  until T23 lands. The codegen shape of `args()` itself is verified.

### Verification
- `cargo test -p buff-lang-codegen-rust env_access` → 7/7 pass
- `cargo test -p buff-lang-types` → 149 pass (all)
- `cargo check --workspace` → clean
- `cargo clippy -p buff-lang-types -p buff-lang-codegen-rust --all-targets -- -D warnings` → clean
- `cargo fmt --check` → clean

## T23 — Vector<T> type + codegen (collections, indexing, closures)

### Status: COMPLETE (all green: test/check/clippy/fmt)

### Two new ADDITIVE AST nodes (precedent: T20/T21 migration-note pattern)
- `Expr::ArrayLit { elements: Vec<Expr>, span }` — collection literal `[1,2,3]`.
- `Expr::Index { base: Box<Expr>, index: Box<Expr>, span }` — `base[index]`.
Both added to `crates/buff-lang-ast/src/expr.rs` with doc-comment migration
notes. MUST extend every `match` on `Expr`: `span()`, `Display` (expr.rs),
`collect_uses` (ir.rs — caught by `cargo check` as non-exhaustive), parser,
type inferencer, codegen. The ir.rs match was NOT obvious — `cargo check`
surfaced it; always run check after adding an Expr variant.

### Closure parsing decision: implemented MINIMAL (not deferred)
`{ params => expr }` (1+ comma-separated ident params, single-expr body)
parses to the existing `Expr::Lambda` node. Codegen lowers to Rust `|p| body`.
This unblocked `.map/.filter/.reduce` WITHOUT waiting for T34. Param types
use a placeholder `TypeRef::Named{name:"_"}` — codegen emits NO type
annotation (Rust infers). Multi-stmt bodies / typed params / captures = T34.

### Codegen forms that WORK (verified by re-parse + substring asserts)
- `[1,2,3]` -> `vec![1, 2, 3]` via `syn::Macro` with `Bracket` delimiter.
  **GOTCHA:** `quote!{ vec![ }` FAILS — proc-macro2 can't take raw `[`/`]`
  as literal tokens in a quote. Build the macro with `MacroDelimiter::Bracket`
  and put ONLY the comma-separated elements in `tokens` (the brackets come
  from the delimiter). Empty -> `vec![]`.
- `v[i]` -> `v[i as usize]`. **GOTCHA:** the shared `cast_to()` helper wraps
  EVERY operand in parens -> `v[(0) as usize]`. Wrote a dedicated
  `cast_to_usize()` that only wraps non-atomics (Binary/Unary/Cast/Range) so
  the common `v[0 as usize]` / `v[i as usize]` forms stay clean.
- `.map/.filter` -> `v.into_iter().<m>(closure).collect::<Vec<_>>()`.
  `.reduce` -> `v.into_iter().reduce(closure)` (returns Option<T>).
  Use `.into_iter()` (NOT `.iter()`) so closure params are OWNED — Buff hides
  references. This consumes the receiver (move-by-default, correct).
- `.push/.pop/.len` need NO special mapping — the default passthrough arm
  `recv.method(args)` already produces the right Rust.

### Auto-width (T22 integration)
`infer_collection_element` calls `range_analysis::collection_int_width` for
all-int-literal collections -> `let v = [1,2,3]` infers `Vec<i8>`.
**GOTCHA:** the parser represents `-200` as `UnaryOp(Neg, Lit(Int(200)))`,
NOT `Literal::Int(-200)`. Added `const_int_value()` helper that recognises
both forms so `[-200, 5]` still auto-widens to `Vec<i16>`.

### Parser: postfix index vs string rejection (T21 preserve)
The OLD parse_postfix `LBracket` arm rejected ALL `expr[...]`. NEW behavior:
reject ONLY string-LITERAL receivers (`"abc"[0]`) with the T21 helpful
error; all other receivers (ident, call, nested) build `Expr::Index`. This
unblocks T99 `args()[0]`. Updated the old `test_string_indexing_rejected`
(used ident `s[0]`) to `test_string_literal_indexing_rejected` (uses
`"abc"[0]`) + added `test_ident_indexing_parses_to_index`.

### Clippy: collapsible nested `if let`
`if let Some(first) = x.first() { if let Expr::Lit = first {...} }` trips
clippy `collapsible_if_let`. Collapse to ONE pattern:
`if let Some(Expr::Literal(lit, _)) = elements.first() { ... }` (match
ergonomics binds `lit: &Literal`). Avoids let-chains (edition-2021 gated).

### Test-assertion discipline (cost me re-runs)
prettyplease inserts SPACES in macro token streams: `vec![- 200, 5]` (not
`-200`), `collect:: < Vec < String > > ()` (spaced turbofish), `() [0]`
(space before index). Assert on the FUNCTIONAL signal (`Vec<i16> =`,
`[0 as usize]`, `as usize`) + `syn::parse_str::<syn::File>` re-parse, NOT
on exact whitespace. The Lambda/Param/Block `Display` impls render
`fn(x: _) { ExprStmt(...) }` — account for `: ty` and the `ExprStmt` wrapper
when writing parser shape() assertions.

### Verification (all GREEN)
- `cargo test -p buff-lang-codegen-rust --test vector_codegen` → 20/20 pass
- `cargo test -p buff-lang-parser -p buff-lang-types -p buff-lang-ast -p buff-lang-codegen-rust` → 0 failed across all binaries
- `cargo check --workspace` → exit 0
- `cargo clippy -p buff-lang-ast -p buff-lang-parser -p buff-lang-types -p buff-lang-codegen-rust --all-targets -- -D warnings` → exit 0
- `cargo fmt -p <4 crates> -- --check` → exit 0



## T24 — Matrix<T> type + codegen (flat storage, 2-D indexing)

### Index-2D decision: extend Expr::Index to indices: Vec<Expr>
T23's Expr::Index { base, index, span } (single index) was GENERALIZED to
{ base, indices: Vec<Expr>, span } so one node serves 1-D Vector and 2-D
Matrix indexing. This is a MIGRATION (renamed/retyped field), not purely
additive — but the ripple was exactly 8 match sites (grep-enumerated first):
expr.rs (span + Display), ir.rs (collect_uses), infer.rs (Index arm),
parser/expr.rs (LBracket arm), codegen rust_codegen.rs (lower_expr Index),
vector_codegen.rs (index_expr helper), parser/expr_tests.rs (2 Display
assertions). All updated in one pass; cargo check --workspace stayed green.

**Key lesson**: when extending an AST variant's field shape, grep for ALL
construction + match sites FIRST (Expr::Index\s*\{), enumerate them, then
update in one batch. The Display format changed from Index(base, index) to
Index(base, [index]) (list wrapped in [...]) — parser snapshot/shape
tests that pin Display strings need updating too.

### Matrix struct injection: emit-on-demand via AST scan
The Matrix<T> builtin struct must be EMITTED into generated Rust (it's not a
user struct — T26 not done). Chose emit-on-demand over always-emit: a
program_uses_matrix(decls) walker scans for any Matrix.new(...)
MethodCall (receiver Ident "Matrix", method "new"). If found, generate()
PREPENDS the struct + impl (2 syn::Items) before any function item. The
struct/impl are built via syn::parse_str::<File> on a fixed Rust template
(same role as the quote! templates elsewhere — NOT raw-string codegen; the
single string producer is still prettyplease::unparse).

**On-demand keeps non-Matrix programs clean** — a program with no Matrix
reference gets no Matrix struct leaked into its output (asserted by
matrix_codegen_not_emitted_when_unused).

### 2-D index codegen: lower base ONCE, clone into two field positions
m[r, c] -> m.data[(r * m.cols + c) as usize]. The base m is lowered
ONCE and the resulting SynExpr is cloned into m.data (field_access) and
m.cols (field_access) positions. This preserves the move analyzer's clone
decision (if any) baked into the lowered base. The flat formula gets ONE
trailing s usize via cast_to (which parenthesises its operand) ->
exactly (r * m.cols + c) as usize. prettyplease prints this verbatim, no
extra parens.

### Storage is flat Vec<T> (GPU-ready) — NOT Vec<Vec<T>>
The Matrix struct carries data: Vec<T> (single contiguous buffer), not
Vec<Vec<T>> (which would fragment and not be directly uploadable to a WGSL
storage buffer). The 
ew(rows, cols) impl fills with ec![T::default();
rows * cols]. The flat-index formula ow * cols + col is the SAME formula
a WGSL shader would compute to address a storage buffer -> the REFACTOR goal
(share flat-storage with GPU buffer codegen) lands naturally here.

### Test discipline (no insta snapshots needed)
Followed vector_codegen.rs precedent: substring assertions on the functional
signal + syn::parse_str::<syn::File> re-parse. No insta snapshots ->
no .snap.new/.pending-snap files to manage. The exact T24 spec string
m.data[(1 * m.cols + 2) as usize] is asserted verbatim and passes because
cast_to + ield_access produce exactly that prettyplease output.

### Verification (all GREEN)
- cargo test -p buff-lang-codegen-rust --test matrix_codegen -> 15/15 pass
- cargo test --workspace -> 0 failed, 0 errors across all binaries
- cargo check --workspace -> exit 0
- cargo clippy --workspace --all-targets -- -D warnings -> exit 0
- cargo fmt -p <4 touched crates> -- --check -> exit 0

## T26 � Struct type + repr(C) codegen

- T11 codegen returns `CodegenError` for unsupported Decl variants; T26
  replaces that arm with a real `lower_struct_decl` that emits
  `#[derive(Clone, Debug)] pub struct Name { pub f: <rust_ty>, ... }`.
- Field types reuse the existing `ast_typeref_to_syn` (NOT
  `buff_type_to_syn` � that one takes the post-inference `Type`, but
  struct fields have a `TypeRef` in the AST). Int?i64, Float?f32,
  String?String, Decimal?rust_decimal::Decimal, etc.
- syn 2.0.119's `syn::Field` requires a `mutability: FieldMutability`
  field (NOT `Option<...>`). Use `syn::FieldMutability::None` for
  immutable fields.
- syn's `Meta::List` for `#[derive(...)]` and `#[repr(...)]` can be
  built by hand via `proc_macro2::TokenStream` + `quote!`; no need for
  `parse_quote!` (which would panic on parse failure).
- Parser struct-init disambiguation: `if cond { ... }`, `for x in iter
  { ... }`, and `Type { field: value }` ALL produce an Ident-typed
  expression followed by `{` at the postfix level. A peek-ahead is
  REQUIRED: only consume the `{` if its contents start with `}` (empty
  struct-init) or `Ident :` (first field). Otherwise fall through and
  let the outer parser handle the `{` as a block body.
- Buff's move-by-default semantics (T33a) insert `.clone()` on the
  second use of a non-Copy local. Two field accesses on the same local
  `p` produce `p.name` THEN `p.clone().age` � that's correct codegen,
  not a bug.
- prettyplease pins the type suffix on typed float literals:
  `1.0f32` (not `1.0`) when the value comes from a Float Expr.
- insta inline snapshot for a struct decl renders as 4 lines:
  `#[derive(Clone, Debug)]` / `pub struct Point {` / `    pub x: f32,`
  / `    pub y: f32,` / `}`.
- The codegen-rust test crate needs `buff-lang-lexer` and
  `buff-lang-parser` as DEV-deps to run parser round-trip tests from
  inside the codegen test binary. Add to `[dev-dependencies]` only
  (NOT regular deps � the codegen lib itself never depends on them).
- pub API additions: `RustCodegen::mark_struct_repr_c` (setter),
  `format_file` (alias for `format`).

### Verification (all GREEN)
- cargo test -p buff-lang-codegen-rust --test struct_codegen -> 19/19
- cargo test --workspace -> 0 failed
- cargo check --workspace -> exit 0
- cargo clippy --workspace --all-targets -- -D warnings -> exit 0
- cargo fmt --all -- --check -> exit 0
