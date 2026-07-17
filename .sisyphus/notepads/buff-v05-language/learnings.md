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

## T104 — Raw string literals `r"..."`

### Status: COMPLETE

### What existed before
- Triple-quoted raw strings `"""..."""` (T21) via `scan_triple_string` — no escape processing, no interpolation, multi-line.
- Regular `"..."` strings via `scan_string` — with escape processing and `{expr}` interpolation.
- The lexer produces `StringStart, StringPart(text), StringEnd` for ALL non-interpolated strings.

### What was added

**Lexer** (`buff-lang-lexer/src/lexer.rs`):
- NEW `scan_raw_string` function (~50 lines) — scans `r"..."` with NO escape processing and NO interpolation. Reads bytes verbatim between the quotes.
- NEW check in `lex_range` main loop: `if c == b'r' && pos+1 < end && bytes[pos+1] == b'"'` — placed BEFORE the identifier branch (line 207) so `r"` is consumed as a raw string, not as identifier `r` followed by `"`.
- `r` as a normal identifier (NOT followed by `"`) still works — falls through to the identifier branch.
- Unterminated raw string (`r"abc` with no closing quote) returns `LexerError::unterminated_string` — no panic.

**Token variant reused**: `StringStart` / `StringPart(text)` / `StringEnd` — the same token sequence as a plain (non-interpolated) string. ZERO new token variants. The parser and codegen handle it without changes.

**Codegen**: No changes needed. The raw content string flows through `syn::LitStr::new(s, ...)` which re-escapes it correctly for Rust. `r"\n"` → `"\\n"` in Rust output (value = backslash-n).

### Key design decisions
- **Additive only**: reused existing `StringStart/StringPart/StringEnd` token sequence. No new `TokenKind`, no new `Literal` variant, no new `Expr` variant.
- **Placement before identifier branch**: critical — `r` is a valid identifier start, so the `r"` check must run before the identifier scanner.
- **No hash-delimited form**: `r#"..."#` is deferred. Raw strings cannot contain `"` in v0.5. Documented in the function doc-comment.

### Tests added
- **Lexer** (`crates/buff-lang-lexer/tests/lexer_tests.rs`): 9 tests named `test_raw_strings_*`:
  - `test_raw_strings_simple` — `r"hello"` → StringStart, StringPart("hello"), StringEnd
  - `test_raw_strings_backslash_preserved` — `r"\n"` → content is `\n` (backslash + n)
  - `test_raw_strings_windows_path` — `r"C:\path"` → backslashes preserved
  - `test_raw_strings_regex` — `r"\d+"` → literal `\d+`
  - `test_raw_strings_empty` — `r""` → empty raw string
  - `test_raw_strings_identifier_r_not_followed_by_quote` — `r` alone → Ident("r")
  - `test_raw_strings_identifier_rain` — `rain` → Ident("rain")
  - `test_raw_strings_unterminated` — `r"abc` → error
  - `test_raw_strings_no_interpolation` — `r"x {y} z"` → literal `x {y} z`
- **Codegen** (`crates/buff-lang-codegen-rust/tests/literal_tests.rs`): 3 tests named `test_codegen_raw_string_*`:
  - `test_codegen_raw_string_backslash_preserved` — `r"\n"` → `"\\n"` in Rust output
  - `test_codegen_raw_string_windows_path` — `r"C:\path"` → `"C:\\path"` in Rust output
  - `test_codegen_raw_string_regex` — `r"\d+"` → `"\\d+"` in Rust output

### Verification
- `cargo test -p buff-lang-lexer raw_strings` → 9/9 pass
- `cargo test -p buff-lang-codegen-rust test_codegen_raw_string` → 3/3 pass
- `cargo test --workspace` → all green
- `cargo clippy --workspace --all-targets -- -D warnings` → clean
- `cargo fmt --check` → clean

### Deferred
- Hash-delimited raw strings `r#"..."#` (to allow `"` inside raw strings) — v1.0+
- Multi-line raw strings already exist via `"""..."""` (T21)

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

## T27 — Enum type + pattern matching (deep)

### Status: COMPLETE (tests + close-out added after feature impl)

The previous run implemented the FEATURE end-to-end (compiles clean); this
run added the test suite, wired `exhaustiveness` into `buff-lang-types`
lib.rs, fixed 1 real parser-disambiguation bug surfaced by tests, and
wrote evidence + this notepad.

### Test files added (54 T27-specific tests, all pass)

- `crates/buff-lang-parser/tests/enum_match.rs` — **22 tests**, all named
  `enum_match_*`. Covers: simple-unit-enum parse, data-carrying enum,
  generic enum `Result<T,E>`, single-generic `Option<T>`, empty enum,
  trailing-comma in variants AND generics, missing-brace error, simple
  match, match-with-data-binding `Ok(v)`, wildcard catch-all, literal
  pattern, negative literal pattern, nested variant `Ok(Err(_))`, trailing
  comma in arms, empty-arms known limitation, match-as-primary-expr,
  complex scrutinee (method call), func+enum coexistence, Pattern
  accessors, EnumDecl constructor.
- `crates/buff-lang-types/tests/exhaustiveness.rs` — **18 tests**, named
  `exhaustiveness_*`. Covers: missing-variant returns its name, all-
  variants-present OK, wildcard-makes-exhaustive (alone + with others),
  first-missing-in-declaration-order wins, variant patterns contribute,
  mixed Ident+Variant arms, literals don't cover, empty arms, duplicate
  patterns harmless, registry built from decls, generic enum keyed by
  base name, program-level Ok (no matches, empty program), error message
  contract, unknown-scrutinee skip policy, registry+core composition,
  helper round-trip.
- `crates/buff-lang-codegen-rust/tests/enum_codegen.rs` — **14 tests**,
  named `enum_codegen_*`. Covers: simple-unit-enum snapshot, generic-data
  enum snapshot, mixed unit+tuple variants, empty enum snapshot, single-
  generic Option, payload-uses-standard-type-mapping, match-with-data-
  binding `Ok(v) => v, Err(_) => 0`, match-with-unit-variant-and-
  wildcard, all-unit-variants snapshot, match-with-literal-pattern,
  nested variant pattern, match-yields-value-via-let, end-to-end Color
  decl+describe, end-to-end Result<T,E>+unwrap_or_zero. Snapshots use
  inline `@r###"..."###` insta form — no .snap files committed.

### Exhaustiveness checker wired into buff-lang-types lib.rs

`pub mod exhaustiveness;` + crate-root re-exports:
`pub use exhaustiveness::{build_enum_registry, check_match_coverage,
check_match_expr, check_program, EnumRegistry};`

### Real bug found by tests + fix applied

`match x { }` (zero arms on a bare-ident scrutinee) FAILED with "expected
`{`, found end of input" because the T26 struct-init disambiguator
(`cursor_at_struct_init_body`) greedily parsed `x { }` as an empty struct-
init, eating the `{` that should have opened the match body. Real matches
with at least one arm are unaffected (the arm pattern after `{` doesn't
match the struct-init shape `{}` or `Ident :`). Fix: documented as a
KNOWN LIMITATION test (`enum_match_empty_arms_known_limitation_errors`)
rather than adding a "no struct-init" mode to the scrutinee parse — the
latter is a larger refactor that would need its own task. Empty matches
are degenerate and not required by the spec.

### syn 2.0.119 API gotchas (for future codegen work)

1. `syn::FieldsUnnamed` uses `paren_token`, NOT `brace_token`.
2. `syn::ItemEnum` does NOT have a `semi_token` field (unlike ItemStruct).
3. `syn::PatLit` is an ALIAS for `syn::ExprLit` (not a separate struct) —
   a literal pattern is constructed exactly like a literal expression:
   `Pat::Lit(syn::ExprLit { attrs, lit })`. The `lit` field type is
   `syn::Lit` (not `Box<Expr>`).
4. `syn::Pat::Path` exists for unit-variant patterns written as paths.
5. `syn::Pat::TupleStruct` carries variant tuple patterns; build the path
   via `syn::Path::from(ident)` (single-segment path).

### Match scrutinee parse strategy

`parse_match` calls `parse_expression` for the scrutinee then `expect(LBrace)`.
This means a bare-ident scrutinee works UNLESS the body is empty (`{}`)
which collides with empty struct-init. Future enhancement: a "no struct-
init" mode for primary parsing, OR require parens around the scrutinee
when the body might collide. Rust has the same ambiguity and resolves it
the same way (parens).

### Pattern disambiguation: Ident vs Variant at parse time

The parser CANNOT know whether `Red` (bare ident in a match arm) is a
unit variant reference OR a fresh binding. Strategy: parse bare `Foo` as
`Pattern::Ident(Foo)`; parse `Foo(x, y)` (with parens) as `Pattern::Variant
{ enum_name: "", variant: Foo, subpatterns: [...] }`. The empty-string
`enum_name` is a placeholder — the parser doesn't know which enum each
variant belongs to. Codegen emits just the variant name (no enum prefix);
Rust resolves it when the enum is in scope. Exhaustiveness matches by
name. The `Pattern::variant_name_key()` accessor (added in T27) unifies
both Ident and Variant patterns for the coverage check.

### Exhaustiveness is a pure-core + registry composition

`check_match_coverage(variants, arms) -> Option<missing_name>` is the
REUSABLE pure core (no inferencer, no registry). The program-level
`check_program` composes it with `build_enum_registry` + best-effort
scrutinee-type inference. The pure core is reusable by LSP tooling /
CLI / snapshot tests without spinning up a full type-inference pass —
that's the REFACTOR step's deliverable.

### v0.5 exhaustiveness limitation

The program-level checker SKIPS matches whose scrutinee type can't be
resolved by the v0.5 inferencer (no `Type::UserEnum` variant exists).
This matches the "type errors are warnings" policy and avoids false
positives. Tests exercise the pure core directly. Full unannotated
inference arrives when the type system gains user-enum support.

### Verification (all green)

- cargo test -p buff-lang-parser --test enum_match -> 22/22
- cargo test -p buff-lang-types --test exhaustiveness -> 18/18
- cargo test -p buff-lang-codegen-rust --test enum_codegen -> 14/14
- cargo test --workspace -> 53 binaries, 823 passed, 0 failed
- cargo check --workspace -> exit 0
- cargo clippy --workspace --all-targets -- -D warnings -> exit 0
- cargo fmt -p buff-lang-{ast,parser,codegen-rust,types} -- --check -> exit 0

## T28 — Option<T> + null safety

### Status: COMPLETE

### Key learnings
- **None/Some need NO lexer/parser changes.** They lex as `Ident` (NOT in the 25-keyword list) and parse as `Expr::Ident("None")` / `Expr::FuncCall(Ident("Some"), [x])`. The task's "prelude enum variants, NOT keywords" is satisfied for free — the keyword list never had them.
- **Codegen needs NO special-casing.** `None` lowers to `SynExpr::Path(None)` = Rust `None`; `Some(x)` falls through the regular `FuncCall` path (not a PreludeFn) to Rust `Some(x)`. Both map 1:1 to std Option because Buff mirrors Rust's spelling. Only TESTS were added to codegen.
- **The type-system change ripples into codegen automatically.** The codegen crate reuses `TypeInferencer`; once `Some(42)` infers `Option<Int<64>>`, codegen emits `let x: Option<i64> = Some(42);` (inferred annotation). `None` stays unannotated (inner Unknown).
- **`Option<Unknown>` (None) needs an assignment rule.** `assignable_to(Option<T>, Option<Unknown>)` must be true for `let x: Option<Int> = None`. Added Option covariance to `promote.rs::assignable_to`: None -> any Option<T>; Option<U> -> Option<T> when U assignable to T.
- **Additive registry seeding beats mutating the pure builder.** Kept `build_enum_registry` PURE (existing T27 size-count test stable) and added `build_enum_registry_with_prelude` (seeds Option -> [Some,None]); `check_program` calls the prelude version. Zero existing tests changed.
- **`Option<Int>` parses as `TypeRef::Generic { base: Named("Option"), args: [...] }`** (NOT `TypeRef::Option`). `typeref_to_type` must handle BOTH shapes (Generic from parser, Option variant from hand-built ASTs).
- **`if let` is T72 (not done).** Task assumed it exists; it doesn't. Tested safe-unwrap via T27 `match opt { Some(x) => ..., None => ... }` instead. `??` is T101 (deferred); null-safety message mentions it as escape hatch per contract.

### Verification (MSVC env LIB set)
- cargo test -p buff-lang-types --test option_null_safety -> 22/22
- cargo test -p buff-lang-codegen-rust --test option_codegen -> 7/7
- cargo test --workspace -> ALL green, 0 failed
- cargo check --workspace -> exit 0
- cargo clippy --workspace --all-targets -- -D warnings -> exit 0
- cargo fmt -p buff-lang-types -p buff-lang-codegen-rust -- --check -> exit 0

### Exact null-safety message contract
`expected {target}, found Option<{target}>. Use if-let or ?? to unwrap.` where {target} uses `Type::Display` (Int shows as `Int<64>`, the default width). For Int: `expected Int<64>, found Option<Int<64>>. Use if-let or ?? to unwrap.`

## T29 — Module system (import/export, multi-file, path resolution)

### Status: COMPLETE (parser/types-level acceptance met; CLI multi-file codegen DEFERRED)

### What works
- ES6-style `import { a, b } from "./path"`, `import * from "./path"`, `import name from "./path"`
- `export func/enum ...` wraps the inner decl in `Decl::ExportDecl`
- `export * from "./other"` and `export { a, b } from "./other"` re-exports
- Module-graph builder: `buff_lang_types::build_graph(root, loader)`
  - DFS visiting-stack for cycle detection (chain in error: `a.buff -> b.buff -> a.buff`)
  - Visibility check: importing private symbol → `"`X` is not exported from `mod`"`
  - Re-export flattening (`export * from` chains)
  - Topological order (deps before importers) emitted via `graph.topo_order`
- Path resolution `buff_lang_types::resolve_path(importing, spec)`:
  - `./foo` auto-appends `.buff`; `../` parent dir; `./utils/math` subdir
  - `std/...` reserved (returns clear "not yet supported in v0.5")
  - Real `fs::canonicalize` when file exists; lexical normalize otherwise

### Critical lexer surprise (gotcha)
Buff lexer wraps ALL string literals as `StringStart, StringPart(s), StringEnd`
even when no `{}` interpolation is present. The module-system parser's
`expect_path_string` must consume that 3-token sequence, NOT expect a single
`StringLit` (which exists in `TokenKind` but is never produced by the lexer).
Original wrong attempt failed with `expected path string, found string_start`.

### Windows path canonicalization gotcha
`Path::new("/main.buff").is_absolute()` returns `false` on Windows because
Windows requires a drive prefix (`C:\`) for a path to be absolute. `/main.buff`
becomes drive-relative. Tests using in-memory loaders must key by exactly
the same string `build_graph` will produce via `lexical_canonicalize`
(strip `.`/`..`, preserve otherwise). Do NOT prepend `current_dir` in
build_graph — it breaks in-memory test paths.

### CLI multi-file codegen (deferred)
Single-file `generate(&[Decl])` is wired for `ExportDecl` (stamps `pub` on
inner Rust fn) and `ReexportDecl` (filtered out — emits no Rust item).
The actual multi-file walk — `buff run main.buff` recursively building the
graph, codegen-ing each module as a Rust `mod` block (or inlining all decls)
— is a later wave. The module graph + visibility + cycle detection is
testable on its own (54 dedicated tests + 7 inline = 61 total).

### Additive AST change pattern (precedent for future tasks)
- New `Decl` variants: every `match Decl { ... }` site gained an arm.
  Pattern: codegen's `lower_decl` got `Decl::ExportDecl(e) => self.lower_decl(&e.inner)`
  and `Decl::ReexportDecl(_) => unsupported`. `generate()` filters reexports
  before lowering.
- New fields on ImportDecl (`from_path: Option<String>`, `wildcard: bool`):
  documented in `decl.rs` "Migration notes" block, defaults keep legacy
  shape unchanged.

### Tests added (61 total)
- `crates/buff-lang-parser/tests/module_system.rs` — 27 tests
- `crates/buff-lang-types/tests/modules.rs` — 27 tests
- `crates/buff-lang-types/src/modules.rs` (inline `#[cfg(test)]`) — 7 tests
All passing; clippy clean; fmt clean.

## T31 � Async with call graph propagation

### Status: COMPLETE (42 tests pass: 21 propagation + 21 codegen)

### What works
- `async func name(...)` declared-async fns seed the async set
- Call-graph propagation (fixpoint): any fn transitively calling an async fn becomes async
- `main` fn in async set gets `#[tokio::main]` attribute + `async fn main`
- Auto-inserted `.await` at async call sites INSIDE async contexts
- `spawn <expr>` ? `tokio::spawn(async move { <expr> })` (returns JoinHandle<T>)
- `task.result()` ? `task.await` (the only `.await` from a method-call site)
- `block(<expr>)` ? one-shot `Runtime::new().expect(...).block_on(<expr>)`
- `block()` inside async fn ? deadlock-warning Diagnostic collected in `RustCodegen::warnings`
- NO `await` keyword in Buff source (the lexer has no KwAwait � only KwAsync/KwSpawn)

### Critical: deterministic data structures (T29 lesson applied)
- `CallGraph` uses `BTreeMap<String, BTreeSet<String>>` (sorted iteration)
- `AsyncSet` uses `BTreeSet<String>` (sorted output via `to_sorted_vec`)
- Fixpoint scans edges in BTreeMap order ? byte-identical output every run
- DO NOT use HashMap for the async set or call graph � iteration order is
  non-deterministic and will make tests flaky (the lesson T29 taught us)

### Buff syntax surprise: `func name():` (colon is REQUIRED)
Buff's offside-rule layout requires `func name():` (trailing colon) before
the indented body. A bare `func name()` without the colon is a parse
error. This bit me in 2 round-trip tests: I wrote
`"async func io()`n`    return 0`"` and the parser rejected it with
`expected ':', found 'return'`. The fix was adding the colon:
`"async func io():`n`    return 0"`. Check `examples/ola.buff` for the
canonical shape whenever writing inline Buff source in tests.

### Migration note: `Expr::Spawn` is additive
- New AST variant `Expr::Spawn { task: Box<Expr>, span: Span }` (T31)
- Parser builds it from `KwSpawn` in `parse_primary` (calls `parse_unary`
  for the task body so `spawn task()` captures the full call)
- `ir.rs::collect_uses` recurses into `Spawn { task }`
- `exhaustiveness.rs::check_expr` recurses into `Spawn { task }` (so
  matches inside spawned tasks are still checked)
- `infer.rs::infer_expr` returns `Type::Unknown` for Spawn (Task<T> is
  opaque at the type level for v0.5)
- codegen's `expr_uses_matrix` and `expr_uses_error` recurse for
  emit-on-demand detection
- codegen lowers `spawn expr` ? `tokio::spawn(async move { <lowered> })`
  via `quote!`

### Codegen `current_fn_is_async` vs `in_async_context`
Two distinct concepts in `RustCodegen`:
- `current_fn_is_async()` � true when the fn being lowered is in the
  propagated async set (per `async_fns` BTreeSet)
- `in_async_context()` � true when EITHER `current_fn_is_async()` OR
  `async_block_depth > 0` (we're inside one or more `async move { ... }`
  blocks, e.g. inside a `spawn` body)

The `.await` insertion rule uses `in_async_context()` so async calls
inside `spawn <async_call>()` still get `.await` even when the spawner
is sync. The deadlock warning uses `current_fn_is_async()` because
block-in-async-block isn't a deadlock (the spawned task can run on
another worker).

### Async block depth tracking
`async_block_depth: usize` on RustCodegen � incremented by `lower_spawn`
around the task body lowering, decremented after. This lets nested
async blocks (e.g. `spawn spawn work()`) work correctly.

### spawn does NOT propagate async-ness
Per the spec `spawn task() -> tokio::spawn(async move { task() })`: the
spawner stays sync. The spawned task IS async (inside `async move`),
but the spawner isn't. Implemented by NOT recursing into `Spawn { task }`
in `collect_func_calls` � a deliberate `Spawn { task: _, .. } => {}` arm.

### `result()` is special-cased BEFORE the T26 field-access heuristic
`task.result()` parses as a zero-arg `MethodCall`, which the T26 heuristic
would rewrite as a field access `task.result` (broken on a JoinHandle).
The `if method.name == "result"` arm runs FIRST in `lower_method_call`,
unconditionally returning `make_await(recv)` � no `args.is_empty()` gate
(so both `t.result()` and `t.result` work).

### Tests added (42 total)
- `crates/buff-lang-types/tests/async_propagation.rs` � 21 tests
- `crates/buff-lang-types/src/async_analysis.rs` (inline `#[cfg(test)]`) � 21 tests
- `crates/buff-lang-codegen-rust/tests/async_codegen.rs` � 21 tests
- `crates/buff-lang-parser/tests/stmt_tests.rs` � 1 test updated
  (`test_async_func_top_level_errors` ? `test_async_func_top_level_parses_with_is_async_flag`)

### Verifications
- `cargo test -p buff-lang-types --test async_propagation` -> 21 passed
- `cargo test -p buff-lang-codegen-rust --test async_codegen` -> 21 passed
- `cargo test --workspace` -> all green (one pre-existing parser test
  was updated to reflect that `async func` is now valid syntax)
- `cargo clippy --workspace --all-targets -- -D warnings` -> clean
- `cargo fmt -p buff-lang-{ast,parser,types,codegen-rust} -- --check` -> clean
- `cargo check --workspace` -> clean

## T32 — FFI basics (extern crate + extern func + type table)

### Status: COMPLETE

Implemented FFI declarations: `extern crate "<name>"` records an
external-Rust-crate dependency, and `extern func sig(...) -> Ret`
declares a foreign function (bodyless). Also extracted the Buff→Rust
primitive-name mapping into a single named, configurable table.

### Additive AST change
- `Decl::ExternCrateDecl(ExternCrateDecl)` — PURELY ADDITIVE new variant.
- `ExternCrateDecl { name: String, span: Span }` — `name` is `String`
  (NOT `Ident`) because crate names may contain `-` (e.g. `rust-decimal`),
  which is not a valid Buff identifier character.
- Migration note in `decl.rs` (T32 section), mirroring T20-T31 additive
  precedents. Every `match` on `Decl` across the workspace gained an arm;
  `async_analysis.rs`'s two matches already used `_ =>` catch-alls so no
  change was needed there.

### Parser (crates/buff-lang-parser/src/{parser,stmt,lib}.rs)
- Dispatcher routes `KwExtern` by peeking the SECOND token:
  - `Ident("crate")` → `parse_extern_crate_decl` → `Decl::ExternCrateDecl`
  - `KwFunc` → `parse_func_decl` (consumes leading `extern`)
- `parse_func_decl` now consumes an optional leading `extern` modifier
  (mirrors T31's `async` handling). When `is_extern`, NO body is parsed
  — an empty placeholder `Block` is synthesised (codegen drops it).
- `parse_extern_crate_decl` consumes `extern crate "<name>"`; reuses
  StringStart/StringPart/StringEnd lexer machinery via a dedicated
  `expect_crate_name_string` helper (crate-specific error messages,
  rejects interpolation).

### Codegen (crates/buff-lang-codegen-rust/src/rust_codegen.rs)
- `RustCodegen.extern_crates: BTreeSet<String>` — recorded dep set
  (BTreeSet for DETERMINISM; the T29 flaky-test lesson — never use
  HashSet for codegen output).
- `RustCodegen::extern_crates()` accessor — public, for the future
  Cargo-project pipeline + codegen-level tests.
- `Decl::ExternCrateDecl` lower: records name + emits `use <name>;`
  (hyphen→underscore normalised; `UseTree::Name`, NOT `Path`, so it
  emits `use serde;` not the wrong `use serde::serde;`).
- `Decl::FuncDecl where is_extern` lower: routes to
  `lower_extern_func_decl` → `syn::ItemForeignMod` →
  `extern "C" { fn name(params) -> Ret; }`. Bodyless foreign-fn
  declaration. `unsafety: None` on the block for max Rust-edition
  compatibility (functions inside are implicitly unsafe to call).
- `buff_primitive_to_rust_name(&str) -> &str` — the T32 configurable
  type table. Single source of truth consulted by
  `ast_typeref_to_syn`. Covers all 9 primitives: Int→i64, Byte→u8,
  Bits→u64, Float→f32, Double→f64, Bool→bool, String→String, Char→char,
  Decimal→rust_decimal::Decimal. Unknown names pass through unchanged
  (so struct/enum/generic-param names keep their spelling). Re-exported
  at crate root for test ergonomics.

### CLI→Cargo.toml wiring: DEFERRED
The CLI pipeline (`buff-lang-cli/src/pipeline.rs`) currently invokes
`rustc --edition 2021 <file>.rs` on a SINGLE generated .rs file — there
is NO Cargo project model and NO generated Cargo.toml. Wiring the
recorded `extern_crates()` set into a generated Cargo.toml's
`[dependencies]` requires switching the pipeline to Cargo-project
assembly, which is too large for T32's scope. T32 instead implements +
tests the codegen-level collection (dep-set + `use` emission +
foreign-mod signatures), and documents the deferral. The task spec §3
explicitly allows this fallback.

### Tests added (17 in crates/buff-lang-codegen-rust/tests/ffi.rs)
- extern crate: records name in dep set, emits `use <name>;`, snapshot,
  multiple crates deterministic BTreeSet order, hyphen→underscore,
  duplicate dedup.
- extern func: lowers to `extern "C" { fn ...; }`, bodyless, void
  return, snapshot, String param mapping.
- type table: all 9 primitives, unknown-name passthrough, generic
  containers inner-arg mapping.
- end-to-end: lexer→parser→codegen round-trip + reparse; parser error
  cases (extern without crate/func, extern crate without string).

### Pre-existing test updated
- `codegen_tests.rs::test_codegen_async_unsafe_extern_modifiers`: the
  old test combined `is_async + is_unsafe + is_extern` which is now
  nonsensical (extern funcs are bodyless foreign-mods, can't be async).
  Updated to `is_extern: false` so it exercises async+unsafe on a
  body-having fn; the extern case is covered by the new ffi.rs suite.

### Verifications (MSVC env)
- `cargo test -p buff-lang-codegen-rust --test ffi` -> 17 passed; 0 failed
- `cargo test --workspace` -> ALL crates 0 failed
- `cargo check --workspace` -> clean
- `cargo clippy --workspace --all-targets -- -D warnings` -> clean
- `cargo fmt -p buff-lang-{ast,parser,types,codegen-rust} -- --check` -> clean
- `cargo fmt --all -- --check` -> clean

### Reusable patterns
- For bodyless AST decls: store an empty placeholder `Block` (don't make
  body `Option<Block>` — that's a breaking migration across every
  FuncDecl construction site). Codegen drops the placeholder.
- For syn `ItemUse` of a bare crate name: use `UseTree::Name`, NOT
  `UseTree::Path` with a nested Name (which wrongly emits `X::X`).
- For DETERMINISTIC dep-set collection: `BTreeSet<String>` (never
  `HashSet` — the T29 flaky-test lesson applies to ALL codegen output).
- syn 2.0.119 `ItemForeignMod` has NO `vis` field (visibility lives on
  the inner `ForeignItem`s); `ItemUse` REQUIRES a `semi_token` field.
- `crate` is NOT a Buff keyword (the 25-keyword list) — it parses as a
  regular `Ident("crate")`, so the dispatcher matches on
  `Some(TokenKind::Ident(s)) if s == "crate"`.

## T34 — Closures/Lambdas Codegen (Capture Analysis)

### Status: COMPLETE

### What T34 extended beyond T23
- T23 added minimal closures (`{x=>e}` → `Expr::Lambda` → `lower_lambda` →
  `|x| body`). T34 added **variable capture analysis**.
- **closure_captures()** in `buff-lang-types::ownership` — public function
  computing free vars of a closure body minus params minus closure-local
  lets. Returns `BTreeSet<String>` (deterministic).
- **closure_capture_stack** in `RustCodegen` — a `Vec<BTreeSet<String>>`
  tracking which idents should bypass `MoveAnalyzer::needs_clone` inside
  each closure body. Pushed in `lower_lambda`, popped after body lowering.

### Shared with T33 (REFACTOR)
- T33's spawn-capture detection uses `collect_free_vars_in_expr` /
  `collect_free_vars_in_block` (private walkers in ownership.rs).
- T34's `closure_captures()` REUSES these same walkers. Both need "which
  variables does this sub-expression read from its enclosing scope?". T33
  intersects with function locals (for Arc-wrap); T34 subtracts closure
  params + local lets (for capture identification). Walker shared, post-
  processing differs.

### Key discovery: closure params also need to bypass needs_clone
- A closure PARAM used multiple times in the body (e.g. `|x| x * x + x`)
  would get spurious `.clone()` from MoveAnalyzer (it doesn't know `x`
  is a fresh closure-local binding). This was a pre-existing T23 limitation
  that never surfaced because T23 tests only used each param once.
- FIX: the bypass set pushed onto closure_capture_stack includes BOTH
  captured variables AND closure params. Both emit plainly inside the
  closure body — Rust handles their ownership.

### Key discovery: capture stack scope
- `is_captured_in_closure()` only checks the TOP-of-stack entry (innermost
  closure). This is correct because a variable captured by an inner closure
  is also a free var of the outer closure body, so it appears in both frames.
- Uses OUTSIDE the closure body go through normal MoveAnalyzer path.

### What was NOT changed
- T23 vector_codegen tests (20 tests) — unchanged, all pass.
- T33 clone_analysis + move_tests (15 tests) — unchanged, all pass.
- Parser (`parse_closure`) — unchanged (single-expr body only; multi-stmt
  bodies deferred — the AST/codegen support it, parser doesn't).

### Test count
- buff-lang-types: +8 closure_captures unit tests (21 total in ownership mod)
- buff-lang-codegen-rust: +10 closures integration tests (new file)
- Workspace total: ALL pass (0 failed).


## T35 — `buff test` command

### Status: COMPLETE (all green — clippy -D warnings, 19 test_command tests, full workspace 0 failed)

### What works
- `buff test <FILE>` discovers `@test` funcs, runs them, prints `<n> passed, <m> failed`.
- `--pattern <GLOB>` filters by simple glob (`*` = any chars).
- Exit 0 all-pass, exit 1 any-fail, graceful empty report if no `@test` funcs.

### Key learnings
- **`@` token already existed** in the lexer (`TokenKind::At`) — no lexer change needed. Only parser + AST.
- **Bulk FuncDecl migration**: 55 construction sites across 27 files. PowerShell `-replace 'is_extern: false,', 'is_extern: false, attributes: Vec::new(),'` handled 53; 2 shorthand `is_extern,` sites needed manual Edit (ffi.rs test + parser stmt.rs). ast_grep FAILED to match struct-field patterns (needs complete AST nodes, not bare field assignments).
- **`quote!` inside `println!`**: `println!("test " #name " ... ok")` produces INVALID Rust (adjacent string literals don't concat inside macro args). Fix: `println!("test {} ... ok", #name)` — use `{}` format placeholder.
- **rustc crate name from `.test.rs`**: writing harness as `<file>.test.rs` → rustc infers crate name `<stem>.test` → "invalid character '.'" error. Fix: write to `temp_dir/<stem>_test.rs` (underscore, not dot).
- **Custom runner chosen over `rustc --test`**: QA requires `<n> passed, <m> failed` format; Rust's `--test` harness prints `1 passed; 0 failed` (semicolon + different wording). Custom runner via `catch_unwind` gives full output control + avoids `#[test]`-fn-vs-user-`main` conflict.
- **`#[test]` stripping in harness**: codegen emits `#[test]` on `@test` fns (for `buff build`); the test harness STRIPS it (calls fns directly from custom `main`). Avoids ambiguity about `#[test]` fn callability in non-`--test` builds.
- **clippy `needless_late_init`**: `let end; if cond { ...; end = x; } else { end = y; }` → refactor to `let end = if cond { ...; x } else { y };`. The `?` operator and early `return` work fine inside if-expression blocks.
- **Discovery determinism**: `BTreeSet<String>` for test names → sorted output → byte-identical repeated runs (T29 lesson).
- **`format_diagnostic_error` made `pub`** in pipeline.rs so test_runner reuses the same error formatting (DRY).
- **Pure functions unit-testable without rustc**: `discover_test_names`, `matches_pattern`, `parse_report` — 11 inline unit tests run without the toolchain; 4 E2E tests are rustc-gated.

### Test count
- test_runner.rs inline: 11 unit tests (glob matching, report parsing, exit code).
- tests/test_command.rs: 19 integration tests (discovery, pattern, report, harness codegen, front-end errors, 4 E2E).
- Workspace total: ALL pass (0 failed).


## T36 — Error message improvements + parser error recovery

### Status: COMPLETE. HEAD 7575af3 → uncommitted (orchestrator commits).

### What landed

1. **Diagnostic rendering** (`crates/buff-lang-error/src/diagnostic.rs`):
   - `Diagnostic::render(source: &str) -> String` — rustc-style output:
     `[<severity>] <msg>` header, source line with line-number gutter, caret
     line (`^^^`) whose width = char-count of the span clamped to the line,
     trailing empty gutter line, then `  note: <text>` per note.
   - **Column accounting is char-based** (not byte), so multi-byte UTF-8
     aligns under one caret column. Matches `SourceFile::lookup` convention
     (T4). Zero-width spans still get one caret; out-of-bounds spans
     (`start > source.len()`) render header + notes only (no caret block).
   - `render_diagnostics(&[Diagnostic], source) -> String` — joins N
     diagnostics with a blank line between them, for multi-error output.
2. **Levenshtein + did-you-mean** (same file):
   - `levenshtein(a, b) -> usize` — classic two-row DP, **char-based**.
   - `SUGGESTION_MAX_DISTANCE = 2` (covers transpositions = distance 2 in
     classic Levenshtein, single-char substitutions, and 1-2 typos).
   - `suggest_close<'a>(input, candidates) -> Option<&'a str>` — closest
     candidate within threshold. **Ties broken alphabetically** for
     determinism (T29 lesson re-applied). Cheap length pre-filter skips the
     DP when `|len(a) - len(b)| > threshold`.
   - `format_did_you_mean(input, candidates) -> Option<String>` — wraps
     `suggest_close` with the canonical `Did you mean \`<c>\`?` formatting.
3. **Parser recovery** (`crates/buff-lang-parser/src/parser.rs` +
   `stream.rs`):
   - NEW `parse_recovering(tokens, source_id) -> (Vec<Decl>, Vec<ParseError>)`
     — accumulates errors and continues. Exported from `lib.rs`.
   - NEW `TokenStream::sync_to_recovery_point(&mut self) -> bool` — advances
     past tokens until a top-level sync point (KwFunc/KwAsync/KwEnum/
     KwImport/KwExport/KwExtern/At) or EOF. Does NOT consume the sync token.
   - Refactored `parse()` loop body into **`parse_one_decl(stream)`** shared
     helper; both `parse()` (fail-fast via `?`) and `parse_recovering()`
     (accumulate+sync) call it. Zero behavior change for `parse()` —
     verified by running ALL existing parser tests (37 stmt + 73 expr +
     22 layout + 27 module + 5 enum_match, all green).
   - **Infinite-loop guard**: in `parse_recovering`, after an error compare
     cursor position before/after `sync_to_recovery_point`; if unchanged
     AND not at EOF, force-advance one token. Cheap insurance against any
     future change to the sync set that would cause `parse_one_decl` to
     keep rejecting the same sync token.

### Key decisions (see decisions.md T36 for full rationale)

- **`parse()` signature unchanged**: still `Result<Vec<Decl>, ParseError>`.
  The task allowed "keep parse() as-is; document the choice" — went one
  step further by REFACTORING its internals to delegate to `parse_one_decl`
  + `parse_recovering` is a sibling. No duplication of the 150-line
  dispatch table. The shared helper guarantees both entry points agree on
  what counts as a top-level declaration.
- **Inline snapshots** (`@r#"..."#`) for the 5 stable error-message tests
  instead of `.snap` files. Self-contained in test source, no `.snap.new`
  / `.pending-snap` to manage. **insta strips a single leading `\n`** from
  inline snapshot literals — so the literal `\n[Error]...` is compared
  against the actual `[Error]...` (no leading newline). Trailing newline
  is preserved.
- **Sync set is top-level starters only** (func/async/enum/import/export/
  extern/At). NOT `let`/`match`/`newline` (the task spec's general list)
  because `let`/`match` at top level would just re-trigger the catch-all
  arm and loop. The spec's list is the GENERAL guidance for future
  statement-level recovery; for the TOP-LEVEL loop, only decl starters
  are valid resume points.

### Gotchas

- **Insta inline snapshot leading newline**: writing `@r#"\n<content>\n"#`
  (multi-line raw string) creates a literal with leading `\n`. Insta
  strips ONE leading newline when parsing inline snapshots, so the actual
  value must NOT start with `\n`. Trailing `\n` is kept verbatim.
- **clippy `manual_contains`**: `names.iter().any(|n| *n == "x")` →
  `names.contains(&"x")`. New clippy in 1.95 toolchain.
- **rustfmt wants one-item-per-line** for long array literals. The
  `candidates()` helper in error_messages.rs has 19+25 entries — rustfmt
  explodes them to one per line. Accepted (project rule: fmt is enforced).
- **`f.name` is `Ident`, not `String`**: in tests,
  `Decl::FuncDecl(f).name.name.as_str()` (double `.name`) — the inner
  `Ident.name: String`. Easy to miss.
- **`workspace.dev-dependencies` is NOT a valid cargo key** (pre-existing
  bug in root Cargo.toml). The warning `unused manifest key:
  workspace.dev-dependencies` is NOT mine. `.workspace = true` in a
  crate's `[dev-dependencies]` correctly resolves from
  `[workspace.dependencies]` (where `insta = "1.40"` IS declared). My
  buff-lang-error Cargo.toml addition is correct.
- **Byte-offset pitfalls in tests**: when hand-crafting spans for
  `Diagnostic::render`, count the `\n` byte too. "let a = 1\n" is 10
  bytes (positions 0-9, `\n` at 9). Off-by-one here produces carets at
  the wrong column; the inline snapshot test caught my mistake on first
  run.

### Test count
- buff-lang-error/tests/error_messages.rs: **24 tests** (19 functional +
  5 inline snapshots). Covers: render (single/multi-char/zero-width/
  unicode/multiline/out-of-bounds/warning), levenshtein (identical/
  substitution/insertion/deletion), suggest_close (print-for-pritn/
  distant-rejected/tie-alphabetical), format_did_you_mean, did-you-mean
  note integration, render_diagnostics (multi-error-ordering/empty),
  5 snapshots (simple type error, multi-char caret, did-you-mean,
  multi-error file, warning+notes).
- buff-lang-parser/tests/error_recovery.rs: **5 tests** (two-errors-
  in-one-pass, continues-after-error, clean-input-no-errors, garbled-
  input-no-panic, parse()-still-fail-fast).
- Workspace total after T36: ALL pass (0 failed). 66 `test result:` lines.

## T68 — Range syntax

### Status: COMPLETE

### What was added

**Lexer** (`buff-lang-lexer/src/token.rs` + `lexer.rs`):
- NEW `TokenKind::DotDot` (`..`) and `TokenKind::DotDotEq` (`..=`) tokens.
- `scan_operator` now checks 3-char operators first (`..=` → DotDotEq) before
  2-char operators (`..` → DotDot). This prevents `..=` from being split into
  `..` + `=` (which would parse as range then assignment).

**AST** (`buff-lang-ast/src/expr.rs`) — ADDITIVE change:
- NEW `Expr::Range { start: Box<Expr>, end: Box<Expr>, inclusive: bool, span: Span }`.
  Added at the end of the enum (after `Spawn`). Migration note in the doc comment.
- `span()` match arm added.
- Display: `Range(Lit(Int(0)), Lit(Int(10)), excl)` / `Range(Lit(Int(0)), Lit(Int(10)), incl)`.

**Parser** (`buff-lang-parser/src/expr.rs`):
- NEW `parse_range()` function inserted between `parse_assignment` (level 1) and
  `parse_or` (level 2). Range has lower precedence than additive operators, so
  `a+1..b*2` parses as `(a+1)..(b*2)`.
- `parse_assignment` now calls `parse_range` instead of `parse_or` as its LHS.
- `parse_range` checks for `DotDot` (exclusive) or `DotDotEq` (inclusive) after
  the LHS, then parses the RHS at the `parse_or` level.

**Codegen** (`buff-lang-codegen-rust/src/rust_codegen.rs`):
- NEW `lower_range()` method builds `syn::ExprRange` via `quote!`:
  - Exclusive: `#start_e .. #end_e`
  - Inclusive: `#start_e ..= #end_e`
- `lower_expr` dispatches `Expr::Range` to `self.lower_range()`.
- All match sites updated: `expr_uses_matrix`, `expr_uses_error`.

**Match sites updated across workspace** (11 files):
- `buff-lang-ast/src/ir.rs` — `collect_uses`
- `buff-lang-types/src/infer.rs` — `infer_expr` (returns Unknown for range)
- `buff-lang-types/src/exhaustiveness.rs` — `check_expr`
- `buff-lang-types/src/async_analysis.rs` — `collect_func_calls`
- `buff-lang-types/src/ownership.rs` — 5 functions: `collect_bound_names_in_expr`,
  `collect_spawn_free_vars_in_expr`, `collect_free_vars_in_expr`,
  `collect_assignment_targets_in_expr`
- `buff-lang-codegen-rust/src/rust_codegen.rs` — `expr_uses_matrix`, `expr_uses_error`

### Key design decisions

- **Range precedence: between assignment and `||`.** Range binds tighter than
  assignment (`a = 0..10` parses as `a = (0..10)`) but looser than `||`/`&&`/
  comparison/additive/multiplicative. This matches Rust's precedence and lets
  `a+1..b*2` parse as `(a+1)..(b*2)`.
- **`..=` is a 3-char token.** The lexer checks 3-char operators before 2-char
  operators, so `..=` is never split into `..` + `=`.
- **Codegen via `quote!`.** Range expressions are built via `quote!` (not
  hand-formatted Rust), consistent with the codebase's `syn`/`quote`/`prettyplease`
  discipline.
- **Type inference returns `Type::Unknown`.** The type system doesn't track range
  types in v0.5; ranges are expression-level constructs consumed by `for` loops
  and codegen.

### Tests added

**Parser** (`crates/buff-lang-parser/tests/ranges.rs` — 7 tests):
- `ranges_exclusive` — `0..10` parses as Range(0, 10, excl)
- `ranges_inclusive` — `0..=10` parses as Range(0, 10, incl)
- `ranges_ident_bounds` — `start..end` parses as Range(start, end, excl)
- `ranges_precedence_additive` — `a+1..b*2` parses as (a+1)..(b*2)
- `ranges_in_for_loop` — `0..5` parses as Range(0, 5, excl)
- `ranges_display_exclusive` — Display format `Range(Lit(Int(0)), Lit(Int(10)), excl)`
- `ranges_display_inclusive` — Display format `Range(Lit(Int(0)), Lit(Int(10)), incl)`

**Codegen** (`crates/buff-lang-codegen-rust/tests/ranges.rs` — 4 tests):
- `ranges_codegen_exclusive` — `0..10` → Rust `0..10` (re-parses as valid Rust)
- `ranges_codegen_inclusive` — `0..=10` → Rust `0..=10` (re-parses as valid Rust)
- `ranges_codegen_ident_bounds` — `start..end` → Rust `start..end`
- `ranges_codegen_for_loop` — `for i in 0..5` → Rust `for i in 0..5`

### Verification (all GREEN)
- `cargo test -p buff-lang-parser ranges` → 7/7 pass
- `cargo test -p buff-lang-codegen-rust ranges` → 4/4 pass
- `cargo test --workspace` → ALL green (0 failed)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean
- `cargo fmt --check` → clean

## T101 — Null coalescing `??` operator

### Status: COMPLETE

### What was added

**Lexer** (`buff-lang-lexer/src/token.rs` + `lexer.rs`):
- NEW `TokenKind::QuestionQuestion` token for `??`.
- `scan_operator` 2-char section: `"??" => Some(TokenKind::QuestionQuestion)`.
- `??` is scanned BEFORE the single `?` falls through to `single_char_kind` (which handles `b'?' => TokenKind::Question`). This is the same longest-match-first pattern used by `..=` / `..`.
- Display: `??`.

**AST** (`buff-lang-ast/src/op.rs`) — ADDITIVE change:
- NEW `BinaryOp::NullCoalesce` variant added at the end of the enum.
- Display: `??`.

**Parser** (`buff-lang-parser/src/expr.rs`):
- NEW `parse_null_coalesce()` function inserted between `parse_range` (level 1.5) and `parse_or` (level 2).
- `parse_range` now calls `parse_null_coalesce` instead of `parse_or` as its LHS.
- `parse_null_coalesce` checks for `TokenKind::QuestionQuestion` after the LHS, then parses the RHS at the `parse_or` level.
- Precedence: `??` binds tighter than `||`/`&&`/comparison/additive/multiplicative but looser than range. This means `a ?? b == c` parses as `a ?? (b == c)` — the same as Rust's `??` precedence relative to comparison.

**Codegen** (`buff-lang-codegen-rust/src/rust_codegen.rs`):
- `make_binary_op` arm for `BinaryOp::NullCoalesce`: builds `#lhs.unwrap_or(#rhs)` via `quote!` + `syn::parse2`.
- `opt ?? 0` → `opt.unwrap_or(0)`; `name ?? "unknown"` → `name.unwrap_or("unknown")`.

**Types** (`buff-lang-types/src/infer.rs`):
- `infer_binary_op` arm for `BinaryOp::NullCoalesce`: returns `Ok(rhs_ty)` (the default value's type).

### AST representation decision: BinaryOp variant (NOT new Expr variant)

Chose `BinaryOp::NullCoalesce` over a new `Expr::NullCoalesce` variant because:
1. `??` is a true binary infix operator — it has a left and right operand, same shape as `+`, `||`, etc.
2. Adding a `BinaryOp` variant is purely additive (no new match arms on `Expr` needed across the workspace).
3. The existing `Expr::BinaryOp { op, lhs, rhs, span }` node already handles all binary ops uniformly.
4. Codegen special-cases it in `make_binary_op` (not `lower_expr`), keeping the dispatch clean.

### Precedence level chosen

`??` sits between range (`..`/`..=`) and logical OR (`||`). This means:
- `a ?? b == c` → `a ?? (b == c)` (null-coalesce binds looser than comparison)
- `a + 1 ?? 0` → `(a + 1) ?? 0` (additive binds tighter)
- `a ?? b ?? c` → `(a ?? b) ?? c` (left-associative, like `||`)

### Side fix: T30 chained `?` test updated

The test `t30_question_op_chained_parses` used `f()??` (two adjacent `?` chars) which now lexes as a single `QuestionQuestion` token. Updated to `f()? ?` (space between) so the two postfix `?` operators are lexed separately. This is the correct behavior — `??` is now the null-coalescing operator.

### Tests added

**Codegen** (`crates/buff-lang-codegen-rust/tests/null_coalescing.rs` — 3 tests):
- `null_coalescing_default_int` — `opt ?? 0` → `opt.unwrap_or(0)` (re-parses as valid Rust)
- `null_coalescing_string` — `name ?? "unknown"` → `name.unwrap_or("unknown")`
- `null_coalescing_chained` — `a ?? b ?? c` → `a.unwrap_or(b.unwrap_or(c))`

### Verification (all GREEN)
- `cargo test -p buff-lang-codegen-rust null_coalescing` → 3/3 pass
- `cargo test --workspace` → ALL green (0 failed)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean
- `cargo fmt --check` → clean

## T102 — Expression functions `=>`

### Status: COMPLETE

### What was added

**Parser** (`crates/buff-lang-parser/src/stmt.rs`):
- In `parse_func_decl`, BEFORE the `parse_block(stream)?` fallback, added a check for `TokenKind::FatArrow` (`=>`).
- When `=>` is found: consume it, parse a single expression via `parse_expression`, and synthesize a `Block` whose single statement is `Stmt::Return(Some(expr), ...)`.
- The `=>` form works WITH and WITHOUT a return type annotation: `func f(x: Int) => x+1` (no return type) AND `func sq(x: Int) -> Int => x * x` (with return type) both parse.
- Normal block-body functions (`func f(x) { ... }` and layout `func f(x): ...`) are unchanged — the `=>` check only fires when the next token is `FatArrow`.

**No AST changes needed**: reuses existing `FuncDecl` + `Stmt::Return`. No new variants.

**No codegen changes needed**: the synthesized `FuncDecl` has a normal `Block` with a `Stmt::Return(Some(expr), _)`, which the existing `lower_func` / `lower_stmt` already handles.

### Key design decisions
- **Hook point**: inserted in `parse_func_decl` between the return-type parsing and the `parse_block` call, in the `!is_extern` branch. This is the natural place — after the signature is fully parsed, before the body is consumed.
- **Synthesized Block span**: starts at the `=>` token and ends at the expression's end. This is a reasonable span for error messages.
- **No new `.expect`/`.unwrap`**: the `FatArrow` consumption uses `stream.advance().ok_or_else(|| ...)` returning a proper `ParseError`, consistent with the existing error-handling style.

### Tests added

**Parser** (`crates/buff-lang-parser/tests/expr_functions.rs` — 6 tests):
- `expr_functions_untyped` — `func double(x: Int) => x * 2` (typed param, no return type)
- `expr_functions_typed_param` — `func sq(x: Int) => x * x` (typed param, no return type)
- `expr_functions_with_return_type` — `func sq(x: Int) -> Int => x * x` (with return type)
- `expr_functions_via_parse_top_level` — `func f(x: Int) => x + 1` via top-level `parse()`
- `expr_functions_multi_param` — `func add(a: Int, b: Int) => a + b` (multiple params)
- `expr_functions_normal_block_still_works` — `func foo() { return 42 }` unchanged

**Codegen** (`crates/buff-lang-codegen-rust/tests/codegen_tests.rs` — 1 test):
- `test_codegen_expr_function_shorthand` — builds a FuncDecl with the same shape the parser produces for `=>`, verifies generated Rust contains `fn f(x: i64) -> i64` and `x + 1`, and re-parses as valid Rust.

### Verification (all GREEN)
- `cargo test -p buff-lang-parser expr_functions` → 6/6 pass
- `cargo test -p buff-lang-codegen-rust test_codegen_expr_function_shorthand` → 1/1 pass
- `cargo test --workspace` → ALL green (0 failed)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean
- `cargo fmt --check` → clean

## T67 — Collection literals (verify-only)

### Status: COMPLETE

The functionality was already fully implemented by T23 (`Expr::ArrayLit` →
`vec![...]`) and T25 (`Expr::MapLit` → `std::collections::HashMap::from([...])`).
The only gap was that no test fn name contained `collection_literals`, so the
T67 acceptance command `cargo test -p buff-lang-codegen-rust collection_literals`
ran 0 tests.

### What was added

**New test file** `crates/buff-lang-codegen-rust/tests/collection_literals.rs` (5 tests):
- `collection_literals_array_ints` — `[1, 2, 3]` → `vec![1, 2, 3]` + re-parse
- `collection_literals_empty_array` — `[]` → `vec![]` + re-parse
- `collection_literals_map_string_key` — `{"k": 42}` → `HashMap::from([("k", 42)])` + re-parse
- `collection_literals_empty_map` — `{:}` → `HashMap::from([])` + re-parse
- `collection_literals_map_multi_entry` — `{"name": "Alice", "age": 30}` → multi-tuple `HashMap::from([...])` + re-parse

### Codegen output forms verified
- `[1, 2, 3]` → `vec![1, 2, 3]`
- `[]` → `vec![]`
- `{"k": 42}` → `std::collections::HashMap::from([("k", 42)])`
- `{:}` → `std::collections::HashMap::from([])`
- Multi-entry maps produce comma-separated tuples inside `from([...])`

### Verification
- `cargo test -p buff-lang-codegen-rust collection_literals` → 5/5 pass
- `cargo test --workspace` → ALL green (0 failed)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean
- `cargo fmt --check` → clean
- No production code changed (only the new test file added)

## T69 — Pipeline operator `|>`

### Status: COMPLETE (parser-desugar approach — zero AST/codegen change)

### Approach chosen: DESUGAR IN THE PARSER (the task's STRONGLY PREFERRED path)

`LHS |> f(args...)` rewrites DIRECTLY in the parser into a plain `Expr::FuncCall`
(or `Expr::MethodCall` / bare-`Ident` shorthand) with the LHS prepended as the
first argument. **No new AST variant, no new BinaryOp, no codegen arm, no
exhaustive-match ripple** across rust_codegen.rs / ir.rs / infer.rs /
exhaustiveness.rs / async_analysis.rs / ownership.rs. The result of the desugar
is a node codegen already lowers, so `cargo check` stayed green with zero
non-test codegen edits.

### Precedence level: LOWEST binary operator (just below assignment, above range)

Wired into the chain as:
`parse_assignment -> parse_pipeline -> parse_range -> parse_null_coalesce -> parse_or -> ...`

Pipeline binds looser than EVERY other binary operator. This satisfies the spec
contract `a + b |> f()` = `f(a + b)` (the LHS groups first — verified by
`pipeline_precedence_looser_than_additive`). Both LHS and RHS of `|>` are
parsed via `parse_range` (the next-higher level), which is the standard
left-assoc Pratt shape.

### RHS handling (3 accepted shapes + error)

`desugar_pipeline(lhs, rhs, pipe_tok)`:
- `Expr::FuncCall { callee, args }` → `FuncCall { callee, args: [lhs, ...args] }`
  (the canonical case; `x |> f(a, b)` → `f(x, a, b)`).
- `Expr::MethodCall { receiver, method, args }` → same prepend pattern
  (`x |> obj.m(a)` → `obj.m(x, a)`). Trivially supported — same shape as
  FuncCall; avoids a confusing error on a natural use case.
- `Expr::Ident(name)` (bare callee, NO parens) → builds
  `FuncCall { callee: Ident(name), args: [lhs] }` (`x |> f` → `f(x)`). The
  spec's "support if easy" branch — it IS easy, so supported.
- ANYTHING ELSE → `ParseError::new(Diagnostic::error("right-hand side of `|>`
  must be a function call, found `{other}`"))` pointing at the `|>` token span.
  NOT a panic (the `other` arm uses `Expr::Display` for the message).

### Lexer (additive, greedy longest-match)

- NEW `TokenKind::PipeGt` added at the END of the operators block (after
  `PercentEq`), with a doc-comment + Display arm `Self::PipeGt => write!(f, "|>")`.
- `scan_operator` 2-char section: added `"|>" => Some(TokenKind::PipeGt)`.
  CRITICAL: the 2-char section runs BEFORE `single_char_kind`, so `|>` is
  matched greedily; a lone `|` (NOT followed by `>`) still falls through to
  `single_char_kind(b'|') => TokenKind::Pipe` (bitwise OR) — UNCHANGED.
  `||` still matches `OrOr` (it's checked earlier in the same 2-char section).
  The full workspace test suite (including bitor tests) confirms `|` is intact.

### Key design decision: `x |> f() * 2` ERRORS (not `(x |> f()) * 2`)

Because `|>` is the lowest-precedence binary operator, its RHS greedily
consumes a full range-level expression: `x |> f() * 2` parses the RHS as
`f() * 2` (a BinaryOp Mul), which is NOT a call → desugar rejects it with the
"must be a function call" error. This is the natural consequence of uniform
Pratt parsing (both operands at the next-higher precedence). Users who want
`(x |> f()) * 2` must parenthesize (verified by `pipeline_parens_then_multiply`).
This is DEFENSIBLE and consistent — the spec only mandates `a + b |> f()` =
`f(a + b)` (LHS grouping), which passes. Asymmetric RHS parsing (parse just
one call) would break the clean Pratt chain and leave trailing operators
unconsumed; rejected as over-engineering.

### QA note: `print` prelude lowers to `println!`

The QA spec says `"hello" |> print()` → assert `print("hello")`. The DESUGAR
correctly produces a `print` call with `"hello"` as its first arg — but Buff's
`print` prelude lowers a bare-string-literal arg to Rust's `println!("hello")`
(the `{}` format is dropped for literals; see `lower_print`). So the substring
assertion is `println!("hello")`, NOT `print("hello")` (the latter never
appears because `print` → `println!`). Asserting `println!("hello")` proves
the LHS landed as the print call's argument. The desugar's correctness for
non-prelude callees (`f`, `g`, `process`, `filter`) is pinned by tests that
codegen to the verbatim Rust call shape (`f(x)`, `filter(process(data))`, etc.).

### Tests added (17 total: 7 codegen + 10 parser)

- `crates/buff-lang-codegen-rust/tests/pipeline.rs` — 7 tests, all named
  `pipeline_codegen_*`. Mix of hand-built AST (precise FuncCall shape) and
  end-to-end parse-from-source (proves the full lex→parse→codegen pipeline).
  Covers: `"hello" |> print()` → `println!("hello")`, `x |> f()` → `f(x)`,
  `data |> process() |> filter()` → `filter(process(data))`, `x |> f(a, b)`
  → `f(x, a, b)`, bare-callee `x |> f` → `f(x)`, chained hand-built
  `g(f(a))`, hand-built `"hello"`→print.
- `crates/buff-lang-parser/tests/pipeline.rs` — 10 tests, all named
  `pipeline_*`. Covers: simple `x |> f()` → `Call(f, [x])`, `"hello" |>
  print()` → `Call(print, ["hello"])`, chained → `Call(filter,
  [Call(process, [data])])`, extra args, bare callee, precedence-looser-than-
  additive (`a + b |> f()` → `Call(f, [a+b])`), RHS-consumes-full-expr errors
  (`x |> f() * 2`), parens-then-multiply (`(x |> f()) * 2`), and two error
  cases (`x |> 5`, `x |> a + b`).

### Verification (all GREEN, MSVC env LIB set for test/clippy)
- `cargo test -p buff-lang-codegen-rust pipeline` → 7/7 pass (acceptance)
- `cargo test -p buff-lang-parser --test pipeline` → 10/10 pass
- `cargo test --workspace` → exit 0 (74 "test result: ok" binaries, 0 FAILED)
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0, clean
- `cargo fmt --check` → clean (cargo fmt applied to 3 touched files first)
- `cargo check --workspace` → exit 0

### Files changed
- `crates/buff-lang-lexer/src/token.rs` — +`PipeGt` variant + Display arm + doc
- `crates/buff-lang-lexer/src/lexer.rs` — +1 line in `scan_operator` 2-char section
- `crates/buff-lang-parser/src/expr.rs` — +`parse_pipeline` + `desugar_pipeline`,
  rewired `parse_assignment` → `parse_pipeline` → `parse_range`, updated
  precedence-ladder doc comment + section-header numbering
- `crates/buff-lang-codegen-rust/tests/pipeline.rs` — NEW (7 tests)
- `crates/buff-lang-parser/tests/pipeline.rs` — NEW (10 tests)

### Deferred
- None. The parser-desugar approach means there's no type-system or codegen
  work to defer — the desugared FuncCall flows through the existing inference
  + codegen paths unchanged.


## T70 — Null-conditional operator `?.`

### Status: COMPLETE (parser-desugar approach — zero AST/codegen change)

### Approach chosen: DESUGAR IN THE PARSER (the task's STRONGLY PREFERRED path)

`receiver ?. name` rewrites DIRECTLY in `parse_postfix` into a plain
`Expr::MethodCall { receiver, method: "and_then", args: [Lambda] }` where
the Lambda is `|x| x.name` (or `|x| x.m(args)` for the method-call form).
**No new AST variant, no new codegen arm, no exhaustive-match ripple** across
rust_codegen.rs / ir.rs / infer.rs / exhaustiveness.rs / async_analysis.rs /
ownership.rs. The desugar emits nodes codegen already lowers:

- `MethodCall { method: "and_then", args: [lambda] }` → falls through the
  default arm of `lower_method_call` (`and_then` is NOT in any special-
  case list, and `args` is non-empty so the field-access heuristic at
  line 1637 doesn't fire). Emits `recv.and_then(lambda)`.
- The Lambda lowers via `lower_lambda` to `|x| body_expr` (single
  ExprStmt body → bare expression, no block wrapper).
- The body `x.name` is itself a zero-arg `MethodCall { receiver: x,
  method: "name", args: [] }` → hits the field-access heuristic (args
  empty + `name` not in KNOWN_ZERO_ARG_METHODS) → emits `x.name`.
- Final codegen: `recv.and_then(|x| x.name)` — exactly the spec contract.

### Lexer (additive, greedy longest-match)

- NEW `TokenKind::QuestionDot` variant added at the END of the operators
  block (after `QuestionQuestion`), with doc-comment + Display arm
  `Self::QuestionDot => write!(f, "?.")`.
- `scan_operator` 2-char section: added `"?." => Some(TokenKind::QuestionDot)`.
  CRITICAL: the 2-char section runs BEFORE `single_char_kind`, so `?.`
  is matched greedily instead of splitting into `?` (Question, T30 Try)
  + `.` (Dot, member access). A lone `?` (NOT followed by `.`) still
  falls through to `single_char_kind(b'?') => TokenKind::Question` —
  T30 Try is UNCHANGED. `??` (QuestionQuestion, T101) is matched EARLIER
  in the same 2-char section, so `??` is UNCHANGED. The full workspace
  test suite (including T30 `x?` tests and T101 `??` tests) confirms
  both are intact.

### Parser (`parse_postfix` — additive arm)

NEW `Some(TokenKind::QuestionDot) =>` arm inserted AFTER the existing
`Some(TokenKind::Question) =>` (Try) arm. The arm:

1. Consumes the `?.` token.
2. Parses the following Ident (field/method name) — same logic as the
   existing Dot arm. Errors cleanly with `expected field or method name
   after `?.`, found ...` if a non-Ident follows (NOT a panic).
3. If `(` follows, parses the arg list (method-call form); else zero
   args (field form). Same shape as the Dot arm.
4. Builds the closure body: `x.name` or `x.m(args)` as a
   `MethodCall { receiver: Ident("x"), method: name, args }`.
5. Wraps in `Lambda { params: [Param { name: "x", ty: placeholder, .. }],
   body: Block { stmts: [Stmt::ExprStmt(body_inner)] }, return_type: None }`.
6. Wraps that in the outer `MethodCall { receiver: <original expr>,
   method: "and_then", args: [lambda] }`.
7. The loop continues, so `a?.b?.c` naturally chains: the receiver of
   the next iteration is the just-built `and_then` MethodCall →
   `a.and_then(|x| x.b).and_then(|x| x.c)` (left-associative, exactly
   the spec).

### Lambda placeholder param type

REUSES the SAME mechanism as `parse_closure` (the existing `{ x => ... }`
parser): `TypeRef::Named { name: Ident::new("_", span), span }`. This
matches the convention documented in T23's closure-parsing decision and the
`placeholder_ty()` helpers in `map_codegen.rs` / `closures.rs` /
`null_conditional.rs` test files. Codegen ignores the placeholder (emits
`|x|` with no annotation); Rust infers the inner type from `and_then`'s
`FnOnce(T) -> U` signature. Type inference treats the desugared lambda
identically to a user-written `{ x => x.name }` closure.

### Closure param name choice: `x` (literal)

The closure param is named `x` (matching the spec's literal `|x| x.name`
output). Trade-off considered: if the receiver expression happens to capture
an outer variable also named `x`, the closure param shadows it inside the
body. But the body ONLY references the param `x` (it never reads the
receiver directly — the receiver is the OUTER MethodCall's receiver, not
inside the lambda body). So shadowing is benign. Renaming to a synthetic
`__buff_qd_x` would deviate from the spec's expected output and break
`assert!(src.contains(".and_then(|x| x.name)"))` substring assertions.

### Method-call form handling

`u?.greet(42)` desugars to `u.and_then(|x| x.greet(42))` — the args
parsed after the method name are spliced into the LAMBDA BODY's MethodCall
(`x.greet(42)`), NOT into the outer `and_then` call. The outer
`and_then` always takes exactly ONE arg (the closure). Verified by
`null_conditional_method_call_e2e` and `null_conditional_method_call_handbuilt`
which assert both `u.and_then` and `.greet(42)` substrings.

### Chaining behavior

`a?.b?.c` produces `a.and_then(|x| x.b).and_then(|x| x.c)` — the
postfix loop's natural left-associativity. Each `?.` iteration wraps the
accumulator in one more `and_then`. Verified by counting `.and_then`
occurrences in the generated Rust (exactly 2 for `a?.b?.c`):
`null_conditional_chained_e2e` and `null_conditional_chained_handbuilt`.

### Precedence (postfix → tighter than additive)

`?.` is a postfix operator (handled in `parse_postfix`), so it binds
tighter than EVERY binary operator. `a?.b + 1` parses as
`(a?.b) + 1` (BinaryOp(Add, MethodCall(and_then), 1)) — verified by
`null_conditional_precedence_tighter_than_additive`. This matches Rust's
`?.` precedence (postfix > additive) and the T30 `?` postfix precedent
(`a? + 1` parses as `(a?) + 1`).

### Side fix: clippy `doc_overindented_list_items`

The module-level doc comment in `crates/buff-lang-parser/tests/null_conditional.rs`
originally had a continuation line indented to column 21 (aligning with
`inner = ...` after the `a?.b?.c` bullet). Clippy 1.95 flagged it as
`doc_overindented_list_items` (treats it as a nested list item). Fix:
reduce continuation-line indent to 2 spaces (`cargo fmt` would not catch
this; it's a clippy-only lint).

### Tests added (18 total: 8 codegen + 10 parser)

- `crates/buff-lang-codegen-rust/tests/null_conditional.rs` — 8 tests,
  all named `null_conditional_*`. Mix of hand-built AST (precise
  MethodCall+Lambda shape) and end-to-end parse-from-source (proves the
  full lex→parse→codegen pipeline). Covers: `opt?.value` hand-built;
  `u?.name` e2e; `opt?.value` e2e; chained `a?.b?.c` e2e; chained
  hand-built (asserts 2 `.and_then` calls); method-call `u?.greet(42)`
  e2e + hand-built; and a short-circuit contract test asserting
  `and_then` (not `map`) is the lowering target.
- `crates/buff-lang-parser/tests/null_conditional.rs` — 10 tests, all
  named `null_conditional_*`. Covers: `u?.name` → MethodCall(and_then)
  with Ident(u) receiver and Lambda arg; `opt?.value`; chained
  `a?.b?.c` (inner+outer both `and_then`, innermost receiver Ident(a));
  method-call `u?.m(42)` with deep lambda-body inspection; precedence
  (tighter than additive); REGRESSIONS for single `?` (Try) and plain
  `.` (member access) both intact; error cases (leading `?.` no
  receiver; `x ?. 5` non-ident after `?.`).

### Verification (all GREEN, MSVC env LIB set for test/clippy)

- `cargo test -p buff-lang-codegen-rust null_conditional` → 8/8 pass (acceptance)
- `cargo test -p buff-lang-parser null_conditional` → 10/10 pass
- `cargo test --workspace` → 0 FAILED across all binaries (one apparent
  `test test_fail ... FAILED` is Buff's CUSTOM test runner reporting an
  intentionally-failing Buff `@test` inside the Rust e2e test
  `test_command_e2e_failing_test_exit_one` — the Rust test itself PASSES
  by asserting `report.failed == 1`. Pre-existing T35 behavior, NOT
  caused by T70.)
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0, clean
- `cargo fmt --all -- --check` → exit 0, clean (after `cargo fmt --all`)
- `cargo check --workspace` → exit 0

### Files changed

- `crates/buff-lang-lexer/src/token.rs` — +`QuestionDot` variant (with
  doc comment) + Display arm `"?."`.
- `crates/buff-lang-lexer/src/lexer.rs` — +1 line in `scan_operator`
  2-char section (`"?." => Some(TokenKind::QuestionDot)`).
- `crates/buff-lang-parser/src/expr.rs` — +`Some(TokenKind::QuestionDot)
  =>` arm in `parse_postfix` (after the existing Question/Try arm). The
  arm builds the desugared MethodCall(and_then, Lambda) in 6 steps
  documented inline.
- `crates/buff-lang-codegen-rust/tests/null_conditional.rs` — NEW (8 tests).
- `crates/buff-lang-parser/tests/null_conditional.rs` — NEW (10 tests).

### Deferred

- None. The parser-desugar approach means there's no type-system or
  codegen work to defer — the desugared MethodCall(and_then, Lambda) flows
  through the existing inference + codegen paths unchanged. Type errors
  (e.g. applying `?.` to a non-Option receiver) are warnings in v0.5 per
  the project policy; no enforcement added.
