# Buff Language — Master Orchestrator Plan

> **This is the master plan file.** It contains shared context, guardrails, and links to the 3 phased execution plans.
> Execute phases sequentially: **v0.1 → v0.5 → v1.0**

---

## TL;DR

> **Buff** — a high-level language that transpiles to Rust, with automatic CPU/GPU parallelism, native async (Tokio), and Go/C#-like simplicity. Written in Rust (dogfooding). Delivers Rust performance without the borrow-checker pain via intelligent clone+move semantics.
>
> **Total scope**: ~111 tasks + 4 final verification across 3 phases
> (Phase 1: 20 tasks, Phase 2: 47 tasks, Phase 3: 44 active + 3 deferred)
>
> **Phase Files**:
> - **Phase 1**: [`.sisyphus/plans/buff-v01-mvp.md`](./buff-v01-mvp.md) — "Olá Buff" MVP (Tasks 1-17, T33a, T88, T110)
> - **Phase 2**: [`.sisyphus/plans/buff-v05-language.md`](./buff-v05-language.md) — Complete Language (Tasks 18-37 + enhancements T67-T112)
> - **Phase 3**: [`.sisyphus/plans/buff-v10-production.md`](./buff-v10-production.md) — Heterogeneous Computing (Tasks 38-66 + enhancements T80-T113)
> - **Reference**: [`.sisyphus/plans/buff-numeric-system.md`](./buff-numeric-system.md) — Complete numeric type specification
> - **Reference**: [`.sisyphus/plans/buff-project-structure.md`](./buff-project-structure.md) — Project layout standard (buff.toml, templates, workspaces)
> - **Reference**: [`.sisyphus/plans/buff-conventions.md`](./buff-conventions.md) — 18 coding conventions (naming, formatting, docs, errors, testing, APIs)

---

## Shared Context

### Architecture
- **Transpiler pipeline**: `.buff` source → Lexer (logos) → Parser (chumsky) → Type Checker → Rust Codegen (syn/quote/prettyplease) → rustc/LLVM → native binary
- **Cargo workspace**: 9 crates (`buff-lang-ast`, `buff-lang-lexer`, `buff-lang-parser`, `buff-lang-types`, `buff-lang-codegen-rust`, `buff-lang-codegen-wgsl`, `buff-lang-runtime`, `buff-lang-cli`, `buff-lang-error`)
- **NO raw string codegen** — MUST use syn/quote/prettyplease
- **NO raw string WGSL** — MUST use AST-based WGSL codegen
- **Borrow strategy**: intelligent clone + move (v0.1-v0.5), design for Arc+CoW (v1.0+)

### 25 Reserved Keywords (All English)
func, let, mut, struct, enum, trait, type, if, else, for, return, break, continue, in, match, async, spawn, import, export, from, as, true, false, extern, unsafe

> **Removed keywords** (simplified): `try` (use `?`), `await` (auto-inserted), `const` (let auto-detects), `while` (merged into `for`), `throw` (use `return Error()`), `Int`/`Float`/`Double`/`Bool`/`Byte` (prelude types), `None`/`Some` (Option enum variants)

### Numeric Type System (See `buff-numeric-system.md` for full spec)
- **Two modes**: `Int` (flexible — compiler tracks range, grows/shrinks) vs `Int<W>` (fixed — checked overflow)
- Same pattern for `Bits<W>` (unsigned), `Float<W>` (16/32/64), `Decimal` (128-bit fixed)
- **GPU accepts ONLY WGSL-native**: Float<32>, Float<16>, Int<32>, Bits<32>

### OOP Decision (No class, no inheritance)
OOP-ergonomics via: Structs with methods + Traits with default methods + Struct embedding/delegation + Trait inheritance + Polymorphism via traits

### Async Strategy (Call Graph Propagation — v1.0)
- **`async` keyword marks I/O boundary functions** (usually stdlib: http, file, db operations)
- **Compiler propagates async up the call graph**: any function calling an async function auto-becomes async
- **Auto-suspension**: compiler inserts suspension points at async call sites automatically
- **User business logic: NO async, NO await needed** — just call functions, compiler handles everything
- `spawn` for concurrent background tasks, `task.result()` for joining
- `func main()` auto-async (propagation reaches it)
- `block(expr)` for sync→async bridge (rare: FFI callbacks, sync tests)
- Function types carry async-ness: `async func(String) -> String`
- FFI functions: user marks `async` explicitly (can't analyze Rust)
- **Solves the function coloring problem**: 95% of user code never needs `async/await` keywords
- **NO `await` keyword** — compiler auto-inserts suspension points. Users use `task.result()` to join spawned tasks.
- Keywords: `async` (I/O boundary marker), `spawn` (background task launcher)

### Global Guardrails (Apply to ALL phases)
- **NO** class, inheritance, any/dynamic, manual pointers, visible lifetimes, visible borrow checker errors
- **NO** premature optimization — correctness first
- **NO** raw string concatenation for codegen (syn/quote mandatory)
- **NO** LSP/IDE/self-hosting/macros in v1.0 (deferred to v2.0)
- **NO** non-WGSL quantization formats in v1.0 (BFloat16, FP8, FP4, NF4, Trit deferred to v2.0)
- **TDD mandatory**: RED→GREEN→REFACTOR for every task
- **Snapshot tests** via insta from Wave 1
- **Property tests** via proptest for lexer/parser
- **NEVER panic** in compiler code — all fallible operations return Result<T, E>
- **NO unwrap()/expect()** in non-test code — use ? operator or explicit match
- **NO unimplemented!()/todo!()** in committed code
- **NO unsafe Rust** in generated code unless wrapped in safe abstraction
- **Codegen MUST be deterministic** (same AST → identical output byte-for-byte)
- **Error messages are public API** — changes require snapshot update
- **AST structure is FROZEN** after T2 — changes require migration plan
- **Types defined in v0.1** must not have breaking changes in v0.5/v1.0
- **AI slop forbidden**: no premature trait abstraction, no Cargo feature flags unless needed, no excessive generics, no defensive null checks (Option handles it)

---

## Phase Overview

### Phase 1: v0.1 "Olá Buff" — MVP Foundation
**Goal**: Prove transpilation works end-to-end with IR-based architecture.
**Exit**: `buff run ola.buff` prints "Olá, Buff!"
**Tasks**: 20 (17 numbered + T88 IR design, T33a move semantics, T110 scaffolding)
**File**: [`buff-v01-mvp.md`](./buff-v01-mvp.md)

### Phase 2: v0.5 "Real Language" — Complete Type System
**Goal**: All types, pattern matching, modules, async, modern syntax, stdlib.
**Exit**: Multi-file programs with collections, enums, closures, error handling.
**Tasks**: 47 (20 core + 27 enhancement including stdlib prelude, parser recovery, module resolution)
**File**: [`buff-v05-language.md`](./buff-v05-language.md)

### Phase 3: v1.0 "Production" — Heterogeneous Computing
**Goal**: CPU parallelism, GPU compute, full tooling, release.
**Exit**: GPU-accelerated programs with automatic dispatch.
**Tasks**: 44 active + 3 deferred (26 core + 18 enhancement, T51-T53 deferred)
**File**: [`buff-v10-production.md`](./buff-v10-production.md)

---

## Verification Strategy (ALL phases)

- **TDD**: Tests written BEFORE implementation (RED→GREEN→REFACTOR)
- **Framework**: Rust `#[test]` + `insta` (snapshots) + `proptest` (property)
- **QA per task**: Tool + concrete steps + expected result + evidence path
- **Evidence**: `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`

---

## Commit Strategy

- **Branches**: `v0.1-dev`, `v0.5-dev`, `v1.0-dev` with feature branches per wave
- **Pattern**: `test(scope): ...` [RED] → `feat(scope): ...` [GREEN] → `refactor(scope): ...` (optional)
- **Tags**: `v0.1.0` (after Phase 1), `v0.5.0` (after Phase 2), `v1.0.0` (after Phase 3 + F1-F4)

---

## Final Verification Wave (After ALL 3 phases complete)

> 4 review agents in PARALLEL. ALL must APPROVE.

- [ ] **F1. Plan Compliance Audit** (`oracle`) — Verify all Must Have present, all Must NOT Have absent
- [ ] **F2. Code Quality Review** (`unspecified-high`) — `cargo clippy -- -D warnings` + `cargo test --workspace`
- [ ] **F3. Real Manual QA** (`unspecified-high`) — Execute ALL QA scenarios from ALL tasks
- [ ] **F4. Scope Fidelity Check** (`deep`) — Verify 1:1 spec-to-implementation, no scope creep
