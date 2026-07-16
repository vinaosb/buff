
# Deox v0.1 "Ol� Deox" � MVP Foundation

> **Phase 1 of 3.** Foundational phase � proves transpilation works end-to-end.
> Next: [Phase 2 (v0.5)](./deox-v05-language.md) | Master: [deox-master.md](./deox-master.md)
>
> **Exit criteria**: `deox run ola.deox` prints "Ol�, Deox!"
> **Tasks**: 20 (fully detailed with TDD steps, QA scenarios, acceptance criteria)

## TL;DR

> **Quick Summary**: Build "Deox" — a high-level language that transpiles to Rust, with automatic CPU/GPU parallelism, native async (Tokio), and Go/C#-like simplicity. Written in Rust (dogfooding). Delivers performance of Rust without the borrow-checker pain via intelligent clone+move semantics.
>
> **Deliverables**:
> - Complete transpiler pipeline: Lexer (logos) → Parser (chumsky) → Type Checker → Rust Codegen (syn/quote) → WGSL Codegen
> - Runtime library: wgpu 26.0 (GPU), Rayon (CPU parallel), Tokio (async)
> - CLI tooling: `deox run`, `deox build`, `deox test`, `deox fmt`
> - Full type system: 13 types (Int, Bits, Float, Double, Bool, Byte, String, Decimal, Vector, Matrix, Map, Struct, Enum)
> - Heterogeneous computing: automatic CPU/GPU dispatch with hints (`@prefer(gpu)`)
> - Low-precision AI types: FP4/FP8/Trits (DEFERRED to v2.0 — not WGSL-native)
>
> **Estimated Effort**: XL (multiple months, ~100+ tasks across 3 release phases)
> **Parallel Execution**: YES — 3 phases, 6-8 waves each, 5-8 tasks per wave
> **Critical Path**: Lexer → Parser → AST → Type Checker → Rust Codegen → CLI → CPU Parallel → GPU Compute

---

## Context

### Original Request
Create a simplified Rust-like language ("Deox") that transpiles to Rust, with maximum performance, maximum productivity, and automatic multithreading across CPU/GPU. Named "Deoxidizer" — removes the "rust/complexity" leaving pure performance.

### Interview Summary
**Key Discussions**:
- **Architecture**: Source-to-source transpiler (Deox → Rust), no VM/GC
- **Borrow strategy**: Intelligent clone + move semantics (v0.1-v0.5), design prepared for Arc+CoW (v1.0+)
- **Types**: 13-type system with Float (f32) + Double (f64) as separate types, Decimal for finance
- **GPU**: WebGPU via wgpu 26.0 → WGSL shaders → DirectX/Vulkan/Metal
- **CPU**: Rayon work-stealing parallelism
- **Async**: Native async with call graph propagation (Tokio) runtime
- **Syntax**: Layout-sensitive (indentation-based), no braces in flow control, braces for structs/lambdas/interpolation
- **Scope**: Full master plan covering v0.1 → v0.5 → v1.0

**Research Findings**:
- **Parser stack**: logos (lexer) + chumsky (parser) — chumsky explicitly supports Python-style semantic indentation
- **Codegen**: syn/quote/prettyplease (NOT raw string concat — critical for maintainability)
- **Testing**: insta (snapshots) + proptest (property tests)
- **wgpu 26.0**: Compute pipeline patterns documented, lazy init + pipeline caching + buffer pooling for cold start mitigation
- **GPU integer limitation**: i64 slow on consumer GPUs, i32 native — transpiler converts with overflow checks

### Metis Review
**Identified Gaps** (addressed):
- **Borrow strategy underspecified** → RESOLVED: intelligent clone + move with Arc/CoW design prepared
- **Scope underestimated** → RESOLVED: restructured into 3 phased releases with explicit sub-divisions
- **f32 foot-gun** → RESOLVED: Float (f32) + Double (f64) as separate types
- **Async unclear** → RESOLVED: native Tokio async
- **No module system** → Added to v0.5 scope
- **No pattern matching** → Added to v0.5 scope
- **Error message quality** → Phased: basic in v0.1, ariadne-based diagnostics in v1.0

---

## Work Objectives

### Core Objective
Deliver a production-ready programming language that achieves "Rust performance with Go productivity" through source-to-source transpilation, with invisible CPU/GPU heterogeneous computing.

### Concrete Deliverables
- `deox` CLI binary (run/build/test/fmt/check)
- Transpiler crates: `deox-lexer`, `deox-parser`, `deox-ast`, `deox-types`, `deox-codegen-rust`, `deox-codegen-wgsl`
- Runtime crate: `deox-runtime` (wgpu + rayon + tokio integration)
- Standard library prelude: `print`, type conversions, collection methods
- Example programs: `ola.deox`, `fibonacci.deox`, `gpu_demo.deox`, `web_server.deox`
- **Unified numeric type system**: `Int<X>` (signed, auto-width), `Bits<X>` (unsigned, auto-width), `Float<X>` (16/32/64), `Decimal` (128-bit fixed)

### Definition of Done
- [ ] `deox run examples/ola.deox` prints "Olá, Deox!"
- [ ] `deox build --release examples/gpu_demo.deox` runs compute on GPU (or CPU fallback)
- [ ] `cargo test --workspace` passes 100%
- [ ] `cargo clippy -- -D warnings` clean across all crates
- [ ] Generated Rust passes `rustfmt --check`
- [ ] Benchmark: Deox code within 10% of equivalent hand-written Rust

### Must Have
- All 13 types implemented with full type inference
- Layout-sensitive parser with proper error recovery
- Automatic CPU parallel dispatch (Rayon) for `par_map`
- Automatic GPU dispatch (wgpu) when data exceeds threshold
- Native async with call graph propagation (Tokio)
- Error propagation via `?` operator
- Null-safety via Option type (no null/nil)
- Source maps for error reporting (Deox line ↔ Rust line)
- Graceful GPU degradation (fallback to CPU when GPU unavailable/VRAM insufficient)

### Must NOT Have (Guardrails)
- **NO `class` keyword or implementation inheritance** — OOP via structs + traits + embedding (Tasks T92, T93). Provides `obj.method()`, polymorphism, and code reuse WITHOUT diamond problem, v-tables, or fragile base class
- **NO `any`/`dynamic` types** — static typing only (union types `A | B` are allowed)
- **NO manual pointers** (`*`, `&`) in user syntax — transpiler manages memory
- **NO visible lifetimes** (`'a`) in user syntax — transpiler handles
- **NO visible borrow checker** errors from Rust leaking to user
- **NO premature optimization** — correctness first, optimization in v1.0 polish phase
- **NO LSP/IDE tooling in v1.0** — deferred to v2.0
- **NO self-hosting in v1.0** — deferred to v2.0
- **NO macros/metaprogramming in v1.0** — deferred to v2.0
- **NO custom operators** — fixed operator set only
- **NO WGSL via raw string concatenation** — use AST-based WGSL codegen
- **NO raw string concat for Rust codegen** — use syn/quote/prettyplease
- **AI slop to avoid**: over-abstracting types, premature standard library bloat, excessive error handling for impossible states, unnecessary derive macros

### Numeric Type System (Unified Design)

| Type | Default Width | Explicit Forms | Signed? | Rust Mapping | GPU (WGSL) |
|------|--------------|----------------|---------|--------------|------------|
| `Int` | auto (8→128) | `Int<8>`, `Int<16>`, `Int<32>`, `Int<64>`, `Int<128>` | ✅ Yes | i8/i16/i32/i64/i128 | i32 (native), others convert |
| `Bits` | auto (8→128) | `Bits<8>`, `Bits<16>`, `Bits<32>`, `Bits<64>`, `Bits<128>` | ❌ No | u8/u16/u32/u64/u128 | u32 (native) |
| `Float` | 32 | `Float<16>`, `Float<32>`, `Float<64>` | ✅ Yes | f16/f32/f64 | f32 (native), f16 (modern GPU) |
| `Decimal` | 128 | `Decimal` (fixed) | ✅ Yes | rust_decimal::Decimal | CPU only (parallel) |

**Friendly Aliases**: `Byte` = `Bits<8>`, `Double` = `Float<64>`, `Int` defaults to `Int<64>` for individual vars

**Auto-detection rules**:
- Individual variables: default to `Int<64>` (safe for arithmetic, 64-bit CPU native)
- Collections (`Vector<Int>`): compiler analyzes literal values, picks smallest width fitting ALL elements
- Dynamic data without literals: defaults to `Int<64>` unless explicitly annotated
- GPU dispatch: `Float<32>` and `Int<32>` are zero-conversion native; others convert with overflow checks

**Literal Suffixes**: `42` (Int auto), `42b` (Bits auto), `3.14` (Float32), `3.14d` (Float64/Double), `3.14m` (Decimal), `0xFF` (Bits hex), `0b1010` (Bits binary)

### Extended Numeric System (AI/ML + Ternary + Auto-Sizing)
> **Full specification**: See `.sisyphus/plans/deox-numeric-system.md` for complete details

**AI/ML Quantization Formats**:
- `BFloat16` — brain float (AI training, wider range)
- `Float<FP8_E4M3>` — NVIDIA Hopper inference (precision-focused)
- `Float<FP8_E5M2>` — NVIDIA Hopper inference (range-focused)
- `Float<FP4>` — extreme compression (4-bit float)
- `Float<NF4>` — NormalFloat 4-bit (QLoRA fine-tuning)
- Quantization API: `matrix.quantize(.FP8_E4M3)`, `.dequantize()`

**Trits (First-Class Ternary)**:
- `Trit` — single ternary value (-1, 0, +1), compiler-enforced
- `Trits<N>` — packed ternary storage (5 trits per byte optimal)
- Trit multiplication = sign logic only (no hardware multiplier needed!)
- BitNet b1.58 compatible. Emulated now, future native hardware ready.

**Auto-Sizing Arithmetic Rules** (MANDATORY — compiler tracks output widths):
| Operation | Output Width | Example |
|-----------|-------------|---------|
| `Int<W1> + Int<W2>` | `max(W1,W2)+1` | i8 + i8 → i16 (carry bit) |
| `Int<W1> * Int<W2>` | `W1+W2` | i8 × i8 → i16 |
| `Int<W> << n` | `W+n` | i8 << 4 → i12→i16 |
| `Bits<W1> & \| ^ Bits<W2>` | `max(W1,W2)` | u8 \| u16 → u16 |
| `Float<W> * Float<W>` | `min(W*2, 64)` | f16 × f16 → f32 (precision) |
| `Float<W1> OP Float<W2>` | `max(W1,W2)` | f16 + f32 → f32 |
| Quantized OP Quantized | promotes to Float<32> | FP8 + FP8 → f32 |

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: NO (greenfield project)
- **Automated tests**: YES (TDD) — tests written BEFORE implementation
- **Framework**: Rust built-in `#[test]` + `insta` (snapshots) + `proptest` (property tests)
- **TDD workflow**: Each task follows RED (failing test) → GREEN (minimal impl) → REFACTOR

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Transpiler crates**: Use Bash (`cargo test`) — Run tests, assert pass count, check snapshots
- **CLI**: Use Bash (`deox run/build`) — Execute commands, parse output, assert exit codes
- **Codegen**: Use Bash (`cargo test codegen_snapshot`) — Compare generated Rust/WGSL against fixtures
- **GPU**: Use Bash (`deox run gpu_demo.deox`) — Run compute demo, assert output correctness

---

## Execution Strategy

### Phase Structure

```
═══════════════════════════════════════════════════════════════
PHASE v0.1 "Olá Deox" — MVP Foundation (Waves 1-4)
Goal: Prove transpilation works. Compile & run basic Deox programs.
Exit: `deox run ola.deox` prints "Olá, Deox!"
═══════════════════════════════════════════════════════════════

PHASE v0.5 "Real Language" — Complete Type System (Waves 5-8)
Goal: All 13 types, pattern matching, modules, async, FFI basics.
Exit: Multi-file programs with collections, enums, error handling.
═══════════════════════════════════════════════════════════════

PHASE v1.0 "Production" — Heterogeneous Computing (Waves 9-14)
Goal: CPU parallelism, GPU compute, low-precision, full tooling.
Exit: GPU-accelerated programs with automatic dispatch.
═══════════════════════════════════════════════════════════════
```

### Parallel Execution Waves

```
PHASE v0.1 — MVP Foundation:

Wave 1 (Start Immediately — scaffolding + contracts + IR foundation):
├── T1: Cargo workspace + CI + repo structure [quick]
├── T2: deox-ast crate (ALL AST node types, includes IR dataflow nodes) [deep]
├── T3: deox-lexer crate skeleton (logos tokens, preserves whitespace option) [quick]
├── T4: deox-error crate (error types, source spans) [quick]
├── T5: Testing infrastructure (insta + proptest setup) [quick]
└── T88: Deox IR design — dataflow graph for algorithm/schedule separation [ultrabrain]

Wave 2a (After Wave 1 — lexer):
└── T6: Lexer implementation — tokens + indentation [deep]

Wave 2b (After T6 — expression parser):
└── T7: Parser — expressions (literals, binary/unary ops) [deep]

Wave 2c (After T7 — statements + layout, can overlap):
├── T8: Parser — statements (let, if/else, func) [deep]
└── T9: Parser — layout-sensitive blocks (offside rule) [deep]

Wave 3a (After Wave 2 — type system + codegen infra, parallel):
├── T10: deox-types crate — type representation + inference + flow-sensitive [deep]
├── T11: deox-codegen-rust — syn/quote infra + move-by-default semantics [deep]
└── T33a: Codegen move semantics — every codegen task assumes moves (T33b adds clones later) [deep]

Wave 3b (After Wave 3a — actual codegen, parallel):
├── T12: Codegen — literals, let, arithmetic → Rust (via IR) [deep]
└── T13: Codegen — if/else expression, func → Rust (via IR) [deep]

Wave 4 (After Wave 3 — CLI + integration):
├── T14: deox-cli — `deox build` command [unspecified-high]
├── T15: deox-cli — `deox run` command [unspecified-high]
├── T16: Source map infrastructure (Deox ↔ Rust line mapping) [unspecified-high]
├── T110: `deox new` + `deox init` — project scaffolding with standard structure [deep]
└── T17: v0.1 milestone — "Olá Deox" example + integration tests [deep]


PHASE v0.5 — Real Language:

Wave 5 (After v0.1 — remaining primitive types):
├── T18: Double (f64) type support [quick]
├── T19: Byte type support [quick]
├── T20: Decimal (128-bit) type support [deep]
├── T21: String operations + interpolation codegen [deep]
└── T22: Numeric coercion rules (Int↔Float↔Double) [deep]

Wave 6 (After Wave 5 — collections + user types):
├── T23: Vector<T> type + codegen (Vec<T>) [deep]
├── T24: Matrix<T> type + codegen (flat 2D) [deep]
├── T25: Map<K,V> type + codegen (HashMap) [deep]
├── T26: Struct type + #[repr(C)] codegen [deep]
├── T27: Enum type + pattern matching (match/case) [deep]
└── T28: Option<T> + null safety enforcement [deep]

Wave 7 (After Wave 6 — modules + async + FFI):
├── T29: Module system (import/export, multi-file) [deep]
├── T30: Error types + `?` operator propagation [deep]
├── T31: async support (Tokio integration) [deep]
├── T32: FFI basics — import Rust crates [unspecified-high]
└── T33: intelligent clone analysis (insert clone only where needed) [ultrabrain]

Wave 8 (After Wave 7 — closures + polish):
├── T34: Closures/lambdas codegen [deep]
├── T35: `deox test` command (test runner) [unspecified-high]
├── T36: Error message improvements (spans, context) [unspecified-high]
└── T37: v0.5 milestone — comprehensive example suite [deep]


PHASE v1.0 — Production:

Wave 9 (After v0.5 — runtime + CPU parallelism):
├── T38: deox-runtime crate skeleton (wgpu + rayon + tokio) [deep]
├── T39: CPU parallel dispatch (Rayon par_map/par_filter) [deep]
├── T40: Automatic dispatch threshold logic [deep]
├── T41: Data race detection in parallel closures [deep]
└── T42: AtomicI64 auto-insertion for shared state [deep]

Wave 10 (After Wave 9 — GPU compute):
├── T43: wgpu context initialization (lazy, cached) [deep]
├── T44: deox-codegen-wgsl crate (AST → WGSL) [deep]
├── T45: GPU dispatch pipeline (buffer create → shader → dispatch → readback) [deep]
├── T46: VRAM check + tiling + CPU fallback [deep]
├── T47: Cold start mitigation (async background init) [unspecified-high]
└── T48: Recursion detection (mark CPU-only) [deep]

Wave 11 (After Wave 10 — advanced GPU features):
├── T49: Hints system (@prefer(gpu/npu)) + multi-version codegen [deep]
├── T50: GPU memory alignment (#[repr(C)] auto for GPU structs) [deep]
├── T51: Low-precision: FP8/FP4 bit-packing in WGSL [ultrabrain]
├── T52: Low-precision: Trits (-1,0,1) bit-packing [ultrabrain]
└── T53: Matrix quantization API (.quantizar(Layout.FP4)) [deep]

Wave 12 (After Wave 11 — tooling + polish):
├── T54: `deox fmt` formatter [unspecified-high]
├── T55: `deox check` type-checker-only mode [quick]
├── T56: `deox build --release` optimization mode [quick]
├── T57: LSP-friendly AST (lossless, incremental-ready) [deep]
└── T58: Wasm target support (wasm32-unknown-unknown) [deep]

Wave 13 (After Wave 12 — diagnostics + DX):
├── T59: ariadne-based error diagnostics [visual-engineering]
├── T60: Source map improvements (full Deox→Rust→binary) [deep]
├── T61: Standard library expansion (I/O, HTTP, JSON) [unspecified-high]
└── T62: Documentation + examples + README [writing]

Wave 14 (After Wave 13 — release):
├── T63: Performance benchmarks (Deox vs Rust vs Go) [deep]
├── T64: Cross-platform testing (Windows/Linux/macOS) [unspecified-high]
├── T65: v1.0 release preparation [deep]
└── T66: Comprehensive integration test suite [deep]

Wave FINAL (After ALL tasks — 4 parallel reviews):
├── F1: Plan compliance audit (oracle)
├── F2: Code quality review (unspecified-high)
├── F3: Real manual QA (unspecified-high)
└── F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: T1→T6→T9→T10→T12→T14→T17→T23→T29→T38→T43→T45→T49→T65
Parallel Speedup: ~65% faster than sequential
Max Concurrent: 6 (Waves 5, 6, 9)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave |
|------|-----------|--------|------|
| 1-5 | None | 6-17 | 1 |
| 6 | 3 | 7-9 | 2 |
| 7-9 | 2, 6 | 10-13 | 2 |
| 10 | 2 | 12-13, 18-28 | 3 |
| 11 | 2 | 12-13 | 3 |
| 12-13 | 7-9, 10, 11 | 14-17 | 3 |
| 14-16 | 12-13 | 17 | 4 |
| 17 | 14-16 | 18-37 | 4 |
| 18-22 | 10 | 23-28 | 5 |
| 23-28 | 11, 18-22 | 29-37 | 6 |
| 29-33 | 23-28 | 34-37 | 7 |
| 34-37 | 29-33 | 38-66 | 8 |
| 38-42 | 29-33 | 43-53 | 9 |
| 43-48 | 38-42 | 49-53 | 10 |
| 49-53 | 43-48 | 54-58 | 11 |
| 54-58 | 49-53 | 59-62 | 12 |
| 59-62 | 54-58 | 63-66 | 13 |
| 63-66 | 59-62 | F1-F4 | 14 |

### Agent Dispatch Summary

- **Wave 1** (5 tasks): T1→`quick`, T2→`deep`, T3→`quick`, T4→`quick`, T5→`quick`
- **Wave 2** (4 tasks): All→`deep`
- **Wave 3** (4 tasks): All→`deep`
- **Wave 4** (4 tasks): T14-T16→`unspecified-high`, T17→`deep`
- **Wave 5** (5 tasks): T18-T19→`quick`, T20-T22→`deep`
- **Wave 6** (6 tasks): All→`deep`
- **Wave 7** (5 tasks): T29-T31→`deep`, T32→`unspecified-high`, T33→`ultrabrain`
- **Wave 8** (4 tasks): T34→`deep`, T35-T36→`unspecified-high`, T37→`deep`
- **Wave 9** (5 tasks): All→`deep`
- **Wave 10** (6 tasks): T43-T46→`deep`, T47→`unspecified-high`, T48→`deep`
- **Wave 11** (5 tasks): T49-T50→`deep`, T51-T52→`ultrabrain`, T53→`deep`
- **Wave 12** (5 tasks): T54→`unspecified-high`, T55-T56→`quick`, T57-T58→`deep`
- **Wave 13** (4 tasks): T59→`visual-engineering`, T60→`deep`, T61→`unspecified-high`, T62→`writing`
- **Wave 14** (4 tasks): T63→`deep`, T64→`unspecified-high`, T65-T66→`deep`
- **FINAL** (4 tasks): F1→`oracle`, F2→`unspecified-high`, F3→`unspecified-high`, F4→`deep`

---

## TODOs

### ═══ PHASE v0.1 "Olá Deox" — MVP Foundation ═══

- [x] 1. Cargo Workspace + CI + Repository Structure

  **What to do**:
  - Create Cargo workspace with crates: `deox-ast`, `deox-lexer`, `deox-parser`, `deox-types`, `deox-codegen-rust`, `deox-codegen-wgsl`, `deox-runtime`, `deox-cli`, `deox-error`
  - Root `Cargo.toml` with workspace dependencies (logos, chumsky, syn, quote, insta, proptest, wgpu, rayon, tokio, bytemuck, rust_decimal)
  - Create `examples/`, `tests/fixtures/`, `tests/snapshots/` directories
  - Add `.gitignore`, `rust-toolchain.toml` (stable channel), GitHub Actions CI (fmt + clippy + test on push)
  - Add `examples/ola.deox` placeholder (will be runnable after Wave 4)

  **Must NOT do**:
  - NO implementation logic yet — just scaffolding
  - NO dependency version conflicts — pin all to latest compatible
  - NO custom build scripts (build.rs) yet

  **Recommended Agent Profile**:
  - **Category**: `quick` — Mechanical scaffolding, well-defined structure
  - **Skills**: `[]` — Standard Rust workspace setup

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4, 5)
  - **Blocks**: Tasks 6-66 (everything depends on workspace existing)
  - **Blocked By**: None (can start immediately)

  **References**:
  - **Pattern**: Rust workspace convention — `crates/` directory with one crate per concern
  - **External**: https://doc.rust-lang.org/cargo/reference/workspaces.html — workspace manifest format
  - **External**: https://github.com/gfx-rs/wgpu — wgpu crate version 26.0+ dependency syntax

  **Acceptance Criteria**:

  - [ ] `cargo check --workspace` succeeds
  - [ ] `cargo test --workspace` runs (0 tests, exits 0)
  - [ ] GitHub Actions CI file exists at `.github/workflows/ci.yml`
  - [ ] All 9 crate directories exist with `Cargo.toml` + `src/lib.rs`

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Workspace compiles cleanly
    Tool: Bash (cargo)
    Preconditions: Rust toolchain installed
    Steps:
      1. Run `cargo check --workspace`
      2. Assert exit code 0
      3. Run `cargo fmt --check`
      4. Assert exit code 0
    Expected Result: All 9 crates compile without errors or warnings
    Failure Indicators: Compilation errors, missing dependencies, version conflicts
    Evidence: .sisyphus/evidence/task-1-workspace-check.txt

  Scenario: CI workflow file is valid
    Tool: Bash (yaml validation)
    Preconditions: .github/workflows/ci.yml exists
    Steps:
      1. Read ci.yml content
      2. Verify it contains: jobs with rustup, cargo fmt, cargo clippy, cargo test
      3. Verify matrix includes ubuntu-latest, windows-latest, macos-latest
    Expected Result: Valid GitHub Actions workflow
    Failure Indicators: Missing jobs, invalid YAML syntax
    Evidence: .sisyphus/evidence/task-1-ci-validation.txt
  ```

  **Commit**: YES
  - Message: `chore: initialize deox cargo workspace with 9 crates`
  - Files: `Cargo.toml`, `crates/*/Cargo.toml`, `.github/workflows/ci.yml`, `rust-toolchain.toml`
  - Pre-commit: `cargo check --workspace`

---

- [x] 2. deox-ast Crate — Complete AST Node Definitions

  **What to do**:
  - Define ALL AST node types covering the full Deox language:
    - Expressions: Literal (Int/Float/Double/Bool/String/Byte), BinaryOp, UnaryOp, IfExpr, FuncCall, MethodCall, Lambda (closure), StructInit, MatchExpr, SuspendExpr
    - Statements: LetDecl, Assignment, ExprStmt, Return, Break, Continue
    - Declarations: FuncDecl, StructDecl, EnumDecl, ImportDecl, ModuleDecl
    - Types: TypeRef (named), GenericType, OptionType, FunctionType
  - Each node carries `Span` info (start/end byte offsets) for error reporting
  - Implement `Display` for pretty-printing AST (used in error messages)
  - Add `#[derive(Debug, Clone, PartialEq)]` to all nodes
  - Write snapshot tests: parse examples → AST → pretty-print → snapshot

  **Must NOT do**:
  - NO parsing logic (that's the parser crate)
  - NO type checking (that's the types crate)
  - NO semantic validation — pure data structures
  - NO lifetime annotations (own all data via String, Vec)

  **Recommended Agent Profile**:
  - **Category**: `deep` — Architectural data modeling, affects everything downstream
  - **Skills**: `[]` — Pure Rust type design

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4, 5)
  - **Blocks**: Tasks 7-9 (parser), 10 (types), 11 (codegen)
  - **Blocked By**: Task 1 (workspace must exist)

  **References**:
  - **Pattern**: Rust's own AST (`rustc_ast`) — node + span pattern
  - **External**: https://doc.rust-lang.org/reference/statements.html — statement categories
  - **External**: Rust syn crate `Expr` enum — reference for expression node design

  **Acceptance Criteria**:

  - [ ] `cargo test -p deox-ast` passes
  - [ ] All AST nodes have `Span` fields
  - [ ] `format!("{}", ast_node)` produces readable output
  - [ ] Snapshot test: 5 AST examples produce stable snapshots

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: AST nodes are constructible and displayable
    Tool: Bash (cargo test)
    Preconditions: deox-ast crate exists
    Steps:
      1. Run `cargo test -p deox-ast`
      2. Assert all tests pass
      3. Run `cargo test -p deox-ast -- --nocapture pretty_print`
      4. Assert output contains readable AST structure
    Expected Result: All AST node types constructible, display correctly
    Failure Indicators: Missing derive macros, Span not propagated, Display not implemented
    Evidence: .sisyphus/evidence/task-2-ast-tests.txt

  Scenario: Snapshot stability
    Tool: Bash (cargo insta)
    Preconditions: insta crate configured
    Steps:
      1. Run `cargo insta test -p deox-ast`
      2. Assert 0 pending snapshots (all accepted)
      3. Run `git diff --exit-code tests/snapshots/`
      4. Assert exit code 0 (no uncommitted snapshot changes)
    Expected Result: Snapshots stable and committed
    Failure Indicators: Snapshot drift, missing snapshots
    Evidence: .sisyphus/evidence/task-2-snapshot-stability.txt
  ```

  **Commit**: YES
  - Message: `feat(ast): define complete AST node types with spans`
  - Pre-commit: `cargo test -p deox-ast`

---

- [x] 3. deox-lexer Crate Skeleton — Token Definitions

  **What to do**:
  - Define `TokenKind` enum with all token types: identifiers, keywords (func, let, mut, struct, enum, trait, type, if, else, for, return, break, continue, in, match, async, spawn, import, export, from, as, true, false, extern, unsafe), literals (int, float, double suffix `d`, string, byte), operators (+, -, *, /, %, ==, !=, <, >, <=, >=, &&, ||, !, ?, =>, ->), delimiters (`{`, `}`, `(`, `)`, `[`, `]`, `:`), and special tokens (Newline, Indent, Dedent, EOF)
  - Define `Token` struct with `kind: TokenKind`, `span: Span`, and optionally the lexed text
  - Define `LexerError` types (unexpected char, unterminated string, invalid number)
  - Set up logos derive macros scaffolding (actual lexing in Task 6)
  - Write tests for token equality and span construction

  **Must NOT do**:
  - NO actual lexing implementation (Task 6)
  - NO indentation algorithm (Task 6)
  - NO parser integration

  **Recommended Agent Profile**:
  - **Category**: `quick` — Token enum definition, mechanical
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4, 5)
  - **Blocks**: Task 6 (lexer implementation)
  - **Blocked By**: Task 1 (workspace)

  **References**:
  - **External**: https://logos.maciej.codes/ — logos derive macro syntax
  - **Pattern**: Rust's `rustc_token::TokenKind` — comprehensive token categorization

  **Acceptance Criteria**:

  - [ ] `TokenKind` covers all Deox syntax elements
  - [ ] `Token` struct has kind + span
  - [ ] `LexerError` enum defined with Display impl
  - [ ] `cargo test -p deox-lexer` passes

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Token types are complete
    Tool: Bash (cargo test)
    Steps:
      1. Run `cargo test -p deox-lexer token_coverage`
      2. Assert test verifies all keywords exist: func, let, if, else, for, return, struct, enum
      3. Assert test verifies all operators exist: +, -, *, /, ==, !=, <=, >=, &&, ||, !, ?, =>
    Expected Result: All token variants present and testable
    Evidence: .sisyphus/evidence/task-3-token-coverage.txt
  ```

  **Commit**: YES
  - Message: `feat(lexer): define token types and lexer error types`

---

- [x] 4. deox-error Crate — Error Types + Source Spans

  **What to do**:
  - Define `Span` struct: `start: ByteOffset`, `end: ByteOffset`, `source_file: SourceId`
  - Define `SourceMap`: maps `SourceId` → file path, stores line/column info for byte offsets
  - Define `DeoxError` enum: `LexError`, `ParseError`, `TypeError`, `CodegenError`, `RuntimeError`
  - Define `Diagnostic` struct: severity (Error/Warning), message, span, optional notes/suggestions
  - Implement `SourceSpan` lookup: given a byte offset, return (line, column)
  - Write tests for span calculation (multi-line files, unicode chars)

  **Must NOT do**:
  - NO fancy terminal formatting (ariadne integration is Task 59)
  - NO source map persistence across files (single-file in v0.1)

  **Recommended Agent Profile**:
  - **Category**: `quick` — Well-defined error types
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1
  - **Blocks**: Tasks 6 (lexer errors), 9 (parser errors), 16 (source maps)
  - **Blocked By**: Task 1

  **References**:
  - **Pattern**: Rust's `Span` and `SourceMap` in rustc
  - **External**: `codespan-reporting` crate — reference for span-based diagnostics

  **Acceptance Criteria**:

  - [ ] `Span` and `SourceMap` correctly calculate line/column from byte offset
  - [ ] `DeoxError` covers all error categories
  - [ ] `Diagnostic` has severity + message + span
  - [ ] Test: 3-line file with unicode → correct (line, col) for offsets 0, 15, 30

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Span calculation handles unicode
    Tool: Bash (cargo test)
    Steps:
      1. Create test string: "olá\nmundo\n✓"
      2. Query span at byte offset 8 (inside "mundo")
      3. Assert returns line=2, column=3
      4. Query byte offset 12 (the ✓ char)
      5. Assert returns line=3, column=1
    Expected Result: Correct line/column even with multi-byte UTF-8
    Failure Indicators: Off-by-one errors, ignoring multi-byte chars
    Evidence: .sisyphus/evidence/task-4-span-unicode.txt
  ```

  **Commit**: YES
  - Message: `feat(error): define spans, source maps, and diagnostic types`

---

- [x] 5. Testing Infrastructure — insta + proptest Setup

  **What to do**:
  - Configure `insta` crate for snapshot testing across all crates
  - Create `tests/snapshots/` directory with `.gitignore` for pending snapshots
  - Configure `proptest` for property-based testing
  - Write helper macros/functions: `assert_snapshot!(actual)` for codegen output
  - Write property test template for lexer (fuzz roundtrip: tokens → string → tokens)
  - Create `tests/fixtures/` with sample `.deox` files (valid + invalid examples)
  - Document testing conventions in `crates/deox-ast/TESTING.md`

  **Must NOT do**:
  - NO actual test implementations (each task writes its own tests)
  - NO test runner framework (use cargo test)

  **Recommended Agent Profile**:
  - **Category**: `quick` — Infrastructure setup
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1
  - **Blocks**: All subsequent tasks (testing infra must exist)
  - **Blocked By**: Task 1

  **References**:
  - **External**: https://insta.rs/ — insta snapshot testing docs
  - **External**: https://proptest-rs.github.io/proptest/ — proptest docs

  **Acceptance Criteria**:

  - [ ] `insta` configured in workspace Cargo.toml as dev-dependency
  - [ ] `proptest` configured
  - [ ] `tests/snapshots/` exists with README
  - [ ] `tests/fixtures/valid/ola.deox` exists with sample code
  - [ ] Helper function `assert_codegen_snapshot` compiles

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Snapshot infrastructure works
    Tool: Bash (cargo insta)
    Steps:
      1. Create dummy test using insta::assert_snapshot!
      2. Run `cargo insta test`
      3. Assert creates .pending-snap file
      4. Run `cargo insta review` (or accept all)
      5. Assert .snap file committed
    Expected Result: Snapshot workflow functional
    Evidence: .sisyphus/evidence/task-5-insta-setup.txt
  ```

  **Commit**: YES
  - Message: `test: configure insta snapshots and proptest infrastructure`

---

### Wave 2: Core Lexing + Parsing

- [x] 6. Lexer Implementation — Tokens + Indentation Algorithm

  **What to do** (TDD — write tests FIRST):
  - **RED**: Write failing tests for: tokenizing identifiers, keywords, int literals, float literals (with `d` suffix for double), string literals (with interpolation `{expr}`), operators, comments (`//` line, `/* */` block)
  - **RED**: Write failing tests for indentation: newline followed by more spaces → emit `Indent` token; fewer spaces → emit `Dedent` token; same → nothing
  - **RED**: Write property test: `lex(source).map(unlex) == source` for valid inputs
  - **GREEN**: Implement lexer using `logos` derive macros
  - **GREEN**: Implement indentation algorithm (offside rule): maintain stack of indent levels, emit Indent/Dedent on level change
  - **GREEN**: Handle edge cases: tabs vs spaces (error on mixed), blank lines (ignored), comments at end of line, CRLF normalization
  - **GREEN**: Handle string interpolation: `"text {expr} more"` → tokenize as StringStart, StringPart, InterpolationStart, ...tokens..., InterpolationEnd, StringPart, StringEnd
  - **REFACTOR**: Extract string interpolation into separate state machine

  **Must NOT do**:
  - NO parser logic
  - NO type annotations in tokens
  - NO semantic validation

  **Recommended Agent Profile**:
  - **Category**: `deep` — Core algorithm (indentation is notoriously tricky)
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO (parser tasks 7-9 depend on this)
  - **Parallel Group**: Sequential within Wave 2
  - **Blocks**: Tasks 7, 8, 9 (parser)
  - **Blocked By**: Tasks 3, 4 (token definitions, error types)

  **References**:
  - **Pattern**: astral-sh/ruff Python lexer — indentation tracking with stack
  - **Pattern**: Glyphack/enderpy — whitespace tracking approach
  - **External**: https://logos.maciej.codes/ — logos API for custom tokenization
  - **Algorithm**: Offside rule (Landin 1966) — indentation-based block detection

  **Acceptance Criteria**:

  - [ ] `cargo test -p deox-lexer` passes all tests (target: 30+ tests)
  - [ ] Lexer correctly tokenizes: `func ola():\n    print("Olá")`
  - [ ] Indentation: 4-space indent → Indent token; return to 0 → Dedent
  - [ ] String interpolation: `"valor {x}"` → correct token sequence
  - [ ] Mixed tabs/spaces → LexerError
  - [ ] Property test: 1000 fuzz inputs, roundtrip succeeds for valid
  - [ ] Snapshot: 10 fixture files produce stable token snapshots

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Tokenize hello world
    Tool: Bash (cargo test)
    Steps:
      1. Input: `func main():\n    print("Olá, Deox!")`
      2. Run lexer
      3. Assert token sequence: [Func, Ident("main"), LParen, RParen, Colon, Newline, Indent, Ident("print"), LParen, StringStart, StringLit("Olá, Deox!"), StringEnd, RParen, Dedent, EOF]
    Expected Result: Exact token sequence match
    Evidence: .sisyphus/evidence/task-6-hello-world-tokens.txt

  Scenario: Indentation tracking
    Tool: Bash (cargo test)
    Steps:
      1. Input: `if x:\n    a()\n    b()\nc()` (4-space indent, return to 0)
      2. Assert Indent emitted before `a()`, no change before `b()`, Dedent before `c()`
    Expected Result: [If, Ident, Colon, Newline, Indent, ...a..., Newline, ...b..., Newline, Dedent, ...c..., EOF]
    Evidence: .sisyphus/evidence/task-6-indent-tracking.txt

  Scenario: Error on mixed tabs and spaces
    Tool: Bash (cargo test)
    Steps:
      1. Input: `if x:\n\t    a()` (tab then spaces)
      2. Assert LexerError with message mentioning "mixed tabs and spaces"
    Expected Result: LexerError, no panic
    Evidence: .sisyphus/evidence/task-6-mixed-indent-error.txt

  Scenario: String interpolation tokenization
    Tool: Bash (cargo test)
    Steps:
      1. Input: `"valor {x + 1} fim"`
      2. Assert tokens: [StringStart, StringPart("valor "), InterpStart, Ident("x"), Plus, IntLit(1), InterpEnd, StringPart(" fim"), StringEnd]
    Expected Result: Interpolation tokens correctly nested
    Evidence: .sisyphus/evidence/task-6-string-interp.txt
  ```

  **Commit**: YES (TDD: test commit + impl commit)
  - Messages: `test(lexer): add tokenization and indentation tests` then `feat(lexer): implement logos lexer with indentation tracking`

---

- [x] 7. Parser — Expressions (Literals, Binary/Unary Ops, Function Calls)

  **What to do** (TDD):
  - **RED**: Tests for: int literal `42`, float `3.14`, double `99.90d`, bool `true`/`false`, string `"hello"`, byte `0xFF`
  - **RED**: Tests for: binary ops with precedence (`a + b * c` = `a + (b * c)`), comparison (`a < b`), logical (`a && b || c`), equality (`a == b`)
  - **RED**: Tests for: unary ops (`-x`, `!flag`), chaining (`- -x`)
  - **RED**: Tests for: function calls (`foo()`, `foo(a, b)`, `obj.method()`), nested (`foo(bar())`)
  - **RED**: Tests for: expression in operand position (`let x = a + if cond { 1 } else { 2 }`)
  - **GREEN**: Implement expression parser using chumsky combinators
  - **GREEN**: Implement Pratt parsing for operator precedence
  - **GREEN**: Handle all literal types with correct suffixes
  - **REFACTOR**: Extract precedence table into config

  **Must NOT do**:
  - NO statement parsing (Task 8)
  - **Must NOT do**: NO layout-sensitive blocks here (Task 9)
  - **Must NOT do**: NO error recovery (fail-fast for now, improve later)

  **Recommended Agent Profile**:
  - **Category**: `deep` — Pratt parsing + chumsky combinators require careful design
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: NO (needs lexer Task 6)
  - **Blocks**: Task 12 (codegen for expressions)
  - **Blocked By**: Tasks 2 (AST), 6 (lexer)

  **References**:
  - **External**: https://github.com/zesterer/chumsky — combinator API, Pratt parsing support
  - **Pattern**: Rust's expression precedence table — reference for operator priority
  - **Algorithm**: Pratt parsing (top-down operator precedence)

  **Acceptance Criteria**:

  - [ ] `cargo test -p deox-parser expr` passes (target: 20+ expression tests)
  - [ ] Correct precedence: `2 + 3 * 4` parses as `2 + (3 * 4)`
  - [ ] Double suffix: `99.90d` → DoubleLiteral(99.90)
  - [ ] Function call: `foo(a, b)` → FuncCall("foo", [a, b])
  - [ ] Method call: `obj.method(x)` → MethodCall(obj, "method", [x])
  - [ ] Snapshot: 5 expression fixtures stable

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Operator precedence
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `2 + 3 * 4`
      2. Assert AST: BinaryOp(Add, IntLit(2), BinaryOp(Mul, IntLit(3), IntLit(4)))
      3. Parse: `(2 + 3) * 4`
      4. Assert AST: BinaryOp(Mul, BinaryOp(Add, IntLit(2), IntLit(3)), IntLit(4))
    Expected Result: Precedence correct, parentheses override
    Evidence: .sisyphus/evidence/task-7-precedence.txt

  Scenario: Double suffix parsing
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `99.90d`
      2. Assert AST: DoubleLiteral(99.90)
      3. Parse: `99.90` (no suffix)
      4. Assert AST: FloatLiteral(99.90)
    Expected Result: Suffix distinguishes Double from Float
    Evidence: .sisyphus/evidence/task-7-double-suffix.txt

  Scenario: Nested function calls
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `foo(bar(), baz(x, y))`
      2. Assert: FuncCall("foo", [FuncCall("bar", []), FuncCall("baz", [Ident("x"), Ident("y")])])
    Expected Result: Correct nesting
    Evidence: .sisyphus/evidence/task-7-nested-calls.txt
  ```

  **Commit**: YES
  - Message: `feat(parser): implement expression parser with Pratt precedence`

---

- [x] 8. Parser — Statements (let, if/else, func, return, for)

  **What to do** (TDD):
  - **RED**: Tests for: `let x = 42`, `let mut y = 0` (mutable), `let nome = "Deox"`
  - **RED**: Tests for: `if cond { ... } else { ... }` as expression (returns value)
  - **RED**: Tests for: `func nome(param: Tipo) -> Retorno { corpo }`
  - **RED**: Tests for: `return valor`, `return` (void)
  - **RED**: Tests for: `for x in colecao { ... }`, `for count > 0 { count -= 1 } (conditional loop — runs while condition is true, equivalent to while)`
  - **RED**: Tests for: assignment `x = novo_valor`, compound `x += 1`
  - **GREEN**: Implement statement parsers using chumsky
  - **GREEN**: Handle `if/else` as expression (in let binding: `let x = if cond { 1 } else { 2 }`)
  - **GREEN**: Parse function signatures with typed params and return type
  - **REFACTOR**: Extract statement parser into module

  **Must NOT do**:
  - **Must NOT do**: NO layout-sensitive parsing here (uses braces in tests, Task 9 adds indentation)
  - **Must NOT do**: NO async parsing (Task 31)
  - **Must NOT do**: NO match/case parsing (Task 27)

  **Recommended Agent Profile**:
  - **Category**: `deep` — Statement grammar design
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 9, both depend on Task 6/7)
  - **Parallel Group**: Wave 2 (with Task 9)
  - **Blocks**: Task 13 (codegen for statements)
  - **Blocked By**: Tasks 2 (AST), 6 (lexer), 7 (expressions, for if-as-expression)

  **References**:
  - **Pattern**: Rust statement grammar — let, if, fn, return
  - **External**: chumsky docs for sequence combinators

  **Acceptance Criteria**:

  - [ ] `cargo test -p deox-parser stmt` passes (target: 15+ statement tests)
  - [ ] `let x = 42` → LetDecl("x", IntLit(42), mutable=false)
  - [ ] `if cond { 1 } else { 2 }` → IfExpr(cond, [1], Some([2]))
  - [ ] Function decl with params: `func add(a: Int, b: Int) -> Int`
  - [ ] For loop: `for x in items { ... }`

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Let declaration parsing
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `let x = 42`
      2. Assert: LetDecl { name: "x", value: IntLit(42), mutable: false }
      3. Parse: `let mut count = 0`
      4. Assert: LetDecl { name: "count", value: IntLit(0), mutable: true }
    Expected Result: Correct mutable flag
    Evidence: .sisyphus/evidence/task-8-let-decl.txt

  Scenario: If/else as expression
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `let status = if idade >= 18 { "maior" } else { "menor" }`
      2. Assert: LetDecl("status", IfExpr(BinaryOp(GreaterEq, Ident("idade"), IntLit(18)), [StringLit("maior")], Some([StringLit("menor")])))
    Expected Result: if/else returns value, nested in let
    Evidence: .sisyphus/evidence/task-8-if-expr.txt

  Scenario: Function declaration
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `func add(a: Int, b: Int) -> Int:\n    return a + b`
      2. Assert: FuncDecl { name: "add", params: [("a", Int), ("b", Int)], return_type: Int, body: [Return(BinaryOp(Add, Ident("a"), Ident("b")))] }
    Expected Result: Full signature parsed
    Evidence: .sisyphus/evidence/task-8-func-decl.txt
  ```

  **Commit**: YES
  - Message: `feat(parser): implement statement parser (let, if/else, func, return, for)`

---

- [x] 9. Parser — Layout-Sensitive Blocks (Offside Rule)

  **What to do** (TDD):
  - **RED**: Tests for: function body via indentation (no braces): `func foo():\n    stmt1\n    stmt2`
  - **RED**: Tests for: nested indentation: `if x:\n    if y:\n        z()`
  - **RED**: Tests for: if/else without braces: `if cond:\n    a()\nelse:\n    b()`
  - **RED**: Tests for: dedent returns to outer scope: `if x:\n    a()\nb()` (b is outside if)
  - **RED**: Tests for: mixing layout and braces (braces for struct init, lambdas, interpolation)
  - **RED**: Property test: parse(unparse(ast)) == ast for layout-sensitive code
  - **GREEN**: Implement layout-sensitive block parser consuming Indent/Dedent tokens from lexer
  - **GREEN**: Handle `else` binding to nearest `if` (dangling else)
  - **GREEN**: Allow braces `{ ... }` to override indentation (explicit blocks)
  - **REFACTOR**: Extract indentation context tracking

  **Must NOT do**:
  - **Must NOT do**: NO support for tabs (error, not silent acceptance)
  - **Must NOT do**: NO mixed tabs+spaces (error)

  **Recommended Agent Profile**:
  - **Category**: `deep` — Layout-sensitive parsing is notoriously hard
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 8)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 12, 13 (codegen)
  - **Blocked By**: Tasks 2 (AST), 6 (lexer with Indent/Dedent), 7, 8

  **References**:
  - **Pattern**: Haskell offside rule — Landin 1966
  - **Pattern**: F# lightweight syntax — indentation-based blocks
  - **Pattern**: Python indentation grammar in ruff/enderpy
  - **External**: chumsky README mentions "Python-style semantic indentation"

  **Acceptance Criteria**:

  - [ ] `cargo test -p deox-parser layout` passes (target: 15+ layout tests)
  - [ ] Function body via indentation parses correctly
  - [ ] Nested indentation (2+ levels) works
  - [ ] `else` binds to nearest `if`
  - [ ] Dedent correctly ends blocks
  - [ ] Braces override indentation (struct init, lambdas)

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Function body via indentation
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `func foo():\n    print("a")\n    print("b")`
      2. Assert: FuncDecl with body containing 2 statements
      3. Parse: `func bar():\n    print("a")\nprint("fora")`
      4. Assert: "fora" is OUTSIDE func body (dedent detected)
    Expected Result: Indentation correctly delimits blocks
    Evidence: .sisyphus/evidence/task-9-layout-func.txt

  Scenario: Dangling else
    Tool: Bash (cargo test)
    Steps:
      1. Parse: `if a:\n    if b:\n        c()\n    else:\n        d()`
      2. Assert: else binds to inner if (b), not outer (a)
    Expected Result: else binds to nearest if
    Evidence: .sisyphus/evidence/task-9-dangling-else.txt
  ```

  **Commit**: YES
  - Message: `feat(parser): implement layout-sensitive parsing (offside rule)`

---

### Wave 3: Type System + Codegen

> **ENHANCEMENT**: Wave 3 now includes collection literals and range syntax (from best practices research)

- [x] 10. deox-types Crate — Type Representation + Inference + Flow-Sensitive Typing

  **What to do** (TDD):
  - **RED**: Tests for type inference from literals (`42` → Int, `3.14` → Float, `99.9d` → Double, `true` → Bool, `"hi"` → String)
  - **RED**: Tests for inference from binary ops (`Int + Int` → Int, `Int + Float` → Float promotion)
  - **RED**: Tests for function param/return inference
  - **GREEN**: Define `Type` enum with v0.1 primitive types (Int, Float, Double, Bool, String, Byte)
  - **GREEN**: Implement local type inference (bidirectional checking)
  - **GREEN**: Implement numeric promotion rules (Int → Float → Double widening)
  - **GREEN**: Type error reporting (mismatch, undefined variable) with span

  **Must NOT do**: NO generics inference (Task 23), NO user-defined types (Tasks 26-27), NO borrow checking emulation (Task 33)

  **Recommended Agent Profile**:
  - **Category**: `deep` — Type inference algorithm design
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 11, 12, 13)
  - **Parallel Group**: Wave 3
  - **Blocks**: Tasks 12, 13 (codegen needs typed AST)
  - **Blocked By**: Tasks 2 (AST), 9 (complete parser)

  **Acceptance Criteria**:
  - [ ] `cargo test -p deox-types` passes (target: 20+ tests)
  - [ ] `42` infers as Int, `3.14` as Float, `99.9d` as Double
  - [ ] `Int + Float` promotes to Float
  - [ ] Type errors reported with span info

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Literal type inference
    Tool: Bash (cargo test)
    Steps:
      1. Infer type of `42` → assert Int
      2. Infer type of `3.14` → assert Float
      3. Infer type of `99.9d` → assert Double
    Expected Result: Correct inference for all literals
    Evidence: .sisyphus/evidence/task-10-literal-inference.txt

  Scenario: Type error reporting
    Tool: Bash (cargo test)
    Steps:
      1. Check: `let x: Int = "hello"`
      2. Assert TypeError with message "expected Int, found String"
    Expected Result: Clear type error with location
    Evidence: .sisyphus/evidence/task-10-type-error.txt
  ```

  **Commit**: YES — Message: `feat(types): implement type representation and local inference`

---

- [x] 11. deox-codegen-rust — syn/quote Infrastructure

  **What to do**:
  - Set up codegen crate with `syn`, `quote`, `prettyplease` dependencies
  - Define `CodegenContext` struct: tracks generated names, module structure, source map
  - Define `RustCodegen` that converts AST → `syn::File` (NOT raw strings!)
  - Implement `syn::File` → formatted Rust source via `prettyplease`
  - Snapshot testing infrastructure: generated `.rs` → insta snapshot

  **Must NOT do**: NO raw string formatting (MUST use syn/quote), NO optimization passes, NO Arc/clone insertion yet

  **Recommended Agent Profile**:
  - **Category**: `deep` — Architectural setup
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 10, 12, 13)
  - **Blocks**: Tasks 12, 13
  - **Blocked By**: Tasks 2 (AST), 4 (error/source maps)

  **Acceptance Criteria**:
  - [ ] Codegen produces `syn::File`, not raw strings
  - [ ] Output passes `rustfmt --check` via prettyplease

  **QA Scenarios**:
  ```
  Scenario: syn-based codegen produces valid Rust
    Tool: Bash (cargo test)
    Steps:
      1. Codegen empty function `func empty():`
      2. Assert output is syn::File, pretty-prints as `fn empty() {}`
      3. Run through `rustfmt --check` → pass
    Expected Result: Generated Rust is valid and formatted
    Evidence: .sisyphus/evidence/task-11-codegen-infra.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): set up syn/quote/prettyplease codegen infrastructure`

---

- [x] 12. Codegen — Literals, Let Bindings, Arithmetic → Rust

  **What to do** (TDD):
  - **RED**: Snapshot tests for: `let x = 42` → `let x: i64 = 42;`, `let pi = 3.14` → `let pi: f32 = 3.14;`, `let z = 99.9d` → Decimal macro
  - **RED**: Snapshot for arithmetic with precedence: `a + b * c`
  - **GREEN**: Implement codegen for all v0.1 literal types
  - **GREEN**: Implement let binding codegen with inferred type annotations
  - **GREEN**: Implement binary/unary operation codegen

  **Must NOT do**: NO clone insertion yet, NO Arc wrapping

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **Parallelization**: Parallel with Task 13 | **Blocks**: Task 14 | **Blocked By**: Tasks 10, 11, 7

  **Acceptance Criteria**:
  - [ ] `cargo test -p deox-codegen-rust literals` passes
  - [ ] All 6 v0.1 types codegen correctly (Int→i64, Float→f32, Double→f64, Bool→bool, String→String, Byte→u8)
  - [ ] Generated code compiles via `cargo check`

  **QA Scenarios**:
  ```
  Scenario: Literal codegen snapshots
    Tool: Bash (cargo insta)
    Steps:
      1. Codegen `let x = 42` → snapshot: `let x: i64 = 42;`
      2. Codegen `let pi = 3.14` → snapshot: `let pi: f32 = 3.14;`
      3. Codegen `let z = 99.90d` → snapshot with rust_decimal_macros::dec!
    Expected Result: All literals produce idiomatic Rust
    Evidence: .sisyphus/evidence/task-12-literal-codegen.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): implement literal and arithmetic codegen`

---

- [x] 13. Codegen — If/Else Expression, Function Declarations → Rust

  **What to do** (TDD):
  - **RED**: Snapshot for `if cond { a } else { b }` → Rust if-expression
  - **RED**: Snapshot for `func add(a: Int, b: Int) -> Int: return a + b` → Rust fn
  - **RED**: Snapshot for `func main(): print("Olá")` → Rust main with println!
  - **RED**: Snapshot for `for x in items { ... }` and `for count > 0 { count -= 1 } (conditional loop — runs while condition is true, equivalent to while)`
  - **GREEN**: Implement if/else codegen as Rust expression
  - **GREEN**: Implement function codegen with proper signatures
  - **GREEN**: Map `print(x)` → `println!("{}", x)`
  - **GREEN**: Implement for loop (iterator + conditional modes) codegen

  **Must NOT do**: NO closure codegen (Task 34), NO match codegen (Task 27)

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **Parallelization**: Parallel with Task 12 | **Blocks**: Task 14 | **Blocked By**: Tasks 8, 9, 11

  **Acceptance Criteria**:
  - [ ] `cargo test -p deox-codegen-rust stmts` passes
  - [ ] if/else generates as Rust expression
  - [ ] `print("Olá")` → `println!("{}", "Olá")`
  - [ ] Generated main function compiles and runs

  **QA Scenarios**:
  ```
  Scenario: Full program codegen
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `func main():\n    let nome = "Deox"\n    print("Olá, {nome}!")`
      2. Write to temp file, compile with rustc, run
      3. Assert stdout: "Olá, Deox!"
    Expected Result: End-to-end codegen produces runnable program
    Evidence: .sisyphus/evidence/task-13-full-program.txt
  ```

  **Commit**: YES — Message: `feat(codegen-rust): implement if/else, function, and loop codegen`

---

### Wave 4: CLI + Integration

- [x] 14. deox-cli — `deox build` Command

  **What to do** (TDD):
  - **RED**: Test `deox build examples/ola.deox` → creates executable
  - **RED**: Test `deox build nonexistent.deox` → error with clear message
  - **GREEN**: Implement CLI with `clap` derive
  - **GREEN**: Pipeline: read .deox → lex → parse → type check → codegen → write .rs → invoke rustc/cargo
  - **GREEN**: Capture rustc errors, map back to .deox lines via source map

  **Must NOT do**: NO `deox run` (Task 15), NO optimization flags, NO incremental compilation

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **Parallelization**: Parallel with Tasks 15, 16 | **Blocks**: Task 17 | **Blocked By**: Tasks 12, 13

  **Acceptance Criteria**:
  - [ ] `deox build examples/ola.deox` produces executable
  - [ ] Running executable prints correct output
  - [ ] Invalid .deox file → clear error, no panic

  **QA Scenarios**:
  ```
  Scenario: Build hello world
    Tool: Bash (deox CLI)
    Preconditions: examples/ola.deox exists
    Steps:
      1. Run `deox build examples/ola.deox`
      2. Assert exit code 0
      3. Assert executable exists
      4. Run executable
      5. Assert stdout contains "Olá, Deox!"
    Expected Result: Full build pipeline works end-to-end
    Evidence: .sisyphus/evidence/task-14-build-hello.txt

  Scenario: Build with syntax error
    Tool: Bash (deox CLI)
    Steps:
      1. Create invalid file: `func main():\n    let x =`
      2. Run `deox build invalid.deox`
      3. Assert exit code non-zero
      4. Assert stderr references .deox line number
    Expected Result: Graceful error with source location
    Evidence: .sisyphus/evidence/task-14-build-error.txt
  ```

  **Commit**: YES — Message: `feat(cli): implement deox build command with full pipeline`

---

- [x] 15. deox-cli — `deox run` Command

  **What to do** (TDD):
  - **RED**: Test `deox run examples/ola.deox` → prints output, cleans up
  - **GREEN**: Implement `deox run` = build to temp + execute + cleanup
  - **GREEN**: Stream stdout/stderr in real-time

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **Parallelization**: Parallel with Tasks 14, 16 | **Blocked By**: Task 14

  **Acceptance Criteria**:
  - [ ] `deox run examples/ola.deox` prints "Olá, Deox!"
  - [ ] No executable left behind (temp cleanup)

  **QA Scenarios**:
  ```
  Scenario: Run hello world
    Tool: Bash (deox CLI)
    Steps:
      1. Run `deox run examples/ola.deox`
      2. Assert stdout: "Olá, Deox!"
      3. Assert no temp files left in CWD
    Expected Result: Program runs and cleans up
    Evidence: .sisyphus/evidence/task-15-run-hello.txt
  ```

  **Commit**: YES — Message: `feat(cli): implement deox run command`

---

- [x] 16. Source Map Infrastructure (Deox ↔ Rust Line Mapping)

  **What to do** (TDD):
  - **RED**: Test runtime error at Rust line 15 → maps to Deox line 8
  - **GREEN**: During codegen, build mapping: Deox span → Rust line/col
  - **GREEN**: Intercept rustc/panics, translate locations
  - **GREEN**: Backtrace filtering (hide Rust stdlib frames)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: `[]`

  **Parallelization**: Parallel with Tasks 14, 15 | **Blocked By**: Tasks 4, 11

  **Acceptance Criteria**:
  - [ ] Rust panic location → mapped to correct Deox line
  - [ ] Error shows Deox filename and line, not temp .rs

  **QA Scenarios**:
  ```
  Scenario: Error maps to Deox source
    Tool: Bash (deox CLI)
    Steps:
      1. Create program with index out of bounds
      2. Run `deox run prog.deox`
      3. Assert error references line of prog.deox (NOT temp .rs)
    Expected Result: Error points to user's .deox file
    Evidence: .sisyphus/evidence/task-16-source-map.txt
  ```

  **Commit**: YES — Message: `feat(error): implement source maps for Deox-to-Rust line mapping`

---

- [x] 17. v0.1 Milestone — "Olá Deox" Example + Integration Tests

  **What to do**:
  - Create `examples/ola.deox`, `examples/fibonacci.deox`, `examples/calculadora.deox`
  - Write integration tests: run each example, assert output
  - Update README.md with installation + quick start
  - Tag release: `v0.1.0`
  - Verify: `cargo test --workspace` all pass, `cargo clippy` clean

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: `[]`

  **Parallelization**: Sequential (final v0.1 task) | **Blocks**: All v0.5 | **Blocked By**: Tasks 14, 15, 16

  **Acceptance Criteria**:
  - [ ] `deox run examples/ola.deox` → "Olá, Deox!"
  - [ ] `deox run examples/fibonacci.deox 10` → "55"
  - [ ] `cargo test --workspace` → 100% pass
  - [ ] `cargo clippy --workspace -- -D warnings` → clean
  - [ ] Git tag `v0.1.0` created

  **QA Scenarios**:
  ```
  Scenario: v0.1 milestone — all examples work
    Tool: Bash (deox CLI)
    Steps:
      1. `deox run examples/ola.deox` → assert "Olá, Deox!"
      2. `deox run examples/fibonacci.deox 10` → assert "55"
      3. `cargo test --workspace` → assert 0 failures
      4. `cargo clippy --workspace -- -D warnings` → exit 0
    Expected Result: All v0.1 functionality working, clean codebase
    Evidence: .sisyphus/evidence/task-17-v01-milestone.txt
  ```

  **Commit**: YES — Message: `release: v0.1.0 "Olá Deox" — MVP transpiler working`

---


---

### Additional Wave 1 Task: T88 — Deox IR Design (Dataflow Graph)

- [x] T88. Deox IR Design — Dataflow Graph for Algorithm/Schedule Separation

  **What to do** (TDD):
  - **RED**: Write tests for IR node types: ComputeNode (pure computation),IONode (I/O boundary — async), TransferNode (CPU↔GPU data movement), ScheduleNode (dispatch decision)
  - **RED**: Write tests for IR graph construction: AST → IR lowering (basic expressions, let bindings, function calls)
  - **RED**: Write test: IR graph correctly identifies data dependencies (node A depends on node B)
  - **GREEN**: Define `IrGraph` struct with nodes and edges (dependency graph)
  - **GREEN**: Implement `AstLowerer` that converts typed AST → IR graph
  - **GREEN**: Implement dependency analysis: build edges between nodes that share data
  - **GREEN**: Mark I/O nodes (function calls to `async` functions) as suspension points
  - **REFACTOR**: Extract IR types into `deox-ast/src/ir.rs` module

  **Must NOT do**:
  - NO scheduling logic (that is T40 in v1.0 — this task only builds the graph)
  - NO GPU-specific nodes (that is T44 in v1.0)
  - NO optimization passes (IR is a faithful representation of the AST, not optimized)

  **Recommended Agent Profile**:
  - **Category**: `ultrabrain` — Fundamental architectural design
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T1-T5 in Wave 1 — IR types are independent of lexer/parser)
  - **Blocks**: T12, T13 (codegen goes through IR: AST → IR → Rust)
  - **Blocked By**: T2 (AST types must exist to lower from)

  **References**:
  - **Pattern**: Halide IR (algorithm/schedule separation) — represent WHAT to compute, not HOW
  - **Pattern**: MLIR (modular IR with dialects) — different IR levels for different optimization stages
  - **External**: https://halide-lang.org/docs/tutorial.html — Halide algorithm/schedule concepts
  - **Design**: IR nodes track data dependencies → enables automatic parallelization (v1.0) and CPU/GPU scheduling (v1.0)

  **Acceptance Criteria**:
  - [ ] `cargo test -p deox-ast ir` passes (target: 10+ tests)
  - [ ] IR graph can represent: let bindings, binary ops, function calls, if/else
  - [ ] Dependency edges correctly identify: A uses output of B → edge from B to A
  - [ ] I/O nodes (async function calls) are marked as suspension points
  - [ ] `format!("{}", ir_graph)` produces readable output
  - [ ] Snapshot: 3 IR graph examples produce stable snapshots

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: IR graph represents data dependencies
    Tool: Bash (cargo test)
    Steps:
      1. Lower AST for: `let a = 1; let b = a + 2; let c = b * 3`
      2. Assert IR has 3 compute nodes
      3. Assert dependency edges: b depends on a, c depends on b
      4. Assert NO edge between a and c (transitive only)
    Expected Result: Correct dependency graph with 3 nodes, 2 edges
    Evidence: .sisyphus/evidence/task-88-ir-dependencies.txt

  Scenario: I/O nodes marked as suspension points
    Tool: Bash (cargo test)
    Steps:
      1. Lower AST for code calling an async function: `let data = http_get(url)`
      2. Assert the http_get call node is marked as IONode (suspension point)
      3. Assert dependent nodes are downstream of the IONode
    Expected Result: I/O calls correctly identified in IR
    Evidence: .sisyphus/evidence/task-88-ir-io-nodes.txt
  ```

  **Commit**: YES
  - Message: `feat(ast): implement Deox IR dataflow graph with dependency tracking`

---

### Additional Wave 3a Task: T33a — Codegen Move-by-Default Semantics

- [x] T33a. Codegen Move Semantics — Every Codegen Task Assumes Moves

  **What to do** (TDD):
  - **RED**: Write test: `let x = 42; let y = x` generates Rust `let x = 42; let y = x;` (move, not copy)
  - **RED**: Write test: `let s = "hello"; let s2 = s` generates Rust move (String is moved, not copied)
  - **RED**: Write test: function parameters are moved (not borrowed): `func process(data: Vector<Int>)` → `fn process(data: Vec<i64>)` (no `&`)
  - **RED**: Write test: using a variable after move generates `.clone()` automatically (basic case)
  - **GREEN**: Implement codegen rule: all bindings are moved by default (Rust move semantics)
  - **GREEN**: Implement basic clone detection: if a variable is used after being moved, insert `.clone()` at the move site
  - **GREEN**: Function parameters: always move (never borrow) — generated Rust has no `&` in function signatures
  - **REFACTOR**: Extract clone detection into reusable analysis pass

  **Must NOT do**:
  - NO Arc<T> wrapping (that is T33b in v1.0)
  - NO CoW (Arc::make_mut) logic (that is T33b in v1.0)
  - NO lifetime annotations in generated Rust (Deox never exposes lifetimes)
  - NO borrow checker errors leaking to user — if Rust borrow checker complains, Deox codegen is wrong

  **Recommended Agent Profile**:
  - **Category**: `deep` — Core codegen pattern, affects all subsequent codegen
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T10, T11 in Wave 3a)
  - **Blocks**: T12, T13 (codegen tasks must use move-by-default)
  - **Blocked By**: T2 (AST), T88 (IR for tracking data flow)

  **References**:
  - **Pattern**: Rust ownership model — moves by default, explicit clone when needed
  - **Design Decision**: Deox users never see `&`, `&mut`, or lifetimes. Transpiler generates Rust with direct ownership.
  - **Future**: T33b (v1.0) will add Arc<T> for cross-thread sharing and CoW for shared mutation

  **Acceptance Criteria**:
  - [ ] `cargo test -p deox-codegen-rust move_semantics` passes (target: 8+ tests)
  - [ ] Generated Rust has NO `&` or `&mut` in function signatures
  - [ ] Generated Rust has NO lifetime annotations (`'a`)
  - [ ] Variables used after move get `.clone()` inserted at move site
  - [ ] Snapshot: `let s = "hi"; use(s); use(s)` generates `let s = String::from("hi"); use(s.clone()); use(s);`
  - [ ] Generated Rust compiles without borrow checker errors

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Move semantics generate valid Rust
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let s = "hello"; let s2 = s`
      2. Assert output: `let s = String::from("hello"); let s2 = s;` (move, no clone)
      3. Compile generated Rust → success (no borrow checker errors)
    Expected Result: Move semantics produce compilable Rust
    Evidence: .sisyphus/evidence/task-33a-move-semantics.txt

  Scenario: Clone inserted when variable used after move
    Tool: Bash (cargo test)
    Steps:
      1. Codegen: `let v = [1, 2, 3]; print(v.len()); print(v.len())`
      2. Assert: first use is move, second use has `.clone()` OR uses by reference internally
      3. Compile generated Rust → success
    Expected Result: No "use of moved value" errors
    Evidence: .sisyphus/evidence/task-33a-clone-insertion.txt
  ```

  **Commit**: YES
  - Message: `feat(codegen-rust): implement move-by-default semantics with auto-clone`

---

### Additional Wave 4 Task: T110 — Project Scaffolding (deox new / deox init)

- [x] T110. `deox new` + `deox init` — Project Scaffolding with Standard Structure

  **What to do** (TDD):
  - **RED**: Test: `deox new my_app` creates directory with `deox.toml`, `src/main.deox`, `tests/`, `.gitignore`
  - **RED**: Test: `deox new my_app` generates `main.deox` with `func main(): print("Hello, Deox!")`
  - **RED**: Test: `deox init` scaffolds in current directory (not new subdirectory)
  - **RED**: Test: generated `deox.toml` has `[package]` section with name, version, edition
  - **RED**: Test: generated `.gitignore` includes `target/` and `deox.lock`
  - **GREEN**: Implement `deox new <name>` — creates new directory, scaffolds project
  - **GREEN**: Implement `deox init` — scaffolds in current directory
  - **GREEN**: Template files: `deox.toml`, `main.deox`, `.gitignore`, `README.md`
  - **GREEN**: Validate project name (valid identifier, not a keyword)
  - **REFACTOR**: Extract template rendering into reusable module

  **Must NOT do**:
  - NO `--lib`, `--server`, `--gpu` templates (those are T112 in v0.5)
  - NO workspace support (that is T111 in v0.5)
  - NO dependency management (`deox add` — future)
  - NO editions system (that is T113 in v1.0)

  **Recommended Agent Profile**:
  - **Category**: `deep` — CLI feature with file system interaction
  - **Skills**: `[]`

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T14, T15, T16 in Wave 4)
  - **Blocks**: None directly (nice-to-have for v0.1 milestone)
  - **Blocked By**: T1 (workspace must exist — CLI is in the workspace)

  **References**:
  - **Pattern**: `cargo new` — Rust project scaffolding (gold standard)
  - **Pattern**: `go mod init` — Go module initialization
  - **Spec**: `.sisyphus/plans/deox-project-structure.md` — canonical project layout
  - **Convention**: `.sisyphus/plans/deox-conventions.md` — naming, formatting rules

  **Acceptance Criteria**:
  - [ ] `deox new my_app` creates correct directory structure
  - [ ] Generated `main.deox` contains valid Deox code
  - [ ] Generated `deox.toml` has valid TOML with required fields
  - [ ] Generated `.gitignore` includes `target/` and `deox.lock`
  - [ ] `deox init` works in current directory
  - [ ] Invalid project names (keywords, spaces) produce clear error

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: deox new creates valid project
    Tool: Bash (deox CLI)
    Steps:
      1. Run `deox new test_app` in temp directory
      2. Assert directory `test_app/` exists
      3. Assert `test_app/deox.toml` exists with `[package]` section
      4. Assert `test_app/src/main.deox` exists with `func main()` entry point
      5. Assert `test_app/.gitignore` contains `target/`
      6. Run `deox run test_app/src/main.deox` → assert "Hello, Deox!" output
    Expected Result: Fully scaffolded, runnable project
    Evidence: .sisyphus/evidence/task-110-deox-new.txt

  Scenario: Invalid project name rejected
    Tool: Bash (deox CLI)
    Steps:
      1. Run `deox new "func"` (keyword as name)
      2. Assert exit code non-zero
      3. Assert error message: "invalid project name: 'func' is a reserved keyword"
    Expected Result: Clear error for invalid names
    Evidence: .sisyphus/evidence/task-110-invalid-name.txt
  ```

  **Commit**: YES
  - Message: `feat(cli): implement deox new and deox init project scaffolding`
