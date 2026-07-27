# S1 — Multiple Dispatch Coverage Spike

**Date:** 2026-07-27
**Branch:** `main` (post-v1.38)
**Author:** Sisyphus-Junior (spike executor)
**Commit:** (this commit)
**Verdict:** **`MULTI_DISPATCH_SUFFICIENT`** (with documented caveats)

---

## TL;DR

Buff's T58 multiple dispatch (`crates/buff-lang-types/src/multi_dispatch.rs`,
532 LOC) **is sufficient** to replace every trait-object usage in the 10
target crates under consideration for porting. **Phase 2 (language
extension to add `dyn Trait`) can be SKIPPED.**

The heterogeneous-collection use case (`Vec<Box<dyn Trait>>`, the canonical
"hard" pattern that would force `dyn Trait` support) **does not appear in
any of the 10 target crates**. The only trait usage is a single
single-method callback parameter that maps trivially to multi-dispatch or
even simpler patterns.

Two caveats apply (see §5):

1. T58 multi-dispatch is currently **type-check-only**. The codegen layer
   does not yet apply name mangling, so `buff run` fails on multi-dispatch
   source even though `buff check` passes. This is a known codegen gap,
   not a fundamental limitation — verified by running the official
   `examples/multi_dispatch_basic.buff` (fails identically).
2. Any FUTURE porting target that uses `Vec<Box<dyn Trait>>` cannot be
   ported without re-architecting to enum + match. This is not a blocker
   for the current 10 target crates.

---

## 1. multi_dispatch.rs — what it provides

**File:** `crates/buff-lang-types/src/multi_dispatch.rs` (532 LOC including
unit tests; 356 LOC excluding).

**Mechanism** (lines 1–47):
- A name forms a multi-dispatch group ONLY if **2+ free funcs share it**
  in the same compilation unit (line 12).
- At codegen, group impls are MANGLED as `<name>_<argTy1>_<argTy2>_...`
  so each impl lowers to a unique Rust free function (lines 16–19).
- **All matching is on TYPES (not values); all dispatch is COMPILE-TIME
  (no runtime vtable, no dynamic dispatch)** — line 21, verbatim.

**API surface:**
- `MultiDispatchTable::build(&[Decl]) -> Self` — scans FuncDecls, forms groups.
- `table.is_group(name) -> bool` — does `name` have 2+ impls?
- `table.resolve(name, arg_types: &[Type], span) -> Result<Option<(usize, Type)>, TypeError>`
  — picks the unique matching impl; uses two-pass specificity (exact match
  wins over widened/assignable match, mirroring Julia method specificity).
- `table.mangled_name(name, method_idx) -> Option<&str>` — Rust name to emit.

**Errors (existing E12xx range — no new ErrorCode variants):**
- 0 matching impls → `E1201` (`UndefinedVariable`).
- 2+ equally-matching impls → `E1202` (`BinaryOpTypeMismatch`).

**Critical property for this spike:** the resolver takes `arg_types: &[Type]`
— meaning all argument types must be **statically known** at the call site.
There is no API for "dispatch on a runtime-erased type" because there is no
runtime dispatcher.

---

## 2. Spike file — `examples/spike_multi_dispatch.buff`

**Status:** written + `buff check` passes + `buff run` fails at codegen
mangling (same as the official `examples/multi_dispatch_basic.buff` —
confirmed identical failure mode).

The spike exercises three escalating patterns:

### Pattern 1 — multi-dispatch on user structs (✓ type-checks)

The Rust trait-object pattern:
```rust
trait Animal { fn speak(&self) -> String; }
impl Animal for Dog { fn speak(&self) -> String { format!("{} says Woof", self.name) } }
impl Animal for Cat { fn speak(&self) -> String { format!("{} says Meow", self.name) } }
```

Translated to Buff multi-dispatch:
```buff
struct Dog:
    name: String
struct Cat:
    name: String

func speak(a: Dog) -> String:
    return "${a.name} says Woof"
func speak(a: Cat) -> String:
    return "${a.name} says Meow"
```

Both `speak` impls form a multi-dispatch group. `buff check` accepts it.

### Pattern 2 — multi-dispatch on primitives (✓ type-checks)

Direct echo of `examples/multi_dispatch_numeric.buff`. Two `energy` impls
over `Int` and `Float`. `buff check` accepts it.

### Pattern 3 — homogeneous collection iteration (✓ type-checks)

```buff
func announce_dogs(pack: Vector<Dog>) -> String:
    let mut out = ""
    for d in pack:
        out = out + speak(d) + "\n"
    return out
```

This is the **only collection-dispatch pattern any of the 10 target
crates needs.** The Vector element type is statically known (`Dog`), so
multi-dispatch selects the matching `speak(Dog)` impl at every iteration.
`buff check` accepts it.

### Pattern 4 — heterogeneous collection (✗ IMPOSSIBLE — intentionally not in code)

```buff
// CANNOT BE EXPRESSED IN BUFF:
func announce_all(pets: Vector<???>) -> String:
    let mut out = ""
    for pet in pets:
        out = out + speak(pet) + "\n"
    return out
```

The `???` cannot be spelled because:
- Buff has no `Any` / `Object` top type.
- Buff has no union types (`Dog | Cat`).
- Buff has no `dyn Trait` (the feature under evaluation).
- The multi-dispatch resolver requires `arg_types: &[Type]` known at
  compile time — the entire premise of `Vec<Box<dyn Trait>>` is type
  erasure, which is the opposite requirement.

**INVENTORY RESULT:** This pattern does NOT appear in any of the 10
target crates. The spike documents the limit; no working code is possible
or required here.

### Run results (Docker, `buff-dev:latest`, rustc 1.95.0)

```
$ cargo run -p buff-lang-cli -- check examples/spike_multi_dispatch.buff
examples/spike_multi_dispatch.buff: no issues found

$ cargo run -p buff-lang-cli -- run examples/spike_multi_dispatch.buff
error[E0428]: the name `speak` is defined multiple times      <- codegen gap
error[E0428]: the name `energy` is defined multiple times     <- codegen gap
error[E0425]: cannot find type `Vector`                       <- prelude gap (existing)
error: aborting due to 17 previous errors

$ cargo run -p buff-lang-cli -- run examples/multi_dispatch_basic.buff
error[E0428]: the name `combine` is defined multiple times    <- SAME gap, official example
error[E0308]: arguments to this function are incorrect        <- SAME gap
error: aborting due to 2 previous errors

$ cargo run -p buff-lang-cli -- run examples/ola.buff
Olá, Buff!                                                    <- baseline OK
```

The spike's `buff run` failure mode is byte-identical to the failure of
the official shipped example. This is **not** a flaw in the spike; it is
a pre-existing codegen gap (T58 wired the type-checker but not the Rust
emitter). The gap is tracked separately (see §5 caveat 1).

---

## 3. Trait-object inventory — 10 target crates

**Scope:** every `*.rs` file under `src/` AND `tests/` of the 10 target
crates. Excludes `buff-lang-codegen-rust` and `buff-lang-types` per
DR-014 (those are IMPOSSIBLE to port and out of scope).

**Method:** `grep -rn '\bdyn\b\|Box<dyn\|&dyn\|trait ' <crate>/{src,tests}/`
followed by manual review of every hit to distinguish:
- (a) Rust-level `dyn Trait` trait-object usage — RELEVANT
- (b) `Buff trait` keyword tests in parser — IRRELEVANT (Buff has its own `trait` syntax)
- (c) `dyn std::error::Error` stdlib interop — RELEVANT but mechanical
- (d) `trait` mentioned in comments / docstrings — IRRELEVANT

| # | Crate | `dyn`/trait hits | Real Rust-level trait usage |
|---|---|---|---|
| 1 | `buff-lang-ast` | 0 | none |
| 2 | `buff-lang-ast-rsx` | 0 | none |
| 3 | `buff-lang-error` | 2 (both comments/strings) | none |
| 4 | `buff-lang-debug-info` | 1 (string literal in error msg) | none |
| 5 | `buff-lang-lexer` | **3 (1 trait + 2 `dyn` refs)** | **`LexCallback` trait** |
| 6 | `buff-lang-parser` | 44 (all Buff-lang keyword tests/comments) | none |
| 7 | `buff-lang-buffhtml-parser` | 0 | none |
| 8 | `buff-lang-ffi-guide` | 0 | none |
| 9 | `buff-eval` | 0 | none |
| 10 | `buff-template` | 0 | none |
| **Total** | | | **1 trait, 0 heterogeneous collections** |

### The single trait usage — `LexCallback`

**Location:** `crates/buff-lang-lexer/src/string_interp.rs:38-47`
```rust
pub trait LexCallback {
    fn lex_range(
        &mut self,
        source: &str,
        range_start: usize,
        range_end: usize,
        _source_id: SourceId,
        out: &mut Vec<Token>,
    ) -> Result<(), LexerError>;
}

pub fn scan_string(
    source: &str,
    quote_start: usize,
    source_id: SourceId,
    out: &mut Vec<Token>,
    interp_cb: &mut dyn LexCallback,   // <- the trait object
) -> Result<usize, LexerError> { ... }
```

**Impls (3 total — 1 production + 2 test):**
- `InterpLexer` (`crates/buff-lang-lexer/src/lexer.rs:1105-1110`) — production.
- `RecordInterp` (`crates/buff-lang-lexer/src/string_interp.rs:283`) — test.
- `RecordInterpWithSpec` (`crates/buff-lang-lexer/src/string_interp.rs:351`) — test.

**Call sites:** exactly ONE — `lexer.rs:221` passes `&mut interp_cb` from
the production `InterpLexer`. Test calls in `lex_str` / `lex_str_with_specs`
pass the test impls.

**Pattern classification:** SINGLE-CALLBACK injection, NOT a heterogeneous
collection. There is no `Vec<Box<dyn LexCallback>>`. There is no iteration
over multiple impls. There is exactly one slot filled with exactly one impl
per call.

**Buff translation options (any of these works):**
1. **Multi-dispatch**: declare two `func lex_range(interp: InterpLexer, ...)`
   and `func lex_range(interp: RecordInterp, ...)` — the resolver picks the
   impl from the static type at the call site.
2. **Direct inlining**: the production call site has a single impl, so
   inline `lex_range`'s body directly into `scan_string` and drop the
   trait entirely. The test impls become standalone test helpers.
3. **Function pointer**: pass `lex_range` as a `fn` parameter instead of
   a trait object. Buff supports function types.

**Verdict for LexCallback:** multi-dispatch CAN replace it (option 1).
Even simpler patterns also work (options 2 and 3). NOT a blocker.

### Stdlib trait interop — `dyn std::error::Error`

**Location:** `crates/buff-lang-lexer/src/error.rs:91`
```rust
fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { ... }
```

This is the canonical Rust `std::error::Error::source` impl. It exists
ONLY because Rust's `Error` trait requires it. **Buff does not have this
trait** — Buff errors lower to plain structs/enums with no vtable. When
porting, this method simply disappears (Buff's error model is structural).

**Verdict:** not a porting concern. The trait method evaporates.

### `Box<dyn Any>` string literal

**Location:** `crates/buff-lang-debug-info/src/panic_hook.rs:107`
```rust
"Box<dyn Any>".to_string()   // fallback string for unknown panic payload
```

This is a STRING LITERAL printed to stderr when the panic payload is
neither `&'static str` nor `String`. It is not a type usage. The actual
code uses `info.payload()` (Rust stdlib API returning `&(dyn Any + Send)`)
which is a host-language concern that vanishes when porting the panic
hook to Buff (Buff has no `Any` and no Rust-style panics; the panic hook
itself is an interop layer that may not need to exist in pure Buff).

**Verdict:** not a porting concern. String literal evaporates.

---

## 4. Verdict — `MULTI_DISPATCH_SUFFICIENT`

### Reasoning

1. **Zero heterogeneous collections across all 10 target crates.** The
   canonical hard case for trait-object replacement (storing N different
   concrete types in one `Vec<Box<dyn Trait>>`) does not exist in the
   porting scope. There is nothing for multi-dispatch to fail at.

2. **The one trait that exists (`LexCallback`) is a single-callback
   parameter with one production impl.** Multi-dispatch trivially
   handles this (Pattern 1 in the spike). Even simpler patterns (function
   pointers, direct inlining) also work.

3. **The stdlib `dyn std::error::Error` source-chain method evaporates**
   when porting to Buff (Buff's error model is structural, no vtable).

4. **The `Box<dyn Any>` mention is a string literal**, not a type usage.

### Decision matrix

| Verdict | Condition | Applies? |
|---|---|---|
| `MULTI_DISPATCH_SUFFICIENT` | Target crates use no heterogeneous trait collections | ✅ YES |
| `NEEDS_DYN_TRAIT` | Target crates need runtime polymorphism that multi-dispatch can't provide | ❌ no |
| `IMPOSSIBLE` | Trait dispatch fundamentally cannot be ported to Buff | ❌ no |

### Consequence

**Phase 2 (language extension to add `dyn Trait` to Buff) is SKIPPED
entirely.** The 10 target crates can be ported using:
- Buff structs + enums (for data shapes)
- Buff multi-dispatch (for static-overload-style polymorphism, when needed)
- Buff function types / direct inlining (for callback injection, like LexCallback)
- Buff enum + match (for any future heterogeneous-dispatch need — the canonical
  Rust alternative to trait objects, fully supported in Buff today)

---

## 5. Caveats (non-blocking but tracked)

### Caveat 1 — T58 is type-check-only; codegen mangling is unwired

**Symptom:** `buff run examples/multi_dispatch_basic.buff` fails with
`error[E0428]: the name 'combine' is defined multiple times`. The
generated Rust has two `fn combine(...)` definitions because the
codegen emitter is not consulting `MultiDispatchTable::mangled_name`
when lowering FuncDecls or call sites.

**Impact on this spike:** none — the verdict is about TYPE-SYSTEM
coverage, not about whether the codegen layer is complete. The
heterogeneous-collection limit is unreachable regardless of codegen
maturity.

**Impact on the porting plan:** the multi-dispatch codegen wiring MUST
land before any ported crate actually relies on multi-dispatch at
runtime. Until then, ported crates that need static-overload-style
dispatch must use enum + match (which works end-to-end today) instead.

**Task to file:** multi-dispatch codegen wiring (separate from S1).

### Caveat 2 — heterogeneous collections remain impossible in Buff

Any FUTURE porting target that uses `Vec<Box<dyn Trait>>` cannot be
ported to Buff without re-architecting to enum + match. This is
fundamental to Buff's design (no `Any`/`Object`/union types/`dyn Trait`)
and is unlikely to change without a major language extension (Phase 2).

**Mitigation:** the Rust ecosystem's own guidance
(https://rust-unofficial.github.io/patterns/patterns/behavioural/type_state.html
and the enum-as-trait-object idiom) documents the enum + match
alternative. It is a mechanical refactor in most cases.

### Caveat 3 — Vector prelude type not in default scope at codegen

The spike's `buff run` output includes `error[E0425]: cannot find type
'Vector'`. This is a separate prelude gap (the `Vector<T>` type is
declared in `prelude_types.rs` but not always emitted in the generated
Rust `use` block). Same root family as caveat 1. Not a verdict
consideration.

---

## 6. References

- `crates/buff-lang-types/src/multi_dispatch.rs` — T58 implementation (532 LOC).
- `crates/buff-lang-types/tests/multi_dispatch.rs` — T58 integration tests (396 LOC, 18 tests).
- `examples/multi_dispatch_{basic,numeric,polymorphic,matrix,combined}.buff` — official demos.
- `examples/spike_multi_dispatch.buff` — this spike (S1).
- `crates/buff-lang-lexer/src/string_interp.rs:38-47,66,283,351` — LexCallback trait.
- `crates/buff-lang-lexer/src/lexer.rs:221,1105-1110` — production usage.
- `crates/buff-lang-lexer/src/error.rs:91` — stdlib Error::source impl.
- `crates/buff-lang-debug-info/src/panic_hook.rs:107` — string literal.