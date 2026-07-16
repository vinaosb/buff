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
