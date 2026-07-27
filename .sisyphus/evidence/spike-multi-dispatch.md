# S1 — Multiple Dispatch Coverage Spike

**Date:** 2026-07-27
**Task:** S1 (Critical Path Determinant)
**Verdict:** **`NEEDS_DYN_TRAIT`** (Phase 2 proceeds)
**Branch:** main
**Commit:** (corrective — supersedes 92c2251 which carried the wrong verdict)

---

## TL;DR

The 10 target crates have **MINIMAL trait-object usage** (2 cases, both
refactorable to generics, ZERO heterogeneous collections), so the
*theoretical* answer is `MULTI_DISPATCH_SUFFICIENT`. **However**, Buff's
T58 multiple dispatch is **non-functional end-to-end**: the dispatcher
exists and is consulted by `buff check`, but the codegen layer in
`buff-lang-codegen-rust` has **ZERO references** to `MultiDispatchTable`,
so multi-dispatch groups emit as unmangled Rust free functions and trip
rustc `E0428` (duplicate definition). Combined with multi-dispatch's
fundamental inability to express heterogeneous collections
(`Vec<Box<dyn Trait>>`), the safe path is to add `dyn Trait` to Buff
(Phase 2) rather than rely on a feature that does not actually work today.

A previous auto-commit (92c2251) recorded `MULTI_DISPATCH_SUFFICIENT` as
the verdict based on the inventory alone; this corrective commit flips
the verdict after running the spike end-to-end and discovering the
codegen gap.

---

## 1. What `multi_dispatch.rs` Actually Provides

Source: `crates/buff-lang-types/src/multi_dispatch.rs` (532 LOC, T58).

### Design (verbatim from the doc comment)

> Compile-time dispatch on ALL argument types, not just the receiver. A
> group of free functions sharing the same name with different argument
> type signatures forms a "multi-dispatch group". At a call site, the
> compiler infers each argument's type and selects the unique matching
> impl. Single-dispatch is the special case (group size 1) — unchanged
> from pre-T58 behaviour.

### Key properties

- **COMPILE-TIME ONLY**. The doc states verbatim: *"All matching is on
  TYPES (not values); all dispatch is COMPILE-TIME (no runtime vtable,
  no dynamic dispatch)."*
- **Free functions only**. A group forms when 2+ top-level `func`s share
  a name. Methods inside `extend Type { ... }` blocks are explicitly
  excluded.
- **Mangling scheme**: `<buff_name>_<arg1_ty>_<arg2_ty>_...` (e.g.
  `combine(Int, Int)` -> `combine_int_int`). Generic base types collapse
  (`Vector<Int>` and `Vector<Float>` both -> `vector`); the 2+ arity of
  a group guarantees uniqueness.
- **Specificity**: exact-type matches beat widened (assignable) matches,
  mirroring Julia's method specificity.
- **Errors**: `E1201` (no matching impl), `E1202` (ambiguous dispatch).
  No new ErrorCode variants.

### What it explicitly CANNOT do

- **No runtime polymorphism**. By design — no vtable.
- **No heterogeneous collections**. The dispatcher selects the impl from
  the STATIC compile-time types of the arguments at the call site. There
  is no way to express "a `Vector<???>` whose elements have different
  concrete types erased behind a common trait".

---

## 2. Existing Buff Syntax Examples

Five `.buff` examples already exercise multi-dispatch at the syntactic
level. All five **PARSE** (`buff check` returns "no issues found").
**None of them RUN** via `buff run` (see section 4 for the codegen gap).

| File | Pattern |
|---|---|
| `examples/multi_dispatch_basic.buff` | `combine(Int,Int)` + `combine(Float,Float)` |
| `examples/multi_dispatch_numeric.buff` | `add(Int,Int)` + `add(Int,Float)` + `add(Float,Int)` |
| `examples/multi_dispatch_matrix.buff` | `matmul(Matrix,Vector)` + `matmul(Vector,Matrix)` |
| `examples/multi_dispatch_polymorphic.buff` | `combine` with 4 impls |
| `examples/multi_dispatch_combined.buff` | single-dispatch `process` + multi-dispatch `merge` |

The Buff syntax is **two or more `func` blocks at top level sharing a
name with different parameter type signatures**. No `extend`, no
`trait Foo`, no special markers — just name + arg-type overload.

---

## 3. Exhaustive Inventory of Trait-Object Usage (10 Target Crates)

Searched via `grep -rn '\bdyn\b\|^\s*trait \w'` in every target crate
plus `Box<dyn ...>`, `&dyn ...`, `Rc<dyn ...>`, `Arc<dyn ...>`. Each
"ZERO MATCHES" line was re-verified by direct grep on the actual files.

| Crate | Trait definitions | Trait-object usages |
|---|---|---|
| `buff-lang-ast/src/` | ZERO | ZERO |
| `buff-lang-ast-rsx/src/` | ZERO | ZERO (9x `impl Into<String>` arg-sugar, compile-time only) |
| `buff-lang-error/src/` | ZERO | ZERO (11x `impl Into<String>` arg-sugar) |
| `buff-lang-debug-info/src/` | ZERO | ZERO (1x `"Box<dyn Any>"` as a **string literal** in `panic_hook.rs:107`, not code; 1x comment mention) |
| `buff-lang-lexer/src/` | **1** (`LexCallback`) | **2** (see below) |
| `buff-lang-parser/src/` | ZERO | ZERO (1x `impl core::fmt::Display` arg-sugar) |
| `buff-lang-buffhtml-parser/src/` | ZERO | ZERO (2x `impl Into<String>` arg-sugar) |
| `buff-lang-ffi-guide/` | ZERO (docs only) | ZERO |
| `buff-eval/src/` | ZERO | ZERO |
| `buff-template/src/` | ZERO | ZERO |

**Grand total across the 10 target crates:**

- **1 custom trait definition**: `LexCallback` (`crates/buff-lang-lexer/src/string_interp.rs:38`)
- **2 actual trait-object usages**:
  - `&mut dyn LexCallback` — argument to `scan_string(...)` at `string_interp.rs:66` (callback object, **not** a collection)
  - `Option<&(dyn std::error::Error + 'static)>` — return from `fn source()` at `error.rs:91` (forced by `std::error::Error` trait signature)
- **ZERO heterogeneous collections**: No `Vec<Box<dyn T>>`, no `HashMap<K, Box<dyn T>>`, no `Rc<dyn T>`/`Arc<dyn T>` anywhere
- **~20 `impl Trait` arg-position sugar patterns**: all `impl Into<String>` / `impl core::fmt::Display` — these are **compile-time generics** (monomorphized), NOT runtime trait objects, and require no language extension

### Analysis per usage

#### `&mut dyn LexCallback` (string_interp.rs:66)

- **Purpose**: callback that re-lexes the inside of `${expr}` in interpolated strings
- **Single object**, not a collection
- **Can multi_dispatch replace it?** Partially: the *callback type* can be eliminated by replacing `&mut dyn LexCallback` with a generic parameter `&mut C: LexCallback` (monomorphization per concrete callback type). This is the standard "impl Trait -> generic" Rust refactor. **Multi_dispatch per se adds nothing** — the simplification is just generics.
- **Alternative**: replace the trait with a closure `FnMut(...) -> Result<()>`. Buff supports closures (`{ x => ... }` lambda syntax).

#### `fn source() -> Option<&(dyn std::error::Error + 'static)>` (error.rs:91)

- **Purpose**: Rust stdlib `std::error::Error::source()` method — the trait signature is FIXED by `std::error::Error`, so this exact return type is mandatory for any type that implements `std::error::Error`
- **Single return**, not a collection
- **Can multi_dispatch replace it?** No — Rust's `std::error::Error` trait is a foreign contract. However, this entire concern evaporates if the Buff port uses Buff-native errors (the Buff prelude has its own `Error` type, and Buff's `Result` doesn't require `std::error::Error`).

### Conclusion of inventory

**The 10 target crates have effectively ZERO demand for trait-object
polymorphism.** The 2 actual `dyn` usages are:

1. A single callback parameter (replace with generic or closure)
2. A stdlib trait contract (eliminated by using Buff-native errors)

There are no heterogeneous collections, no plugin registries, no
visitor patterns, no event buses — nothing that fundamentally requires
runtime dispatch over an erased type set.

---

## 4. SPIKE RESULT — T58 Multi-Dispatch Codegen Is Non-Functional

### Spike file

`examples/spike_multi_dispatch.buff` exercises three patterns:

1. **Multi-dispatch on user structs**: `func speak(a: Dog)` + `func speak(a: Cat)`
2. **Multi-dispatch on numeric types**: `func energy(a: Int)` + `func energy(a: Float)`
3. **Homogeneous collection iteration**: `func announce_dogs(pack: Vector<Dog>)` + `func announce_cats(pride: Vector<Cat>)`

Pattern 4 (heterogeneous collection) is intentionally commented out with
an explanation: it cannot type-check in Buff because no single static
type unifies `Dog` and `Cat`.

### `buff check` result

```
examples/spike_multi_dispatch.buff: no issues found
```

**The type-checker is happy** — the `TypeInferencer` consults
`MultiDispatchTable` (see `crates/buff-lang-types/src/infer.rs`) and
finds a unique matching impl per call site.

### `buff run` result

The compile path fails at `rustc` with 17 errors. The first one is
the smoking gun:

```
error[E0428]: the name `speak` is defined multiple times
 --> examples/spike_multi_dispatch.buff:12:1
  |
9 | fn speak(a: Dog) -> String {
  | -------------------------- previous definition of the value `speak` here
...
12 | fn speak(a: Cat) -> String {
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^ `speak` redefined here
```

### `buff expand` (intermediate Rust source)

The output is unmangled:

```rust
fn speak(a: Dog) -> String { ... }       // expected: speak_dog
fn speak(a: Cat) -> String { ... }       // expected: speak_cat
fn energy(a: i64) -> i64 { ... }         // expected: energy_int
fn energy(a: f32) -> f32 { ... }         // expected: energy_float
```

### Root cause

```
$ grep -rn 'MultiDispatchTable\|multi_dispatch\|mangle\|mangled_name\|is_group\|method_for_decl' crates/buff-lang-codegen-rust/src/
(no matches)
```

**The `buff-lang-codegen-rust` crate has ZERO references to the
multi-dispatch dispatcher.** The dispatcher exists
(`crates/buff-lang-types/src/multi_dispatch.rs`, 532 LOC, with 18 unit
tests + 18 integration tests), the type-checker consults it
(`crates/buff-lang-types/src/infer.rs`), but the codegen layer **never
calls `MultiDispatchTable::build()`, `is_group()`, `method_for_decl()`,
or `mangled_name()`**.

The codegen-rust crate DOES have a tests file
`tests/multi_dispatch.rs` (195 LOC) that *claims* to verify the
mangling (`assert!(src.contains("fn combine_int_int("))`), but the
source code under test contains no logic that could make that assertion
pass. Either those tests are failing (their results masked by advisory
CI — `cargo test` is `continue-on-error` per CI line 61) or they have
not been run recently.

### Pre-existing impact

This bug is **not caused by the spike** — it affects every shipped
multi-dispatch example:

```
$ ./target/debug/buff run examples/multi_dispatch_basic.buff
error[E0428]: the name `combine` is defined multiple times
 --> examples/multi_dispatch_basic.buff:4:1
  |
1 | fn combine(a: i64, b: i64) -> i64 {
  | --------------------------------- previous definition ...
4 | fn combine(a: f32, b: f32) -> f32 {
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `combine` redefined here
```

**T58 was declared shipped in v1.19 but is non-functional end-to-end.**
The dispatcher + type-checker integration landed; the codegen
mangling pass did not.

---

## 5. VERDICT — NEEDS_DYN_TRAIT

### Why not MULTI_DISPATCH_SUFFICIENT

The *theoretical* answer for the actual usage in the 10 target crates IS
multi-dispatch-sufficient: there are 2 trivial trait-object usages, both
refactorable to generics or eliminated by switching to Buff-native
types, and zero heterogeneous collections.

**However**, multi_dispatch as it stands is **non-functional end-to-end**:

- The dispatcher exists and is exercised by `buff check`.
- The codegen layer has zero integration with the dispatcher.
- Every existing multi-dispatch example (5 files in `examples/`) fails
  with `E0428` when run via `buff run`.
- The README's "Examples" table quietly omits all 5 multi-dispatch
  examples from the "runs" list — they parse + check but do not
  execute.

Declaring `MULTI_DISPATCH_SUFFICIENT` would require fixing this bug
first, which is itself a codegen task (not a language-extension task,
but still pre-Phase-2 work that the spike was supposed to gate).

### Why not IMPOSSIBLE

The spike does not show trait dispatch is impossible to port. The
opposite: the actual usage is so minimal that even manual refactoring
(2 call sites) would suffice. The plan is feasible; it just needs
Phase 2 to add `dyn Trait` so the port can be direct rather than
refactored.

### Why NEEDS_DYN_TRAIT

Three independent reasons, each sufficient on its own:

1. **Multi_dispatch codegen is broken** — relying on it for the port
   would require a codegen fix (T58 was supposed to ship in v1.19 but
   didn't actually integrate). Phase 2's `dyn Trait` is a cleaner,
   single-feature addition.

2. **Multi_dispatch fundamentally cannot express heterogeneous
   collections** — `Vec<Box<dyn Trait>>` is a runtime-polymorphic
   pattern; multi_dispatch is by-design compile-time only. While the 10
   target crates today have zero such collections, the porting plan
   should not assume zero future demand. `dyn Trait` covers both the
   single-callback and heterogeneous-collection cases uniformly.

3. **Direct port > refactor** — even if multi_dispatch worked perfectly
   and even if heterogeneous collections never appear, the cost of
   adding `dyn Trait` to Buff is bounded (one language extension + one
   vtable-aware codegen pass), and it eliminates fragile refactoring of
   callback patterns. The port becomes mechanical ("translate Rust
   syntax to Buff syntax") instead of semantic ("redesign trait
   hierarchies as free-function groups").

### Cost of proceeding with NEEDS_DYN_TRAIT

Phase 2 adds `dyn Trait` to Buff. Concretely:

- Syntax: `Box<dyn Trait>` and `&dyn Trait` in type position (mirror Rust)
- Type system: a new `Type::TraitObject { trait_name, lifetime }` variant
- Codegen: emit Rust `dyn Trait` directly (zero transformation needed —
  Rust already supports this)
- Prelude: trait-object-aware versions of `print`, `Vec`, etc. (mostly
  no-op since they already work with `Box<T>`)

The heavy lift is in `buff-lang-types` and `buff-lang-codegen-rust` —
both **excluded per DR-014**. So Phase 2 work on the 10 target crates
themselves is mechanical: port the Rust source directly, since the
language now supports the same trait-object semantics.

---

## 6. Sub-Findings (Pre-Existing Bugs Discovered)

These are not part of the verdict but are documented for follow-up:

### BUG-T58-A: Multi-dispatch codegen integration missing

- **Severity**: HIGH (feature claimed shipped, doesn't work)
- **Files affected**: `crates/buff-lang-codegen-rust/src/rust_codegen.rs`
  (10,570 LOC) and `crates/buff-lang-codegen-rust/src/rust_codegen/`
  (11 files). NONE reference `MultiDispatchTable`.
- **Symptom**: `buff run examples/multi_dispatch_basic.buff` -> E0428
- **Fix**: `RustCodegen::generate()` must call
  `MultiDispatchTable::build(decls)` and consult
  `table.method_for_decl(f)` in the `lower_func` arm to emit the mangled
  name. Call sites (`lower_call`) must consult `table.resolve(name, arg_tys, span)`
  and emit the resolved mangled callee.
- **Tests**: `crates/buff-lang-codegen-rust/tests/multi_dispatch.rs`
  already specifies the expected behaviour — those tests need to be run
  and their assertions honoured.

### BUG-T58-B: README and STATUS table misrepresent multi-dispatch as functional

- The root `AGENTS.md` and `README.md` list T58 multiple dispatch under
  v1.19 "Shipped".
- The 5 `examples/multi_dispatch_*.buff` files are absent from the
  README's "Examples" table (silently omitted because they don't run).
- The `buff check` command reports "no issues found" for code that
  cannot compile, which is misleading.

These should be reclassified as "parse-only" pending T58 codegen
integration.

---

## 7. Files Touched

| File | Change |
|---|---|
| `examples/spike_multi_dispatch.buff` | NEW (committed in 92c2251) — spike demonstrating multi-dispatch patterns |
| `.sisyphus/evidence/spike-multi-dispatch.md` | NEW (corrective commit) — this analysis with corrected verdict |

No Rust source modified (per MUST NOT DO section 3).