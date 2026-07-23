# Buff v1.25+ — Launch Readiness (Speed + CPU/GPU MOAT + Self-Host-Frontend + Stdlib + Language + Diagnostics + Code Hygiene + Launch Infra)

> **Recreated 2026-07-22** after original file loss. This plan supersedes the lost `buff-v1.25-launch-readiness.md`. All content reconstructed from session context (Metis analysis, Momus approvals, competitive analysis, codebase inventory). If the original file is recovered, diff against this one — this version includes ALL session work (T104-T113, wave optimization, concurrency policy, competitive gap tasks).
>
> **This plan REPLACES `.sisyphus/plans/buff-v2-mlir-selfhost.md`.** The inherited plan optimized for MLIR migration, a custom 5-layer memory model, and dropping rustc — all of which serve goals (FPGA, compiler independence) the user has explicitly **deprioritized**.
> **SemVer note:** this is **non-breaking** work and ships as **`v1.25+` minor versions**. "2.0.0" is reserved for the first genuinely breaking change.

## TL;DR

> **Quick Summary**: Keep Buff's proven `.buff → Rust → rustc/LLVM → native` pipeline (and its WGSL/wgpu GPU path) — preserving rustc's borrow-checker as a *free* safety net — and **make Buff launch-ready** by investing in nine areas: **(A) Go-like compile speed**, **(B) a best-in-class automatic CPU/GPU dispatch MOAT**, **(C) rewriting the compiler's own source in Buff (still transpiling to Rust)**, **(D) stdlib + language-feature gaps**, **(E) diagnostics + tech-debt time-bombs**, **(F) launch infrastructure**, **(G) performance-control surface**, **(H) developer-experience tooling**, and **(I) code hygiene prerequisite for Track C**.
>
> **Deliverables** (9 tracks):
> - **Track A — Compile Speed**: fast-linker defaults, Cranelift dev backend, debuginfo tuning, salsa incremental, multi-crate emission, sccache, cross-compilation `--target`, `buff profile` CPU/allocation profiler, `--detect-races`.
> - **Track B — CPU/GPU MOAT**: data-locality-aware dispatch, dynamic runtime workload inspection, refined cost model, profile-guided kernel fusion, profile cache, `--explain` dispatch diagnostic.
> - **Track C — Self-Host-Frontend**: generics + monomorphization, then port compiler crates (lexer → parser → types → codegen-rust) into Buff, still emitting Rust, with byte-identical bootstrap gate.
> - **Track D — Stdlib + Language Gaps**: JSON, File I/O, HTTP client, expanded test assertions, expanded collections, user-defined generics, bounds/traits, or-patterns, pattern guards, struct patterns, lazy iterators, ranges, defer, Decimal, CLI parsing, raw strings, import aliases, tuple destructuring.
> - **Track E — Diagnostics + Tech-Debt**: color output, ErrorCode→LSP propagation, `--explain`, LSP codeAction/codeLens/inlayHint/semanticTokens, E14xx runtime codes, E15xx warnings, WGSL+runtime span preservation, invalid-fixture coverage, time-bomb defusers, stability tiers.
> - **Track F — Launch Infrastructure**: Buff 1.0 Compatibility Document, "The Book" tutorial (mdbook), `buff doc`, production registry (invite-only beta), prebuilt binaries + installers, MEMORY_SAFETY.md, tree-sitter published, CI arm64, AI-tooling MCP bridge, cargo-audit + SBOM, AGENTS.md update for 64-crate reality, release runbook, decision record.
> - **Track G — Performance Control Surface**: `@prefer(cpu)` + `@force(gpu|cpu)`, `@blocking`, `@workgroup(N)`, `BUFF_FAIL_LOUD_GPU`, `@no-alloc`, `@pin`, `Box<dyn Trait>`, `@inline`/`@no_inline`, DCE, constant propagation.
> - **Track H — DX Tooling**: `buff watch`, `buff fix`, `buff bench`, snapshot testing, `buff test --watch`, `buff doc --serve`, `buff generate`.
> - **Track I — Code Hygiene (prerequisite for Track C)**: best-practices audit of all 64 workspace crates, tier-1 god-class splits (`rust_codegen.rs` 17.5K→≤5K, `prelude_types.rs` 9.3K→≤2K), tier-2 medium god classes + idiom cleanup, tier-3 naming + dead-code polish.
>
> **Estimated Effort**: XXL (multi-release program; **122 tasks across 9 tracks (A-I) + 6 waves + Final Verification**; every sub-unit is independently shippable as a v1.MINOR)
> **Parallel Execution**: YES — 7 tracks across 6 waves; see Execution Strategy.
> **Critical Path**: T22/T13/T104 (Wave 0 sequential start) → T105a/b + Track D stdlib (Wave 1) → T37/T38 (Wave 2a language) → T17/T18 (Wave 3 ports) → T19 (Wave 4 bootstrap) → F1-F4 → user okay.

---

## Context

### User's Governing Priorities (verbatim)
> "I don't really give much importance for FPGAs, but I give importance for adoption and a strong MOAT, I myself don't mind Rust that much to give us the security we currently have, but I am not sure we have the best optimizations for autodetection of CPU/GPU workloads as well as slow compilation times for big projects, those are my 2 most problematic concerns."
> "The self-host idea also I dont mind transpiling our code later to rust, the important thing is to have our custom code in Buff to be easier to maintain."

### User's 3 Confirmed Decisions
1. **Direction** — Reframe to Speed + MOAT + Self-host-frontend. Keep transpiling to Rust. Demote MLIR & custom-memory-model to optional far-future V3.
2. **Self-host scope** — Rewrite compiler crates in Buff but still emit Rust. NOT drop-rustc, NOT own backend.
3. **Memory model** — Keep Rust's model entirely; NO custom memory work. Zero new safety risk.

### Version Numbering (SemVer 2.0, user-confirmed)
This is NOT "V2.0". Under SemVer 2.0, MAJOR is reserved for backwards-incompatible changes. Everything here is additive/non-breaking. Ships as `v1.25+` MINOR continuations after `v1.13→v1.24` frameworks roadmap (`buff-v1x-frameworks.md`). `2.0.0` reserved for first genuinely BREAKING change.

### Versioning Principle (verbatim)
> "everything we can ship together as a coherent unit we do in a single version; if they don't affect each other they should be separate versions."

### Codebase Reality (verified 2026-07-22)
- **64 crates** in workspace (AGENTS.md says "19" — T108 fixes this)
- **God classes**: `rust_codegen.rs` (17,453 LOC), `prelude_types.rs` (9,317 LOC), `stmt.rs` (3,189 LOC), `kernel.rs` (2,156 LOC)
- **Hard-rule violations**: 1,258 unwrap/expect + 287 panic!/todo!/unimplemented! in non-test src/
- **Intentional duplication**: `with_exe_extension` duplicated in cli/pipeline.rs + buff-eval/lib.rs (AGENTS.md: "Keep the two copies in sync manually" — for dependency isolation; DO NOT EXTRACT)

---

## Work Objectives

### Core Objective
Make Buff **launch-ready**: (1) compile at near-Go speed, (2) automatically dispatch CPU vs GPU better than any existing language, (3) make compiler source maintainable by writing it in Buff, (4) close stdlib + language-feature gaps, (5) close diagnostics + tech-debt gaps, (6) ship launch infrastructure, (7) provide perf-control surface, (8) ship DX tooling, (9) ensure code hygiene before self-host ports — **all without sacrificing the free memory/thread safety Rust gives us.**

### Must Have
- Keep the `.buff → Rust → rustc` pipeline as production backend throughout v1.25+.
- Keep rustc's borrow-checker as sole memory/thread-safety authority.
- Every optimization is **opt-in-safe**: correctness never depends on a speed flag.
- Preserve runtime's "GPU failure invisible; CPU fallback always correct" guarantee.
- Every launch-readiness gap is either closed by a task OR explicitly accepted in decision record (T110).

### Must NOT Have (Guardrails)
- ❌ No MLIR in v1.25+ (demoted optional V3 spike only).
- ❌ No custom memory model (no ARC, no Perceus, no borrow-inference, no `weak<T>`).
- ❌ No dropping rustc (remains safety oracle + production backend).
- ❌ No `cc-rs` / C-shim dependencies (pure-Rust philosophy).
- ❌ No raw-string Rust codegen (syn/quote/prettyplease only; WGSL exception in `codegen-wgsl/shader.rs`).
- ❌ No renumbering/reusing existing ErrorCodes (E10xx-E13xx STABLE FOREVER; new ranges E14xx/E15xx only).
- ❌ No scope creep into deep learning / tensor frameworks (MOAT = general-purpose dispatch).
- ❌ No unwrap/expect/panic!/unimplemented!/todo! in non-test code (repo hard rule).
- ❌ No silent rustc-error-leak for NEW error categories (existing verbatim leak accepted for ownership/lifetime; NEW diagnostics MUST surface as Buff diagnostics with ErrorCode + Span).
- ❌ No HashMap/HashSet in compiler-internal crates (deterministic codegen; BTree only). User-facing HashSet/HashMap from T27 is the EXCEPTION (codegen-rust type-lowerer + prelude_types registry only).
- ❌ No `with_exe_extension` extraction (intentional duplication per AGENTS.md).

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — all verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (insta snapshots + proptest + per-crate tests/, 3-OS CI).
- **Automated tests**: YES (TDD) — RED → GREEN → REFACTOR.
- **Framework**: `cargo test` + `insta` (snapshots) + `proptest` (properties). Runtime tests use `MockGpuBackend`.

### QA Policy
Every task includes agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

> **QA format note**: Tasks T7-T12, T22-T27, T31-T35, T37-T53 use an abbreviated one-line QA format with Evidence paths. The executing agent should expand these to full scenario blocks (Tool / Steps / Expected Result / Evidence) before or during execution. The Evidence path is the contract — the format is a guideline.
- **CLI/compile-speed**: Bash — run `cargo run -p buff-lang-cli -- ...`, time it, assert output + exit code.
- **Runtime/MOAT**: Bash — `cargo test -p buff-lang-runtime`; drive dispatch via `MockGpuBackend`.
- **Errors**: Bash — run compiler on fixture, assert rendered diagnostic (snapshot) + `--error-format json`.
- **Self-host**: Bash — bootstrap script; diff Stage-2 vs Stage-3 byte-for-byte.
- **Refactoring (Track I)**: Bash — `git tag pre-hygiene-v1.25` baseline + codegen-hash comparison (byte-identical output).

---

## Execution Strategy

### Parallel Execution Waves

> **Optimized 2026-07-22** (Metis-reviewed, Momus-approved): T13 pulled into Wave 0; Wave 2 split into 2a (independent) + 2b (dependent); Track F long-lead items pulled earlier; T15/T16 ports moved to Wave 2a; T19 separated into Wave 4; T105 split into T105a + T105b; added T108-T118 (AGENTS.md, release runbook, decision record, cross-compile, profiler, race detector, .env loading, buff expand, community health files, playground rebuild, VSCode extension update). **122 tasks across 6 waves + Final Verification.**

```
Wave 0 (Foundation — DO FIRST: baselines + generics + audit + time-bombs + critical docs):
│   ── SEQUENTIAL START (these 3 land before parallel dispatch begins) ──
├── T22:  Benchmark harness + baseline capture (FIRST ACTION)
├── T13:  Generics + monomorphization (highest-leverage task, gates 8+ downstream)
├── T104: Code-hygiene audit (produces inventory doc; gates T105a/b-T107)
│   ── PARALLEL BATCH ──
├── T1:   P0 error-system (multi-span + suggestions + JSON)
├── T108: AGENTS.md update for 64-crate reality (LAUNCH CREDIBILITY BLOCKER)
├── T110: Decision record finalization (BLOCKS T20, T21)
├── T2-T6: Track A/B quick wins (linker, debuginfo, cranelift, dispatch, explain)
├── T28-T36: Tech-debt time-bomb batch (6 atomic commits, one delegated task)
├── T52:  Missing error doc pages
├── T59, T61, T63: Track F infrastructure (MEMORY_SAFETY, CI arm64, cargo-audit)
├── T81-T86: Track D/E ergonomic fixes + bug fixes
├── T92:  Track F: Editions design doc
├── T112: Cross-compilation --target flag
├── T116: Community health files (CODE_OF_CONDUCT, SECURITY, templates, dependabot)
└── T119: CHANGELOG.md expansion for v1.13-v1.25

Wave 1 (Build infra + stdlib + long-lead launch items — MAX PARALLEL):
├── T7-T12: Track A/B medium efforts (salsa, multi-crate, sccache, data-locality, cost-model, profile-cache)
├── T23-T27: Track D stdlib gaps (Json, File, HTTP, assertions, collections)
├── T31, T32, T35: Track E tech-debt medium
├── T44, T49, T50: Diagnostics quick wins (ErrorCode→LSP, WGSL span, runtime span)
├── T60:  Track F: tree-sitter published
├── T79, T88: Track D prelude types (Json, algorithms)
├── T105a: Track I: split rust_codegen.rs (BLOCKED BY T104+T22)
├── T105b: Track I: split prelude_types.rs (BLOCKED BY T104+T22; PARALLEL WITH T105a)
├── T54:  Track F: Compatibility Document (BLOCKED BY T33)
├── T57:  Track F: Production registry — invite-only beta (no blockers)
├── T58:  Track F: Prebuilt binaries (BLOCKED BY T33/T34/T61)
├── T95:  Track F: Formal language specification
└── T109: Track F: Release process runbook (BLOCKED BY T33)

Wave 2a (Language + Diagnostics + Perf + DX — ALL INDEPENDENT):
├── T14:  Track B: dataflow-graph IR + fusion
├── T15:  Track C: port lexer → Buff (only needs T13, NOT T105)
├── T16:  Track C: port parser → Buff (only needs T13)
├── T37-T41: Track D language features (generics, bounds, or-patterns, guards, struct patterns)
├── T43, T45-T48: Track E diagnostics (color, --explain, LSP capabilities, error codes)
├── T53: Track E (did-you-mean wiring)
├── T56:  Track F: buff doc real implementation ← BLOCKED BY T26
├── T62:  Track F: MCP bridge (wraps v1.2 LSP; extends when T46 lands)
├── T65-T67, T70: Track G perf control (@blocking, @workgroup, FAIL_LOUD, @pin)
├── T72-T74: Track D (cross-file resolution, string ops, comptime)
├── T76-T78: Track G compiler passes (@inline, DCE, const-prop)
├── T84, T87, T89-T90: Track D (ranges, CLI parsing, Decimal, defer)
├── T93-T94, T96: Track D (raw strings, import aliases, tuple destructuring)
├── T97, T99-T100, T103: Track H DX (watch, bench, snapshots, generate)
├── T106-T107: Track I tier 2/3
├── T111: Track A: buff profile (CPU/allocation profiler)
├── T113: Track A: --detect-races (ThreadSanizer passthrough)
├── T114: Track D: .env file loading (Env.load prelude)
└── T115: Track A: buff expand (show generated Rust for debugging)

Wave 2b (Dependent layer — needs Wave 2a outputs):
├── T42:  Complex pattern type inference ← BLOCKED BY T37 (moved from 2a)
├── T51:  Invalid test fixture coverage ← BLOCKED BY T47+T48 (moved from 2a)
├── T64:  @prefer(cpu)/@force(gpu) ← BLOCKED BY T47
├── T68:  Box<dyn Trait> ← BLOCKED BY T38
├── T69:  @no-alloc lint ← BLOCKED BY T48
├── T71:  Lazy iterators ← BLOCKED BY T75
├── T75:  Associated types ← BLOCKED BY T38
├── T80:  Http prelude type ← BLOCKED BY T79
├── T91:  Stability tiers ← BLOCKED BY T48
├── T98:  buff fix ← BLOCKED BY T53
├── T101: buff test --watch ← BLOCKED BY T97
└── T118: VSCode extension v1.3 update ← BLOCKED BY T46
├── T120: buff fmt completeness verification ← after Wave 2a language features
└── T122: buff-dap debugger verification ← after T13+T37

Wave 3 (Showcase — self-host types/codegen ports + The Book):
├── T17:  Port buff-lang-types → Buff ← BLOCKED BY T13+T105b+T37+T38
├── T18:  Port buff-lang-codegen-rust → Buff ← BLOCKED BY T13+T105a+T17
├── T55:  "The Book" tutorial ← BLOCKED BY T54+T23-T25
├── T102: buff doc --serve ← BLOCKED BY T56+T97
└── T117: Playground rebuild for v1.25 features ← needs Wave 2a/2b language features
└── T121: Benchmark publication page ← uses T22 baseline data

Wave 4 (Self-host closure):
└── T19:  Bootstrap determinism gate (Stage 2 == Stage 3) ← BLOCKED BY T15-T18

Wave FINAL (Parallel reviews — ALL must APPROVE, then user okay):
├── F1: Plan compliance audit (oracle)
├── F2: Code quality review (unspecified-high)
├── F3: Real manual QA — benchmarks + bootstrap + stdlib + launch smoke (unspecified-high)
└── F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay -> PUBLIC LAUNCH

Critical Path:
  T22 → T13 → T38 → T75 → T71
  T22 → T13 → T37 → T17 → T18 → T19 → F3 → user okay
  T104 → T105b → T17 (types port needs clean prelude_types.rs)
  T104 → T105a → T18 (codegen port needs clean rust_codegen.rs)
  T13 → T15 → T19 (lexer port NOT blocked by T105)
  T33 → T58 → launch announcement

Parallel Speedup: ~65% faster than sequential
Theoretical Max Concurrent: 42 (Wave 2a); PRACTICAL: 3-6 (hard cap — see Concurrency Policy)
```

### Concurrency Policy (MANDATORY — 3-6 tasks in parallel, optimizing for maximum safe parallelism)

> **Target: 3-6 tasks running concurrently at any time, always optimizing for the maximum that doesn't conflict.** The orchestrator should aim for 5-6 when safe combinations exist, falling back to 3-4 when most remaining tasks share files. Waves define dependency layers (what CAN start), not dispatch batches (what SHOULD start simultaneously). The hard constraint: no two parallel tasks edit the same files.

#### Safe Parallel Combinations
- **Cross-track**: tasks in different tracks touch different crates
- **Writing tasks**: T54/T55/T59/T92/T95/T109/T110 — touch only .md files
- **Different-crate refactors**: T105a (codegen-rust) + T105b (types) — zero file overlap
- **CI + docs + code**: T61 (.github/) + T108 (AGENTS.md) + T1 (buff-lang-error/)
- **Append-only stdlib**: concurrent prelude_types.rs tasks append variants at end, use section comments

#### Unsafe Parallel Combinations (serialize these)
- T2+T3+T4 (all touch pipeline.rs) → serialize: T2→T3→T4
- T1+T43+T44 (all touch diagnostic.rs) → serialize: T1→T43→T44
- T37+T38+T42 (all touch infer.rs) → serialize: T37→T38→T42
- T105a + any codegen-rust task → T105a MUST complete first
- T105b + any types/prelude task → T105b MUST complete first
- T46+T62 (both touch handlers.rs) → T46 before T62
- T15+T16 (both touch Buff-side parser files) → T15→T16
- T17+T18 (T18 depends on T17's type definitions) → T17→T18
- T112+T113+T2/T3/T4 (all touch pipeline.rs) → serialize
- T105a + T111 (both touch codegen-rust source) → T105a MUST complete before T111

#### Rolling Dispatch Algorithm
1. Collect READY tasks (wave active + deps satisfied)
2. Filter out tasks conflicting with CURRENTLY RUNNING tasks
3. Pick TOP 3-6 by priority: critical path → deep/long → quick → writing
4. Dispatch; refill slots as tasks complete (don't wait for wave boundary)

### Track ↔ Version Mapping
- **Track A** → v1.MINOR per optimization (linker, debuginfo, Cranelift, salsa, multi-crate, sccache, cross-compile, profiler)
- **Track B** → v1.MINOR per feature (dynamic dispatch, --explain, data-locality, cost-model, profile-cache, fusion)
- **Track C** → v1.MINOR per crate port, gated on T13+T105, closed by T19
- **Track D** → v1.MINOR per coherent unit (v1.30 stdlib; v1.31 generics; v1.32 patterns)
- **Track E** → v1.25 time-bomb patch; v1.33 diag-quality
- **Track F** → v1.34 launch-infra (announces Buff publicly — v1.0 "Production" already shipped; this is the public launch)
- **Track G** → v1.MINOR per attribute
- **Track H** → v1.MINOR per tool
- **Track I** → v1.25-docs (audit); v1.26-codegen-hygiene; v1.27-broad-hygiene; v1.28-polish

### Dependency Notes
- **T13 in Wave 0** (highest-leverage; gates 8+ downstream)
- **T15/T16 BLOCKED BY T13 only** (NOT T105) → Wave 2a
- **T17/T18 BLOCKED BY T13 + T105b/T105a** (with fallback: ports proceed against original files if T105 delayed)
- **T105 SPLIT into T105a + T105b** (fully parallel, different crates)
- **T106/T107 DO NOT BLOCK anything** (T107 DEFERRABLE)
- **T19 in Wave 4** (separated from F1-F4)
- **T62 relaxed dep** (wraps v1.2 LSP; extends when T46 lands)
- **T20/T21 BLOCKED BY T110** (decision record)
- **Wave 2a = "all independent"** (zero intra-wave deps)
- **Wave 2b = "dependent layer"** (14 tasks consuming 2a outputs)

---

## TODOs

<!-- TASKS WILL BE APPENDED HERE VIA INCREMENTAL EDITS -->

- [ ] 1. P0 Error System — Multi-Span Diagnostics + Fix Suggestions + JSON Output (Wave 0, unspecified-high)

  **What to do**:
  - Extend `crates/buff-lang-error/src/diagnostic.rs`: add `MultiSpan` (primary Span + Vec<(Span, String)> secondary labeled spans) + `Subdiagnostic` (Note/Help/Label on specific spans). Add optional `labels: Vec<SpanLabel>` field to `Diagnostic` (defaults empty; backwards-compatible).
  - Add suggestion API: `Applicability` enum (MachineApplicable/MaybeIncorrect/HasPlaceholders/Unspecified) + `CodeSuggestion { span, replacement, applicability }` + `Diagnostic::with_suggestion(...)`.
  - Add `--error-format json`: serializable Diagnostic (spans as byte offsets + line/col, labels, suggestions, code). Wire through `buff-lang-cli`.
  - Update LSP to consume suggestions as CodeActions with applicability.

  **Must NOT do**: Do NOT renumber existing ErrorCodes. Do NOT add deps to buff-lang-error beyond minimal serde. Do NOT change default rendered output of existing single-span diagnostics.

  **Recommended Agent Profile**: `unspecified-high` — cross-crate (error + cli + lsp), API-design-heavy.
  **Skills**: none.

  **Parallelization**: YES (Wave 0). Blocks: improves later diagnostics but none hard-blocked. Blocked By: None. **Unsafe with**: T43+T44 (both touch diagnostic.rs — serialize).

  **References**: `crates/buff-lang-error/src/diagnostic.rs:36-45` (Diagnostic struct); `crates/buff-lang-error/src/code.rs` (ErrorCode STABLE FOREVER); `crates/buff-lang-error/src/source_map.rs` (line/col for JSON); `crates/buff-lang-error/AGENTS.md` (LEAF-crate rules).

  **Acceptance Criteria**: Multi-span renders primary + secondary labels; suggestion renders fix-it block; JSON has code/spans/suggestions. `cargo test -p buff-lang-error` PASS.

  **QA Scenarios**:
  ```
  Scenario: Multi-span diagnostic renders
    Tool: Bash (cargo run)
    Steps: cargo run -p buff-lang-cli -- check tests/invalid/<multi_span>.buff; assert stderr has primary + secondary labels
    Expected Result: rustc-style render with both spans
    Evidence: .sisyphus/evidence/task-1-multispan.txt

  Scenario: JSON error format
    Tool: Bash (cargo run + jq)
    Steps: cargo run -p buff-lang-cli -- check --error-format json tests/invalid/<type_err>.buff | jq '.code, .span, .suggestions'
    Expected Result: valid JSON with all fields
    Evidence: .sisyphus/evidence/task-1-json.json
  ```

  **Commit**: YES — `feat(buff-lang-error): multi-span diagnostics, fix suggestions, JSON output`

- [ ] 2. Track A — Fast-Linker Defaults (Wave 0, quick)

  **What to do**: In `crates/buff-lang-cli/src/pipeline.rs`, default rustc to `rust-lld` on x86_64-linux, prefer `mold` when detected. Add `--linker={auto,mold,lld,system}`. Probe PATH; fall back gracefully. Mirror in `crates/buff-eval/src/lib.rs`.

  **Must NOT do**: Do NOT hard-require mold. Do NOT introduce cc-rs/C dep. Do NOT break Windows.

  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). **Unsafe with**: T3/T4/T112/T113 (all touch pipeline.rs — serialize).

  **References**: `crates/buff-lang-cli/src/pipeline.rs` (`compile_rust_to_exe`); `crates/buff-eval/src/lib.rs` (duplicated path).

  **Acceptance Criteria**: `--linker=auto` resolves to available linker; `--linker=mold` errors clearly only if requested and absent. `cargo test -p buff-lang-cli` PASS.

  **QA Scenarios**:
  ```
  Scenario: Build with fast linker
    Tool: Bash
    Steps: cargo run -p buff-lang-cli -- run --linker=auto examples/fibonacci.buff; assert stdout=="55"
    Expected Result: correct output; auto-linker ≤ system-linker time
    Evidence: .sisyphus/evidence/task-2-linker-bench.txt
  ```

  **Commit**: YES — `perf(buff-lang-cli): default to fast linker (rust-lld/mold) with graceful fallback`

- [ ] 3. Track A — Dev-Build Debuginfo Control (Wave 0, quick)

  **What to do**: Add `--debuginfo={line-tables-only,full,none}` (default `line-tables-only` for dev). Plumb to rustc as `-C debuginfo=1/2/0`.

  **Must NOT do**: Do NOT strip debuginfo for release builds by default.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). **Unsafe with**: T2/T4/T112/T113 (pipeline.rs).

  **References**: `crates/buff-lang-cli/src/pipeline.rs` (add `-C debuginfo=N`).
  **Acceptance Criteria**: Flag maps to correct rustc arg. `cargo test -p buff-lang-cli` PASS.

  **QA Scenarios**:
  ```
  Scenario: Default dev build uses line-tables-only
    Tool: Bash
    Steps: cargo run -p buff-lang-cli -- run examples/fibonacci.buff; assert stdout=="55"; run with --debuginfo=full, still "55"
    Expected Result: both produce "55"
    Evidence: .sisyphus/evidence/task-3-debuginfo.txt
  ```

  **Commit**: YES — `perf(buff-lang-cli): --debuginfo control, default line-tables-only for dev`

- [ ] 4. Track A — Cranelift Dev Backend Flag (Wave 0, unspecified-high)

  **What to do**: Add `--backend=cranelift` (dev only) setting `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift`. Detect toolchain; fall back to LLVM with clear note. Release always LLVM.

  **Must NOT do**: Do NOT enable for release. Do NOT make correctness depend on backend.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 0). **Unsafe with**: T2/T3/T112/T113 (pipeline.rs).

  **References**: `crates/buff-lang-cli/src/pipeline.rs`; `crates/buff-eval/src/lib.rs`.
  **Acceptance Criteria**: Cranelift output behaviorally identical to LLVM. `cargo test -p buff-lang-cli` PASS.

  **QA Scenarios**:
  ```
  Scenario: Cranelift dev build identical output
    Tool: Bash
    Steps: run examples/collections.buff with LLVM (A) and --backend=cranelift (B); assert A==B
    Expected Result: byte-identical stdout
    Evidence: .sisyphus/evidence/task-4-cranelift-parity.txt
  Scenario: Missing Cranelift falls back
    Steps: run --backend=cranelift on toolchain without it; assert graceful fallback
    Evidence: .sisyphus/evidence/task-4-fallback.txt
  ```

  **Commit**: YES — `perf(buff-lang-cli): optional Cranelift dev backend`

- [ ] 5. Track B — Dynamic Workload-Aware Dispatch (Wave 0, deep)

  **What to do**: Extend `crates/buff-lang-runtime/src/threshold.rs::decide()` to inspect actual runtime data size (element count) + GPU availability. Keep pure, deterministic, sub-microsecond. Preserve DispatchKind variant ordering.

  **Must NOT do**: Do NOT reorder DispatchKind. Do NOT break "GPU failure invisible; CPU fallback correct" guarantee.
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocks: informs T11. Blocked By: None.

  **References**: `crates/buff-lang-runtime/src/threshold.rs` (decide, SINGLE_THREAD_MAX=999, CPU_PARALLEL_MAX=50_000); `crates/buff-lang-runtime/src/hints.rs` (decide_with_prefer, PREFER_GPU_MIN_ELEMENTS=1024); `crates/buff-lang-runtime/src/dispatch.rs` (DispatchKind — do not reorder); `crates/buff-lang-runtime/src/mock_gpu.rs` (MockGpuBackend).

  **Acceptance Criteria**: Small input→SingleThread; large→GpuCompute when available; large+GPU unavailable→CpuParallel. `cargo test -p buff-lang-runtime` PASS.

  **QA Scenarios**:
  ```
  Scenario: Runtime data size drives dispatch
    Tool: Bash (cargo test with MockGpuBackend)
    Steps: cargo test -p buff-lang-runtime dynamic_dispatch; assert 500→SingleThread, 10_000→CpuParallel, 100_000+GPU→GpuCompute, 100_000-GPU→CpuParallel
    Expected Result: correct DispatchKind per size+availability
    Evidence: .sisyphus/evidence/task-5-dynamic-dispatch.txt
  ```

  **Commit**: YES — `feat(buff-lang-runtime): dynamic runtime workload-aware CPU/GPU dispatch`

- [ ] 6. Track B — `--explain` Dispatch Diagnostics (Wave 0, quick)

  **What to do**: Add `--explain` mode reporting WHY CPU vs GPU was chosen: element count, arithmetic intensity, data locality, GPU availability, DispatchKind, @prefer override. Zero-overhead when off.

  **Must NOT do**: Do NOT alter dispatch decision. Do NOT add overhead to default path.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.

  **References**: `crates/buff-lang-runtime/src/threshold.rs`+`hints.rs`; `crates/buff-lang-cli/src/cli.rs` (add --explain).
  **Acceptance Criteria**: Explain string contains DispatchKind + deciding factors. Default emits nothing. `cargo test -p buff-lang-runtime` PASS.

  **QA Scenarios**:
  ```
  Scenario: --explain reports factors
    Tool: Bash
    Steps: cargo run -p buff-lang-cli -- run --explain examples/collections.buff; assert line with DispatchKind + element count
    Expected Result: explain output present; program output unchanged
    Evidence: .sisyphus/evidence/task-6-explain.txt
  ```

  **Commit**: YES — `feat(buff-lang-runtime,cli): --explain dispatch decision diagnostics`

- [ ] 7. Track A — Salsa Incremental Buff Front-End (Wave 1, deep)

  **What to do**: Integrate `salsa` crate for incremental compilation. Wire salsa queries into the lex→parse→typecheck→codegen pipeline so unchanged input files skip re-processing. Target: incremental rebuild <5s on large projects.

  **Must NOT do**: Do NOT break the `compile_to_rust` vs `compile_rust_to_exe` split. Do NOT make correctness depend on salsa (same AST → same output with or without salsa).
  **Recommended Agent Profile**: `deep`. **Skills**: none (use context7 for salsa docs).
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/pipeline.rs`; salsa docs (https://github.com/salsa-rs/salsa).
  **Acceptance Criteria**: Incremental rebuild <5s on large project (baseline from T22). `cargo test --workspace` PASS.
  **QA**: Run `buff run` twice (no change → second run <5s); modify one file → only affected crate recompiles. Evidence: task-7-incremental.txt
  **Commit**: YES — `perf(buff-lang-cli): salsa incremental compilation front-end`

- [ ] 8. Track A — Multi-Crate Emission (Wave 1, unspecified-high)

  **What to do**: Emit one `.rs` file per Buff module instead of one monolithic file. Enables rustc parallelism + better incremental. Modify `crates/buff-lang-codegen-rust/src/` to produce `mod`-structured output.

  **Must NOT do**: Do NOT break single-file compilation (keep as fallback). Do NOT change codegen determinism.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (codegen entry); `crates/buff-lang-codegen-rust/src/lib.rs`.
  **Acceptance Criteria**: Multi-module Buff project emits multiple .rs files. `cargo test --workspace` PASS.
  **QA**: Build examples/modules/ → assert multiple .rs files generated. Evidence: task-8-multi-crate.txt
  **Commit**: YES — `feat(codegen-rust): multi-crate emission mode`

- [ ] 9. Track A — sccache Integration (Wave 1, quick)

  **What to do**: Add `--use-sccache` flag. When set, set `RUSTC_WRAPPER=sccache` when spawning rustc. Detect sccache on PATH; fall back gracefully.

  **Must NOT do**: Do NOT hard-require sccache. Do NOT cache across different buff flags.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1). **Unsafe with**: T2/T3/T4 (pipeline.rs).
  **References**: `crates/buff-lang-cli/src/pipeline.rs`; sccache docs.
  **Acceptance Criteria**: `--use-sccache` wraps rustc. `cargo test -p buff-lang-cli` PASS.
  **QA**: Run with and without sccache; assert identical output. Evidence: task-9-sccache.txt
  **Commit**: YES — `perf(buff-lang-cli): sccache integration`

- [ ] 10. Track B — Data-Locality-Aware Dispatch (Wave 1, deep)

  **What to do**: Extend runtime to track WHERE data currently lives (CPU RAM vs GPU VRAM). Chained GPU operations avoid redundant CPU↔GPU transfers. If data is already on GPU, prefer GPU dispatch even for smaller inputs.

  **Must NOT do**: Do NOT break "GPU failure invisible" guarantee. Do NOT add non-determinism.
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Complements T5/T11. Blocked By: None.
  **References**: `crates/buff-lang-runtime/src/{threshold,hints,dispatch,gpu,gpu_pipeline}.rs`.
  **Acceptance Criteria**: Chained GPU ops avoid redundant transfers. `cargo test -p buff-lang-runtime` PASS.
  **QA**: Benchmark chained .map().filter() on large data; assert GPU-stays-GPU path avoids transfer. Evidence: task-10-locality.txt
  **Commit**: YES — `feat(buff-lang-runtime): data-locality-aware CPU/GPU dispatch`

- [ ] 11. Track B — Refined Cost Model (Wave 1, deep)

  **What to do**: Replace arithmetic-intensity-only threshold with multi-factor cost model: transfer time + launch overhead + occupancy + arithmetic intensity. `decide()` takes richer inputs.

  **Must NOT do**: Do NOT make cost model non-deterministic. Do NOT reorder DispatchKind.
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Complements T5/T10. Blocked By: None.
  **References**: `crates/buff-lang-runtime/src/threshold.rs`.
  **Acceptance Criteria**: Cost model picks GPU for large+intensive, CPU for small+memory-bound. Tests PASS.
  **QA**: Run benchmarks showing cost model outperforms old threshold. Evidence: task-11-cost-model.txt
  **Commit**: YES — `feat(buff-lang-runtime): refined multi-factor cost model`

- [ ] 12. Track B — Profile Cache (Wave 1, unspecified-high)

  **What to do**: Cache dispatch decisions per (function signature + input shape profile). On subsequent calls with same shape, skip cost-model evaluation. Store as BTreeMap (deterministic).

  **Must NOT do**: Do NOT use HashMap (non-deterministic). Do NOT persist cache across runs (in-memory only for v1.25).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-runtime/src/{cold_start,threshold}.rs`.
  **Acceptance Criteria**: Repeated calls with same shape skip cost model. Tests PASS.
  **QA**: Benchmark: 1000 repeated calls → cached path < uncached. Evidence: task-12-profile-cache.txt
  **Commit**: YES — `feat(buff-lang-runtime): dispatch profile cache`

- [ ] 13. Track C — Generics + Monomorphization (Wave 0, deep) **CRITICAL PATH — gates all self-host**

  **What to do**: Add generics support: type parameters on functions/structs/enums. AST: extend `crates/buff-lang-ast/src/decl.rs` generic param lists. Types: extend `crates/buff-lang-types/src/infer.rs` to resolve `TypeRef::Generic` for user-defined types. Codegen: emit Rust generics `<T>` directly (monomorphization via rustc). This is THE foundation for Track C self-host ports — without generics, the compiler can't be expressed in Buff.

  **Must NOT do**: Do NOT implement generic bounds in this task (that's T38). Do NOT break built-in generic resolution (Vector/Map/Option/Result).
  **Recommended Agent Profile**: `deep` — cross-crate (AST + types + codegen), language-semantics-heavy.
  **Skills**: none.
  **Parallelization**: YES (Wave 0 — MOVED from Wave 1 per optimization). Blocks: T15-T18 (ports), T37 (user generics), T38 (bounds). Blocked By: None.

  **References**: `crates/buff-lang-ast/src/decl.rs:263-321` (struct+enum generic param field); `crates/buff-lang-types/src/infer.rs:856-933` (typeref_to_type — currently only built-ins); `crates/buff-lang-types/src/ty.rs` (Type enum + TypeRef::Generic); `crates/buff-lang-codegen-rust/src/` (generic type lowering).

  **Acceptance Criteria**: `struct Pair<T, U> { x: T, y: U }` compiles + instantiates `Pair<Int, String>`. `func id<T>(x: T) -> T { return x }` works. `cargo test --workspace` PASS.

  **QA Scenarios**:
  ```
  Scenario: Generic function works end-to-end
    Tool: Bash
    Steps: write .buff with func id<T>(x: T) -> T { return x }; print(id(42)); print(id("hello"))
    Assert: output "42\nhello"
    Expected Result: generics compile and run
    Evidence: .sisyphus/evidence/task-13-generics.txt
  ```

  **Commit**: YES — `feat(ast,types,codegen): generics + monomorphization foundation`

- [ ] 14. Track B — Dataflow-Graph IR + Simple Element-wise Fusion (Wave 2a, deep)

  **What to do**: Build a dataflow graph IR in the runtime that detects element-wise operation chains (`.map().map().filter()`) and fuses them into a single GPU kernel pass. Avoid intermediate buffer allocations.

  **Must NOT do**: Do NOT implement complex fusion (stencil/reduction fusion — post-launch). Do NOT break non-fused path (fusion is optimization, not correctness).
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-runtime/src/{gpu_pipeline,tiling,gpu}.rs`; `crates/buff-lang-codegen-wgsl/src/`.
  **Acceptance Criteria**: `.map(f).map(g)` fused into single kernel. `cargo test -p buff-lang-runtime` PASS.
  **QA**: Benchmark fused vs non-fused on 100K elements; assert fused is faster. Evidence: task-14-fusion.txt
  **Commit**: YES — `feat(buff-lang-runtime): dataflow-graph IR + element-wise kernel fusion`

- [ ] 22. Benchmark Harness + Baseline Capture (Wave 0, deep) **DO FIRST — feeds F3 + T105**

  **What to do**: Build a benchmark harness that captures compile-time and runtime metrics for representative Buff programs. This is the "before" baseline that F3 (Final Verification) uses to prove "after" improvements, AND that T105 uses for codegen-hash comparison.

  **Steps**:
  - Create `crates/buff-lang-cli/src/bench_harness.rs` (or `bench/` module).
  - Measure: time-to-first-error, clean build time, incremental build time, binary size, runtime dispatch decisions.
  - Capture codegen output hashes for representative fixtures (ola, fibonacci, closures, collections, pattern_matching, error_handling).
  - Store baseline as `.sisyphus/evidence/baseline-v1.25.json` (or similar).
  - Use `hyperfine` where available for wall-clock timing.

  **Must NOT do**: Do NOT optimize anything in this task — it's measurement only.
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 0 — FIRST ACTION). Blocks: T105a/b (hash comparison), F3 (before/after). Blocked By: None.

  **References**: `examples/{ola,fibonacci,closures,collections,pattern_matching,error_handling}.buff`; `crates/buff-lang-cli/src/pipeline.rs`.
  **Acceptance Criteria**: Baseline file exists with compile times + codegen hashes. `cargo test --workspace` PASS.
  **QA**: Run harness → assert baseline JSON exists with all fixture hashes. Evidence: task-22-baseline.txt
  **Commit**: YES — `feat(cli): benchmark harness + v1.25 baseline capture (T22)`

- [ ] 15. Track C — Port buff-lang-lexer → Buff (Wave 2a, deep) ← BLOCKED BY T13

  **What to do**: Rewrite `crates/buff-lang-lexer/src/{lexer,token,string_interp}.rs` (combined ~2,044 LOC) in Buff. The ported lexer must produce byte-identical token streams. Uses or-patterns (T39), pattern guards (T40) in match arms for token classification. Still emits Rust (the Buff-written lexer transpiles to Rust that is functionally identical).

  **Must NOT do**: Do NOT change token classification or offside-rule semantics. Do NOT break existing lexer tests.
  **Recommended Agent Profile**: `deep` — cross-language port, correctness-critical.
  **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocks: T19 (bootstrap). Blocked By: T13 (generics). **NOT blocked by T105** (lexer source is reasonably sized).
  **References**: `crates/buff-lang-lexer/src/{lexer.rs(1412),token.rs(379),string_interp.rs(253)}`; `crates/buff-lang-lexer/tests/`.
  **Acceptance Criteria**: Buff-written lexer produces identical tokens to Rust-written lexer on all test fixtures. `cargo test --workspace` PASS.
  **QA**: Run both lexers on all `tests/` fixtures; assert byte-identical token streams. Evidence: task-15-lexer-port.txt
  **Commit**: YES — `feat(self-host): port buff-lang-lexer to Buff (T15)`

- [ ] 16. Track C — Port buff-lang-parser → Buff (Wave 2a, deep) ← BLOCKED BY T13

  **What to do**: Rewrite `crates/buff-lang-parser/src/{parser,stmt,expr,stream}.rs` (combined ~5,428 LOC) in Buff. Uses or-patterns, pattern guards, complex patterns. Recursive-descent + Pratt parsing logic must be identical.

  **Must NOT do**: Do NOT change grammar or parse-tree structure. Do NOT break parser tests.
  **Recommended Agent Profile**: `deep`.
  **Skills**: none.
  **Parallelization**: YES (Wave 2a). **Unsafe with**: T15 (both touch Buff-side parser files — serialize T15→T16). Blocks: T19. Blocked By: T13.
  **References**: `crates/buff-lang-parser/src/{parser.rs(356),stmt.rs(3189),expr.rs(1700),stream.rs(472)}`.
  **Acceptance Criteria**: Buff-written parser produces identical AST. `cargo test --workspace` PASS.
  **QA**: Run both parsers on all fixtures; assert identical AST. Evidence: task-16-parser-port.txt
  **Commit**: YES — `feat(self-host): port buff-lang-parser to Buff (T16)`

- [ ] 17. Track C — Port buff-lang-types → Buff (Wave 3, deep) ← BLOCKED BY T13+T105b+T37+T38

  **What to do**: Rewrite `crates/buff-lang-types/src/` (type inference + 12-module analysis suite) in Buff. Uses generics (T37), bounds (T38), associated types (T75), cross-file resolution (T72). Needs T105b (prelude_types.rs split) to express the prelude cleanly.

  **Must NOT do**: Do NOT change type inference semantics. Do NOT break analysis passes.
  **Recommended Agent Profile**: `deep` — the most complex port (type system + analyses).
  **Skills**: none.
  **Parallelization**: YES (Wave 3). Blocks: T18, T19. Blocked By: T13, T105b, T37, T38. **Fallback**: if T105b delayed, proceed against original monolithic file with TODO marker.
  **References**: `crates/buff-lang-types/src/{infer,ty,prelude_types,prelude,exhaustiveness,ownership,async_analysis,recursion,modules,comptime,cross_file,range_analysis,project}.rs`.
  **Acceptance Criteria**: Buff-written types produces identical inference results. Bootstrap participates in T19 gate.
  **QA**: Run type-checker on all fixtures; assert identical results. Evidence: task-17-types-port.txt
  **Commit**: YES — `feat(self-host): port buff-lang-types to Buff (T17)`

- [ ] 18. Track C — Port buff-lang-codegen-rust → Buff (Wave 3, deep) ← BLOCKED BY T13+T105a+T17

  **What to do**: Rewrite `crates/buff-lang-codegen-rust/src/` (Rust code generation via syn/quote/prettyplease) in Buff. Uses dyn Trait for lowering-pass dispatch (T68), lazy iterators (T71). Needs T105a (rust_codegen.rs split) for clean module structure.

  **Must NOT do**: Do NOT change codegen output (byte-identical Rust must be produced). Do NOT break codegen determinism.
  **Recommended Agent Profile**: `deep` — the largest port (codegen is the most complex crate).
  **Skills**: none.
  **Parallelization**: YES (Wave 3 — after T17; T18 serializes after T17 within Wave 3 per Concurrency Policy). Blocks: T19. Blocked By: T13, T105a, T17 (types port — codegen depends on type definitions).
  **References**: `crates/buff-lang-codegen-rust/src/{rust_codegen,lib,format,context,atomic_analysis,race_analysis,gpu_alignment,move_analysis}.rs`.
  **Acceptance Criteria**: Buff-written codegen produces byte-identical Rust output. Bootstrap participates in T19 gate.
  **QA**: Compile all fixtures; assert byte-identical emitted Rust. Evidence: task-18-codegen-port.txt
  **Commit**: YES — `feat(self-host): port buff-lang-codegen-rust to Buff (T18)`

- [ ] 19. Track C — Bootstrap Determinism Gate (Wave 4, deep) ← BLOCKED BY T15-T18

  **What to do**: Build the bootstrap pipeline: Stage 1 (Rust-written compiler compiles Buff-written compiler) → Stage 2 (Buff-written compiler compiles itself) → Stage 3 (Buff-written compiler compiles itself again). Assert Stage 2 == Stage 3 byte-identical. This proves the self-hosting is deterministic.

  **Must NOT do**: Do NOT skip the Stage 2 == Stage 3 comparison. Do NOT fix non-determinism silently (report it).
  **Recommended Agent Profile**: `deep`.
  **Skills**: none.
  **Parallelization**: YES (Wave 4 — separated from F1-F4 per optimization). Blocked By: T15, T16, T17, T18 (all 4 ports must land first).
  **References**: All 4 ported crates; `crates/buff-lang-cli/src/pipeline.rs` (bootstrap script entry).
  **Acceptance Criteria**: Stage 2 == Stage 3 byte-identical on all 3 CI OSes.
  **QA**: Run bootstrap script; diff Stage 2 vs Stage 3; assert empty diff. Evidence: task-19-bootstrap.txt
  **Commit**: YES — `feat(self-host): bootstrap determinism gate (Stage 2 == Stage 3)`

- [ ] 20. Track F — Register v1.25 Direction Decision in Docs (Wave 1, writing) ← BLOCKED BY T110

  **What to do**: Ensure `.sisyphus/decisions/buff-direction-speed-moat-selfhost.md` (created by T110) is cross-referenced from README.md roadmap section, AGENTS.md (updated by T108), and the buff-v1x-frameworks.md plan. Update any stale "V2 MLIR" references.

  **Must NOT do**: Do NOT delete the old `buff-v2-mlir-selfhost.md` (preserve as historical record; mark as SUPERSEDED).
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: T110 (decision record must exist).
  **References**: `.sisyphus/decisions/buff-direction-speed-moat-selfhost.md`; `README.md`; `.sisyphus/plans/buff-v2-mlir-selfhost.md`.
  **Acceptance Criteria**: Decision record cross-linked from README + AGENTS.md. Old plan marked SUPERSEDED.
  **QA**: grep for "SUPERSEDED" in v2-mlir-selfhost.md; grep for decision record link in README. Evidence: task-20-docs-register.txt
  **Commit**: YES — `docs: register v1.25 direction decision + mark v2 plan superseded`

- [ ] 21. Track F — Delete Obsolete Drafts After Decision Registered (Wave 1, quick) ← BLOCKED BY T20

  **What to do**: Delete the 13 research draft dossiers in `.sisyphus/drafts/buff-v2-decision-*.md` + `buff-v2-plan-review.md` + `buff-v2-blueprint.md` + `buff-launch-readiness-audit.md` AFTER T20 confirms their rationale is preserved in the decision record (T110).

  **Must NOT do**: Do NOT delete drafts BEFORE T20 + T110 confirm preservation. Do NOT delete `buff-conventions.md` or any plan file.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: T20 (decision must be registered first).
  **References**: `.sisyphus/drafts/buff-v2-*.md`; `.sisyphus/decisions/buff-direction-speed-moat-selfhost.md`.
  **Acceptance Criteria**: Obsolete drafts deleted; decision record preserves all rationale.
  **QA**: `Get-ChildItem .sisyphus/drafts/buff-v2-*.md` returns empty. Evidence: task-21-drafts-deleted.txt
  **Commit**: YES — `chore: delete obsolete v2 research drafts (preserved in decision record)`

- [ ] 23. Track D — Json Module (Wave 1, unspecified-high) — CRITICAL stdlib gap

  **What to do**: Add Json parse/stringify/stringify_pretty to prelude. Register serde_json in workspace deps. Codegen-lower to serde_json::from_str/to_string. Add `JsonValue` type (Null/Bool/Int/Float/String/Array/Object).
  **Must NOT do**: Do NOT implement custom JSON parser (use serde_json). Do NOT add schema validation.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude_types.rs` (mirror Toml/Yaml pattern); `crates/buff-lang-codegen-rust/src/` (lowering); `Cargo.toml` (add serde_json).
  **Acceptance Criteria**: `Json.parse("{\"a\": 1}")` returns usable value; `Json.stringify(v)` produces valid JSON. Example json_demo.buff runs.
  **QA**: Run json round-trip; assert correct output. Evidence: task-23-json.txt
  **Commit**: YES — `feat(prelude): add Json module (serde_json)`

- [ ] 24. Track D — File I/O Module (Wave 1, unspecified-high) — CRITICAL stdlib gap

  **What to do**: Add File prelude type: `File.read(path) -> Result<String, Error>`, `File.write(path, content) -> Result<Void, Error>`, `File.append(path, content)`, `File.exists(path) -> Bool`, `File.list_dir(path) -> Vector<String>`. Lower to std::fs.
  **Must NOT do**: Do NOT implement async file I/O (post-launch). Do NOT add file watching (that's T97).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude_types.rs`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `File.read("test.txt")` returns content; `File.write(...)` creates file. Example file_io_demo.buff runs.
  **QA**: Write file → read it back → assert content matches. Evidence: task-24-file-io.txt
  **Commit**: YES — `feat(prelude): add File I/O module`

- [ ] 25. Track D — HTTP Client Module (Wave 1, unspecified-high) — CRITICAL stdlib gap

  **What to do**: Add HTTP prelude type: `HTTP.get(url) -> Result<HttpResponse, Error>`, `HTTP.post(url, body)`, `HTTP.put(url, body)`, `HTTP.delete(url)`. Add `HttpResponse` with `.status()`, `.body()`, `.header()`, `.json()`. Lower to reqwest (rustls-tls, NOT native-tls).
  **Must NOT do**: Do NOT use native-tls. Do NOT implement HTTP server (post-launch). Do NOT implement streaming.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None (T79 Json prelude is a bonus for `.json()` but not blocking).
  **References**: `crates/buff-lang-types/src/prelude_types.rs` (mirror TCP/WebSocket pattern); `Cargo.toml` (reqwest rustls-tls).
  **Acceptance Criteria**: `HTTP.get(url)` returns response with status + body. No native-tls in dep tree.
  **QA**: HTTP GET to mock server; assert status 200 + body. Evidence: task-25-http.txt
  **Commit**: YES — `feat(prelude): add HTTP client module (reqwest+rustls)`

- [ ] 26. Track D — Expand Test Assertions (Wave 1, quick)

  **What to do**: Add `assert_ne`, `assert_true`, `assert_false`, `assert_some`, `assert_none`, `assert_ok`, `assert_err` to prelude. Add `@test(parametric: [1,2,3])` for parametric tests. Lower each to corresponding Rust assert macro.
  **Must NOT do**: Do NOT introduce new test framework. Do NOT break existing `assert_eq`.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude.rs` (assert_eq pattern); `crates/buff-lang-ast/src/decl.rs:167-202` (@test attribute).
  **Acceptance Criteria**: All new assertions work in `@test` functions. Example passes.
  **QA**: Run test with all assertions; assert all pass. Evidence: task-26-assertions.txt
  **Commit**: YES — `feat(prelude): add assert_ne/true/false/some/none/ok/err + parametric tests`

- [ ] 27. Track D — Expand Collections (Wave 1, unspecified-high)

  **What to do**: Add BOTH BTree-backed (BTreeSet, BTreeMap — deterministic, O(log n)) AND Hash-backed (HashSet, HashMap — O(1)) families. Add Queue (VecDeque) + Stack. Document trade-off: BTree = deterministic iteration (compiler internals); Hash = fast lookup (user code, game engines).
  **Must NOT do**: Do NOT use HashMap/HashSet in COMPILER-INTERNAL crates (deterministic codegen rule stays). Do NOT break existing Vector/Map APIs. Do NOT allow bare `Set` (require explicit BTreeSet or HashSet).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-types/src/ty.rs` (add type variants); `crates/buff-lang-types/src/prelude_types.rs` (registry); `crates/buff-lang-codegen-rust/src/` (lower to std::collections::*).
  **Acceptance Criteria**: BTreeSet deterministic iteration (sorted); HashSet O(1) lookup; Queue FIFO; Stack LIFO.
  **QA**: BTreeSet {c,a,b} → iterates a,b,c. HashSet 100K elements faster lookup than BTreeSet. Evidence: task-27-collections.txt
  **Commit**: YES — `feat(types): add BTree+Hash collection families (BTreeSet/BTreeMap/HashSet/HashMap/Queue/Stack)`

- [ ] 28. Track E — Tech-Debt Time-Bomb Batch (Wave 0, quick) — 6 atomic commits, one delegated task

  > **Batch execution**: T28-T36 (skipping T31/T32/T35 which are in Wave 1) are executed as ONE delegated task with multiple atomic commits. Each ships as part of "v1.25-tech-debt-patch".

  **What to do** (6 fixes, one commit each):
  - **T28**: Edition-2024 `unsafe` wrap on `Env.set` — `std::env::set_var` is `unsafe` in Edition 2024. Wrap call sites with `unsafe { }`.
  - **T29**: Dioxus 0.7 caret→exact pin — change `dioxus = "0.7"` to `dioxus = "=0.7.X"` (exact version) to prevent semver-surprise from proc-macro internals.
  - **T30**: serde_yml exact pin — same rationale.
  - **T33**: Version-tier consistency — unify all 64 crate versions from {1.2.0, 1.0.0} to a single tier (or document 3 tiers: 1.2.0 core / 1.0.0 tooling / 0.1.0 experimental). T33 is the heavyweight of this batch (touches ALL Cargo.tomls).
  - **T34**: Workspace `[profile.release]` — add `lto = "thin"` + `opt-level = 3` for release builds.
  - **T36**: Migrate rand 0.8→0.9 — update `rand = "0.8"` to `rand = "0.9"`; fix breaking API changes. **NOTE**: This overrides AGENTS.md's "Conservative pin philosophy: rand 0.8 NOT 0.9" — the override rationale is that rand 0.9 is now stable and the conservative-pin rule was temporary. Update AGENTS.md (T108) to reflect this change.

  **Must NOT do**: Do NOT batch all 6 into one commit (one commit per fix — atomic rollback). Do NOT change functionality.
  **Recommended Agent Profile**: `quick` (one agent, sequential commits).
  **Skills**: none.
  **Parallelization**: YES (Wave 0 — batched as one delegation). Blocked By: None.
  **References**: `Cargo.toml` (root + 64 crate Cargo.tomls); `crates/buff-lang-codegen-rust/src/` (Env.set usage); AGENTS.md (pin philosophy).
  **Acceptance Criteria**: Each fix committed individually. `cargo test --workspace` PASS after each. `cargo clippy --workspace --all-targets -- -D warnings` clean.
  **QA**: After all 6: `grep 'unsafe.*set_var' crates/` finds wrapped calls; `grep '=0\.7' Cargo.toml` finds exact pin; `grep 'rand.*0\.9' Cargo.toml` finds migrated. Evidence: task-28-batch.txt
  **Commit**: YES (6 commits) — `fix(codegen): Edition-2024 unsafe wrap on Env.set (T28)` / `fix(deps): Dioxus 0.7 exact pin (T29)` / etc.

- [ ] 31. Track E — Cargo.lock for buff new Scaffolds (Wave 1, quick)

  **What to do**: `buff new <project>` should generate a `Cargo.lock` alongside the scaffolded project to ensure reproducible builds.
  **Must NOT do**: Do NOT commit Cargo.lock for the compiler workspace itself (only for scaffolds).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/scaffold.rs`.
  **Acceptance Criteria**: `buff new myapp` creates Cargo.lock in myapp/.
  **QA**: Run `buff new testapp`; assert Cargo.lock exists. Evidence: task-31-cargo-lock.txt
  **Commit**: YES — `fix(cli): generate Cargo.lock for buff new scaffolds`

- [ ] 32. Track E — codegen-buffhtml Comment-Drop Fix (Wave 1, quick)

  **What to do**: Fix `crates/buff-lang-codegen-buffhtml/src/lib.rs:525` where comments in `.buffhtml` templates are silently dropped during lowering. Preserve comments in generated RSX.
  **Must NOT do**: Do NOT change RSX semantics. Do NOT drop valid comments.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-codegen-buffhtml/src/lib.rs:522-528` (TODO at those lines).
  **Acceptance Criteria**: Comments in .buffhtml preserved in generated output.
  **QA**: Build .buffhtml with comments; assert comments present in output. Evidence: task-32-comment-drop.txt
  **Commit**: YES — `fix(codegen-buffhtml): preserve comments in RSX lowering`

- [ ] 35. Track E — Extract Shared rustc-invoke Helper (Wave 1, quick)

  **What to do**: The `compile_rust_to_exe` logic is duplicated between `crates/buff-lang-cli/src/pipeline.rs` and `crates/buff-eval/src/lib.rs`. Extract common parts into a shared module (NOT a new crate — keep within an existing crate to avoid workspace churn). **IMPORTANT**: `with_exe_extension` stays duplicated (AGENTS.md: intentional for dependency isolation — DO NOT EXTRACT).

  **Must NOT do**: Do NOT extract `with_exe_extension` (intentional duplication). Do NOT create a new crate. Do NOT pull clap+tokio into buff-eval.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/pipeline.rs`; `crates/buff-eval/src/lib.rs`; AGENTS.md ("Keep the two copies in sync manually").
  **Acceptance Criteria**: Shared helper extracted; both call sites use it. `with_exe_extension` stays duplicated.
  **QA**: `cargo test -p buff-lang-cli && cargo test -p buff-eval` both PASS. Evidence: task-35-rustc-helper.txt
  **Commit**: YES — `refactor: extract shared rustc-invoke helper (de-duplicate cli vs eval)`

- [ ] 37. Track D — User-Defined Generic Types (Wave 2a, deep) **CRITICAL** ← BLOCKED BY T13

  **What to do**: Type system today only resolves built-in generics (Vector/Map/Option/Result). Extend `crates/buff-lang-types/src/infer.rs:856-933` (typeref_to_type) to look up user-defined structs/enums and substitute type parameters. Extend exhaustiveness checking to user-defined generic enums.
  **Must NOT do**: Do NOT implement bounds (T38). Do NOT break built-in generic resolution.
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocks: T38. Blocked By: T13.
  **References**: `crates/buff-lang-types/src/infer.rs:856-933`; `crates/buff-lang-types/src/ty.rs`; `crates/buff-lang-types/src/exhaustiveness.rs:420-472`; `crates/buff-lang-ast/src/decl.rs:263-321`.
  **Acceptance Criteria**: `struct Pair<T,U> { x: T, y: U }` instantiates + type-checks. `enum Tree<T> { Leaf(T), Node(T) }` matches.
  **QA**: check examples/generics_demo.buff → no Unknown types. Evidence: task-37-user-generics.txt
  **Commit**: YES — `feat(types): resolve user-defined generic struct/enum types`

- [ ] 38. Track D — Generic Bounds / Traits (Wave 2a, deep) **CRITICAL** ← BLOCKED BY T13

  **What to do**: Add `bounds: Vec<TraitRef>` to `Param` AST (`crates/buff-lang-ast/src/common.rs` — Param struct is at ~line 84; verify exact location at execution time). Parse `<T: Eq + Hash, U: Clone>`. Enforce bounds in type inference (calling `fn f<T: Hash>(x: T)` with non-Hash type → E12xx error). Lower to Rust trait bounds. Pre-define traits: Eq, Hash, Clone, Default, Show.
  **Must NOT do**: Do NOT implement associated types (T75). Do NOT implement dyn Trait (T68).
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocks: T68, T75. Blocked By: T13.
  **References**: `crates/buff-lang-ast/src/common.rs` (Param struct at ~line 84 — verify at execution time); `crates/buff-lang-types/src/infer.rs`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `fn sort<T: Ord>(xs: Vector<T>)` compiles with Vector<Int>, fails E12xx with non-Ord type.
  **QA**: Test bound satisfied (compiles) + bound violated (E12xx error). Evidence: task-38-bounds.txt
  **Commit**: YES — `feat(types): generic bounds/trait constraints + Eq/Hash/Clone/Default/Show preludes`

- [ ] 39. Track D — Or-Patterns in Match (Wave 2a, quick)

  **What to do**: Add `Pattern::Or(Vec<Pattern>)` variant to `crates/buff-lang-ast/src/expr.rs:713-740`. Parse `A | B | C => ...` in match arms. Treat as union in exhaustiveness. Lower to Rust `pat | pat | pat`.
  **Must NOT do**: Do NOT change single-pattern match behavior.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-ast/src/expr.rs:713-740`; `crates/buff-lang-parser/src/expr.rs`; `crates/buff-lang-types/src/exhaustiveness.rs`.
  **Acceptance Criteria**: `Color.Red | Color.Blue => "primary"` matches both variants.
  **QA**: Match with or-pattern; assert correct matching. Evidence: task-39-or-patterns.txt
  **Commit**: YES — `feat(ast): add Pattern::Or for or-patterns`

- [ ] 40. Track D — Pattern Guards in Match (Wave 2a, unspecified-high)

  **What to do**: Add `Pattern::Guard(Box<Pattern>, Expr)` variant. Parse `Some(x) if x > 0 => ...`. Type-check guard as Bool with pattern bindings in scope. Lower to Rust `pat if guard`. Guards make match non-exhaustive unless catch-all.
  **Must NOT do**: Do NOT support guards in let-patterns (only match arms).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-ast/src/expr.rs`; `crates/buff-lang-parser/src/expr.rs`; `crates/buff-lang-types/src/infer.rs`.
  **Acceptance Criteria**: `Some(x) if x > 0 => "positive"` works.
  **QA**: Match with guard; assert guard filters arm. Evidence: task-40-guards.txt
  **Commit**: YES — `feat(ast): add Pattern::Guard (if guards in match)`

- [ ] 41. Track D — Struct Patterns in Match Arms (Wave 2a, quick)

  **What to do**: Parser already supports struct patterns in let-bindings (`crates/buff-lang-parser/src/stmt.rs` — struct pattern parsing is at ~line 974; verify at execution time). Extend match-arm parser (`crates/buff-lang-parser/src/expr.rs` — `parse_match` is at ~line 1133; verify at execution time) to accept `Point { x, y } => ...`.
  **Must NOT do**: Do NOT change let-pattern behavior.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-parser/src/stmt.rs` (struct pattern parsing at ~line 974 — verify); `crates/buff-lang-parser/src/expr.rs` (`parse_match` at ~line 1171 — verify).
  **Acceptance Criteria**: `Point { x, y } => x + y` works in match.
  **QA**: Struct pattern in match arm; assert destructuring works. Evidence: task-41-struct-patterns.txt
  **Commit**: YES — `feat(parser): allow struct patterns in match arms`

- [ ] 42. Track D — Complex Pattern Type Inference (Wave 2a, unspecified-high) ← BLOCKED BY T37

  **What to do**: Deep nested patterns bind sub-patterns as Unknown (`crates/buff-lang-types/src/infer.rs:747-749`). Extend to propagate types through nested patterns (tuple-of-struct, struct-with-tuple-fields).
  **Must NOT do**: Do NOT change parser behavior (parser is correct; inference is the gap).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: T37.
  **References**: `crates/buff-lang-types/src/infer.rs:747-749`; `crates/buff-lang-ast/src/expr.rs:713-853`.
  **Acceptance Criteria**: `(Point { x, y }, Color.Red) => ...` infers x: Int, y: Int.
  **QA**: Check fixture with nested pattern; assert no Unknown. Evidence: task-42-complex-patterns.txt
  **Commit**: YES — `fix(types): propagate types through nested patterns`

- [ ] 43. Track E — Color/ANSI Terminal Output (Wave 2a, quick) **CRITICAL**

  **What to do**: Add color rendering to `crates/buff-lang-error/src/diagnostic.rs:124-167` render(). Use anstyle crate or hand-emit ANSI. Detect TTY (`std::io::IsTerminal::is_terminal`); emit plaintext when not TTY. Add `--color={auto,always,never}`.
  **Must NOT do**: Do NOT hardcode color ON. Do NOT pull termcolor (heavy).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). **Unsafe with**: T1/T44 (diagnostic.rs — serialize T1→T43→T44). Blocked By: None.
  **References**: `crates/buff-lang-error/src/diagnostic.rs:124-167`; `crates/buff-lang-cli/src/cli.rs`.
  **Acceptance Criteria**: ANSI escapes present when TTY; plaintext when piped.
  **QA**: Run in TTY → assert ANSI escapes; pipe to cat → assert plaintext. Evidence: task-43-color.txt
  **Commit**: YES — `feat(error): colored diagnostic output (TTY-aware)`

- [ ] 44. Track E — ErrorCode Propagates to LSP Diagnostic (Wave 1, quick) **CRITICAL**

  **What to do**: In `crates/buff-lsp/src/handlers.rs:34-48` (diagnostic_to_lsp), set `code: Some(NumberOrString::String(code_str))` (e.g. "E1201"). Set `code_description` with clickable href to `https://buff-lang.org/errors/E1201`.
  **Must NOT do**: Do NOT change ErrorCode enum.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1 — MOVED from Wave 2a per optimization). Blocked By: None.
  **References**: `crates/buff-lsp/src/handlers.rs:34-48`; `crates/buff-lang-error/src/code.rs`.
  **Acceptance Criteria**: VSCode Problems panel shows E1201 with clickable link.
  **QA**: Open .buff with type error; assert Problems panel shows code + link. Evidence: task-44-lsp-errorcode.png
  **Commit**: YES — `feat(lsp): propagate ErrorCode + clickable doc link`

- [ ] 45. Track E — `--explain E1xxx` Flag (Wave 2a, quick) **CRITICAL**

  **What to do**: Add `buff check --explain E1201` that prints `ErrorCode::explanation()` text. Mirror `rustc --explain` UX.
  **Must NOT do**: Do NOT require a .buff file when --explain is used.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/check.rs`; `crates/buff-lang-error/src/code.rs` (explanation()).
  **Acceptance Criteria**: `buff check --explain E1001` prints explanation text.
  **QA**: Run --explain E1001; assert explanation + example + fix recipe. Evidence: task-45-explain.txt
  **Commit**: YES — `feat(cli): add --explain E1xxx flag`

- [ ] 46. Track E — LSP codeAction + codeLens + inlayHint + semanticTokens (Wave 2a, unspecified-high) **CRITICAL** ← BLOCKED BY T1+T44

  **What to do**: Register 4 new LSP capabilities in `crates/buff-lsp/src/handlers.rs` + `server.rs`: codeAction (quick fixes from suggestions), codeLens (Run|Test|Bench above func decls), inlayHint (inferred types on let bindings), semanticTokens (full semantic token provider).
  **Must NOT do**: Do NOT implement typeHierarchy or callHierarchy (defer).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: T1 + T44.
  **References**: `crates/buff-lsp/src/handlers.rs`; `crates/buff-lsp/src/server.rs:304-341`; lsp-types 0.97 docs.
  **Acceptance Criteria**: All 4 capabilities registered; VSCode shows actions, lenses, hints, tokens.
  **QA**: Open .buff with typo → lightbulb "Did you mean?"; let x = 5 → ghost text ": Int". Evidence: task-46-lsp-capabilities.png
  **Commit**: YES — `feat(lsp): add codeAction + codeLens + inlayHint + semanticTokens`

- [ ] 47. Track E — Allocate E14xx Runtime Error Codes (Wave 2a, quick)

  **What to do**: Add E14xx variants for RuntimeError: E1401 GpuUnavailable, E1402 GpuInitialization, E1403 NotImplemented, E1404 UnsupportedOperation. Add explanation text. Wire RuntimeError→Diagnostic to set ErrorCode.
  **Must NOT do**: Do NOT renumber E10xx-E13xx.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocks: T64. Blocked By: None.
  **References**: `crates/buff-lang-error/src/code.rs`; `crates/buff-lang-runtime/src/error.rs`.
  **Acceptance Criteria**: GPU-unavailable surfaces E1401 with explanation.
  **QA**: Run @prefer(gpu) fixture on CI without GPU; assert E1401. Evidence: task-47-runtime-codes.txt
  **Commit**: YES — `feat(error): allocate E14xx runtime error codes`

- [ ] 48. Track E — Allocate E15xx Warning/Lint Range (Wave 2a, unspecified-high)

  **What to do**: Add E15xx for opt-in lints: E1501 DeprecatedFunction, E1502 UnusedImport, E1503 UnusedVariable, E1504 UnreachableCode, E1505 NamingConventionViolation. Add Severity::Warning to Diagnostic. Add `buff check --warnings={all,none,deny}`.
  **Must NOT do**: Do NOT make E15xx a hard error.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocks: T69, T91. Blocked By: None.
  **References**: `crates/buff-lang-error/src/code.rs`; `crates/buff-lang-cli/src/naming_lint.rs`; `crates/buff-lang-cli/src/check.rs`.
  **Acceptance Criteria**: `buff check` emits E1502 for unused imports.
  **QA**: Check fixture with unused import; assert E1502 warning. Evidence: task-48-warning-range.txt
  **Commit**: YES — `feat(error): add E15xx warning/lint range + --warnings flag`

- [ ] 49. Track E — WGSL Errors Carry Source Span (Wave 1, quick)

  **What to do**: `crates/buff-lang-codegen-wgsl/src/error.rs` WgslError has no Span. Add `span: Span` to each variant. Propagate AST span at every WgslError construction site. Wrap as Diagnostic::error(msg, span) instead of Span::dummy().
  **Must NOT do**: Do NOT break WgslError message text.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1 — MOVED from Wave 2a per optimization). Blocked By: None.
  **References**: `crates/buff-lang-codegen-wgsl/src/error.rs`; `crates/buff-lang-codegen-wgsl/src/shader.rs`; `crates/buff-lang-error/src/span.rs`.
  **Acceptance Criteria**: WGSL error points to offending Buff expression, not byte 0.
  **QA**: Fixture with unsupported type in @prefer(gpu) lambda; assert non-zero span. Evidence: task-49-wgsl-span.txt
  **Commit**: YES — `fix(codegen-wgsl): propagate source Span through WgslError`

- [ ] 50. Track E — Runtime Errors Carry Source Span (Wave 1, quick)

  **What to do**: Mirror T49 for runtime: `crates/buff-lang-runtime/src/error.rs` RuntimeError loses span via From. Add span tracking. Simpler alternative for launch: embed file:line as String in panic-info-style (full span table post-launch).
  **Must NOT do**: Do NOT block launch on full span table (String-based file:line is acceptable).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1 — MOVED from Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-runtime/src/error.rs`; `crates/buff-lang-codegen-buffhtml/src/span_map.rs` (existing pattern).
  **Acceptance Criteria**: Runtime error surfaces with Buff file:line (not Span::dummy()).
  **QA**: Run fixture triggering GPU-unavailable; assert .buff filename + line in error. Evidence: task-50-runtime-span.txt
  **Commit**: YES — `fix(runtime): embed Buff source location in RuntimeError`

- [ ] 51. Track E — Comprehensive Invalid Test Fixture Coverage (Wave 2a, deep) ← BLOCKED BY T47+T48

  **What to do**: Today only 2 invalid fixtures exist. Add fixtures for EVERY ErrorCode category: E10xx (14 codes), E11xx (10 codes), E12xx (12 codes), E13xx (4 codes), E14xx (4 codes), E15xx (5 codes). Add insta snapshots. Consider error-annotation syntax `// error[E1201]: expected this`.
  **Must NOT do**: Do NOT skip any ErrorCode category.
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: T47, T48.
  **References**: `tests/fixtures/invalid/`; `crates/buff-lang-error/tests/error_messages.rs`.
  **Acceptance Criteria**: ≥38 invalid fixtures + insta snapshots. `cargo test --workspace` PASS.
  **QA**: `cargo test --workspace`; assert all fixture tests pass. Evidence: task-51-fixtures.txt
  **Commit**: YES — `test: add comprehensive invalid fixtures for all ErrorCodes`

- [ ] 52. Track E — Missing Error Doc Pages (Wave 0, quick)

  **What to do**: Generate missing `docs/errors/E1210.html`, `E1211.html`, `E1212.html`, `E1304.html` via the gen_error_docs mechanism.
  **Must NOT do**: Do NOT change ErrorCode explanations.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: `docs/errors/`; `crates/buff-lang-error/src/code.rs` (explanation()).
  **Acceptance Criteria**: All ErrorCode doc pages exist.
  **QA**: `Test-Path docs/errors/E1210.html` → True. Evidence: task-52-error-docs.txt
  **Commit**: YES — `docs: add missing error code pages E1210/E1211/E1212/E1304`

- [ ] 53. Track E — Wire "Did You Mean?" Into Error Paths (Wave 2a, quick)

  **What to do**: `crates/buff-lang-error/src/diagnostic.rs:257-368` has levenshtein/suggest_close/format_did_you_mean — only used by tests. Wire into: parser undefined identifier, type checker unknown type, module system unknown import.
  **Must NOT do**: Do NOT change suggestion algorithm (Levenshtein works).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocks: T98. Blocked By: None.
  **References**: `crates/buff-lang-error/src/diagnostic.rs:257-368`; `crates/buff-lang-parser/src/`; `crates/buff-lang-types/src/infer.rs`.
  **Acceptance Criteria**: `pritn("hi")` produces "did you mean: print?".
  **QA**: Check fixture with typo; assert suggestion. Evidence: task-53-did-you-mean.txt
  **Commit**: YES — `feat(error): wire did-you-mean into parser + type-checker`

- [ ] 54. Track F — Buff 1.0 Compatibility Document (Wave 1, writing) **CRITICAL** ← BLOCKED BY T33

  **What to do**: Author `COMPATIBILITY.md` in repo root. Model on Go's go1compat. Sections: source-level compatibility promise; what IS covered (syntax, type system, prelude API, ErrorCode values, CLI names); what is NOT covered (generated Rust internals, performance); deprecation process; exceptions; 2.0.0 trigger.
  **Must NOT do**: Do NOT promise binary/ABI compatibility. Do NOT promise performance compatibility.
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 1 — MOVED from Wave 3 per optimization). Blocks: T55. Blocked By: T33.
  **References**: Go go1compat (https://go.dev/doc/go1compat).
  **Acceptance Criteria**: COMPATIBILITY.md exists, ≥6 sections, cross-linked from README.
  **QA**: `Test-Path COMPATIBILITY.md`; grep for required sections. Evidence: task-54-compat.md
  **Commit**: YES — `docs: add COMPATIBILITY.md (Go go1compat-style)`

- [ ] 55. Track F — "The Book" Tutorial (Wave 3, deep) **CRITICAL** ← BLOCKED BY T54+T23-T25

  **What to do**: Set up `book/` using mdbook. 9 chapters: Getting Started, Build a CLI, Build an API server, GPU compute, Build a UI app, Language reference, Stdlib reference, Error code handbook, Migration guides. Every code block compiles (`mdbook test`). Deploy to GitHub Pages.
  **Must NOT do**: Do NOT use Sphinx/Jekyll. Do NOT skip runnable examples.
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 3). Blocks: public launch. Blocked By: T54, T23-T25.
  **References**: Rust's The Book; mdbook (https://rust-lang.github.io/mdBook/).
  **Acceptance Criteria**: ≥9 chapters; every code block compiles; deployed; linked from README.
  **QA**: `mdbook test book/` → all pass; visit deployed URL. Evidence: task-55-book.png
  **Commit**: YES — `docs(book): add The Book (mdbook) with 9 chapters`

- [ ] 56. Track F — Real `buff doc` Implementation (Wave 2a, unspecified-high) **CRITICAL** ← BLOCKED BY T26

  **What to do**: Replace `buff doc` placeholder with rustdoc-quality generator. Parse `///` doc comments. Generate HTML/mdbook-compatible markdown for public API. Cross-reference types. Search index. `buff doc --open`.
  **Must NOT do**: Do NOT depend on rustdoc directly.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a — MOVED from Wave 3). Blocks: T102. Blocked By: T26.
  **References**: `crates/buff-lang-cli/src/commands/doc.rs`; `crates/buff-lang-types/src/prelude_types.rs` (API surface).
  **Acceptance Criteria**: `buff doc` generates HTML with signatures + doc comments + cross-refs.
  **QA**: Run `buff doc examples/mylib/`; assert index.html exists. Evidence: task-56-buff-doc.png
  **Commit**: YES — `feat(cli): implement buff doc (rustdoc-quality API docs)`

- [ ] 57. Track F — Production Package Registry (Wave 1, unspecified-high) **CRITICAL**

  **What to do**: Promote `crates/buff-registry/` from in-memory MVP to production: SQLite persistence (rusqlite, pure-Rust); GitHub OAuth (tower-http + axum-extra); scoped packages (@org/pkg); sparse HTTP index; tarball storage (S3-compatible via rusty-s3 or local FS); rate limiting; search endpoint; download stats. Deploy publicly.
  **Must NOT do**: Do NOT use Postgres/libpq (pure-Rust rule). Do NOT use Docker. Do NOT launch with open public registration — **scope to invite-only beta** (GitHub OAuth + allowlist). Open registration deferred to post-launch v1.35 with privacy policy.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 1 — MOVED from Wave 3). Blocks: public launch. Blocked By: None.
  **References**: `crates/buff-registry/src/{handlers,lib,storage}.rs`; `Cargo.toml` (axum 0.8).
  **Acceptance Criteria**: Registry deployed; `buff publish` works; `buff search` returns results; OAuth login works; invite-only enforced.
  **QA**: buff login → buff publish → buff search → buff add round-trip. Evidence: task-57-registry.txt
  **Commit**: YES — `feat(registry): production persistence + GitHub OAuth + scoped packages (invite-only beta)`

- [ ] 58. Track F — Prebuilt Binaries + Installer Channels (Wave 1, unspecified-high) **CRITICAL** ← BLOCKED BY T33+T34+T61

  **What to do**: GitHub Actions matrix producing 6 release binaries (linux-x64/arm64, macOS-x64/arm64, Windows-x64/arm64). Auto-publish on tag push. Installers: scoop (Windows), homebrew (macOS/Linux), winget (Windows), cargo install, curl|sh. `buffup` integration.
  **Must NOT do**: Do NOT require building from source. Do NOT ship debug builds.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 1 — MOVED from Wave 3). Blocks: public launch. Blocked By: T33, T34, T61.
  **References**: `.github/workflows/ci.yml`; `crates/buffup/`; `setup-buff` GitHub Action.
  **Acceptance Criteria**: 6 binaries on GitHub Releases; scoop/brew/cargo install work.
  **QA**: `scoop install buff` on Windows; `brew install buff-lang/tap/buff` on macOS. Evidence: task-58-install.txt
  **Commit**: YES — `feat(release): prebuilt binaries for 6 targets + scoop/brew/winget/cargo installers`

- [ ] 59. Track F — MEMORY_SAFETY.md Statement (Wave 0, writing)

  **What to do**: Author `MEMORY_SAFETY.md` in repo root. CISA-aligned memory safety statement: Buff inherits Rust's memory safety (no buffer overflows, use-after-free, null derefs, data races). Explain transpiler architecture. Compare to C/C++ memory unsafety.
  **Must NOT do**: Do NOT overstate Buff's safety beyond Rust's guarantees (be precise: Buff inherits Rust's safety for generated code; FFI/unsafe blocks are user's responsibility). Do NOT make performance/safety trade-off claims.
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: CISA memory safety guidance; Rust safety guarantees.
  **Acceptance Criteria**: MEMORY_SAFETY.md exists; cross-linked from README.
  **QA**: `Test-Path MEMORY_SAFETY.md`. Evidence: task-59-memory-safety.md
  **Commit**: YES — `docs: add MEMORY_SAFETY.md (CISA-aligned)`

- [ ] 60. Track F — Publish tree-sitter-buff (Wave 1, quick)

  **What to do**: Open PRs to nvim-treesitter, Helix, Zed, github-linguist. Add buff to parser lists. Verify auto-config picks up Buff.
  **Must NOT do**: Do NOT fork tree-sitter-buff. Do NOT add per-editor scanner workarounds.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `tree-sitter-buff/grammar.js`; `tree-sitter-buff/queries/{highlights,folds,indents,locals}.scm`.
  **Acceptance Criteria**: At least 2 of 5 PRs merged (nvim-treesitter + linguist highest-leverage).
  **QA**: `:TSInstall buff` in Neovim; GitHub shows "Buff" language label. Evidence: task-60-treesitter.png
  **Commit**: NO (cross-repo PRs; commit docs update only) — `docs(editors): document multi-editor support`

- [ ] 61. Track F — CI arm64 Matrix (Wave 0, quick)

  **What to do**: Update `.github/workflows/ci.yml` to add arm64 runners for linux/macos/windows (3 OSes × 2 arches = 6-target matrix).
  **Must NOT do**: Do NOT remove existing x86_64 runners.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocks: T58. Blocked By: None.
  **References**: `.github/workflows/ci.yml`.
  **Acceptance Criteria**: CI runs on 6 targets; all pass.
  **QA**: Check CI workflow runs; assert 6-target matrix. Evidence: task-61-arm64.txt
  **Commit**: YES — `ci: add arm64 matrix (linux/macos/windows)`

- [ ] 62. Track F — AI Tooling MCP Bridge (Wave 2a, unspecified-high)

  **What to do**: Create MCP bridge that exposes Buff LSP capabilities to AI tools (Claude Code, Cursor, etc.). Wraps existing buff-lsp via lsp-mcp compatible protocol. Extends automatically when T46 (new LSP capabilities) lands.
  **Must NOT do**: Do NOT implement custom AI features (just bridge to LSP).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a — MOVED from Wave 3; relaxed dep on T46 — wraps v1.2 LSP). Blocked By: None.
  **References**: `crates/buff-lsp/src/`; lsp-mcp docs.
  **Acceptance Criteria**: AI tool can connect to Buff project via MCP; gets diagnostics, hover, completion.
  **QA**: Connect Claude Code via MCP to Buff project; assert diagnostics flow. Evidence: task-62-mcp.txt
  **Commit**: YES — `feat(tools): AI-tooling MCP bridge (lsp-mcp compatible)`

- [ ] 63. Track F — cargo-audit + SBOM in CI (Wave 0, quick)

  **What to do**: Add `cargo audit` step to CI workflow. Generate SBOM (Software Bill of Materials) via `cargo cyclonedx` or similar. Fail CI on known vulnerabilities advisories.
  **Must NOT do**: Do NOT block CI on license advisories (only security).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: `.github/workflows/ci.yml`; cargo-audit; cargo-cyclonedx.
  **Acceptance Criteria**: `cargo audit` clean; SBOM artifact generated.
  **QA**: Check CI artifacts for SBOM. Evidence: task-63-cargo-audit.txt
  **Commit**: YES — `ci: add cargo-audit + SBOM generation`

- [ ] 64. Track G — `@prefer(cpu)` + `@force(gpu|cpu)` + `--cpu-only`/`--gpu-only` CLI (Wave 2b, unspecified-high) ← BLOCKED BY T47

  **What to do**: Add `@prefer(cpu)` attribute (hint to keep on CPU) and `@force(gpu|cpu)` attribute (hard requirement — raises E1401 if forced GPU unavailable). Add `--cpu-only`/`--gpu-only` CLI flags that override all attributes.
  **Must NOT do**: Do NOT change default dispatch behavior (graceful/ergonomic). Do NOT make @prefer break when hardware absent.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T47 (E1401 GpuUnavailable code).
  **References**: `crates/buff-lang-runtime/src/{threshold,hints}.rs`; `crates/buff-lang-cli/src/cli.rs`.
  **Acceptance Criteria**: `@prefer(cpu)` keeps >50k map on CPU; `@force(gpu)` raises E1401 on GPU-less machine; `--cpu-only` overrides all.
  **QA**: Test each attribute + CLI flag. Evidence: task-64-perf-control.txt
  **Commit**: YES — `feat(runtime,cli): @prefer(cpu)/@force(gpu|cpu) + --cpu-only/--gpu-only`

- [ ] 65. Track G — `@blocking` Attribute (Wave 2a, unspecified-high)

  **What to do**: Add `@blocking` attribute that opts a function out of async propagation. `@blocking func read_file()` runs synchronously despite Buff's auto-async. Lowers to `#[buff::blocking]` proc-macro or direct blocking call.
  **Must NOT do**: Do NOT make @blocking the default (async propagation stays default).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-ast/src/decl.rs:167-202` (attribute parsing); `crates/buff-lang-types/src/async_analysis.rs`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `@blocking func` runs sync despite calling from async context.
  **QA**: Test @blocking function in async context; assert synchronous execution. Evidence: task-65-blocking.txt
  **Commit**: YES — `feat(ast): @blocking attribute (opt out of async propagation)`

- [ ] 66. Track G — `@workgroup(N)` Attribute (Wave 2a, quick)

  **What to do**: Add `@workgroup(N)` attribute controlling WGSL workgroup size (default 64). `@workgroup(256) func gpu_kernel()` lowers to `@workgroup_size(256)` in WGSL.
  **Must NOT do**: Do NOT change default workgroup size (64 stays default).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-codegen-wgsl/src/`; `crates/buff-lang-runtime/src/gpu.rs`.
  **Acceptance Criteria**: `@workgroup(256)` emits correct WGSL `@workgroup_size(256)`.
  **QA**: Compile @workgroup(256) function; assert WGSL output. Evidence: task-66-workgroup.txt
  **Commit**: YES — `feat(codegen-wgsl): @workgroup(N) attribute`

- [ ] 67. Track G — `BUFF_FAIL_LOUD_GPU=1` Env Var (Wave 2a, quick)

  **What to do**: Add env var `BUFF_FAIL_LOUD_GPU=1` that, in debug mode, makes silent GPU→CPU fallbacks LOUD (raises E1401 instead of silently falling back). For debugging GPU dispatch issues.
  **Must NOT do**: Do NOT affect release builds (debug-mode only). Do NOT change default (silent fallback stays default).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-runtime/src/{hints,gpu}.rs`.
  **Acceptance Criteria**: `BUFF_FAIL_LOUD_GPU=1` + debug build + GPU unavailable → E1401 raised (not silent fallback).
  **QA**: Set env var; run GPU test; assert E1401. Evidence: task-67-fail-loud.txt
  **Commit**: YES — `feat(runtime): BUFF_FAIL_LOUD_GPU env var for debug-mode fail-loud`

- [ ] 68. Track G — Minimal `Box<dyn Trait>` Dynamic Dispatch (Wave 2b, unspecified-high) ← BLOCKED BY T38

  **What to do**: Add minimal trait-object support: `Box<dyn Drawable>` for runtime polymorphism. Lower to Rust `Box<dyn Trait>`. Enables heterogeneous collections of trait-implementing types.
  **Must NOT do**: Do NOT implement trait upcasting (post-launch). Do NOT implement `&dyn Trait` (only `Box<dyn>`).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T38 (generic bounds/traits).
  **References**: `crates/buff-lang-types/src/infer.rs`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `Box<dyn Drawable>` collection works; calling trait method dispatches correctly.
  **QA**: Create Vector<Box<dyn Drawable>>; call .draw() on each; assert polymorphic dispatch. Evidence: task-68-dyn-trait.txt
  **Commit**: YES — `feat(types): minimal Box<dyn Trait> dynamic dispatch`

- [ ] 69. Track G — `@no-alloc` Lint (Wave 2b, unspecified-high) ← BLOCKED BY T48

  **What to do**: Add `@no-alloc` attribute for hot-loop auditing. Functions marked `@no-alloc` are checked at compile time for heap allocations (Box::new, Vec::push, String format, clone of heap types). Emit E15xx warning if allocation detected.
  **Must NOT do**: Do NOT implement runtime allocation tracking (compile-time static analysis only).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T48 (E15xx warning range).
  **References**: `crates/buff-lang-types/src/`; `crates/buff-lang-cli/src/naming_lint.rs`.
  **Acceptance Criteria**: `@no-alloc func hot()` with Vec::new inside → E15xx warning.
  **QA**: Mark function @no-alloc with allocation; assert warning. Evidence: task-69-no-alloc.txt
  **Commit**: YES — `feat(lint): @no-alloc attribute for hot-loop allocation auditing`

- [ ] 70. Track G — `@pin(core:N)` + `@thread(priority:...)` (Wave 2a, unspecified-high)

  **What to do**: Add `@pin(core:N)` attribute (pins thread to CPU core N — lowers to `core_affinity` crate) and `@thread(priority:...)` attribute (sets thread priority — lowers to OS-specific APIs). For hard-realtime systems.
  **Must NOT do**: Do NOT implement on platforms that don't support it (fall back gracefully).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-runtime/src/`; core_affinity crate docs.
  **Acceptance Criteria**: `@pin(core:0)` pins to core 0; `@thread(priority: high)` sets high priority.
  **QA**: Test on Linux; verify CPU affinity via `/proc/self/status`. Evidence: task-70-realtime.txt
  **Commit**: YES — `feat(runtime): @pin(core:N) + @thread(priority:...) for real-time threads`

- [ ] 71. Track D — Lazy Iterators (Wave 2b, deep) ← BLOCKED BY T75

  **What to do**: Add lazy iterator chains: `.iter()`, `.map(f).filter(p).collect()` with zero-allocation chaining. New `Iterator<T>` trait with `type Item` (associated type from T75). Lower to Rust iterator chain.
  **Must NOT do**: Do NOT make iterators eager (must be lazy). Do NOT implement async iterators (post-launch).
  **Recommended Agent Profile**: `deep`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T75 (associated types — Iterator needs `type Item`).
  **References**: `crates/buff-lang-types/src/`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `[1,2,3].iter().map({x=>x*2}).filter({x=>x>2}).collect()` produces `[4,6]` with zero intermediate allocations.
  **QA**: Benchmark iterator chain; assert no intermediate Vec allocations. Evidence: task-71-lazy-iterators.txt
  **Commit**: YES — `feat(types): lazy iterators (zero-allocation chaining)`

- [ ] 72. Track D — Cross-File Type Resolution (Wave 2a, unspecified-high)

  **What to do**: Today Buff compiles one file at a time. Enable cross-file type resolution: `import { MyType } from "other.buff"` makes MyType's full type information available. Extends `crates/buff-lang-types/src/cross_file.rs` + `modules.rs`.
  **Must NOT do**: Do NOT implement incremental cross-file caching (post-launch).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocks: T15-T18 (ports need cross-file). Blocked By: None.
  **References**: `crates/buff-lang-types/src/{cross_file,modules}.rs`.
  **Acceptance Criteria**: Multi-file project with imports type-checks correctly across files.
  **QA**: Build examples/modules/ → assert types resolve across files. Evidence: task-72-cross-file.txt
  **Commit**: YES — `feat(types): cross-file type resolution for multi-file projects`

- [ ] 73. Track D — String Slicing + Iteration + Concatenation (Wave 2a, unspecified-high)

  **What to do**: Add String methods: `.chars() -> Iterator<Char>`, `.slice(start, end) -> String`, `.concat(other) -> String`, `.split(sep) -> Vector<String>`, `.trim() -> String`, `.replace(from, to) -> String`, `.contains(sub) -> Bool`.
  **Must NOT do**: Do NOT implement regex String methods (Regex is a separate prelude type).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude_types.rs`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: All string methods work correctly.
  **QA**: Test each method. Evidence: task-73-strings.txt
  **Commit**: YES — `feat(prelude): String slicing + iteration + concatenation`

- [ ] 74. Track D — Comptime Const Folding (Wave 2a, unspecified-high)

  **What to do**: Add `@comptime` blocks that execute at compile time: `@comptime let pi = 3.14159`. Constant folding: literal arithmetic + bindings + conditionals evaluated during compilation.
  **Must NOT do**: Do NOT implement full compile-time function execution (Zig-style — too complex for launch).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-types/src/comptime.rs`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `@comptime let x = 2 + 3` folds to `5` in generated Rust.
  **QA**: Compile comptime block; assert folded constant in output. Evidence: task-74-comptime.txt
  **Commit**: YES — `feat(types): comptime const folding (@comptime blocks)`

- [ ] 75. Track D — Associated Types on Traits (Wave 2b, unspecified-high) ← BLOCKED BY T38

  **What to do**: Add associated types to trait declarations: `trait Iterator { type Item; func next() -> Option<Self.Item> }`. Enables T71 (lazy iterators). Lower to Rust associated types.
  **Must NOT do**: Do NOT implement GATs (generic associated types — post-launch).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocks: T71. Blocked By: T38.
  **References**: `crates/buff-lang-ast/src/decl.rs` (trait declarations); `crates/buff-lang-types/src/`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `trait Container { type Item; func get() -> Self.Item }` compiles + instantiates.
  **QA**: Define trait with associated type; implement it; assert type resolution. Evidence: task-75-assoc-types.txt
  **Commit**: YES — `feat(types): associated types on traits`

- [ ] 76. Track G — `@inline` / `@no_inline` Attributes (Wave 2a, quick)

  **What to do**: Add `@inline` (lowers to `#[inline]`) and `@no_inline` (lowers to `#[inline(never)]`) attributes for inlining hints.
  **Must NOT do**: Do NOT implement cross-crate inlining control (post-launch).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `@inline func` emits `#[inline]` in generated Rust.
  **QA**: Compile @inline function; assert `#[inline]` in output. Evidence: task-76-inline.txt
  **Commit**: YES — `feat(codegen): @inline / @no_inline attributes`

- [ ] 77. Track G — Dead-Code Elimination Pass (Wave 2a, unspecified-high)

  **What to do**: Add `--dce` flag that skips unreferenced functions in generated Rust. Pre-pass over the AST: mark all `func` declarations reachable from `main` (or `@test`/`@bench`); skip unreachable ones in codegen.
  **Must NOT do**: Do NOT remove functions from the AST (observationally pure — same behavior, cleaner output). Do NOT implement cross-module DCE (single-file only for launch).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-codegen-rust/src/lib.rs`.
  **Acceptance Criteria**: Program with unused functions → generated Rust excludes them.
  **QA**: Compile with --dce; assert unreferenced functions absent. Evidence: task-77-dce.txt
  **Commit**: YES — `feat(codegen): dead-code elimination pass (--dce)`

- [ ] 78. Track G — Constant Propagation Pass (Wave 2a, unspecified-high)

  **What to do**: Add constant propagation for local let-bindings: `let x = 10; let y = x * x` → propagate x=10 to use sites. Reduces emitted Rust verbosity.
  **Must NOT do**: Do NOT implement interprocedural propagation. Do NOT change runtime semantics (observationally pure).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-codegen-rust/src/lib.rs`; `crates/buff-lang-types/src/infer.rs`.
  **Acceptance Criteria**: `let x = 10; print(x * x)` → emitted Rust contains folded constant or x=10 visible to rustc.
  **QA**: Compile with const-prop; assert folded output. Evidence: task-78-const-prop.txt
  **Commit**: YES — `feat(codegen): constant propagation pass (local let-binding folding)`

- [ ] 79. Track D — JSON Prelude Type (Wave 1, quick) — CRITICAL stdlib gap

  **What to do**: Add `Json` PreludeType to `prelude_types.rs`: `Json.parse(text) -> Result<Value, Error>` (serde_json::from_str), `Json.stringify(value) -> String`, `Json.stringify_pretty(value) -> String`. Add `JsonValue` type. Register serde_json in extern_crates. Delete `examples/extern_serde_json.buff` → rewrite as `examples/json.buff`.
  **Must NOT do**: Do NOT implement custom JSON parser. Do NOT add schema validation.
  **Recommended Agent Profile**: `quick` — follows Toml/Yaml/Csv pattern exactly.
  **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude_types.rs` (mirror Toml/Yaml); `crates/buff-lang-codegen-rust/src/` (add serde_json to extern_crates); `Cargo.toml`.
  **Acceptance Criteria**: `Json.parse("{\"a\": 1}")` returns usable value. `examples/json.buff` round-trips.
  **QA**: Run json.buff; assert parse + stringify. Evidence: task-79-json-roundtrip.txt
  **Commit**: YES — `feat(stdlib): add Json prelude type (serde_json lowering)`

- [ ] 80. Track D — HTTP Client Prelude Type (Wave 2b, unspecified-high) ← BLOCKED BY T79

  **What to do**: Add `Http` PreludeType: `Http.get(url)`, `Http.post(url, body)`, `Http.put/delete`, `Http.request(method, url, options)`. Add `HttpResponse` with `.status()`, `.body()`, `.header()`, `.json()` (depends T79). Add `HttpRequestOptions`. Lower to reqwest (rustls-tls).
  **Must NOT do**: Do NOT implement HTTP server. Do NOT use native-tls. Do NOT implement streaming.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T79 (for .json()).
  **References**: `crates/buff-lang-types/src/prelude_types.rs` (mirror TCP/WebSocket); reqwest docs.
  **Acceptance Criteria**: `Http.get("https://httpbin.org/get")` returns status 200 + body.
  **QA**: HTTP GET to mock server; assert 200 + body. Evidence: task-80-http-get.txt
  **Commit**: YES — `feat(stdlib): add Http client prelude type (reqwest lowering)`

- [ ] 81. Track D — String Format Specifiers (Wave 0, quick)

  **What to do**: Extend `crates/buff-lang-lexer/src/string_interp.rs` to capture format specifiers after `:` inside `{...}`. Syntax: `{expr:spec}` where spec = `.2` (decimals), `?` (debug), `>10` (pad), `x` (hex), `b` (binary), `o` (octal), `e` (scientific), `05` (zero-pad). Codegen: pass spec through to Rust `format!()` unchanged.
  **Must NOT do**: Do NOT invent new specifier syntax (use Rust's exactly).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: `crates/buff-lang-lexer/src/string_interp.rs`; Rust fmt docs.
  **Acceptance Criteria**: `"{pi:.2}"` → "3.14"; `"{obj:?}"` → debug repr; `"{n:>10}"` → right-pad.
  **QA**: Run format specifier examples. Evidence: task-81-fmt-spec.txt
  **Commit**: YES — `feat(lexer): string format specifiers ({x:.2}, {obj:?}, {n:>10})`

- [ ] 82. Track D — Map Indexing Syntax (Wave 0, quick)

  **What to do**: Add codegen lowering for `m[key]` on Map types: read → `m.get(k).unwrap_or_default()` (Buff hides panic pain); write `m[k] = v` → `m.insert(k, v)`.
  **Must NOT do**: Do NOT panic on missing keys (use unwrap_or_default). Do NOT add indexing to types where it doesn't make sense.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: `crates/buff-lang-codegen-rust/src/`; `crates/buff-lang-types/src/infer.rs`.
  **Acceptance Criteria**: `let m = {"a": 1}; print(m["a"])` → "1"; `m["missing"]` → default (0), not panic.
  **QA**: Map read + write + missing key. Evidence: task-82-map-index.txt
  **Commit**: YES — `feat(codegen): map indexing syntax m[key]`

- [ ] 83. Track D — Nested Collection Literals (Wave 0, quick)

  **What to do**: Fix type inference for nested collections: `[[1, 2], [3, 4]]` should infer `Vec<Vec<i64>>`, not flatten. Also nested maps + mixed nesting.
  **Must NOT do**: Do NOT change flat collection behavior. Do NOT add tuple-array confusion.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: `crates/buff-lang-types/src/infer.rs`; `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `let m = [[1,2],[3,4]]; print(m[1][0])` → "3".
  **QA**: Nested array + nested map + mixed. Evidence: task-83-nested-collections.txt
  **Commit**: YES — `fix(types): nested collection literal type inference`

- [ ] 84. Track D — Range Syntax (Wave 2a, unspecified-high)

  **What to do**: Add `RangeExpr { start, end, inclusive }` to AST. Tokenize `..` and `..=`. Parse `0..10` and `0..=10`. Infer `Range<Int>`. Lower to Rust `(0..10)`. Integrate with `for i in 0..10`. Add `Range<T>` prelude type.
  **Must NOT do**: Do NOT implement range as Vec (must be lazy). Do NOT add custom step syntax.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-ast/src/expr.rs`; `crates/buff-lang-parser/src/expr.rs`; Rust Range docs.
  **Acceptance Criteria**: `for i in 0..5` iterates 5 times; `0..=5` iterates 6 times; `(0..10).contains(5)` → true.
  **QA**: Range iteration examples. Evidence: task-84-range-syntax.txt
  **Commit**: YES — `feat(ast): range syntax a..b and a..=b (lazy Range type)`

- [ ] 85. Track E — Enum Variant Path Resolution Fix (Wave 0, quick) — BUG FIX

  **What to do**: FIX: user-defined enum variants emit as unqualified `Red` instead of `Color::Red`, causing Rust compile errors. Codegen needs to track enum definitions and qualify variant references.
  **Must NOT do**: Do NOT change how Option/Result variants are emitted (they're unqualified in Rust by design).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: `crates/buff-lang-codegen-rust/src/`; `crates/buff-lang-ast/src/decl.rs` (EnumDecl).
  **Acceptance Criteria**: `Color::Red` in generated Rust (not bare `Red`).
  **QA**: Define enum Color, construct + match; assert compiles. Evidence: task-85-enum-paths.txt
  **Commit**: YES — `fix(codegen): qualify user-defined enum variants (Color::Red not Red)`

- [ ] 86. Track E — Match Return Position Trailing Semicolon Fix (Wave 0, quick) — BUG FIX

  **What to do**: FIX: `return match n { ... }` emits trailing `;`, making type `()` instead of matched value. Detect return-position expressions and suppress `;`.
  **Must NOT do**: Do NOT suppress `;` on let bindings or standalone statements.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: `crates/buff-lang-codegen-rust/src/`.
  **Acceptance Criteria**: `return match n { 1 => "one", _ => "other" }` returns the string, not ().
  **QA**: Match in return position; assert correct value. Evidence: task-86-match-return.txt
  **Commit**: YES — `fix(codegen): match in return position (no trailing semicolon)`

- [ ] 87. Track D — CLI Flag Parsing Prelude Type (Wave 2a, unspecified-high)

  **What to do**: Add `Cli` PreludeType: `Cli.parse(spec) -> Result<CliArgs, Error>`. Add `CliSpec` with builder API: `.flag(name, desc)`, `.option(name, desc, default)`, `.positional(name, desc)`, `.subcommand(name, spec)`. Add `CliArgs`: `.flag(name) -> Bool`, `.option(name) -> String`, `.positional(i) -> String`, `.subcommand() -> Option<String>`, `.help() -> String`. Lower to clap 4.x.
  **Must NOT do**: Do NOT implement custom argv parsing (wrap clap). Do NOT add shell completion (post-launch).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude_types.rs`; `Cargo.toml` (add clap); clap docs.
  **Acceptance Criteria**: `--flag`, `--option=value`, positionals, subcommands, auto-`--help` all work.
  **QA**: Parse `--verbose --output=result.txt input.txt`; assert correct values. Evidence: task-87-cli-parse.txt
  **Commit**: YES — `feat(stdlib): Cli flag parser prelude type (clap lowering)`

- [ ] 88. Track D — Vector Algorithms: sort, binary_search, find (Wave 1, quick)

  **What to do**: Add methods to Vector: `.sort() -> Vector<T>`, `.sort_by(cmp)`, `.binary_search(item) -> Option<Int>`, `.find(pred) -> Option<T>`, `.contains(item) -> Bool`, `.reverse() -> Vector<T>`, `.unique() -> Vector<T>`. Return new vectors (functional style).
  **Must NOT do**: Do NOT implement in-place mutation. Do NOT add specialized sort algorithms.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude_types.rs`; Rust Vec methods.
  **Acceptance Criteria**: `[3,1,2].sort()` → `[1,2,3]`; `[1,2,3].binary_search(2)` → `Some(1)`.
  **QA**: Sort + binary_search + find examples. Evidence: task-88-sort.txt
  **Commit**: YES — `feat(stdlib): Vector sort/binary_search/find/contains`

- [ ] 89. Track D — Decimal Type (Wave 2a, unspecified-high)

  **What to do**: Add `Decimal` PreludeType: `Decimal.new(text)`, `Decimal.from_int(n)`, arithmetic operators, comparison, `.round(places)`, `.to_string()`, `.to_float()`. Lower to rust_decimal (128-bit, 28-29 significant digits).
  **Must NOT do**: Do NOT make Float literals default to Decimal. Do NOT add BigInt (post-launch).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude_types.rs`; rust_decimal docs.
  **Acceptance Criteria**: `Decimal.new("0.1") + Decimal.new("0.2") == Decimal.new("0.3")` → true.
  **QA**: Decimal precision test. Evidence: task-89-decimal.txt
  **Commit**: YES — `feat(stdlib): Decimal type for precise financial math (rust_decimal)`

- [ ] 90. Track D — Defer Statement (Wave 2a, unspecified-high)

  **What to do**: Add `DeferStmt { body: Block }` to AST. Parse `defer { cleanup() }`. Lower to `defer!` macro or scopeguard crate. LIFO order (last deferred runs first). Runs on scope exit including early return + error propagation.
  **Must NOT do**: Do NOT implement errdefer (Zig-specific). Do NOT allow defer to change return values.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-ast/src/stmt.rs`; scopeguard crate; Go/Zig defer docs.
  **Acceptance Criteria**: `defer { print("second") }; print("first")` → outputs "first\nsecond".
  **QA**: Defer runs on exit; LIFO order; runs on early return. Evidence: task-90-defer.txt
  **Commit**: YES — `feat(ast): defer statement (scope-exit cleanup, LIFO order)`

- [ ] 91. Track E — Stability Tiers (@stable/@unstable/@deprecated) (Wave 2b, unspecified-high) ← BLOCKED BY T48

  **What to do**: Add `@stable(since: "1.25")`, `@unstable(reason: "...", tracking: "T99")`, `@experimental`, `@deprecated(since: "...", replacement: "...")` attributes. Lint emits E15xx warning when @unstable/@deprecated API is used. Mark all existing prelude items @stable(since: "1.0"); new items @unstable.
  **Must NOT do**: Do NOT implement feature gates. Do NOT auto-remove deprecated APIs.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T48.
  **References**: `crates/buff-lang-ast/src/decl.rs:167-202`; `crates/buff-lang-cli/src/{naming_lint,check}.rs`.
  **Acceptance Criteria**: @unstable usage → warning; @deprecated → warning with replacement.
  **QA**: Mark function @unstable; call it; assert warning. Evidence: task-91-stability.txt
  **Commit**: YES — `feat(lint): stability tiers @stable/@unstable/@deprecated`

- [ ] 92. Track F — "Buff Editions" Mechanism Design (Wave 0, writing)

  **What to do**: PRIMARILY A DESIGN TASK. Create `.sisyphus/decisions/buff-editions-design.md`: what constitutes an Edition, how declared, what changes between editions, cross-edition deps, migration, stability. Add `--edition 2026` flag (default; all editions behave identically at launch — mechanism is forward-compat escape hatch).
  **Must NOT do**: Do NOT implement edition-specific behavior changes (mechanism exists but unused until future edition).
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 0 — MOVED from Wave 3). Blocked By: None.
  **References**: Rust Editions (https://doc.rust-lang.org/edition-guide/).
  **Acceptance Criteria**: Design doc exists; `--edition` flag accepted (no-op); Book chapter planned.
  **QA**: `buff build --edition 2026 file.buff` succeeds. Evidence: task-92-editions.txt
  **Commit**: YES — `feat(cli): --edition flag + design document`

- [ ] 93. Track D — Raw String Literals (Wave 2a, quick)

  **What to do**: Add `r"..."` raw strings to lexer. `r"no escape"`, `r#"contains "quotes""#`. Backslashes literal. Lower to Rust `r"..."` passthrough.
  **Must NOT do**: Do NOT add byte strings (`b"..."`). Do NOT add CStr (`c"..."`).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-lexer/src/lexer.rs`; Rust raw string docs.
  **Acceptance Criteria**: `r"\d+"` → string contains literal backslash-d.
  **QA**: Raw string with backslashes + quotes. Evidence: task-93-raw-strings.txt
  **Commit**: YES — `feat(lexer): raw string literals r"..." and r#"..."#`

- [ ] 94. Track D — Import Aliases and Wildcards (Wave 2a, unspecified-high)

  **What to do**: Extend import parsing: `import { Foo as Bar } from "mod"`, `import * from "mod"`, `import * as M from "mod"`. Handle aliased names in module resolution. Lower to Rust `use ... as ...` and `use ...::*`.
  **Must NOT do**: Do NOT implement conditional/re-export (`pub use`). Do NOT implement lazy imports.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-parser/src/stmt.rs`; `crates/buff-lang-types/src/modules.rs`.
  **Acceptance Criteria**: `import { Foo as Bar }` works; `import * from "mod"` brings symbols in scope.
  **QA**: Alias resolves; wildcard brings symbols. Evidence: task-94-import-aliases.txt
  **Commit**: YES — `feat(parser): import aliases (X as Y) and wildcards`

- [ ] 95. Track F — Formal Language Specification (Wave 1, writing)

  **What to do**: Create `docs/spec/` with 13 chapters: 01-lexical-structure, 02-syntax, 03-types, 04-expressions, 05-statements, 06-patterns, 07-modules, 08-async, 09-gpu, 10-prelude, 11-error-codes, 12-memory, 13-concurrency. Auto-generate prelude from prelude_types.rs and error codes from code.rs.
  **Must NOT do**: Do NOT attempt formal verification (Coq/Lean). Do NOT duplicate The Book (spec is reference, Book is tutorial).
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 1 — MOVED from Wave 3). Blocked By: None (but ideally after T85/T86 bug fixes + T13/T37/T38 generics).
  **References**: Zig spec, Rust reference, Go spec as format references.
  **Acceptance Criteria**: 13 .md files exist, each >500 lines. Prelude section auto-generated.
  **QA**: `Get-ChildItem docs/spec/*.md | Measure-Object` → 13. Evidence: task-95-formal-spec.txt
  **Commit**: YES — `docs: formal language specification (13 chapters)`

- [ ] 96. Track D — Tuple Destructuring in Function Params (Wave 2a, unspecified-high)

  **What to do**: Extend parameter parsing: `func f((x, y): (Int, Int))`. Extend Param AST to support pattern destructuring. Lower to Rust pattern params.
  **Must NOT do**: Do NOT implement struct destructuring in params (too niche). Do NOT change tuple value semantics.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-ast/src/decl.rs` (Param); `crates/buff-lang-parser/src/stmt.rs`.
  **Acceptance Criteria**: `func add((x, y): (Int, Int)) -> Int { return x + y }` works.
  **QA**: Call `add((3, 4))` → returns 7. Evidence: task-96-tuple-destructure.txt
  **Commit**: YES — `feat(parser): tuple destructuring in function parameters`

- [ ] 97. Track H — `buff watch` (Auto-Recompile on Save) (Wave 2a, unspecified-high)

  **What to do**: Add `buff watch <FILE|DIR> [--run] [--test] [--debounce <ms>]` subcommand. Watches .buff files, recompiles on change. Reuse `notify` crate (already used by ui_dev). Debounce rapid saves. Clear screen with `--clear`. Clean SIGINT.
  **Must NOT do**: Do NOT implement incremental compilation (that's T7). Do NOT implement live-reload for non-UI code.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocks: T101. Blocked By: None.
  **References**: `crates/buff-lang-cli/src/ui_dev/mod.rs` (existing file-watcher); `crates/buff-lang-cli/src/cli.rs`.
  **Acceptance Criteria**: `buff watch file.buff --run` recompiles + runs on save. Compile errors don't crash watcher.
  **QA**: Edit file during watch; assert recompile. Evidence: task-97-watch.txt
  **Commit**: YES — `feat(cli): buff watch — auto-recompile on save`

- [ ] 98. Track H — `buff fix` (Auto-Apply Suggestions) (Wave 2b, unspecified-high) ← BLOCKED BY T53

  **What to do**: Add `buff fix <FILE> [--apply] [--dry-run]`. Runs `buff check`, collects diagnostics with suggestions. `--dry-run` prints what WOULD change; `--apply` modifies files. Auto-fixable: naming conventions, import suggestions, trailing whitespace, missing trailing commas, tab→4-spaces.
  **Must NOT do**: Do NOT auto-fix semantic errors. Do NOT implement refactoring. Do NOT apply without --apply.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T53 (suggestions wired).
  **References**: `crates/buff-lang-cli/src/{check,naming_lint,fmt}.rs`; `cargo fix`/`clippy --fix` precedents.
  **Acceptance Criteria**: `buff fix file.buff --apply` fixes naming (CamelCase→snake_case).
  **QA**: Create `func MyFunc()`, fix --apply, assert `func my_func()`. Evidence: task-98-buff-fix.txt
  **Commit**: YES — `feat(cli): buff fix — auto-apply mechanical suggestions`

- [ ] 99. Track H — `buff bench` (User-Facing Benchmarking) (Wave 2a, unspecified-high)

  **What to do**: Add `buff bench <FILE> [--compare <baseline>]`. Discovers `@bench func` annotations. Runs each N times with warm-up. Reports min/max/mean/median/p99. JSON baseline for regression detection.
  **Must NOT do**: Do NOT implement flame-graph profiling (that's T111). Do NOT benchmark the compiler (that's T22).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/test_runner.rs` (mirror for bench); criterion-style stats.
  **Acceptance Criteria**: `@bench func fib_10() { ... }` runs and reports timing table.
  **QA**: Run bench; assert timing table output. Evidence: task-99-bench.txt
  **Commit**: YES — `feat(cli): buff bench — user-facing benchmarking with @bench`

- [ ] 100. Track H — Snapshot Testing for User Code (Wave 2a, unspecified-high)

  **What to do**: Add `assert_snapshot(name, actual)` to prelude. First run creates `.snap` file. Subsequent runs compare. Mismatch → FAIL with diff. `BUFF_UPDATE_SNAPSHOTS=1` updates all. File-based (insta convention).
  **Must NOT do**: Do NOT implement inline snapshots. Do NOT implement snapshot redaction.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude.rs`; `crates/buff-lang-cli/src/test_runner.rs`; insta crate.
  **Acceptance Criteria**: First run creates .snap; second run passes; change → fails with diff.
  **QA**: Write snapshot test; run twice; change output; assert fail. Evidence: task-100-snapshot.txt
  **Commit**: YES — `feat(testing): assert_snapshot() for user-code snapshot testing`

- [ ] 101. Track H — `buff test --watch` (Wave 2b, quick) ← BLOCKED BY T97

  **What to do**: Add `--watch` flag to `buff test`. Re-runs tests on file change. `--filter` persists across re-runs. `--clear` clears terminal. Reuses T97 file-watcher infrastructure.
  **Must NOT do**: Do NOT run ALL tests if --filter is set. Do NOT crash on compile errors.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T97.
  **References**: `crates/buff-lang-cli/src/test_runner.rs`; T97 infrastructure.
  **Acceptance Criteria**: `buff test --watch file.buff` re-runs tests on save.
  **QA**: Edit test during watch; assert re-run. Evidence: task-101-test-watch.txt
  **Commit**: YES — `feat(cli): buff test --watch`

- [ ] 102. Track H — `buff doc --serve` (Wave 3, quick) ← BLOCKED BY T56+T97

  **What to do**: Add `--serve [--port N] [--open]` to `buff doc`. Generates docs, starts HTTP server (axum), WebSocket live-reload on file change. `--open` opens browser.
  **Must NOT do**: Do NOT implement search (mdbook has it). Do NOT implement theming.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 3). Blocked By: T56, T97.
  **References**: `crates/buff-lang-cli/src/commands/doc.rs`; `crates/buff-lang-cli/src/ui_dev/mod.rs` (WebSocket pattern).
  **Acceptance Criteria**: `buff doc --serve` serves docs on port 8765; browser auto-refreshes.
  **QA**: curl localhost:8765 → HTML response. Evidence: task-102-doc-serve.txt
  **Commit**: YES — `feat(cli): buff doc --serve — live docs server`

- [ ] 103. Track H — `buff generate` (Code Scaffolding) (Wave 2a, unspecified-high)

  **What to do**: Add `buff generate <TEMPLATE> <NAME>`. Templates: struct, enum, trait, test, component (.buffhtml), handler (HTTP). Simple string substitution. `--dry-run` shows output. `--path` places file in directory.
  **Must NOT do**: Do NOT implement interactive prompts. Do NOT implement template inheritance.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/scaffold.rs`; Rails/Angular/Django precedents.
  **Acceptance Criteria**: `buff generate struct User` creates user.buff with struct skeleton.
  **QA**: Generate struct; assert file exists with skeleton. Evidence: task-103-generate.txt
  **Commit**: YES — `feat(cli): buff generate — code scaffolding`

- [ ] 104. Track I — Code-Hygiene Audit (Wave 0, deep) **PREREQUISITE for T105-T107**

  **What to do**: Produce structured inventory of best-practice violations across ALL 64 workspace crates. Output: `.sisyphus/audits/code-hygiene-v1.25.md`. Exactly 6 sections: God Classes, Duplication, Coupling, Idioms, Naming, Dead Code. Each finding: `- [P0|P1|P2] file:line — category — fix (≤1 sentence)`. Tag each `[SHIP-v1.25]` or `[DEFER]`. Cap: 50 P0/P1 findings in SHIP crates → saturation stop. Tag INTENTIONAL patterns: `with_exe_extension` duplication (DO NOT EXTRACT), TypeInferencer-in-codegen (INTENTIONAL).

  **Must NOT do**: Do NOT make code changes (doc only). Do NOT propose architecture changes/tooling swaps. Do NOT exceed 50-finding cap.
  **Recommended Agent Profile**: `deep`. **Skills**: [`audit-code-quality`, `audit-architecture`].
  **Parallelization**: YES (Wave 0). Blocks: T105a, T105b, T106, T107. Blocked By: None.
  **References**: `AGENTS.md` ANTI-PATTERNS + UNIQUE STYLES (known patterns NOT to re-flag); `Cargo.toml` (`members = ["crates/*"]`); known god classes: `rust_codegen.rs` (15.5K), `prelude_types.rs` (7.8K), `stmt.rs` (2.9K), `kernel.rs` (2.2K).
  **Acceptance Criteria**: File exists with 6 sections; each finding tagged; INTENTIONAL patterns marked; NO source modified.
  **QA**: Test-Path audit file; grep for 6 sections; grep for INTENTIONAL tags; git status shows only .md. Evidence: task-104-audit.txt
  **Commit**: YES — `docs(audit): code-hygiene inventory for v1.25 (T104)`

- [ ] 105. Track I — Tier-1 Must-Fix: Split King God Classes (Wave 1, deep) **CRITICAL PATH**

  > **SPLIT into T105a + T105b** for parallel execution. T105a = split `rust_codegen.rs` (buff-lang-codegen-rust); T105b = split `prelude_types.rs` (buff-lang-types). Both Wave 1, fully parallel.

  **What to do** (collectively for T105a + T105b):
  - **MECHANICAL EXTRACTION ONLY** — move existing fn/impl/mod blocks into new files. NO logic changes. NO new abstractions. NO new traits. NO signature changes.
  - **T105a: Split `rust_codegen.rs` (17,453 → target ≤10,000 LOC)**: LINE NUMBERS BELOW ARE APPROXIMATE — re-discover extraction boundaries at execution time by searching for `fn`/`impl`/section markers. The 4 named extractions below are ILLUSTRATIVE, NOT EXHAUSTIVE — the agent must discover and extract additional cleanly-separable blocks (e.g., `lower_*` families, prelude-call lowering, specific AST-node handlers) to reach the target. Named extractions: `syn_helpers.rs` (syn-construction helpers, ~900 LOC), `derive_attrs.rs` (attribute builders, ~210 LOC), `dependency_detection.rs` (AST-walking collectors, ~360 LOC), `extern_crate_detection.rs` (the `program_uses_*` family, ~3,400 LOC). PRESERVE exact BTreeSet population order (codegen determinism). Target relaxed from ≤5,000 to ≤10,000 after Metis verification showed named extractions sum to ~4,870, leaving ~12,583 — mechanical extraction of 7,500+ additional LOC without behavior change is the realistic scope.
  - **T105b: Split `prelude_types.rs` (9,317 → ≤2,000 LOC)**: LINE NUMBERS BELOW ARE APPROXIMATE — re-discover at execution time via `impl` block boundaries. Keep `pub enum PreludeType` (445 variants) + enum definitions. Extract `prelude_type_metadata.rs` (impl PreludeType, ~1,800 LOC), `prelude_assoc_fn_impl.rs` (~570 LOC), `prelude_assoc_const_impl.rs` (~1,360 LOC), `prelude_instance_fn_impl.rs` (~2,500 LOC).
  - **Pre-refactor baseline (FIRST ACTION)**: `git tag pre-hygiene-v1.25` + capture baseline-hashes.json (build N fixtures, hash emitted .rs).

  **Must NOT do**: Do NOT introduce new traits/types/abstractions/deps. Do NOT change signatures. Do NOT rename. Do NOT fix bugs (file separate tasks). Do NOT batch multiple extractions per commit. Do NOT change BTreeSet population order. Do NOT introduce HashMap in compiler-internal crates. Do NOT touch ErrorCode values. Do NOT use raw-string codegen.
  **Recommended Agent Profile**: `deep`. **Skills**: [`audit-refactoring`].
  **Parallelization**: YES (Wave 1). T105a + T105b fully parallel (different crates). Blocks: T17 (needs T105b), T18 (needs T105a). **Fallback**: if delayed, T17/T18 proceed against original files with TODO marker. Blocked By: T104, T22 (baseline hashes).

  **References**: `.sisyphus/audits/code-hygiene-v1.25.md` (T104); `crates/buff-lang-codegen-rust/src/{rust_codegen,lib}.rs`; `crates/buff-lang-types/src/{prelude_types,lib}.rs`; spike-validated extractable blocks (line numbers above).

  **Acceptance Criteria**: `rust_codegen.rs` ≤10,000 LOC (relaxed from ≤5K per Metis); `prelude_types.rs` ≤3,000 LOC (relaxed from ≤2K); all new files ≤2,000 LOC; `cargo test --workspace` PASS; `cargo clippy --workspace --all-targets -- -D warnings` clean; **codegen-hash diff = 0** (byte-identical output pre/post); zero unexpected insta snapshot churn; no new HashMap in compiler-internal crates; no new unwrap/expect (baseline captured by T104 audit, not hard-coded).

  **QA Scenarios**:
  ```
  Scenario: Codegen output byte-identical pre/post
    Tool: Bash
    Steps: For each baseline fixture, hash emitted .rs; compare to baseline-hashes.json; assert zero diffs
    Expected Result: mechanical extraction preserved codegen determinism
    Failure Indicators: any hash mismatch = REGRESSION
    Evidence: .sisyphus/evidence/task-105-codegen-hash-diff.txt

  Scenario: LOC targets met
    Steps: Measure rust_codegen.rs ≤5000; prelude_types.rs ≤2000; all new files ≤2000
    Evidence: .sisyphus/evidence/task-105-loc-reduction.txt

  Scenario: No new violations
    Steps: grep unwrap/expect count ≤ baseline (captured by T104 audit); grep HashMap in compiler-internal unchanged
    Evidence: .sisyphus/evidence/task-105-no-new-violations.txt
  ```

  **Commit**: YES (multiple — one per file extraction). Pattern: `refactor({crate}): extract {module} from {source} (T105a/b)`. Pre-commit EVERY commit: `cargo test -p {crate} && cargo clippy -p {crate} --all-targets -- -D warnings && cargo fmt --check`.

- [ ] 106. Track I — Tier-2 Should-Fix (Wave 2a, unspecified-high) ← BLOCKED BY T104

  **What to do**: Address P1 findings from audit. Caps: TOP 5 medium god classes (stmt.rs 2.9K, kernel.rs 2.2K, fmt.rs 1.8K, config.rs 1.7K, expr.rs 1.7K) each reduced >30% LOC; 3 accidental duplications extracted (NOT with_exe_extension); 50 idiom fixes (unwrap→ok_or_else in compiler-internal crates first).
  **Must NOT do**: Do NOT extract with_exe_extension (INTENTIONAL). Do NOT exceed caps. Do NOT touch files T105a/b are splitting. Do NOT introduce HashMap in compiler-internal.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: [`audit-refactoring`].
  **Parallelization**: YES (Wave 2a). Blocked By: T104. Coordinate with T105a/b completion (soft, not hard dep — no file overlap since T105 is Wave 1, T106 is Wave 2a).
  **References**: `.sisyphus/audits/code-hygiene-v1.25.md` sections 2/3/4.
  **Acceptance Criteria**: 5 files reduced >30%; 3 duplications extracted; unwrap count reduced ≥50 in buff-lang-*; codegen-hash diff = 0.
  **QA**: LOC reduction check + violation count check + determinism check. Evidence: task-106-tier2.txt
  **Commit**: YES — `refactor({crate}): split {file} by {concern} (T106)` per extraction.

- [ ] 107. Track I — Tier-3 Nice-to-Have (Wave 2a, quick) ← BLOCKED BY T104; DEFERRABLE

  **What to do**: P2 findings. Caps: 20 naming fixes (snake_case funcs, PascalCase types, missing derives); 10 dead-code removals. DEFERRABLE — no blocking edges; can ship post-launch.
  **Must NOT do**: Do NOT rename pub API items (breaking change). Do NOT remove ErrorCode variants (STABLE FOREVER). Do NOT touch test files.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: T104.
  **References**: Audit sections 5/6.
  **Acceptance Criteria**: 20 naming + 10 dead-code; cargo test PASS; codegen-hash diff = 0; no pub API changes.
  **QA**: cargo test + clippy + hash comparison. Evidence: task-107-tier3.txt
  **Commit**: YES — `style({crate}): naming fixes (T107)` / `chore({crate}): remove dead code (T107)`

- [ ] 108. Track F — Update AGENTS.md for 64-Crate Reality (Wave 0, writing) **LAUNCH CREDIBILITY BLOCKER**

  **What to do**: Update AGENTS.md "19-crate" → "64-crate". Update STRUCTURE tree. Distinguish: 19 launch-critical (buff-lang-*, buff-{lsp,eval,repl,jupyter,registry,playground-wasm,ui-dioxus}) vs ~41 framework/experimental (buff-tensor, buff-ecs, etc.) vs tooling (buffup, bufflings, buff-dap). Update WHERE TO LOOK, version tiers (3 tiers: 1.2.0/1.0.0/0.1.0). **LICENSE consistency check**: verify all 64 Cargo.toml `license` = `MIT OR Apache-2.0`. **CONTRIBUTING.md update**: same treatment if it references crate count.
  **Must NOT do**: Do NOT document framework APIs in detail (Book's job). Do NOT change code. Do NOT remove ANTI-PATTERNS/UNIQUE STYLES (still accurate).
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocked By: None.
  **References**: `AGENTS.md`; `CONTRIBUTING.md`; `Cargo.toml` (`members = ["crates/*"]`); `(Get-ChildItem crates -Directory).Count` = 64.
  **Acceptance Criteria**: AGENTS.md says "64-crate"; `Select-String '19-crate'` returns ZERO; all 64 licenses identical; CONTRIBUTING.md updated.
  **QA**: `Select-String '19-crate' AGENTS.md` → empty; `(Get-ChildItem crates -Directory).Count` matches. Evidence: task-108-agents-md.txt
  **Commit**: YES — `docs: update AGENTS.md for 64-crate workspace (T108)`

- [ ] 109. Track F — Release Process Runbook (Wave 1, writing) ← BLOCKED BY T33

  **What to do**: Create `docs/RELEASE.md`: pre-release checklist (F1-F4 approved, T19 bootstrap, tests green, CHANGELOG); tag→CI→artifacts; registry publish; installer channels (scoop/brew/winget/cargo); docs deploy; announcement; roll-back procedure; post-release metrics. Sign-off matrix.
  **Must NOT do**: Do NOT include actual version numbers. Do NOT automate announcement.
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 1). Blocked By: T33.
  **References**: `.github/workflows/ci.yml`; `crates/buffup/`; T57, T58.
  **Acceptance Criteria**: `docs/RELEASE.md` exists, ≥8 sections, roll-back documented.
  **QA**: Test-Path docs/RELEASE.md; grep for required sections. Evidence: task-109-release.txt
  **Commit**: YES — `docs: add release process runbook (T109)`

- [ ] 110. Track F — Decision Record Finalization (Wave 0, writing) **BLOCKS T20, T21**

  **What to do**: Create `.sisyphus/decisions/buff-direction-speed-moat-selfhost.md` (if not exists). Content: user's governing priorities (verbatim); 3 confirmed decisions (Direction / Self-host scope / Memory model); 10 Accept-With-Rationale items; research basis (13 dossiers); demoted items (MLIR, custom memory, dropping rustc).
  **Must NOT do**: Do NOT include implementation details (decision record, not spec). Do NOT contradict Must NOT Have guardrails.
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 0). Blocks: T20, T21. Blocked By: None.
  **References**: Plan Context section; `.sisyphus/drafts/buff-v2-decision-*.md`.
  **Acceptance Criteria**: File exists; contains 3 decisions + 10 accept-items.
  **QA**: Test-Path; grep for "Decision 1/2/3" + "Accept-With-Rationale". Evidence: task-110-decision.txt
  **Commit**: YES — `docs(decisions): finalize buff-direction-speed-moat-selfhost (T110)`

- [ ] 112. Track A — Cross-Compilation `--target` Flag (Wave 0, quick) **LAUNCH-CRITICAL**

  **What to do**: Add `--target <TRIPLE>` to buff build/run/check. Passthrough to rustc `--target`. If target not installed: error "rustup target add <triple>". Mirror in buff-eval/lib.rs. Document common targets.
  **Must NOT do**: Do NOT auto-install targets. Do NOT add custom toolchain. Do NOT break native compilation.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 0). **Unsafe with**: T2/T3/T4/T113 (all touch pipeline.rs). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/{pipeline,cli}.rs`; `crates/buff-eval/src/lib.rs`; Rust cross-compile docs.
  **Acceptance Criteria**: `buff build --target x86_64-unknown-linux-gnu examples/ola.buff` produces Linux binary; missing target → helpful error.
  **QA**: Cross-compile + missing-target error. Evidence: task-112-cross-compile.txt
  **Commit**: YES — `feat(cli): --target flag for cross-compilation (T112)`

- [ ] 111. Track A — `buff profile` CPU/Allocation Profiler (Wave 2a, unspecified-high) **LAUNCH-CRITICAL**

  **What to do**: Add `buff profile run <FILE> [--alloc] [--output <path>]`. CPU profiling via `pprof-rs` (SIGPROF sampling); generates `profile.flamegraph.svg` via `flamegraph` crate. Allocation profiling via `dhat`; generates `alloc-profile.txt`. Codegen injects profiler init/dump in `fn main()` when profiling mode active. Zero overhead when off. Works on Linux + macOS + Windows.
  **Must NOT do**: Do NOT profile the compiler (that's T22). Do NOT require specific OS. Do NOT add overhead when off. Do NOT implement live streaming profiling.
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). **Unsafe with**: T105a (both touch codegen-rust source — serialize). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/{cli,commands/profile}.rs`; `crates/buff-lang-codegen-rust/src/` (inject profiler); pprof-rs/flamegraph/dhat crate docs; Go pprof UX precedent.
  **Acceptance Criteria**: `buff profile run examples/fibonacci.buff` → `profile.flamegraph.svg` (non-empty, shows function timing); `--alloc` → allocation report; zero overhead when off.
  **QA**: Flamegraph generated + SVG non-empty + fibonacci function visible; alloc profile has data; timing comparison with/without profiling. Evidence: task-111-profile.svg
  **Commit**: YES — `feat(cli): buff profile — CPU/allocation profiler (T111)`

- [ ] 113. Track A — `buff run --detect-races` (Wave 2a, quick)

  **What to do**: Add `--detect-races` to buff run/build/test. Passes `-Zsanitizer=thread` to rustc (requires nightly). If stable toolchain: error "rustup override set nightly". Race detected → program aborts with ThreadSanizer report. Development-time tool (2-10x overhead).
  **Must NOT do**: Do NOT enable for release. Do NOT implement custom race detector. Do NOT require nightly by default.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). **Unsafe with**: T2/T3/T4/T112 (pipeline.rs). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/{pipeline,cli}.rs`; Rust ThreadSanizer docs; Go -race precedent.
  **Acceptance Criteria**: `buff run --detect-races` (nightly) runs with ThreadSanizer; stable → helpful error; no overhead without flag.
  **QA**: Race detector runs on nightly; stable toolchain error; no overhead without flag. Evidence: task-113-race.txt
  **Commit**: YES — `feat(cli): --detect-races flag (ThreadSanizer passthrough) (T113)`

- [ ] 114. Track D — `.env` File Loading (Wave 2a, quick)

  **What to do**: Add `Env.load(path: String = ".env") -> Map<String, String>` to prelude. Reads a `.env` file (KEY=VALUE per line), returns a Map. Also auto-loads `.env` on program start (like Bun/Deno/Node.js dotenv). Lower to `std::env` + simple file parse.
  **Must NOT do**: Do NOT support complex .env syntax (multiline, comments beyond `#`). Do NOT override existing env vars (only set if absent).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-types/src/prelude_types.rs` (Env type exists); Bun/Deno dotenv patterns.
  **Acceptance Criteria**: `.env` with `PORT=8080` → `Env.load()["PORT"]` == "8080".
  **QA**: Create .env, load it, assert value. Evidence: task-114-env-load.txt
  **Commit**: YES — `feat(prelude): Env.load() for .env file loading (T114)`

- [ ] 115. Track A — `buff expand` (Show Generated Rust) (Wave 2a, quick)

  **What to do**: Add `buff expand <FILE>` subcommand that shows the generated Rust source for a .buff file. Like `cargo expand` shows macro expansion. Outputs to stdout (or `--output file.rs`). Useful for debugging codegen issues and understanding what Buff generates.
  **Must NOT do**: Do NOT format the output differently from what rustc sees (raw prettyplease output). Do NOT add syntax highlighting (terminal handles that).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2a). Blocked By: None.
  **References**: `crates/buff-lang-cli/src/pipeline.rs` (compile_to_rust already produces the .rs — just expose it); `cargo expand` as precedent.
  **Acceptance Criteria**: `buff expand examples/ola.buff` prints the generated Rust to stdout.
  **QA**: Run expand on ola.buff; assert valid Rust output containing `fn main`. Evidence: task-115-expand.txt
  **Commit**: YES — `feat(cli): buff expand — show generated Rust source (T115)`

- [ ] 116. Track F — Community Health Files (Wave 0, writing) **LAUNCH-CREDIBILITY**

  **What to do**: Create standard open-source community files that are MISSING:
  - `CODE_OF_CONDUCT.md` — Contributor Covenant 2.1 (https://www.contributor-covenant.org/version/2/1/code_of_conduct/)
  - `SECURITY.md` — how to report vulnerabilities (private disclosure via GitHub Security Advisories; NOT public issues)
  - `.github/ISSUE_TEMPLATE/bug_report.yml` — structured bug report (Buff version, OS, reproduction steps)
  - `.github/ISSUE_TEMPLATE/feature_request.yml` — structured feature request
  - `.github/PULL_REQUEST_TEMPLATE.md` — PR checklist (tests pass, clippy clean, CHANGELOG updated)
  - `.github/dependabot.yml` — weekly dependency update checks for Cargo + npm (editors/vscode)
  **Must NOT do**: Do NOT create LICENSE file (already exists as MIT OR Apache-2.0). Do NOT modify existing .github/workflows/.
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 0 — all .md/.yml files, zero code conflict). Blocked By: None.
  **References**: Contributor Covenant; GitHub community standards; Rust repo's SECURITY.md as template.
  **Acceptance Criteria**: All 6 files exist. `Test-Path CODE_OF_CONDUCT.md, SECURITY.md` → True.
  **QA**: Test-Path each file. Evidence: task-116-community.txt
  **Commit**: YES — `docs: add community health files (CODE_OF_CONDUCT, SECURITY, issue/PR templates, dependabot) (T116)`

- [ ] 117. Track F — Playground Rebuild for v1.25 Features (Wave 3, unspecified-high) ← needs Wave 2a/2b language features

  **What to do**: The playground (`playground/`) ships a static transpile-only wasm (`buff_playground_bg.wasm`, 2.3MB) built during v1.1/v1.2. It does NOT support v1.25 language features (generics, patterns, ranges, raw strings, etc.). Rebuild the wasm from the latest buff-playground-wasm crate so the playground reflects the full v1.25 language.
  - `cd crates/buff-playground-wasm && wasm-pack build --target web --out-dir ../../playground/pkg/`
  - Verify all v1.25 examples transpile correctly in the playground.
  - Update playground UI if needed (new syntax highlighting for ranges `0..10`, raw strings `r"..."`, etc.).
  - Test URL-fragment code sharing still works (`#s=<base64>`).
  **Must NOT do**: Do NOT add runtime/GPU to the playground wasm (it's transpile-only by design). Do NOT change playground hosting (stays static HTML/CSS/JS).
  **Recommended Agent Profile**: `unspecified-high`. **Skills**: none.
  **Parallelization**: YES (Wave 3 — after language features land). Blocks: public launch (playground must showcase v1.25). Blocked By: T13 (generics), T37-T42 (patterns), T84 (ranges), T93 (raw strings) — all Wave 2a.
  **References**: `crates/buff-playground-wasm/`; `playground/{index.html,pkg/}`; wasm-pack docs.
  **Acceptance Criteria**: Playground transpiles generics + patterns + ranges + raw strings without error. All v1.25 examples work.
  **QA**: Open playground in browser; paste generics example; assert transpile succeeds. Evidence: task-117-playground.png
  **Commit**: YES — `feat(playground): rebuild wasm for v1.25 language features (T117)`

- [ ] 118. Track F — VSCode Extension v1.3 Update (Wave 2b, quick) ← BLOCKED BY T46

  **What to do**: The VSCode extension is at v1.2.0. When T46 adds LSP capabilities (codeAction, codeLens, inlayHint, semanticTokens), the extension needs:
  - Bump version to 1.3.0
  - Update `package.json` capabilities list to enable codeLens/inlayHint/semanticTokens in the editor
  - Rebuild and repackage: `cd editors/vscode && npm run build && vsce package`
  - Update `buff.run`/`buff.build`/`buff.check` commands to use new CLI flags (--explain, --error-format)
  - Add `buff.profile` command (for T111)
  - Add `buff.watch` command (for T97)
  - Test in VSCode: open a .buff file; verify lenses appear above funcs; inlay hints show types; semantic highlighting works.
  **Must NOT do**: Do NOT bundle the LSP binary into the .vsix differently (keep existing bundling approach). Do NOT change TextMate grammar (tree-sitter handles highlighting).
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2b). Blocked By: T46 (LSP capabilities must be implemented first).
  **References**: `editors/vscode/{package.json,src/extension.ts}`; `crates/buff-lsp/`.
  **Acceptance Criteria**: Extension v1.3.0 .vsix builds; VSCode shows lenses + hints + semantic tokens after install.
  **QA**: Install .vsix in VSCode; open .buff; assert lenses + hints visible. Evidence: task-118-vscode.png
  **Commit**: YES — `feat(vscode): extension v1.3.0 with codeLens + inlayHint + semanticTokens (T118)`

- [ ] 119. Track F — CHANGELOG.md (Wave 0, writing) **LAUNCH-CREDIBILITY**

  **What to do**: Create/update `CHANGELOG.md` (currently 76 lines — covers v0.1-v1.12). Add v1.13-v1.24 entries from `buff-v1x-frameworks.md` releases. Reserve v1.25+ section with placeholder entries for each Track. Follow Keep a Changelog format (https://keepachangelog.com/). Each entry: Added/Changed/Deprecated/Removed/Fixed/Security sections.
  **Must NOT do**: Do NOT auto-generate from git log (manual curation required). Do NOT include internal-only changes.
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 0 — .md only). Blocked By: None.
  **References**: `CHANGELOG.md` (existing, 76 lines); Keep a Changelog spec.
  **Acceptance Criteria**: CHANGELOG covers v0.1→v1.25; follows Keep a Changelog format; ≥200 lines.
  **QA**: `Get-Content CHANGELOG.md | Measure-Object -Line` ≥ 200. Evidence: task-119-changelog.txt
  **Commit**: YES — `docs: expand CHANGELOG.md for v1.13-v1.25 (T119)`

- [ ] 120. Track E — `buff fmt` Completeness Verification (Wave 2b, quick) ← after language features land

  **What to do**: Verify `buff fmt` handles ALL v1.25 syntax: generics (`struct Pair<T, U>`), pattern guards (`if x > 0`), ranges (`0..10`, `0..=10`), raw strings (`r"..."`), import aliases (`import { X as Y }`), tuple destructuring in params (`func f((x, y): (Int, Int))`), `@attributes` (`@prefer(gpu)`, `@blocking`, `@workgroup(256)`). Add fmt regression tests (insta snapshots of formatted output).
  **Must NOT do**: Do NOT change formatting rules (just verify existing rules handle new syntax). Do NOT break existing fmt behavior.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2b — after Wave 2a language features land). Blocked By: T13, T37-T42, T84, T93, T94, T96.
  **References**: `crates/buff-lang-cli/src/fmt.rs` (1,839 LOC formatter); existing fmt tests.
  **Acceptance Criteria**: All v1.25 syntax formats correctly; regression snapshots pass.
  **QA**: Run `buff fmt` on file with all v1.25 syntax; assert valid output + idempotent. Evidence: task-120-fmt-verify.txt
  **Commit**: YES — `test(fmt): verify buff fmt handles all v1.25 syntax (T120)`

- [ ] 121. Track F — Benchmark Publication Page (Wave 3, writing) ← uses T22 baseline data

  **What to do**: Create `docs/benchmarks.md` with competitive comparison: Buff vs Rust vs Go vs Python for compile-time, runtime, binary size. Use T22 benchmark harness data. Include: time-to-first-hello-world, incremental rebuild, large-project clean build, GPU dispatch speedup vs CPU-only. Charts via mermaid.js or embedded SVG.
  **Must NOT do**: Do NOT fabricate numbers (use actual T22 measurements). Do NOT benchmark against unfair competitors (e.g., interpreted Python for compute).
  **Recommended Agent Profile**: `writing`. **Skills**: none.
  **Parallelization**: YES (Wave 3 — after T22 data is complete). Blocked By: T22 (baseline), T5/T10/T11 (MOAT features benchmarked).
  **References**: `.sisyphus/evidence/baseline-v1.25.json` (T22 output); Swift/Kotlin launch benchmark pages as precedent.
  **Acceptance Criteria**: `docs/benchmarks.md` exists with ≥5 comparison tables; data sourced from actual measurements.
  **QA**: Test-Path; grep for comparison tables. Evidence: task-121-benchmarks.md
  **Commit**: YES — `docs: add competitive benchmark publication page (T121)`

- [ ] 122. Track F — `buff-dap` Debugger Verification (Wave 2b, quick)

  **What to do**: The DAP debugger shipped in v1.10 (`crates/buff-dap/`). Verify it works with v1.25 language features (generics, patterns, ranges). Test: set breakpoint in generic function, step through, inspect variables. Update if DAP protocol messages need new fields for generic types.
  **Must NOT do**: Do NOT add new DAP capabilities (just verify existing works). Do NOT change DAP protocol version.
  **Recommended Agent Profile**: `quick`. **Skills**: none.
  **Parallelization**: YES (Wave 2b — after Wave 2a language features). Blocked By: T13 (generics), T37 (user generics).
  **References**: `crates/buff-dap/src/{server,protocol}.rs`; VSCode debug configuration `.vscode/buff-debug.launch.json`.
  **Acceptance Criteria**: Breakpoint + step + inspect works on generic function; DAP messages correct for v1.25 types.
  **QA**: Debug a generic function in VSCode; assert breakpoint hits + variables visible. Evidence: task-122-dap-verify.png
  **Commit**: YES — `test(dap): verify buff-dap works with v1.25 language features (T122)`

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Verify each "Must Have" is implemented. For each "Must NOT Have", search codebase for forbidden patterns (`melior`, `mlir`, `Perceus`, `weak<T>`, `HashMap` in compiler-internal crates **excluding codegen-rust type-lowering and prelude_types.rs registry** (T27 exception), `native-tls`, `cc-rs` in core).   Confirm evidence files exist for ALL tasks. Verify: Track B (MOAT: data-locality dispatch, dynamic workload inspection, cost model, --explain factors, kernel fusion); Track C (self-host: generics T13, ports T15-T18, bootstrap T19); Track D stdlib (Json/File/HTTP); Track E diagnostics (color+multi-span+JSON+--explain+LSP); Track F launch infra (Compatibility Doc+Book+registry+binaries+MEMORY_SAFETY); Track G perf control (@prefer/@force/@blocking/@workgroup/FAIL_LOUD); Track H DX tooling (buff watch/fix/bench/snapshot/generate); Track I hygiene (audit exists, LOC targets met, with_exe_extension preserved, codegen-hash zero diffs, fallback marker); Track I optimization (AGENTS.md says "64-crate", RELEASE.md exists, decision record exists, T13 in Wave 0, T105 split, T19 in Wave 4); Track A (cross-compile --target works, buff profile generates flamegraph, --detect-races works on nightly, buff expand works, .env loading works).
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [122/122] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo check --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` + `cargo fmt --check`. Review for unwrap/expect/panic in non-test, HashMap in compiler-internal crates, AI slop. Verify Edition-2024 unsafe (T28), Dioxus exact pin (T29), serde_yml pin (T30), Cargo.lock (T31), version tiers (T33), rand 0.9 (T36). **Security review**: T57 registry input validation (path traversal), OAuth token storage (not plaintext), rate limiting. Playground no eval(). All 64 crates `license = "MIT OR Apache-2.0"`.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N/N] | Fmt [PASS/FAIL] | Files [N/N] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Execute every task's QA scenarios from clean state. Run compile-speed benchmarks (assert targets). Run CPU/GPU dispatch via MockGpuBackend + --explain. Run bootstrap (Stage 2 == Stage 3 byte-identical). Run stdlib demos (json_demo, file_io_demo, http_demo). Run `buff check --explain E1201`. Run `buff doc` on example. Run `buff publish` + `buff add` round-trip (staging registry). Install via scoop/brew/cargo.   Run `buff profile` on example. Run `buff build --target`. Connect MCP bridge. Test Track G: `@prefer(cpu)` keeps workload on CPU; `@force(gpu)` raises E1401 on GPU-less machine; `@blocking` runs sync in async context; `@workgroup(256)` emits correct WGSL; `BUFF_FAIL_LOUD_GPU=1` surfaces fallbacks. Test Track H: `buff watch` recompiles on save; `buff fix --apply` fixes naming; `buff bench` reports timing; `buff generate struct X` creates file; `assert_snapshot()` round-trips.
  Output: `Speed [N/N] | Dispatch [N/N] | Bootstrap [Y/N] | Launch-Smoke [N/N] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1. Confirm ZERO MLIR, ZERO custom memory model, ZERO rustc-removal. Verify Accept-With-Rationale items documented (10 items). Verify T108 updated AGENTS.md to "64-crate". Verify T105a/b mechanical extraction only. Verify T106 respected caps. Verify T106 did NOT extract with_exe_extension. Verify T107 either complete OR deferred. Verify Track I zero ErrorCode changes. Verify wave restructure didn't break dependency edges.
  Output: `Tasks [N/N compliant] | Demoted-item leakage [CLEAN/N] | VERDICT`

---

## Commit Strategy

- Per-task atomic commits, Conventional Commits format, scoped to touched crate.
- Pre-commit gate per task: `cargo test -p <crate>` + `cargo clippy -p <crate> --all-targets -- -D warnings`.
- Long-lived `v1.25-launch-readiness` branch; thin version tags per shippable unit; per-release merge to master.
- **Final v1.X tag = public launch announcement.**
- **Track I commits**: one file extraction per commit (atomic rollback; bisect-friendly). Commit body MUST state: test result, clippy result, codegen hash diff, snapshot churn count.
- **Time-bomb batch** (T28-T36): one delegated task, 6 atomic commits.

---

## Success Criteria

### Verification Commands
```bash
cargo check --workspace                                   # clean
cargo clippy --workspace --all-targets -- -D warnings     # clean
cargo test --workspace                                    # 100% pass
cargo fmt --check                                         # clean

# Compile-speed: incremental rebuild <5s; clean build <5s; hello-world <10s
# MOAT: --explain reports dispatch factors; data-locality avoids redundant transfers
# Self-host: Stage 2 == Stage 3 byte-identical; compiler emits Rust

# Stdlib: json_demo.buff, file_io_demo.buff, http_demo.buff all PASS
# Language: generics_demo.buff PASS; patterns_demo.buff PASS
# Diagnostics: colored output on TTY; --explain E1201; did-you-mean
# Tech-debt: unsafe wrap, Dioxus pin, rand 0.9, version tiers unified
# Launch: COMPATIBILITY.md, MEMORY_SAFETY.md, book/, registry, binaries
# Track I: audit doc exists; rust_codegen.rs ≤5K; prelude_types.rs ≤2K; zero hash diffs
# Track A: buff profile generates flamegraph; buff build --target works; --detect-races works

# Security:
cargo audit                                               # no advisories
```

### Final Checklist
- [ ] All "Must Have" present (9 tracks: A-I)
- [ ] All "Must NOT Have" absent (verified by F1 + F4)
- [ ] All tests pass on 6-target CI (3 OSes × 2 arches)
- [ ] Compile-speed targets met
- [ ] MOAT dispatch better than arithmetic-intensity-only threshold
- [ ] Compiler bootstraps in Buff (byte-identical Stage 2 == Stage 3)
- [ ] Stdlib table-stakes shipped (Json + File + HTTP)
- [ ] Language-feature gaps closed (generics + patterns)
- [ ] Diagnostics quality meets 2026 bar
- [ ] Tech-debt time-bombs defused
- [ ] Launch infrastructure shipped
- [ ] Track I: code hygiene complete; god classes split; codegen determinism preserved
- [ ] Track A: profiler + cross-compile + race detector shipped
- [ ] AGENTS.md reflects 64-crate reality; RELEASE.md exists; decision record finalized
- [ ] **NEW (Track A)**: profiler + cross-compile + race detector shipped; `buff expand` works; `.env` loading works
- [ ] **NEW (Community)**: CODE_OF_CONDUCT.md + SECURITY.md + issue templates + PR template + dependabot exist
- [ ] **NEW (Playground)**: playground wasm rebuilt for v1.25 features (generics + patterns + ranges compile in browser)
- [ ] **NEW (VSCode)**: extension v1.3 bundles new LSP capabilities
- [ ] All 10 Accept-With-Rationale items documented
- [ ] Public launch announcement ready
