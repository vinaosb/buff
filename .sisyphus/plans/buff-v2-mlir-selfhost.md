<!-- SUPERSEDED: This plan is SUPERSEDED by `.sisyphus/plans/buff-launch-readiness.md` (T110 decision record).
     The MLIR migration, custom memory model, and rustc-dropping goals have been demoted to optional V3.
     See `.sisyphus/decisions/buff-direction-speed-moat-selfhost.md` for the governing direction decisions. -->

# Buff V2 — MLIR Backend + Self-Hosting (Rust+WGSL → MLIR)

## TL;DR

> **Quick Summary**: Replace Buff's `→ Rust source → rustc/LLVM` backend (and the separate WGSL/wgpu GPU path) with a native **MLIR** backend, giving Buff its own memory-safety engine (a Perceus-style RC + borrow-inference hybrid, **no GC, zero user-visible lifetimes**) and unifying CPU/GPU/FPGA codegen. Do it **MLIR-first (Route α)**: build the MLIR backend while the compiler stays in Rust so **rustc keeps acting as a live soundness oracle**; then self-host the compiler in Buff; then (V3) drop Rust and write replacement libs.
>
> **Deliverables**:
> - A new `buff-lang-codegen-mlir` crate + a `buff` MLIR dialect + a **Buff MIR** analysis IR.
> - Buff's own memory model (borrow-inference + Perceus RC + reuse/FBIP + atomic-ARC + `weak<T>` cycles), replacing every safety guarantee rustc gave for free.
> - A **dual-backend** compiler (`--backend={rust,mlir,wgsl}`) with a **differential + conformance + bootstrap** test harness.
> - Native, **wasm**, and **GPU (MLIR gpu dialects)** targets; FPGA (CIRCT) as research.
> - A self-hosted Buff compiler (Stage 2 == Stage 3 byte-identical), then V3 rustc-removal + replacement libs.
>
> **Estimated Effort**: XL — ~10–12 engineer-quarters (~3 yr @1 eng / ~1.5 yr @2 eng), spanning **V2.0 → V2.x → V3**.
> **Parallel Execution**: YES — 6 waves; heavy parallelism within Waves 1–3.
> **Critical Path**: W0.1 (generics) → W1.1 (MLIR crate) → W1.3 (Buff MIR) → W2.1 (last-use) → W2.3 (Perceus RC) → W2.7 (arc-lowering) → W3.2 (GPU/SPIR-V, long pole) → W4.x (self-host) → W5.1 (drop rustc).

---

## Context

### Original Request
User: elaborate the **V2** plan to (1) "compile our code using buff directly instead of rust" and (2) "convert it to using MLIR", while "not losing the compiler help we get by using Rust as our transpiler — to achieve this we will need to rewrite and add a lot of new code based on Rust itself." Deep research required to validate every step.

### Interview Summary — confirmed decisions
- **Ordering (Route α, MLIR-first)** — user reconsidered and landed here ("MLIR conversions as Phase A, self-hosting as Phase B... we should be able to catch more errors this way"). Rationale (validated): the hardest new code is Buff's own borrow-checker+ARC; the best oracle for it is **rustc itself**, so build the memory model + MLIR path **while the compiler stays in Rust** and differential-test the MLIR path against the rustc-checked Rust path.
- **Memory model**: borrow-checker + ARC hybrid, **NO GC**, **zero user-visible lifetimes/borrows**, maximum performance. (User explicitly open to a more optimized model — research adopted Perceus-style RC-with-reuse.)
- **MLIR non-negotiable** (CPU/GPU/FPGA unification). Fold WGSL/wgpu into MLIR GPU dialects. Keep a **wasm** target (own lib OK). **Strict superset** — write our own libraries where needed.
- **Versioning**: **V2.0** dual-backend (Rust+MLIR coexist) → **V2.x** grow MLIR to 100% coverage → **V3** MLIR-only (drop Rust + write replacement libs). One very large plan.
- **Prerequisite language features → V1.X** (out of V2). Buff deemed "ready to self-host" — but see Oracle finding below.

### Research + Oracle findings (see `.sisyphus/drafts/buff-v2-blueprint.md` for full detail)
- **Buff has ZERO real borrow/lifetime/aliasing analysis today** — `OwnershipFacts` is a `BTreeSet<String>` for codegen sweetness, not soundness. The memory model is the dominant risk (greenfield), not MLIR.
- **KEY IR INSIGHT**: introduce a **Buff MIR** (CFG-of-basic-blocks + ghost instructions) — all of Hylo/Polonius/Perceus need this; analyzing on the AST will fail.
- **rustc oracle ceiling ≈ 70%**: MLIR-only features (SPIR-V GPU, NVVM, FPGA, FP8) have no rustc oracle → two verification regimes (differential-vs-rustc for the overlap subset; conformance/golden/property for MLIR-only).
- **⭐ V1.X hard blocker**: **generics** (functions/structs/traits + bounds + `where`) are **absent from the AST** and gate all of self-hosting; also missing: end-to-end multi-file codegen, `extern crate → Cargo.toml` assembly, FFI decision, BigInt literals, allocator convention. → a new **`v1.25 "Language for Self-Hosting"`** release must precede V2's self-host phase.
- **melior uses `bindgen` → cc-rs**, conflicting with the repo's "no cc-rs" rule → an explicit, documented **rule carve-out** is required (mirrors the existing WGSL-`format!()` and zeromq pure-Rust exceptions).
- **GPU via SPIR-V is the riskiest single piece** (live upstream bug llvm-project#155898) → keep `codegen-wgsl` + `runtime` (wgpu) as the **production** GPU path through V2.x.

---

## Work Objectives

### Core Objective
Give Buff a self-owned MLIR compilation backend and memory-safety engine that preserves 100% of today's behavior and safety, then self-host the compiler in Buff — without ever exposing lifetimes/borrows to users and without a GC.

### Concrete Deliverables
- `buff-lang-codegen-mlir` crate; `buff` MLIR dialect (~10–15 ops); **Buff MIR** analysis IR + pass pipeline.
- Memory model passes: last-use, definite-init, exclusivity, Perceus RC insertion, drop specialization, reuse/FBIP, thread-share tracking, `--ownership-arc-lowering`.
- `--backend={rust,mlir,wgsl}` CLI; native + wasm + MLIR-GPU targets.
- Differential + conformance + bootstrap test harness; determinism CI guard.
- Self-hosted compiler (Stage 2 == Stage 3); V3 rustc-removal + replacement libs.

### Definition of Done
- [ ] Every current `.buff` and `.buffhtml` example compiles+runs **identically** via `--backend=mlir` (semantic equivalence to the Rust backend).
- [ ] MLIR backend is memory-safe with **rustc removed from the pipeline**; differential pass rate ≥99% during transition.
- [ ] Compiler self-bootstraps via MLIR: `Stage 2 == Stage 3` byte-for-byte.
- [ ] No user-visible `&`/lifetimes anywhere; no GC.

### Must Have
- Dual-backend + differential testing throughout the transition (rustc as oracle until V3).
- Buff MIR as the analysis substrate for the memory model.
- Production GPU continuity (WGSL/wgpu stays until MLIR-GPU proven).

### Must NOT Have (Guardrails)
- ❌ **No GC** (including no tracing fallback for cycles — use `weak<T>` + arena-indices).
- ❌ **No user-visible lifetimes/borrows** (`&`, `'a`) — ever.
- ❌ **No user-defined `Drop`** in V2 (ARC is the sole destructor; keeps the semantic oracle valid).
- ❌ No ripping out the Rust backend before Wave 5 (it is the oracle).
- ❌ No reimplementing mature stdlib crates (regex/chrono/yaml/websocket) from scratch — **FFI-reuse** via Rust-shim.
- ❌ No hand-written MLIR raw strings for production (build via melior; textual `--emit-mlir` is debug-only).
- ❌ No renumbering/reusing ErrorCodes (E10xx/E11xx/E12xx/E13xx are stable forever; new MLIR codes take a new range e.g. E14xx).
- ❌ No starting the self-host wave before `v1.25` prerequisites ship + stabilize.

---

## Verification Strategy (MANDATORY)

> **ZERO human intervention** — all verification is agent/CI-executed.

### Test Decision
- **Infrastructure exists**: YES (insta snapshots + proptest, 3-OS CI, `cargo test --workspace`).
- **Approach**: **TDD** (RED→GREEN→REFACTOR) for every task, extended with the dual-backend harness below.
- New MLIR snapshots are **separate** from the Rust-source snapshots (not portable).

### Two verification regimes (from the oracle-ceiling finding)
- **Rust-overlap subset** → **differential**: compile each golden `.buff` via `--backend=rust` and `--backend=mlir`, run both, diff **exit code + stdout + stderr** (semantic, NOT byte-identical binaries). rustc is the memory-safety oracle here.
- **MLIR-only subset** (GPU/FPGA/FP8) → **conformance**: golden outputs computed once via the WGSL/wgpu reference; MLIR path must match. Plus property tests + fuzzing on the lowering passes.

### Harness layers
Snapshot (per dialect, deterministic `.mlir`) · Differential (overlap) · Property (proptest random ASTs, both backends) · Fuzz (cargo-fuzz on lowering) · Conformance (MLIR-only) · Bootstrap (Stage 2 == Stage 3 byte check) · **Determinism guard** (compile same AST twice → byte-diff; MLIR iteration order must stay deterministic).

### Evidence
Per task: evidence to `.sisyphus/evidence/task-{id}-{slug}.{ext}` (CLI transcripts via tmux, `.mlir`/binary diffs, test output).

---

## Execution Strategy

### Waves

```
Wave 0 — V1.X PREREQUISITE GATE (ships as v1.25, BEFORE V2.0):
  W0.1 generics (funcs/structs/traits+bounds+where)      [L, critical path]
  W0.2 multi-file codegen end-to-end + Cargo assembly     [M]  ∥ W0.1
  W0.3 BigInt literals + allocator convention + FFI decision (Rust-shim) [S+M] ∥
  GATE: a small compiler piece (JSON parser) written in Buff compiles via Rust backend.

Wave 1 — V2.0 MLIR scaffolding (dual-backend begins):
  W1.1 buff-lang-codegen-mlir crate + melior/mlir-sys + vendored-LLVM 3-OS CI  [L, blocks all]
  W1.2 minimal `buff` dialect (ODS) + MLIR SpanMap        [M]
  W1.3 Buff MIR (CFG + ghost instructions)                [L, blocks Wave 2]
  W1.4 buff → func/arith/scf/cf/memref lowering           [M]  ∥ W1.3
  W1.5 convert-to-llvm + llc glue + --backend flag        [M]
  W1.6 snapshot + differential harness skeleton           [S]  (ASAP)
  GATE: `buff build --backend=mlir examples/ola.buff` runs, diff-tested vs --backend=rust.

Wave 2 — Memory model (the big lift):
  W2.1 last-use + definite-init (Hylo-style)              [L]
  W2.2 exclusivity checker                                [M]
  W2.3 Perceus RC insertion + drop specialization         [L]  (after W2.1)
  W2.4 reuse analysis / FBIP                              [M]  (after W2.3)
  W2.5 thread-share tracking + atomic RC                  [M]  (after W2.3)
  W2.6 `weak<T>` + cycle lint + cycle-policy              [S]  (decision)
  W2.7 --ownership-arc-lowering + buff.arc<T>             [L]  (after W2.3)
  GATE: memory-safe binaries WITHOUT rustc in pipeline; differential ≥99%.

Wave 3 — Coverage + targets (parallel):
  W3.1 wasm target (wasm32-wasi)                          [S]
  W3.2 GPU SPIR-V + wgpu-host integration                 [XL, highest risk; keep WGSL fallback]
  W3.3 Send/Sync replacement (Perceus thread model)       [M]  (after W2.5)
  W3.4 closure capture classification                     [M]  (after W2.1)
  W3.5 trait coherence + monomorphization on Buff MIR     [L]
  W3.6 overflow policy + arith overflow flags             [M]
  W3.7 exhaustiveness re-verify + extend                  [M]
  GATE: MLIR coverage = 100% of language; GPU live; rustc still fallback.

Wave 4 — Phase B self-host (LAST; sequential ports):
  W4.1 lexer → Buff   [M]   W4.2 parser → Buff   [L]
  W4.3 types → Buff   [L]   W4.4 codegen-rust → Buff [L]
  W4.5 codegen-mlir → Buff  [L]
  W4.6 Stage 2 fixed-point + Stage 3 byte-check           [M]
  W4.7 CI: both backends + Stage2==Stage3                 [M]
  GATE: self-bootstraps via MLIR; Stage 2 == Stage 3.

Wave 5 — V3 drop rustc (after Wave 4 stable ≥1 release):
  W5.1 remove codegen-rust from default pipeline          [S]
  W5.2 replace stdlib/runtime — FFI-reuse mature crates   [XL selective]
  W5.3 NVVM (NVIDIA) hardening                            [M]
  W5.4 FPGA / CIRCT research spike                        [L research]
```

**Critical path**: W0.1 → W1.1 → W1.3 → W2.1 → W2.3 → W2.7 → W3.2 → W4.x → W5.1.

### Agent dispatch
Compiler-internals/memory-model/MLIR tasks → `ultrabrain` or `deep`. Crate scaffolding/CI/glue → `unspecified-high`. Small config/flags → `quick`. Research spikes (FPGA/CIRCT) → `deep`. Docs → `writing`.

---

## Open Decisions (defaults applied — override any)

1. **Cycle policy**: lint + `weak<T>` opt-in + arena-indices convention (no collector). *Default: this.*
2. **Forbid user-defined `Drop` in V2** so the semantic oracle stays valid. *Default: yes.*
3. **V1.X prerequisites** delivery: a demarcated **Wave 0 gate** here that ships as a separate **`v1.25`** release before V2.0. *Default: this.*
4. **V2.0 scope**: ships **CPU + wasm only**; GPU (MLIR) lands in **V2.1**; WGSL/wgpu remains the production GPU path through V2.x. *Default: yes.*
5. **FFI strategy**: **Rust-shim** (Buff emits an MLIR-construction plan consumed by a thin Rust binary calling melior); defer a native Buff C-FFI to V3. *Default: Rust-shim.*

---

## TODOs

> Workstream-level tasks (multi-year roadmap altitude). Each = one W-workstream with agent profile, parallelization, references, and agent-executed QA/exit-gate. Added in batches below.

### Wave 0 — V1.X Prerequisite Gate (ships as `v1.25`, BEFORE V2.0)

> This wave is a hard gate. Buff cannot host its own compiler until these land. Recommended to ship as a distinct `v1.25 "Language for Self-Hosting"` release; tracked here so the dependency is never lost.

- [ ] W0.1. **Generics: functions, structs, traits + bounds + `where`**

  **What to do**:
  - Add generic parameters to the AST: `FuncDecl.generics`, `StructDecl.generics`, `TraitDecl.generics`, trait-bound lists, and `where` clauses (all currently ABSENT).
  - Parser: parse `<T>`, `<T: Bound>`, `where` clauses.
  - Types: generic instantiation, trait-bound satisfaction checking, inference through generic call sites.
  - Codegen-rust: pass generics through to Rust generics (simplest correct path); monomorphization deferred to W3.5 for the MLIR path.

  **Must NOT do**: expose lifetimes/`'a`; break existing non-generic programs; reuse existing ErrorCodes (allocate a new sub-range).

  **Recommended Agent Profile**: `ultrabrain` (type-system-heavy, ripples lexer→parser→types→codegen).

  **Parallelization**: Wave 0. **Blocks**: W3.5 (monomorphization), W4.1–W4.5 (self-host — compiler source is generic-heavy). **Blocked By**: none (start immediately).

  **References**: blueprint §4 (generics = hard blocker); `crates/buff-lang-ast/src/{decl,ty}.rs`; `crates/buff-lang-parser/`; `crates/buff-lang-types/`; `crates/buff-lang-codegen-rust/`; per-crate `AGENTS.md`.

  **Acceptance Criteria + QA Scenarios**:
  - [ ] TDD: parser + type + codegen tests (insta snapshots) for generic func/struct/trait.
  - Scenario (happy): `interactive_bash` runs `buff run <generic example>.buff` → expected output; snapshot generated Rust compiles.
  - Scenario (negative): a generic call violating a trait bound → `buff check` emits the new-range typed error, non-zero exit.
  - Evidence: `.sisyphus/evidence/w0-1-generics.txt`.

  **Commit**: `feat(types): generic functions, structs, traits with bounds` (gated on `cargo test --workspace` + clippy).

- [ ] W0.2. **Multi-file codegen end-to-end + `extern crate` → Cargo.toml assembly**

  **What to do**:
  - Wire the resolved module graph into codegen so multi-file programs compile and LINK (today the graph resolves but the CLI compiles one file at a time).
  - Emit a `Cargo.toml` from the collected `extern_crates()` set so async/module examples link (tokio, etc.).

  **Must NOT do**: change module-resolution semantics; break single-file compilation.

  **Recommended Agent Profile**: `unspecified-high` (CLI + build wiring).

  **Parallelization**: Wave 0, ∥ W0.1. **Blocks**: W4.x (self-host = 19 crates of Buff). **Blocked By**: none.

  **References**: blueprint §4; `crates/buff-lang-cli/src/` module graph + `extern_crates()`; `examples/modules/`, `examples/async_demo.buff` (currently codegen-only).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `buff run examples/modules/<main>.buff` compiles+links+runs end-to-end (previously codegen-only).
  - Scenario: `buff run examples/async_demo.buff` links tokio and runs.
  - Evidence: `.sisyphus/evidence/w0-2-multifile.txt`.

  **Commit**: `feat(cli): end-to-end multi-file codegen + Cargo.toml assembly`.

- [ ] W0.3. **BigInt literals + allocator-passing convention + FFI decision (Rust-shim)**

  **What to do**:
  - Lexer: big integer literals (compiler source needs them).
  - Language: an allocator-passing convention enabling arenas (Roc lesson: hard to retrofit later).
  - **Decide + document** the FFI strategy = **Rust-shim** (Buff emits a serialized "MLIR-construction plan" consumed by a thin Rust binary that calls melior) — NO new `unsafe`/raw-pointer language feature in V2. Record in `.sisyphus/decisions/buff-v2-ffi.md`.

  **Must NOT do**: introduce user-visible pointers/`unsafe` blocks in V2; over-design the allocator API.

  **Recommended Agent Profile**: `deep` (allocator + FFI design are judgment calls).

  **Parallelization**: Wave 0, ∥ W0.1/W0.2. **Blocks**: W1.1 (FFI decision shapes the MLIR binding). **Blocked By**: none.

  **References**: blueprint §3 (binding options) + §4; `crates/buff-lang-lexer/`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: a `.buff` with a big literal parses and runs; snapshot output.
  - Artifact: `.sisyphus/decisions/buff-v2-ffi.md` exists and states Rust-shim + rationale.
  - Evidence: `.sisyphus/evidence/w0-3.txt`.

  **Commit**: `feat(lang): bigint literals + allocator convention; docs(decision): FFI Rust-shim`.

  **Wave 0 EXIT GATE**: a small compiler component (e.g., a JSON parser) written in Buff compiles+runs via the Rust backend, exercising generics + multi-file. Verified by `oracle` review before Wave 1 self-host-dependent work.

### Wave 1 — V2.0 MLIR Scaffolding (dual-backend begins)

- [ ] W1.1. **`buff-lang-codegen-mlir` crate + melior/mlir-sys + vendored-LLVM 3-OS CI**

  **What to do**:
  - New crate `crates/buff-lang-codegen-mlir/` wired into the workspace; add `melior` + `mlir-sys` to `[workspace.dependencies]` pinned to the matching LLVM major.
  - CI: vendored LLVM+MLIR tarball per OS (ubuntu/windows/macos); dynamic-link default on Windows.
  - **Amend the "no cc-rs" rule**: document an explicit carve-out (melior uses `bindgen`→cc-rs) at the crate `Cargo.toml` and in `.sisyphus/plans/buff-conventions.md`, mirroring the WGSL-`format!()`/zeromq exceptions.

  **Must NOT do**: introduce cc-rs into other crates; break the existing 3-OS CI; unpin from the workspace dependency table.

  **Recommended Agent Profile**: `unspecified-high` (build/CI/dependency plumbing across 3 OSes).

  **Parallelization**: Wave 1. **Blocks**: ALL other MLIR work (W1.2–W1.6, Wave 2–5). **Blocked By**: W0.3 (FFI = Rust-shim decision).

  **References**: blueprint §3 (bindings, vendored LLVM, Windows link) + §4 (cc-rs carve-out); root `Cargo.toml` `[workspace.dependencies]`; `.github/workflows/ci.yml`; `AGENTS.md` anti-patterns (no cc-rs rationale).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `cargo build -p buff-lang-codegen-mlir` succeeds on all 3 OSes in CI (a trivial "build empty MLIR module + verify + print" smoke).
  - Scenario: `interactive_bash` runs the smoke binary → prints a valid empty `module {}` and exits 0.
  - Evidence: CI logs + `.sisyphus/evidence/w1-1-mlir-smoke.txt`.

  **Commit**: `feat(codegen-mlir): scaffold crate + melior/mlir-sys + vendored-LLVM CI` (+ `docs(conventions): cc-rs carve-out for MLIR`).

- [ ] W1.2. **Minimal `buff` MLIR dialect (~10–15 ops) + MLIR SpanMap**

  **What to do**:
  - Define a minimal `buff` dialect (ODS/TableGen): `buff.alloc`, `buff.arc<T>`, `buff.span`, `buff.prefer_gpu`, `buff.call` (preserving named args).
  - `buff.span` carries source spans for reverse error mapping — a MLIR-side `SpanMap` (mirror the `.buffhtml` SpanMap pattern) so rustc/LLVM diagnostics map back to `.buff` lines.

  **Must NOT do**: bloat the dialect (anything lowerable belongs in builtin dialects); emit raw-string MLIR (build via melior/ODS).

  **Recommended Agent Profile**: `deep` (dialect design + progressive-lowering forethought).

  **Parallelization**: Wave 1. **Blocks**: W1.4 (lowering targets these ops). **Blocked By**: W1.1.

  **References**: blueprint §3 (dialect strategy, SpanMap); `crates/buff-lang-codegen-buffhtml/` (SpanMap precedent); `crates/buff-lang-error/` (span/diagnostic).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: round-trip test — build a module using each `buff.*` op via melior, `--verify` passes, textual dump snapshot is stable (insta).
  - Scenario: a `buff.span`-annotated op produces a diagnostic that maps to the correct `.buff` line/col.
  - Evidence: `.sisyphus/evidence/w1-2-dialect.txt` + golden `.mlir` snapshots.

  **Commit**: `feat(codegen-mlir): minimal buff dialect + MLIR SpanMap`.

- [ ] W1.3. **Buff MIR (CFG-of-basic-blocks + ghost instructions)**

  **What to do**:
  - Introduce a **Buff MIR** analysis IR between AST and MLIR: control-flow graph of basic blocks with explicit ghost instructions (`borrow`/`end_borrow`/`consume`/`drop`/`arc_retain`/`arc_release`). This is the substrate ALL Wave-2 memory analyses consume (do NOT analyze on the AST — that is why today's `OwnershipFacts` is inadequate).
  - AST → Buff MIR lowering; MIR verifier; MIR pretty-printer for snapshots.

  **Must NOT do**: put memory analysis in this task (Wave 2); leak MIR into user-facing surface; reuse the weak `OwnershipFacts` string-set model.

  **Recommended Agent Profile**: `ultrabrain` (IR design is foundational; a wrong shape blocks all of Wave 2).

  **Parallelization**: Wave 1, ∥ W1.2. **Blocks**: W2.1–W2.7 (entire memory model), W3.5. **Blocked By**: W1.1.

  **References**: blueprint §1 ("KEY IR INSIGHT: introduce a Buff MIR"); `crates/buff-lang-ast/src/ir.rs` (existing IR node, may extend); rustc MIR / Hylo IR as prior art.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: AST→MIR for representative programs (fn calls, `if`/`match`/loops, moves) produces a verified CFG; MIR dump snapshot is deterministic (compile twice → byte-identical).
  - Scenario (negative): a malformed MIR is rejected by the verifier with a clear error.
  - Evidence: `.sisyphus/evidence/w1-3-mir.txt` + golden MIR snapshots.

  **Commit**: `feat(mir): Buff MIR CFG + ghost instructions + verifier`.

- [ ] W1.4. **`buff` → builtin-dialects lowering (func/arith/scf/cf/memref/index/math)**

  **What to do**: a `--convert-buff-to-std` pass lowering the `buff` dialect ops (except ownership/`arc`, which Wave 2 handles) into builtin dialects; cover the current language surface (functions, arithmetic, control flow, structs→memref, calls with named args).

  **Must NOT do**: handle ownership/ARC here (Wave 2 owns `--ownership-arc-lowering`); emit raw-string MLIR.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 1. **Blocks**: W1.5. **Blocked By**: W1.2 (dialect), W1.3 (MIR is the input).

  **References**: blueprint §3 (lowering pipeline); W1.2/W1.3 outputs.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: lower `fibonacci`/`calculadora`-class programs to builtin dialects; `--verify` passes; golden `.mlir` snapshot stable.
  - Evidence: `.sisyphus/evidence/w1-4-lowering.txt` + snapshots.

  **Commit**: `feat(codegen-mlir): buff → builtin-dialects lowering`.

- [ ] W1.5. **`convert-to-llvm` + `llc` glue + `--backend={rust,mlir,wgsl}` CLI flag**

  **What to do**: complete the path builtin-dialects → `llvm` dialect → LLVM IR (`mlir-to-llvmir`) → `llc`/`opt` → object → native binary; add the `--backend` flag to the CLI (default `rust` initially) so both backends coexist.

  **Must NOT do**: make `mlir` the default backend yet (Rust stays default/oracle until Wave 5); duplicate the `compile_rust_to_exe`/`with_exe_extension` logic without keeping both copies in sync (known duplication CLI↔eval).

  **Recommended Agent Profile**: `unspecified-high`.

  **Parallelization**: Wave 1. **Blocks**: W1.6, Wave 2 end-to-end runs. **Blocked By**: W1.4.

  **References**: blueprint §3; `crates/buff-lang-cli/src/pipeline.rs` (`compile_to_rust`/`compile_rust_to_exe`); `crates/buff-lang-cli/src/cli.rs` (Command enum).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `buff build --backend=mlir examples/ola.buff` produces a native binary; `interactive_bash` runs it → `Olá, Buff!`, exit 0.
  - Evidence: `.sisyphus/evidence/w1-5-native.txt`.

  **Commit**: `feat(cli): --backend flag + MLIR→LLVM→native path`.

- [ ] W1.6. **Snapshot + differential harness skeleton**

  **What to do**: CI harness that compiles the golden `.buff` corpus via `--backend=rust` and `--backend=mlir`, runs both, and diffs exit-code+stdout+stderr (semantic); insta golden `.mlir` per fixture; a determinism check (compile-same-AST-twice → byte-diff).

  **Must NOT do**: compare binary bytes/perf (semantic only); block CI on MLIR-only features (none yet).

  **Recommended Agent Profile**: `unspecified-high`.

  **Parallelization**: Wave 1 (ASAP). **Blocks**: every later wave's exit gate. **Blocked By**: W1.5.

  **References**: blueprint §"Verification Strategy" + §3 harness table; existing `tests/` insta setup.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `scripts/diff-backends.sh examples/` reports 0 semantic diffs on the currently-supported subset; determinism check green.
  - Evidence: `.sisyphus/evidence/w1-6-harness.txt`.

  **Commit**: `test(mlir): differential + snapshot + determinism harness`.

  **Wave 1 EXIT GATE**: `buff build --backend=mlir examples/ola.buff` runs and is differential-tested equal to `--backend=rust`. `oracle` review before Wave 2.

### Wave 2 — Memory Model (the big lift; replaces every guarantee rustc gave for free)

> All Wave-2 passes run on **Buff MIR** (W1.3). Exit only when the MLIR backend is memory-safe with rustc REMOVED from that path.

- [ ] W2.1. **Last-use + definite-initialization analysis (Hylo-style, L0)**

  **What to do**: dataflow passes on Buff MIR marking each value's final use (enabling move-out / no-retain) and proving definite initialization before use. Zero-cost stack/affine layer.

  **Must NOT do**: insert refcount ops here (W2.3); surface lifetimes to users.

  **Recommended Agent Profile**: `ultrabrain` (dataflow correctness is foundational).

  **Parallelization**: Wave 2. **Blocks**: W2.3, W3.4. **Blocked By**: W1.3 (Buff MIR).

  **References**: blueprint §1 (L0 layer, Hylo last-use); W1.3 MIR + ghost instructions.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: last-use markers verified against hand-computed expectations on move/branch/loop fixtures; use-before-init rejected with a new-range typed error.
  - Evidence: `.sisyphus/evidence/w2-1-lastuse.txt` + MIR snapshots.

  **Commit**: `feat(mem): last-use + definite-init on Buff MIR`.

- [ ] W2.2. **Exclusivity checker (borrow soundness without lifetimes)**

  **What to do**: verify no aliasing-mutation violations (law of exclusivity) by interpreting `borrow`/`end_borrow`/`consume` ghost instructions — reject programs rustc would reject, but surface as **Buff** errors, fully inferred (no user annotations).

  **Must NOT do**: require user lifetime/`&` annotations; defer to rustc (this is what makes rustc removable).

  **Recommended Agent Profile**: `ultrabrain`.

  **Parallelization**: Wave 2, ∥ W2.1. **Blocks**: "rustc leaves the pipeline" (Wave-2 gate). **Blocked By**: W1.3.

  **References**: blueprint §1 (exclusivity) + §2 (divergence-triage rule); "GAPS RUSTC COVERS FOR US" in `.sisyphus/drafts/buff-v2-research-codebase.md`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario (negative): aliased mutable access → Buff exclusivity error, non-zero exit.
  - Scenario (differential): programs accepted here must also be accepted by rustc on the Rust backend (oracle agreement) across the corpus.
  - Evidence: `.sisyphus/evidence/w2-2-exclusivity.txt`.

  **Commit**: `feat(mem): exclusivity checker (inferred, no user lifetimes)`.

- [ ] W2.3. **Perceus reference-counting insertion + drop specialization (L1)**

  **What to do**: insert precise `arc_retain`/`arc_release`/`drop` on Buff MIR only where L0 can't prove refcount ≤ 1 (escape/capture/store); specialize unique drops. Implements the Perceus algorithm (Reinking/Xie/de Moura).

  **Must NOT do**: use tracing GC; adopt Send/Sync (thread atomics come in W2.5); support user-defined `Drop` (forbidden in V2).

  **Recommended Agent Profile**: `ultrabrain` (algorithm-faithful; correctness-critical).

  **Parallelization**: Wave 2. **Blocks**: W2.4, W2.5, W2.7. **Blocked By**: W2.1.

  **References**: blueprint §1 (L1 Perceus); `.sisyphus/drafts/buff-v2-research-memory-selfhost.md` (Perceus detail).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: RC-instrumented programs run leak-free under a leak checker on the corpus (except documented `weak` cycles from W2.6).
  - Scenario (differential): observable I/O identical to the Rust backend.
  - Evidence: `.sisyphus/evidence/w2-3-perceus.txt`.

  **Commit**: `feat(mem): Perceus RC insertion + drop specialization`.

- [ ] W2.4. **Reuse analysis / FBIP (L2 — recover the RC cost)**

  **What to do**: Perceus "functional but in-place" reuse — when a unique value is dropped and a same-shape value is allocated shortly after, reuse the memory in place (turns `map`/`filter`/AST-rewrite chains into in-place mutation).

  **Must NOT do**: reuse across aliased/shared values; change observable semantics.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 2. **Blocked By**: W2.3.

  **References**: blueprint §1 (L2 FBIP); memory dossier (Perceus §2.4).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: a `map` over a uniquely-owned vector performs zero heap allocations (verified via allocation counter); output identical to Rust backend.
  - Evidence: `.sisyphus/evidence/w2-4-reuse.txt`.

  **Commit**: `feat(mem): reuse analysis (FBIP)`.

- [ ] W2.5. **Thread-share tracking + atomic RC (L3; Send/Sync replacement foundation)**

  **What to do**: static analysis tagging values that cross thread boundaries (spawn/channel/shared clone); emit **atomic** retain/release only there, non-atomic elsewhere. Replaces rustc's Send/Sync without library infection (Perceus thread-share + Vale seamless-concurrency model).

  **Must NOT do**: adopt Send/Sync auto-traits; make all RC atomic (perf regression).

  **Recommended Agent Profile**: `ultrabrain`.

  **Parallelization**: Wave 2. **Blocks**: W3.3. **Blocked By**: W2.3.

  **References**: blueprint §1 (L3, thread safety); memory dossier (thread-share §7.2).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `async`/`spawn` example is data-race-free (tsan-equivalent) and semantically equal to Rust backend; single-thread code uses non-atomic RC (verified in emitted IR).
  - Evidence: `.sisyphus/evidence/w2-5-threadshare.txt`.

  **Commit**: `feat(mem): thread-share tracking + atomic RC`.

- [ ] W2.6. **`weak<T>` + cycle lint + cycle policy (L4)**

  **What to do**: implement the confirmed cycle policy — a `weak<T>` opt-in reference type + a lint warning on likely-cyclic data + document the arena-indices convention for compiler-internal cyclic structures. **No cycle collector.**

  **Must NOT do**: add a tracing collector (violates no-GC); make `weak` user-mandatory in the common case.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 2. **Blocked By**: W2.3.

  **References**: blueprint §1 ("hardest sub-problem: cycles") + Open Decision #1.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: a doubly-linked/parent-pointer structure using `weak<T>` is leak-free; the lint fires on a strong-reference cycle.
  - Artifact: cycle-policy documented in the plan/conventions.
  - Evidence: `.sisyphus/evidence/w2-6-cycles.txt`.

  **Commit**: `feat(mem): weak<T> + cycle lint (no GC)`.

- [ ] W2.7. **`--ownership-arc-lowering` MLIR pass + `buff.arc<T>` type**

  **What to do**: lower the Buff-MIR ownership/RC facts into MLIR: a `buff.arc<T>` type + an `--ownership-arc-lowering` pass producing `memref` + refcount side-table; then hand off to bufferization/`convert-to-llvm`.

  **Must NOT do**: hand-write MLIR strings; duplicate W2.3 logic (this consumes its facts).

  **Recommended Agent Profile**: `ultrabrain`.

  **Parallelization**: Wave 2. **Blocks**: Wave-2 gate. **Blocked By**: W2.3, W1.4.

  **References**: blueprint §1 (arc-lowering) + §3 (bufferize).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: RC-annotated MIR lowers to verified LLVM; runs leak-free; differential-equal to Rust backend on the corpus.
  - Evidence: `.sisyphus/evidence/w2-7-arclowering.txt` + golden `.mlir`.

  **Commit**: `feat(codegen-mlir): ownership-arc-lowering + buff.arc<T>`.

  **Wave 2 EXIT GATE**: MLIR backend produces memory-safe binaries with **rustc removed from the MLIR path**; differential pass rate ≥99% on the corpus; leak-free (except documented `weak` cycles). `oracle` memory-safety review before Wave 3.

### Wave 3 — Coverage + Targets (parallel; drive MLIR to 100% of the language)

- [ ] W3.1. **wasm target (`wasm32-wasi`)**

  **What to do**: lower through LLVM with `--mtriple=wasm32-wasi` + `wasm-ld`; add `buff build --backend=mlir --target=wasm`. Preserves the playground story (own UI lib later; do not depend on Dioxus here).

  **Must NOT do**: block on the Dioxus UI path (Rust-specific; replaced separately).

  **Recommended Agent Profile**: `unspecified-high`.

  **Parallelization**: Wave 3. **Blocked By**: W1.5 (LLVM path).

  **References**: blueprint §3 (wasm strategy).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `examples/ola.buff` → `.wasm`; run in a wasm runtime (wasmtime) → `Olá, Buff!`, exit 0.
  - Evidence: `.sisyphus/evidence/w3-1-wasm.txt`.

  **Commit**: `feat(codegen-mlir): wasm32-wasi target`.

- [ ] W3.2. **GPU via MLIR `gpu`→SPIR-V + integrate existing wgpu host** ⚠️ highest-risk

  **What to do**: `--gpu-kernel-outlining` → `--convert-gpu-to-spirv` producing SPIR-V consumed by the EXISTING `buff-lang-runtime` wgpu host; ship behind `--backend=mlir-gpu`. **Keep `buff-lang-codegen-wgsl` + `buff-lang-runtime` as the PRODUCTION GPU path** through V2.x.

  **Must NOT do**: remove the WGSL/wgpu path; make MLIR-GPU the default; block V2.0 on this (GPU may slip to V2.1).

  **Recommended Agent Profile**: `deep` (novel + upstream instability).

  **Parallelization**: Wave 3. **Blocked By**: W2.7. **Risk**: live upstream bug llvm-project#155898 (`unrealized_conversion_cast` leak) — budget an upstream fix/workaround.

  **References**: blueprint §3 (GPU staged strategy) + Risk #3; existing `codegen-wgsl` binding contract (`@group(0) @binding(0/1)`, workgroup 64) + `buff-lang-runtime` dispatch.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario (conformance, no rustc oracle): a `@prefer(gpu)` map kernel via `--backend=mlir-gpu` yields byte-identical results to the WGSL/wgpu reference.
  - Scenario (fallback): with no GPU present, execution falls back to CPU and still produces correct output.
  - Evidence: `.sisyphus/evidence/w3-2-gpu.txt`.

  **Commit**: `feat(codegen-mlir): GPU via SPIR-V (behind --backend=mlir-gpu; WGSL stays production)`.

- [ ] W3.3. **Send/Sync-equivalent replacement (Perceus thread model)**

  **What to do**: replace every place the Rust backend relied on rustc's `Send`/`Sync` auto-traits with Buff's thread-share analysis (W2.5) as the canonical data-race-freedom guarantee for the MLIR path.

  **Must NOT do**: reintroduce auto-trait "infection"; weaken race-freedom.

  **Recommended Agent Profile**: `ultrabrain`.

  **Parallelization**: Wave 3. **Blocked By**: W2.5.

  **References**: blueprint §1 (thread safety) + §2 (oracle-triage rule).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: concurrency examples run race-free on the MLIR path; a program that would be `!Send` in Rust is either safely handled or rejected with a clear Buff error (documented policy).
  - Evidence: `.sisyphus/evidence/w3-3-sendsync.txt`.

  **Commit**: `feat(mem): thread-safety guarantee replaces Send/Sync`.

- [ ] W3.4. **Closure capture classification (FnOnce/FnMut/Fn-equivalent)**

  **What to do**: classify closure captures (by-move / by-mut / by-shared) on Buff MIR so codegen picks the right RC/borrow strategy without user annotations.

  **Must NOT do**: surface closure-kind annotations to users.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 3. **Blocked By**: W2.1.

  **References**: blueprint §5 (W3.4); `examples/closures.buff`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `examples/closures.buff` + capturing-closure fixtures run correctly on the MLIR path, differential-equal to Rust backend.
  - Evidence: `.sisyphus/evidence/w3-4-closures.txt`.

  **Commit**: `feat(mem): closure capture classification`.

- [ ] W3.5. **Trait coherence + monomorphization on Buff MIR**

  **What to do**: for the MLIR path, monomorphize generic functions/structs/traits (from W0.1) on Buff MIR and enforce trait coherence (no overlapping impls). This is what lets the MLIR backend handle generics that the Rust backend passed through to rustc.

  **Must NOT do**: rely on rustc for coherence on the MLIR path.

  **Recommended Agent Profile**: `ultrabrain`.

  **Parallelization**: Wave 3. **Blocked By**: W0.1 (generics), W1.3 (MIR).

  **References**: blueprint §5 (W3.5); W0.1 generics.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: generic programs compile+run via `--backend=mlir`, differential-equal to Rust backend; an overlapping-impl program is rejected with a Buff coherence error.
  - Evidence: `.sisyphus/evidence/w3-5-mono.txt`.

  **Commit**: `feat(codegen-mlir): monomorphization + trait coherence`.

- [ ] W3.6. **Numeric overflow policy + `arith` overflow flags**

  **What to do**: define Buff's overflow semantics and propagate them into MLIR `arith` overflow flags (wrapping/checked as specified), preserving current numeric-system behavior.

  **Must NOT do**: silently change existing arithmetic results vs the Rust backend.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 3. **Blocked By**: W1.4.

  **References**: blueprint §5 (W3.6); `.sisyphus/plans/buff-numeric-system.md`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: overflow fixtures produce identical results/traps on MLIR vs Rust backend.
  - Evidence: `.sisyphus/evidence/w3-6-overflow.txt`.

  **Commit**: `feat(codegen-mlir): numeric overflow policy`.

- [ ] W3.7. **Exhaustiveness re-verification + extension (or-patterns, ranges, guards)**

  **What to do**: ensure match-exhaustiveness holds on the MLIR path independent of rustc; extend pattern support (or-patterns, ranges, guards) as needed for self-host source.

  **Must NOT do**: depend on rustc's exhaustiveness on the MLIR path.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 3. **Blocked By**: W1.3.

  **References**: blueprint §5 (W3.7); existing `buff-lang-types` exhaustiveness module; `examples/pattern_matching.buff`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: a non-exhaustive match is rejected with a Buff error; or-pattern/range/guard fixtures run correctly, differential-equal to Rust backend.
  - Evidence: `.sisyphus/evidence/w3-7-exhaustiveness.txt`.

  **Commit**: `feat(types): exhaustiveness for MLIR path + pattern extensions`.

  **Wave 3 EXIT GATE**: `--backend=mlir` compiles+runs 100% of the Buff language (every `.buff`/`.buffhtml` example, differential-equal to Rust backend); GPU path live via SPIR-V with WGSL fallback; rustc still present as fallback/oracle. `oracle` + full differential review before Wave 4.

### Wave 4 — Phase B Self-Host (LAST; sequential; requires Wave 3 complete + `v1.25` generics)

> Port the compiler from Rust to Buff, one crate at a time, keeping BOTH backends and differential-testing every step. The Rust compiler stays in-tree as the bootstrap seed. **No V2-only language features in the compiler's own source until V3** (Go 1.5 lesson).

- [ ] W4.1. **Port lexer → Buff (Stage 0.5 proving ground)**

  **What to do**: reimplement `buff-lang-lexer` in Buff (hand-rolled byte-scanner + offside rule); the current Rust compiler (Stage 0) compiles it; differential-test token streams against the Rust lexer on the whole corpus.

  **Must NOT do**: change token semantics; use V2-only features in the source.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 4 (sequential start). **Blocks**: W4.2. **Blocked By**: W0.1, Wave 3 gate.

  **References**: blueprint §5 (W4.1) + §2 (bootstrap staging); `crates/buff-lang-lexer/`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: Buff-lexer token stream == Rust-lexer token stream on every `.buff` fixture (differential).
  - Evidence: `.sisyphus/evidence/w4-1-lexer.txt`.

  **Commit**: `feat(selfhost): lexer in Buff`.

- [ ] W4.2. **Port parser → Buff**

  **What to do**: reimplement `buff-lang-parser` (recursive-descent + Pratt) in Buff; differential-test the produced AST against the Rust parser.

  **Must NOT do**: alter AST shape/semantics.

  **Recommended Agent Profile**: `ultrabrain` (large, intricate).

  **Parallelization**: Wave 4. **Blocks**: W4.3. **Blocked By**: W4.1.

  **References**: blueprint §5 (W4.2); `crates/buff-lang-parser/`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: Buff-parser AST == Rust-parser AST (structural diff) on the corpus.
  - Evidence: `.sisyphus/evidence/w4-2-parser.txt`.

  **Commit**: `feat(selfhost): parser in Buff`.

- [ ] W4.3. **Port type inference/analysis → Buff**

  **What to do**: reimplement `buff-lang-types` (inference + the analysis suite) in Buff; differential-test typing/diagnostics against the Rust implementation.

  **Must NOT do**: change inference results or error codes.

  **Recommended Agent Profile**: `ultrabrain`.

  **Parallelization**: Wave 4. **Blocks**: W4.4. **Blocked By**: W4.2.

  **References**: blueprint §5 (W4.3); `crates/buff-lang-types/`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: same accept/reject + same ErrorCodes as the Rust implementation across `tests/valid` + `tests/invalid`.
  - Evidence: `.sisyphus/evidence/w4-3-types.txt`.

  **Commit**: `feat(selfhost): type inference in Buff`.

- [ ] W4.4. **Port codegen-rust → Buff (completes Stage 0.5)**

  **What to do**: reimplement `buff-lang-codegen-rust` in Buff (syn/quote/prettyplease equivalents, or a Buff-side codegen producing the same Rust source). Stage 0.5 = a Buff-written compiler that still emits Rust.

  **Must NOT do**: change generated Rust (must remain byte-identical to keep the Rust-oracle valid).

  **Recommended Agent Profile**: `ultrabrain`.

  **Parallelization**: Wave 4. **Blocks**: W4.5. **Blocked By**: W4.3.

  **References**: blueprint §5 (W4.4); `crates/buff-lang-codegen-rust/`.

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: Buff-written codegen emits byte-identical Rust to the Rust implementation on the corpus (insta parity).
  - Evidence: `.sisyphus/evidence/w4-4-codegen-rust.txt`.

  **Commit**: `feat(selfhost): codegen-rust in Buff (Stage 0.5)`.

- [ ] W4.5. **Port codegen-mlir → Buff (Stage 1: first MLIR-backend Buff compiler)**

  **What to do**: reimplement `buff-lang-codegen-mlir` + the Buff MIR passes in Buff, using the FFI Rust-shim (W0.3) to drive melior. Stage 1 = a Buff-written compiler that emits MLIR.

  **Must NOT do**: introduce a native Buff C-FFI yet (Rust-shim per Open Decision #5).

  **Recommended Agent Profile**: `ultrabrain`.

  **Parallelization**: Wave 4. **Blocks**: W4.6. **Blocked By**: W4.4 + all of Wave 2/3.

  **References**: blueprint §3 (Rust-shim binding) + §5 (W4.5).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: Stage-1 (Buff-written, MLIR-emitting) compiler builds `examples/` differential-equal to the Rust-written MLIR backend.
  - Evidence: `.sisyphus/evidence/w4-5-codegen-mlir.txt`.

  **Commit**: `feat(selfhost): codegen-mlir in Buff (Stage 1)`.

- [ ] W4.6. **Stage 2 fixed-point + Stage 3 byte-identical bootstrap check**

  **What to do**: Stage 1 compiles the compiler source → Stage 2; Stage 2 compiles it again → Stage 3; assert **Stage 2 == Stage 3 byte-for-byte** (reproducible self-host; catches non-determinism + "Trusting Trust").

  **Must NOT do**: accept a non-reproducible bootstrap.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 4. **Blocks**: W4.7, Wave-4 gate. **Blocked By**: W4.5.

  **References**: blueprint §5 (W4.6) + §"determinism guardrail".

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `scripts/bootstrap.sh` builds Stage 2 and Stage 3; `diff` reports byte-identical.
  - Evidence: `.sisyphus/evidence/w4-6-bootstrap.txt`.

  **Commit**: `feat(selfhost): reproducible Stage2==Stage3 bootstrap`.

- [ ] W4.7. **Bootstrap + dual-backend CI**

  **What to do**: CI runs, every commit: golden corpus through BOTH backends (differential) + the Stage 2 == Stage 3 byte check; keep the Rust seed build green.

  **Recommended Agent Profile**: `unspecified-high`.

  **Parallelization**: Wave 4. **Blocked By**: W4.6.

  **References**: blueprint §5 (W4.7).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: a CI run demonstrates differential-green + Stage2==Stage3 + Rust-seed-green.
  - Evidence: CI logs + `.sisyphus/evidence/w4-7-ci.txt`.

  **Commit**: `ci(selfhost): dual-backend + bootstrap determinism gate`.

  **Wave 4 EXIT GATE**: Buff compiler bootstraps itself end-to-end via the MLIR backend; **Stage 2 == Stage 3 byte-identical**; Rust seed still builds. `deep` bootstrap-integrity review before Wave 5.

### Wave 5 — V3: Drop rustc (only after Wave 4 stable for ≥1 release cycle)

> The MLIR-only endpoint. Enter only when the differential suite has shown <0.01% divergence for a full release cycle.

- [ ] W5.1. **Remove `buff-lang-codegen-rust` from the default pipeline**

  **What to do**: make MLIR the default backend; retire the Rust backend from the default build path (keep the crate available as an opt-in bootstrap seed, not default).

  **Must NOT do**: remove the Rust seed entirely (still the bootstrap origin); flip the default before the stability gate.

  **Recommended Agent Profile**: `unspecified-high`.

  **Parallelization**: Wave 5 (start). **Blocks**: W5.2. **Blocked By**: Wave-4 gate + ≥1-release stability.

  **References**: blueprint §5 (W5.1) + §6 Risk #7 (keep seed).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: default `buff build` uses MLIR; every `.buff`/`.buffhtml` example still passes; `strace`/log shows rustc is NOT invoked.
  - Evidence: `.sisyphus/evidence/w5-1-default-mlir.txt`.

  **Commit**: `feat(v3): MLIR is the default backend`.

- [ ] W5.2. **Replace Rust-crate-backed prelude/stdlib/runtime (FFI-reuse, not reimplement)**

  **What to do**: for every prelude free-fn / prelude type that today lowers to a Rust crate (chrono/regex/tokio/rayon/base64/sha2/hmac/…), provide an MLIR-only path — **prefer FFI-reuse of the mature crate via the Rust-shim** over reimplementing; reimplement only where trivial or where a shim is impossible.

  **Must NOT do**: reimplement regex/chrono/yaml/websocket from scratch (quarters of work, no benefit — Oracle Concern #3).

  **Recommended Agent Profile**: `deep` (per-library judgment; XL, likely itself sub-planned).

  **Parallelization**: Wave 5. **Blocked By**: W5.1.

  **References**: blueprint §5 (W5.2) + Concern #3; `crates/buff-lang-types/src/prelude*.rs` (surface inventory in `.sisyphus/drafts/buff-v2-research-codebase.md`).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: `examples/prelude_demo.buff` + DateTime/Regex/Hash examples run on the MLIR-only pipeline, output identical to the historical Rust-backend outputs.
  - Evidence: `.sisyphus/evidence/w5-2-stdlib.txt`.

  **Commit**: `feat(v3): MLIR-only stdlib/runtime (FFI-reuse)`.

- [ ] W5.3. **NVVM (NVIDIA) GPU hardening**

  **What to do**: mature the `gpu`→`nvvm` (PTX/CUDA) lowering path for NVIDIA hardware (lower-risk than SPIR-V).

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 5. **Blocked By**: W3.2.

  **References**: blueprint §3 (GPU V2.1 NVVM) + §5 (W5.3).

  **Acceptance Criteria + QA Scenarios**:
  - Scenario: a GPU kernel via `nvvm` produces results conforming to the WGSL/wgpu reference; CPU fallback intact.
  - Evidence: `.sisyphus/evidence/w5-3-nvvm.txt`.

  **Commit**: `feat(v3): NVVM GPU backend`.

- [ ] W5.4. **FPGA / CIRCT research spike (no production commitment)**

  **What to do**: evaluate `buff`/gpu → CIRCT (`hw`/`comb`/`seq`/`sv`) → Verilog/HLS feasibility; produce a written recommendation. Research only.

  **Recommended Agent Profile**: `deep`.

  **Parallelization**: Wave 5. **Blocked By**: W3.2.

  **References**: blueprint §3 (FPGA/CIRCT) + §5 (W5.4).

  **Acceptance Criteria + QA Scenarios**:
  - Artifact: `.sisyphus/decisions/buff-v3-fpga.md` — feasibility, effort, go/no-go recommendation.
  - Evidence: the decision doc itself.

  **Commit**: `docs(v3): FPGA/CIRCT feasibility spike`.

  **V3 ENDPOINT**: MLIR-only; rustc removed from the default pipeline (seed retained for bootstrap); Buff fully self-hosted and unifying CPU/GPU(/FPGA-research) via MLIR. Strict-superset preserved throughout.

---

## Final Verification Wave

> After each wave, a review gate runs before the next wave starts. At V2.0, V2.x-complete, and V3 milestones, run the full 4-agent review; all must APPROVE, then get explicit user okay.

- [ ] FV.1 **Plan/scope compliance** (`oracle`) — every "Must Have" present, every "Must NOT Have" absent (grep for `&`/lifetimes in user-facing surface, GC, user `Drop`, raw-string MLIR, ErrorCode reuse); deliverables match.
- [ ] FV.2 **Backend equivalence** (`unspecified-high`) — full corpus differential (rust vs mlir): exit/stdout/stderr parity ≥ target; MLIR-only conformance vs WGSL reference.
- [ ] FV.3 **Memory-safety audit** (`oracle`) — no UAF/double-free/leak (except documented `weak` cycles) on the test corpus; thread-share soundness; determinism guard green.
- [ ] FV.4 **Bootstrap integrity** (`deep`) — Stage 2 == Stage 3 byte-identical; Rust seed still builds; no V2-only features in compiler source pre-V3.

## Commit Strategy

- One commit per workstream slice, Conventional Commits: `feat(codegen-mlir): ...`, `feat(mir): ...`, `feat(mem): ...`.
- Pre-commit gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and (once the harness exists) the differential suite.
- Determinism check in CI on every commit touching codegen.

## Success Criteria

### Verification Commands
```bash
buff build --backend=mlir examples/ola.buff && ./ola            # → Olá, Buff!
# differential:
scripts/diff-backends.sh examples/                              # rust vs mlir: 0 semantic diffs
cargo test --workspace                                          # all green
# bootstrap (Wave 4):
scripts/bootstrap.sh && diff stage2-buff stage3-buff           # byte-identical
```

### Final Checklist
- [ ] All current examples pass via `--backend=mlir` (semantic parity).
- [ ] rustc removed (V3); memory model sound without it.
- [ ] Self-host fixed point (Stage 2 == Stage 3).
- [ ] Zero user-visible lifetimes/borrows; no GC.
- [ ] `v1.25` prerequisites shipped before self-host wave.
