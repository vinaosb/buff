# Buff v1.13-v1.24 — Frameworks Roadmap

## Version Mapping (USER-CONFIRMED — COHERENT RELEASE UNITS)

This roadmap groups tasks into **coherent shippable releases**. Each task within a release gets its own commit; the release ships as one version tag when all its tasks complete. Releases can be developed **in parallel** (max 10 concurrent tasks) — the dependency graph determines what can start when.

### Release Map (12 coherent releases, v1.13.0 → v1.24.0)

| Release | Theme | Tasks | Can start when |
|---|---|---|---|
| **v1.13.0** | **Foundations** — SDK, linking, Channel, traces, spikes, comptime | T0, T1, T2, T24, T3, T4, T6, T53 (8 tasks) | Immediately (no deps) |
| **v1.14.0** | **Compute + Mocking** — dataframe, tensor, image, audio, dsp, ecs, mock | T7-T12, T25 (7 tasks) | v1.13 ships (need T0+T1) |
| **v1.15.0** | **Production Wrappers + Security** — web, db, template, reactive, observe, audit, fuzz | T17-T21, T26, T27 (7 tasks) | v1.13 ships (need T0+T1) |
| **v1.16.0** | **Tier 1 Common** — validate, config, cache, cli, http-client, auth, jobs, resilience | T29-T36 (8 tasks) | v1.13 ships (need T0+T1) |
| **v1.17.0** | **Tier 2 Common** — fake, assertions, archive, fsm, pubsub, email, scrape, i18n | T37-T44 (8 tasks) | v1.13 ships (need T0+T1) |
| **v1.18.0** | **Tier 3 Specialized** — geo, nlp, chat, web3, crypto-extras, xml, msgpack, protobuf | T45-T52 (8 tasks) | v1.13 ships (need T0+T1+T4) |
| **v1.19.0** | **Language Evolution** — SIMD, compile-speed, property wrappers, math syntax, multiple dispatch, actors, binary size | T54-T60 (7 tasks) | v1.13 ships (+ T6 for SIMD, T53+T20 for property wrappers, T2+T20 for actors) |
| **v1.20.0** | **Developer Experience** — cold-start, PGO, error quality, hot reload, AI, refactoring, docs site | T61-T67 (7 tasks) | v1.13 ships (+ T16+T17 for hot reload, T24 for error/refactoring) |
| **v1.21.0** | **Community & Quality** — cookbook, onboarding, quality signals, stability promise, plugins | T68-T72 (5 tasks) | v1.20 ships (T68/T69 need T67 docs site) |
| **v1.22.0** | **Domain Frameworks** — science, pipeline, ML, game | T13, T14, T15, T16 (4 tasks) | v1.14 ships (need T8 tensor, T12 ecs) |
| **v1.23.0** | **Integration + Flagship** — API compat spike + Data Science Workbench | T22, T23 (2 tasks) | v1.14 + v1.15 + v1.22 ship (need frameworks) |
| **v1.24.0** | **Audit & Polish** — iterative audit until convergence | T28 (1 task) | v1.23 ships (needs everything done) |

**Critical path**: v1.13 → v1.14 → v1.22 → v1.23 → v1.24 → F1-F4

**Parallel tracks** (all start after v1.13, run alongside critical path):
- Track A (critical): v1.14 → v1.22 → v1.23 → v1.24
- Track B (parallel): v1.15, v1.16, v1.17, v1.18 (wrappers + common frameworks)
- Track C (parallel): v1.19, v1.20, v1.21 (language + DX + community)

### Execution Batches (Max 10 Concurrent)

Tasks execute in batches of max 10 parallel. Batches overlap — when a task finishes, the next ready task fills the slot.

| Batch | Tasks running (max 10) | Notes |
|---|---|---|
| **1** | T0, T2, T3, T4, T6, T24, T53 + 3 slots reserved for T1 (starts when T0 done) + early starters | All foundations, no deps |
| **2** | T1, T7, T8, T9, T10, T17, T18, T29, T30, T55 | v1.13 done. Mix of v1.14 (compute) + v1.15 (wrappers) + v1.16 (Tier 1) + v1.19 (compile-speed) |
| **3** | T11, T12, T25, T19, T20, T21, T31, T32, T33, T34 | Continuing v1.14 + v1.15 + v1.16 |
| **4** | T26, T27, T35, T36, T37, T38, T45, T46, T54, T56 | v1.15 + v1.16 + v1.17 + v1.18 + v1.19 |
| **5** | T39, T40, T41, T42, T47, T48, T49, T57, T58, T59 | v1.17 + v1.18 + v1.19 |
| **6** | T43, T44, T50, T51, T52, T60, T61, T62, T63, T64 | v1.17 + v1.18 + v1.19 + v1.20 |
| **7** | T65, T66, T67, T68, T69, T70, T71, T72, T13, T14 | v1.20 + v1.21 + early v1.22 (T13/T14 deps met) |
| **8** | T15, T16, T22 | v1.22 (T15 needs T8, T16 needs T12, T22 needs many) |
| **9** | T23 | v1.23 flagship (needs many) |
| **10** | T28 | v1.24 audit (needs T23) |
| **FINAL** | F1, F2, F3, F4 | 4 parallel verification agents |

**Concurrency rule**: Max 10 tasks at any time. When one finishes, the next ready task (deps met, priority-ordered) fills the slot immediately.

### Commit & Tag Strategy

- **Per-task commits**: Each task produces 1+ atomic commits (Conventional Commits format). Example: `feat(buff-dataframe): MVP columnar DataFrames with CSV/JSON load`.
- **Per-release tags**: When all tasks in a release group complete, tag the release. Example: `git tag v1.14.0` after T7-T12 + T25 all merged.
- **Branch**: Long-lived `v1x-frameworks` off master. Each task merges to `v1x-frameworks`. Release tags on `v1x-frameworks`. Master merge per-release.
- **CHANGELOG**: Updated per-task commit; release entry added per-tag.

**v2.0 Roadmap** (post-v1.24, separate planning): MLIR backend for unified CPU/GPU/TPU/NPU/FPGA codegen (Mojo-inspired).

**Other deferred items**: targeted for v1.25+ (post-v1.24 audit). The v1.24 audit task (T28) produces comprehensive followup doc at `.sisyphus/decisions/v1.24-followup.md`.

## TL;DR

> **Quick Summary**: Post-v1.12 incremental roadmap (v1.13 → v1.24) focused on making Buff genuinely productive across multiple domains. Establishes the **Buff SDK 2.0 project model & conventions** (single comprehensive foundation), adds language foundations (multi-file linking, Channel<T> primitive, macro decision), then ships MVP-quality framework crates spanning dataframes, tensors, ML, game dev/ECS, scientific computing, image/audio/DSP, data pipelines, plus production wrappers (web, db, template, reactive, observe). Capstoned by a Data Science Workbench flagship app integrating 5 frameworks end-to-end.
>
> **Deliverables**:
> - 1 SDK foundation: Buff SDK 2.0 conventions + buff.toml v2 + templates + visibility + versioning + docs + CI/DX + workspace dep inheritance + @feature conditional compilation
> - 4 language foundations: multi-file linking + cross-compilation, Channel<T> MPSC primitive, Stack traces with Buff spans, Mocking framework (buff-mock)
> - 7 language inspirations: comptime (Zig), SIMD types (Mojo), compile-speed program (V/Go), property wrappers (Swift), mathematical syntax (Julia), multiple dispatch (Julia), actor model (Gleam)
> - 4 decision/spike artifacts: macro system decision, FFI safety guide, WGSL extensibility assessment, (T5 migration removed)
> - 9 domain framework crates (Wave 2-3): buff-{dataframe, tensor, science, image, audio, dsp, pipeline, ecs, ml, game}
> - 5 production wrapper crates (Wave 4): buff-{web, db, template, reactive, observe}
> - 3 security/quality tools: buff-audit + code signing, buff-fuzz, (mocking counted in foundations above)
> - 8 Tier 1 common frameworks (Wave 7 v1.19, universal pain points): buff-{validate, config, cache, cli, http-client, auth, jobs, resilience}
> - 8 Tier 2 common frameworks (Wave 8 v1.20, testing + I/O + state): buff-{fake, assertions, archive, fsm, pubsub, email, scrape, i18n}
> - 8 Tier 3 specialized frameworks (Wave 9 v1.21, Rust leverage): buff-{geo, nlp, chat, web3, crypto-extras, xml, msgpack, protobuf}
> - 1 actor framework (Wave 12 v1.24, Gleam-inspired): buff-actors
> - 1 flagship app: Data Science Workbench
> - 1 iterative audit task (T28 v1.24): refines docs, fixes outdated/missing items, logs deferred work for future
> - 4-task final verification wave
>
> **Estimated Effort**: XL (multi-year)
> **Parallel Execution**: YES - 3 parallel tracks (A: critical path, B: wrappers+common, C: language+DX), max 10 concurrent
> **Releases**: 12 coherent shippable units (v1.13.0 → v1.24.0). Each task = own commit; each release = own tag.
> **Critical Path**: v1.13 (Foundations) → v1.14 (Compute) → v1.22 (Domains) → v1.23 (Flagship) → v1.24 (Audit) → F1-F4
>
> **v2.0 Roadmap** (post-v1.24, separate planning): MLIR backend for unified CPU/GPU/TPU/NPU/FPGA codegen (Mojo-inspired).
>
> **v1.4 stdlib status**: ✅ VERIFIED SHIPPED via codegen lowering in `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (12,777 lines). DateTime/Log/Regex/Toml/Math/Random/Strings/Args/Env/URL/Base64/Hex/UUID/YAML/CSV/Path/Dir/Tempfile/Hash/HMAC/Process/OS/TCP/UDP/WebSocket all present. Frameworks BUILD ON TOP.
>
> **Existing async coverage** (re-assessment post-user-feedback): Buff already has `async func` + auto-propagation (no `await` keyword), `spawn expr` → `Task<T>`, `task.result()`, `sleep(duration)`, async TCP/UDP/WebSocket `.recv()`, par_map/par_filter/par_reduce, Arc-shared captures. The only genuinely missing async primitive is in-process `Channel<T>` MPSC for producer/consumer patterns. `Stream<T>` general type and `select` expression are NOT needed for MVP frameworks (network `.recv()` covers external streams; sync `Vector<T>` + `for x in` covers batch; callbacks cover reactive).

---

## Context

### Original Request
User wants to plan the next phase after `buff-post-v10-tooling.md` finishes. Initial idea: "make some initial frameworks for the language." Refined through interview into a comprehensive v1.13-v1.24 framework roadmap.

### Mid-Plan Correction (user feedback after Momus approval)
User flagged that async streams may already be done "in an alternative way". Direct codebase re-verification (no agents) confirmed Buff already has rich async coverage:
- **Async core**: `async func` + auto-propagation (no `await` keyword) — production
- **`spawn expr`** → `Task<T>` (alias for `tokio::task::JoinHandle<T>`)
- **`task.result()`** — Buff's await equivalent for spawned tasks
- **`sleep(duration)`** — async sleep via tokio
- **Async networking**: `TCP.connect().recv()`, `UDP.bind().recv_from()`, `WebSocket.connect().recv()` — all use tokio with auto-await
- **CPU parallel**: `par_map`, `par_filter`, `par_reduce` (rayon-backed)
- **Arc-shared captures** across spawn boundaries

Genuinely missing: `Stream<T>` general type, `Channel<T>` MPSC primitive, `select` expression. Of these, only `Channel<T>` is needed for MVP frameworks (producer/consumer in pipelines, internal scheduler queues). `Stream<T>` and `select` deferred to v1.18+.

**Plan consequence**: T2 shrunk to Channel-only scope (no new keywords — exposed as `Channel.new()` method-style). T5 (migration tool) REMOVED entirely — no new keywords means no migration needed. T14 (buff-pipeline) and T20 (buff-reactive) task descriptions updated to use Channel + callbacks respectively.

### Interview Summary
**Key Decisions**:
- **Plan timing**: Starts AFTER v1.12 ships (v1.9 RSX, v1.10 debugger/coverage, v1.11 Bufflings, v1.12 distribution are NOT in scope — already planned in tooling plan)
- **Vision**: Buff = developer productivity; Rust = speed. Leverage Rust ecosystem via `extern` FFI; frameworks build on existing v1.4 stdlib rather than reinventing.
- **Framework scope (all 4 vectors selected)**:
  1. Domain frameworks (new) — ALL 6 domains: data pipelines, image/audio/DSP, scientific, game dev/ECS, dataframes, ML/tensors/autodiff
  2. Production hardening of v1.x — LIMITED to thin idiomatic wrappers (no full reimplementation; deep hardening deferred to v1.18+)
  3. Language-level features enabling frameworks (foundation-first IF truly blocking)
  4. Idiomatic wrappers for Rust ecosystem (axum, sqlx, askama, tracing)
- **Flagship**: Data Science Workbench (load CSV → dataframe ops → train ML on GPU → visualize in web UI)
- **Quality bar**: MVP across the board (working happy-path, examples, basic tests, doc comments, "experimental" registry badge)
- **Publishing**: In-repo under `crates/` (e.g., `crates/buff-ml/`)

**Research Findings**:
- **v1.4 stdlib VERIFIED SHIPPED** via codegen lowering in `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (500+ lines). Available: DateTime, Log, Regex, Toml, Math/Random/Sort/Strings, Args/Env/sleep, URL/Base64/Hex/URLEncode/UUID, YAML/CSV, Path/Dir/Tempfile, Hash/HMAC, Process/OS, TCP/UDP/WebSocket. Frameworks BUILD ON TOP, do NOT recreate.
- **Existing v1.x crates** in repo: buff-eval, buff-jupyter, buff-lsp, buff-playground-wasm, buff-registry, buff-repl, buff-ui-dioxus, buff-lang-{error,ast,lexer,parser,types,codegen-rust,codegen-wgsl,runtime,cli}, buff-lang-{ast-rsx,buffhtml-parser}. v1.13-v1.24 frameworks reuse these.
- **Language feature surface**: structs/enums/traits/generics/pattern matching/closures/async (auto-propagation, no `await` keyword)/error handling with `?`/defer. ES6-style import/export PARSES but does not link end-to-end (single-file rustc pipeline).
- **Prelude today**: math, conversions, print/println/read_line/input, args/env/exit/sleep, assert_eq. Plus implicit stdlib namespaces (DateTime.*, Log.*, Regex.*, etc.).

### Metis Review
**Critical Gaps Addressed** (see "Decisions Needed" section for unresolved items):
- G2 Multi-file linking fallback: classify each Wave 2 framework HARD/SOFT/NONE multi-file dependency — embedded in task descriptions
- G3 Macro spike decision rule: 5-day timebox, output = decision doc committed to `.sisyphus/`, defer to v1.18+ if >1500 LOC or <2 frameworks need it
- G6 Async abstraction: hide tokio behind `buff-lang-runtime` traits; frameworks depend on traits not tokio directly
- G7 GPU scope matrix: ML + Science + Tensor USE GPU; Image/DSP CPU-only for MVP (defer GPU); Game uses existing WGSL; others CPU
- G8 FFI safety: dedicated Wave 1 task to write conventions doc; hard rule = no raw pointers exposed to Buff users
- G9 Per-framework LOC/API budgets locked: Wave 2 ≤2500 LOC/≤25 fns; Wave 3 ≤4000 LOC/≤40 fns; Wave 4 ≤1500 LOC/≤20 fns; Flagship ≤3000 LOC
- G10 API compat spike inserted as Wave 5 T22 before flagship
- G15 Lexer/parser impact: folded into T1 (multi-file) and T2 (async) — both tasks include syntax assessment substeps
- A8 WGSL extensibility: dedicated Wave 1 T6 to assess before tensor/ML commit

---

## Work Objectives

### Core Objective
Make Buff genuinely productive across multiple problem domains by:
1. **Establishing Buff SDK 2.0**: a coherent project model, conventions, and bundled experience — the .NET-Core-style unification of layout, tooling, and documentation that lets frameworks compose consistently and users learn Buff once.
2. **Shipping MVP-quality framework crates** that leverage the existing Rust ecosystem via safe `extern` FFI, built on a foundation of multi-file linking and a `Channel<T>` primitive for producer/consumer patterns. The existing async model (auto-propagation without `await` keyword, `spawn`/`Task<T>`, async networking `.recv()`) covers most async needs; only the in-process `Channel<T>` MPSC primitive is added.

### Concrete Deliverables
- 1 SDK foundation: Buff SDK 2.0 conventions + buff.toml v2 + scaffolding templates + visibility + versioning + docs + CI/DX + workspace dep inheritance + @feature conditional compilation
- 4 language foundations: multi-file linking + cross-compilation targets, `Channel<T>` MPSC primitive, Stack traces with Buff spans (.buffmap + panic hook), buff-mock mocking framework
- 3 decision artifacts: macro system decision doc, FFI safety guide, WGSL extensibility assessment
- 9 domain frameworks: buff-{dataframe, tensor, science, image, audio, dsp, pipeline, ecs, ml, game}
- 5 production wrappers: buff-{web, db, template, reactive, observe}
- 3 security/quality tools: buff-audit + code signing, buff-fuzz, (mocking counted in foundations above)
- 1 flagship: Data Science Workbench app
- Per-framework: ≥3 examples, ≥10 unit tests, ≥5 snapshot tests, README, registry entry with "experimental" badge

### Definition of Done
- [ ] All 25 implementation tasks completed with QA scenarios passing
- [ ] `cargo check --workspace` passes (no errors)
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo test --workspace` passes
- [ ] All v1.x examples still compile (backward compat verified)
- [ ] All v1.x snapshot tests still pass (no regression)
- [ ] Flagship Data Science Workbench runs end-to-end and produces expected output
- [ ] All framework crates registered with "experimental" stability badge
- [ ] F1-F4 final verification wave complete with APPROVE verdicts

### Must Have
- Multi-file linking works end-to-end (`buff build` compiles multi-file project)
- Async streams/channels/select primitives available in Buff source
- Each framework crate follows AGENTS.md conventions (workspace deps, edition 2021, MIT OR Apache-2.0, version 2.0.0)
- ZERO `unwrap`/`expect`/`panic!`/`todo!` in non-test code
- All Rust codegen via syn/quote/prettyplease (no raw strings)
- Per-framework MVP acceptance checklist satisfied (see Verification Strategy)

### Must NOT Have (Guardrails)
- **NO production hardening of frameworks in v1.13-v1.17** — deep optimization, full test matrices, edge case handling deferred to v1.18+. MVP = happy-path + basic tests only.
- **NO new dependencies outside root `Cargo.toml [workspace.dependencies]`** — every framework crate uses `dep.workspace = true`.
- **NO `unwrap`/`expect`/`panic!`/`unimplemented!`/`todo!`** in non-test code (AGENTS.md hard rule).
- **NO raw-string Rust codegen** — all codegen via syn/quote/prettyplease (AGENTS.md hard rule).
- **NO `_async` suffix** on async functions (Buff language rule §6).
- **NO positional boolean args** — named args mandatory (§11).
- **NO `new Person()` / `Person.create()` / `Person.build()`** — constructors are `Type.new()` / `Type.from()` only (§7).
- **NO tabs** — 4 spaces only (Buff lexer rejects tabs).
- **NO committing `.snap.new` / `.pending-snap`** — insta pending files (gitignored).
- **NO populating `crates-io/`** without coordination (currently empty/reserved).
- **NO framework exceeding LOC/API budget** — exceeding budget triggers deferral to v1.18+, not "work harder".
- **NO unsafe Rust exposure to Buff users** — all `extern` FFI must be wrapped in safe Buff API (no raw pointers).
- **NO breaking changes to consumed compiler APIs** — `buff-lang-{lexer,parser,types,codegen-rust,codegen-wgsl,runtime}` public APIs are stable; extensions only.
- **NO new keywords without backward-compat lint** — `stream`/`channel`/`select`/`macro` reservations must come with `buff fix --v1-to-v1x` migration tool.
- **NO GPU work in Wave 2 image/audio/dsp** — those are CPU-only for MVP (GPU deferred). Only ML/Science/Tensor use GPU.
- **NO scope creep into v1.9 RSX, v1.10 debugger, v1.11 Bufflings, v1.12 distribution** — those are owned by `buff-post-v10-tooling.md`.

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.
> Acceptance criteria requiring "user manually tests/confirms" are FORBIDDEN.

### Test Decision
- **Infrastructure exists**: YES (per-crate `tests/`, insta snapshots, proptest, `@test` attribute + `assert_eq`)
- **Automated tests**: Tests-after (each framework task includes test cases as part of deliverable)
- **Framework**: cargo test + insta + proptest (existing infrastructure)

### QA Policy
Every task MUST include agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Frontend/UI**: Use Playwright (playwright skill) — Navigate, interact, assert DOM, screenshot
- **TUI/CLI**: Use interactive_bash (tmux) — Run command, send keystrokes, validate output
- **API/Backend**: Use Bash (curl) — Send requests, assert status + response fields
- **Library/Module**: Use Bash (cargo) — `cargo test`, `cargo run -p buff-lang-cli -- run examples/<framework>/<name>.buff`, compare output
- **Compiler/Language features**: Use Bash — write small `.buff` test programs, run via `buff run` or `buff check`, assert output / errors

### Per-Framework MVP Acceptance Checklist (apply to EVERY framework task)

**Code**:
- [ ] Crate exists at `crates/buff-<name>/` with standard Cargo.toml (workspace deps, edition 2021, MIT OR Apache-2.0, version 2.0.0)
- [ ] Crate `AGENTS.md` committed (per-crate convention)
- [ ] LOC within budget (Wave 2: ≤2500, Wave 3: ≤4000, Wave 4: ≤1500)
- [ ] Public API ≤ budget (Wave 2: ≤25 fns, Wave 3: ≤40 fns, Wave 4: ≤20 fns)
- [ ] ZERO `unwrap`/`expect`/`panic!`/`todo!` in non-test code
- [ ] All codegen (if any) via syn/quote/prettyplease (no raw strings)

**Examples**:
- [ ] ≥3 examples in `examples/<framework>/` (hello-world, core-feature, integration)
- [ ] All examples runnable via `cargo run -p buff-lang-cli -- run examples/<framework>/<name>.buff`
- [ ] Examples follow naming convention (PT-BR educational ok, EN technical preferred)

**Tests**:
- [ ] Unit tests in `crates/buff-<name>/tests/` (≥10 test cases)
- [ ] Snapshot tests committed (≥5, ≤20)
- [ ] Proptest IF math-heavy framework (dataframe, tensor, ml, science)
- [ ] All tests pass on Linux (Windows/macOS verification in CI)

**Docs**:
- [ ] Module-level doc comment in lib.rs
- [ ] Public API has doc comments (every `pub fn`)
- [ ] `README.md` in crate root (purpose, install, hello-world, links)
- [ ] Registry entry with `"experimental"` stability badge

**Integration**:
- [ ] `cargo check --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] No new dependencies outside root `Cargo.toml [workspace.dependencies]`

**Error Handling**:
- [ ] Errors via `thiserror::Error` derive, mapped to `buff_lang_error` variants
- [ ] Error messages include spans (no bare strings)

---

## Execution Strategy

### Parallel Execution Waves

```
v1.13.0 (Foundations — 8 tasks, MAX 10 concurrent):
├── T0:  Buff SDK 2.0 — Project Model, Conventions, Templates [deep]
├── T1:  Multi-file linking + Cross-compilation targets [deep]
├── T2:  Channel<T> MPSC primitive [deep]
├── T24: Stack traces with Buff spans [deep]
├── T3:  Macro system spike (5-day decision) [quick]
├── T4:  FFI safety guide [writing]
├── T6:  WGSL extensibility assessment [quick]
└── T53: comptime compile-time execution [deep]
   Ships as v1.13.0 when all 8 complete. Unblocks ALL downstream releases.

PARALLEL TRACK A (Critical Path — starts when v1.13 ships):

v1.14.0 (Compute + Mocking — 7 tasks):
├── T7:  buff-dataframe [deep]
├── T8:  buff-tensor [deep]           (needs T6 from v1.13)
├── T9:  buff-image [unspecified-high]
├── T10: buff-audio [unspecified-high]
├── T11: buff-dsp [unspecified-high]
├── T12: buff-ecs [deep]
└── T25: buff-mock [deep]

v1.22.0 (Domain Frameworks — 4 tasks, depends on v1.14):
├── T13: buff-science  (needs T8) [deep]
├── T14: buff-pipeline (needs T2) [unspecified-high]
├── T15: buff-ml       (needs T8) [deep]
└── T16: buff-game     (needs T12) [deep]

v1.23.0 (Integration + Flagship — 2 tasks, depends on v1.14+v1.15+v1.22):
├── T22: API compatibility spike [deep]
└── T23: Flagship Data Science Workbench [deep]

v1.24.0 (Audit — 1 task, depends on v1.23):
└── T28: Iterative audit until convergence [deep]

PARALLEL TRACK B (Wrappers + Common Frameworks — starts when v1.13 ships):

v1.15.0 (Production Wrappers + Security — 8 tasks):
├── T17: buff-web      [unspecified-high]
├── T18: buff-db       [unspecified-high]
├── T19: buff-template [quick]
├── T20: buff-reactive [deep]          (needs T2)
├── T21: buff-observe  [quick]
├── T26: buff-audit    [unspecified-high]
└── T27: buff-fuzz     [unspecified-high]

v1.16.0 (Tier 1 Common — 8 tasks): T29-T36 (validate/config/cache/cli/http-client/auth/jobs/resilience)
v1.17.0 (Tier 2 Common — 8 tasks): T37-T44 (fake/assertions/archive/fsm/pubsub/email/scrape/i18n)
v1.18.0 (Tier 3 Specialized — 8 tasks): T45-T52 (geo/nlp/chat/web3/crypto-extras/xml/msgpack/protobuf)

PARALLEL TRACK C (Language + DX + Community — starts when v1.13 ships):

v1.19.0 (Language Evolution — 7 tasks):
├── T54: SIMD types        (needs T6) [deep]
├── T55: Compile-speed     [deep]
├── T56: Property wrappers (needs T53+T20) [deep]
├── T57: Math syntax       [deep]
├── T58: Multiple dispatch [deep]
├── T59: Actor model       (needs T2+T20) [deep]
└── T60: Binary size       [deep]

v1.20.0 (Developer Experience — 7 tasks):
├── T61: Cold-start benchmarks [quick] ✅
├── T62: PGO support           [quick] ✅
├── T63: Error quality         (needs T24) [unspecified-high] ✅
├── T64: Hot reload            (needs T16+T17) [unspecified-high] ✅
├── T65: AI integration        [unspecified-high] ✅
├── T66: Refactoring tools     (needs T24) [unspecified-high] ✅
└── T67: Docs site             [unspecified-high] ✅

v1.21.0 (Community & Quality — 5 tasks, depends on v1.20 for T67):
├── T68: Cookbook         (needs T67) [writing]
├── T69: Onboarding paths (needs T67) [writing]
├── T70: Quality signals  [unspecified-high]
├── T71: Stability promise [writing]
└── T72: Plugin architecture (needs T53) [deep]

FINAL (After v1.24 ships — 4 parallel reviews, then user okay):
├── F1: Plan compliance audit (oracle)
├── F2: Code quality review (unspecified-high)
├── F3: Real manual QA (unspecified-high + playwright)
└── F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: v1.13 → v1.14 → v1.22 → v1.23 → v1.24 → F1-F4
Parallel Tracks B + C run alongside Track A (max 10 concurrent total)
```

### Dependency Matrix

| Task | Depends On | Blocks |
|---|---|---|
| T0 | - | T1, T7-T12, T17-T23, T25, T26, T27 (all need buff.toml v2 + conventions) |
| T1 | T0 (uses buff.toml v2) | T7-T12, T14, T17-T23, T25, T26 |
| T2 | - | T14, T20, T23 |
| T3 | - | (decision only) |
| T4 | - | T17-T21 (soft) |
| ~~T5~~ | ~~REMOVED~~ | — |
| T6 | - | T8, T13, T15 |
| T7 | T0, T1 | T22, T23 |
| T8 | T0, T1, T6 | T13, T15, T22, T23 |
| T9 | T0, T1 | - |
| T10 | T0, T1 | - |
| T11 | T0, T1 | - |
| T12 | T0, T1 | T16 |
| T13 | T8 | T23 |
| T14 | T1, T2 | T23 |
| T15 | T8 | T23 |
| T16 | T12 | - |
| T17 | T1 (soft: T4) | T22, T23 |
| T18 | T1 (soft: T4) | - |
| T19 | T1 (soft: T4) | - |
| T20 | T1 (soft: T4) | T22, T23 |
| T21 | T1 (soft: T4) | - |
| T22 | T7, T8, T14, T15, T17, T20, T25 | T23 |
| T23 | T7, T8, T13, T14, T15, T17, T20, T22 | F1-F4 |
| T24 | - | F3 (manual QA uses Buff traces) |
| T25 | T0, T1 | T22, T23 (test infra for flagship) |
| T26 | T0, T1 | - |
| T27 | T0, (T7-T12 frameworks to fuzz) | - |
| T28 | T23 (v1.23 complete) | F1-F4 (final verification after audit) |
| T29-T36 (v1.16) | T0, T1 (some also T4 soft, T2 for buff-jobs internal queue, T34 for buff-resilience retry composition) | F1-F4 |
| T37-T44 (v1.17) | T0, T1 (some also T2 for buff-pubsub, T4 for extern wrapping, T19 for buff-email templates, T25 for buff-fake in tests, T34 for buff-crypto-extras shared argon2) | F1-F4 |
| T45-T52 (v1.18) | T0, T1, T4 (some also T17 for buff-protobuf gRPC, T34 for buff-web3 auth) | F1-F4 |
| T53 (comptime) | T0, T1, T24 | T56, F1-F4 |
| T54 (SIMD) | T0, T1, T6 | F1-F4 |
| T55 (compile-speed) | T0, T1 | F1-F4 |
| T56 (property wrappers) | T0, T1, T20, T53 OR T3 (needs comptime or macros) | F1-F4 |
| T57 (math syntax) | T0, T1, T8 (tensor validates matrix literals) | F1-F4 |
| T58 (multiple dispatch) | T0, T1, T8, T13 | F1-F4 |
| T59 (actors) | T0, T1, T2, T20 | F1-F4 |
| T60 (binary size) | T0 | F1-F4 |
| T61 (cold-start) | T0, T1 | F1-F4 |
| T62 (PGO) | T0 | F1-F4 |
| T63 (error quality) | T0, T1, T24 | F1-F4 |
| T64 (hot reload) | T0, T1, T16, T17 | F1-F4 |
| T65 (AI integration) | T0, T1 | F1-F4 |
| T66 (refactoring) | T0, T1, T24 | F1-F4 |
| T67 (docs site) | T0 | F1-F4 |
| T68 (cookbook) | T67 | F1-F4 |
| T69 (onboarding) | T67 | F1-F4 |
| T70 (quality signals) | T0, T26 | F1-F4 |
| T71 (stability promise) | T0 | F1-F4 |
| T72 (plugin arch) | T0, T1, T53 | F1-F4 |

### Agent Dispatch Summary

- **v1.13.0 (8)**: T0 → `deep`, T1 → `deep`, T2 → `deep`, T24 → `deep`, T3 → `quick`, T4 → `writing`, T6 → `quick`, T53 → `deep`
- **v1.14.0 (7)**: T7 → `deep`, T8 → `deep`, T9-T11 → `unspecified-high`, T12 → `deep`, T25 → `deep`
- **v1.22.0 (4)**: T13 → `deep`, T14 → `unspecified-high`, T15 → `deep`, T16 → `deep`
- **v1.15.0 (7)**: T17-T18 → `unspecified-high`, T19 → `quick`, T20 → `deep`, T21 → `quick`, T26 → `unspecified-high`, T27 → `unspecified-high`
- **v1.23.0 (2)**: T22 → `deep`, T23 → `deep`
- **v1.24.0 (1)**: T28 → `deep`
- **v1.16.0 (8)**: T29-T36 → all `unspecified-high`
- **v1.17.0 (8)**: T37, T41 → `quick`; T38-T40, T42-T44 → `unspecified-high`
- **v1.18.0 (8)**: T50-T51 → `quick`; T45-T49, T52 → `unspecified-high`
- **v1.19.0 (7)**: T54-T60 → all `deep`
- **v1.20.0 (7)**: T61-T63 → `quick`/`unspecified-high`, T64-T67 → `unspecified-high`
- **v1.21.0 (5)**: T68-T69 → `writing`, T70 → `unspecified-high`, T71 → `writing`, T72 → `deep`
- **FINAL (4)**: F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high` + playwright, F4 → `deep`

---

## TODOs

> Implementation + Test = ONE Task. Never separate.
> Tasks T0-T59 MUST have: Recommended Agent Profile + Parallelization info + QA Scenarios.
> Tasks T60-T72 (quality/meta tasks) use COMPACT format: What / LOC budget / Deps / Acceptance + Commit. Full QA scenarios deferred to implementation time.
> A task WITHOUT QA Scenarios is INCOMPLETE for T0-T59. No exceptions for those.

- [x] 0. **Buff SDK 2.0 — Project Model, Conventions, Templates, and Reference Implementation**

  **Goal**: Establish the unified Buff developer experience — the .NET-Core-style evolution that lets frameworks compose consistently and users learn Buff once. Covers all 10 convention categories (A-J). Single comprehensive foundation task; everything in Wave 2-5 follows these conventions.

  **What to do** (sub-deliverables grouped by category A-J):

  **MUST ship** (blocks downstream tasks cleanly):
  - **(A1) `buff.toml` v2 schema** — extend existing `crates/buff-lang-cli/src/config.rs` (1132 lines, v1 schema) with: `[workspace]`, `[features]`, `[lints]`, `[profile.{dev,release,bench,test}]` (multiple profiles, not just release), `[prelude]` (project-wide implicit imports), `edition = "2026"` field. Backward-compatible with v1 manifests (v1 fields still parse; v2 adds new optional sections).
  - **(A2) Standard project layout** — document the canonical directory structure in `.sisyphus/decisions/sdk-conventions-v1x.md`:
    ```
    my_app/
    ├── buff.toml
    ├── src/{main.buff, lib.buff, modules/, prelude.buff}
    ├── tests/{unit, integration, snapshots, fixtures}
    ├── examples/
    ├── benches/
    ├── docs/
    ├── .github/workflows/ci.yml
    ├── .buff/vscode/settings.json
    └── buff.lock   (gitignored)
    ```
  - **(A3) Workspace support** — `buff.workspace.toml` for multi-crate projects (analogous to Cargo workspaces). Shared `[workspace.dependencies]`, per-crate `buff.toml` inherits.
  - **(A3b) Workspace dep inheritance** *(added post-comparative-analysis)* — `[workspace.dependencies]` and `[workspace.extern]` sections in `buff.workspace.toml` let monorepos declare deps once. Member crates write `my-dep.workspace = true` instead of repeating version. Prevents version drift across crates. Mirrors Cargo's well-loved pattern.
  - **(B1) `index.buff` barrel convention** — when a directory contains `index.buff`, importing the directory path re-exports from `index.buff`. Document + implement in T1's resolver.
  - **(C1) Templates for `buff new --template <name>`** — ship 7 built-in templates in `crates/buff-lang-cli/templates/`:
    - `console` (default binary)
    - `lib` (library crate)
    - `web` (buff-web + buff-template scaffold)
    - `ml` (buff-ml + buff-tensor project)
    - `game` (buff-game + buff-ecs project)
    - `pipeline` (buff-pipeline + buff-dataframe project)
    - `workspace` (multi-crate workspace skeleton)
    Each template ships: `buff.toml`, `src/main.buff` (or `src/lib.buff`), `tests/test_hello.buff`, `examples/hello.buff`, `README.md`, `.github/workflows/ci.yml`, `.gitignore`. Existing `--lib`/`--server`/`--gpu`/`--workspace` flags become aliases for the new templates (backward compat).
  - **(E1) Conventions specification** — commit `.sisyphus/decisions/sdk-conventions-v1x.md` as the canonical reference (target ~3000 words). Covers all 10 categories with examples.

  **SHOULD ship** (improves DX, deferable per item if LOC budget hit):
  - **(A4) Build profiles** — `[profile.dev]`/`[profile.release]`/`[profile.bench]` with `opt-level`, `lto`, `codegen-units`, `debug`, `panic`. `BUFF_PROFILE=prod buff run` selects non-default profile.
  - **(B2) `@internal` attribute** — parser accepts `@internal` on `export` decls; LSP / docs surface warning when used outside the declaring crate. Convention only (not enforced at compile time).
  - **(B3) Project prelude** — `[prelude]` section in `buff.toml` lists module paths whose `export`s become implicitly available in every file of the project. Codegen injects equivalent of `import * from "<path>"` at file head.
  - **(B4) `@feature(name)` conditional compilation** *(added post-comparative-analysis)* — parser accepts `@feature(name)` attribute on any `export` declaration; codegen emits only if `name` is enabled in buff.toml `[features]`. Mirrors Rust `#[cfg(feature = "...")]` and Go build tags. Required to make the `[features]` section of buff.toml actually useful in source code. Without this, features exist in manifest but cannot gate code.
  - **(C2) `buff gen` subcommand** — generators: `buff gen module <name>` (creates `src/modules/<name>.buff` + test file), `buff gen test <name>`, `buff gen example <name>`. Reduces boilerplate.
  - **(D1) `.env` auto-loading** — `env("KEY")` in prelude auto-reads `.env` if present in project root (dev profile only). Use `dotenvy` crate via extern.
  - **(D2) `.env.example` convention** — templates ship `.env.example` (committed) alongside `.env` (gitignored).
  - **(E2) Doc comments standard** — formalize `///` for items, `//!` for module-level. Update `buff check` to warn on undocumented `export`.
  - **(F1) Test layout enforcement** — `buff test` discovers `tests/unit/*.buff`, `tests/integration/*.buff`, `tests/snapshots/*` per convention. Document in spec.
  - **(F2) Test attribute standards** — parser accepts `@bench`, `@property`, `@should_panic`, `@ignore` alongside existing `@test`. Codegen lowers to appropriate Rust test attributes.
  - **(G1) SemVer strict validation** — `buff publish` rejects versions not matching SemVer 2.0 regex. Reuses existing v1.6 registry code.
  - **(G2) Stability badges** — `[package] stability = "experimental"|"beta"|"stable"|"locked"` in buff.toml; surfaced in registry.
  - **(G3) `@deprecated` attribute** — parser accepts `@deprecated(since = "X", replacement = "Y")`; codegen emits warning at call sites.
  - **(I1) CI template** — `.github/workflows/ci.yml` in every template: runs `buff fmt --check`, `buff check -D`, `buff test` on ubuntu/windows/macos matrix.
  - **(I2) `Dockerfile` template** — multi-stage build (builder + slim runtime) in `console` template.
  - **(J1) "Buff SDK 2.0" bundle definition** — document in spec what ships bundled (stdlib, buff-{web,db,template,reactive,observe} wrappers) vs what's `buff add`-able (buff-{ml,game,dataframe,tensor,etc.}).

  **NICE to ship** (scaffold only if time/LOC budget allows; otherwise defer to v1.18+):
  - **(E3) `buff doc` HTML generator** — scaffold command that emits per-crate HTML API docs (Rustdoc-style). Full rendering can defer to v1.18+; command must exist and produce placeholder.
  - **(E4) Doc tests** — extract fenced ```buff blocks from doc comments; run as tests via `buff test --doctest`.
  - **(H1) `.buff/vscode/settings.json` convention** — template writes format-on-save + LSP config. VSCode extension already exists from v1.2; this just standardizes per-project config.
  - **(H2) Snippet library** — VSCode extension gains snippets for `fn`, `match`, `Result<T,E>`, `Vector<T>`, struct/enum templates.
  - **(I3) `buff release` command** — bumps version (patch/minor/major), updates CHANGELOG.md, tags git, invokes `buff publish`. Convention: requires clean working tree.
  - **(D3) `buff.config.buff` programmatic config** — optional build-time Buff script that runs at `buff build` start. Advanced feature; scaffold only.

  **Must NOT do** (scope locks):
  - Do NOT exceed 6000 LOC + spec doc + template files. If budget hit, defer NICE-to-ship items to v1.18+.
  - Do NOT break v1.x buff.toml manifests — v2 schema is additive (new optional sections, no removed fields).
  - Do NOT remove existing `--lib`/`--server`/`--gpu`/`--workspace` flags on `buff new` — alias them to new templates.
  - Do NOT enforce `@internal` at compile time (convention only for v1.13-v1.17; enforcement deferred).
  - Do NOT build full HTML doc renderer (scaffold command only; rendering is v1.18+).
  - Do NOT change Buff language syntax (no new keywords, no new operators) — T0 is purely additive conventions + tooling.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Largest Wave 1 task; spans spec writing, parser/codegen extensions, CLI commands, template files, docs. Cross-cutting and high-impact.
  - **Skills**: [`/git-master`]
    - `/git-master`: Many files touched across CLI + templates + spec; atomic commits per sub-deliverable.

  **Parallelization**:
  - **Can Run In Parallel**: PARTIALLY — T0 can start immediately (spec + buff.toml v2 parser). T1 (multi-file linking) consumes buff.toml v2 schema, so T1 starts after T0's A1 deliverable. Other Wave 1 tasks (T2, T3, T4, T6) are independent of T0.
  - **Parallel Group**: Wave 1 (sequential entry: T0 starts first, T1 joins after T0-A1 done, T2/T3/T4/T6 parallel throughout)
  - **Blocks**: T1 (uses buff.toml v2 schema), all Wave 2-5 framework tasks (follow conventions), T23 (flagship uses templates + conventions)
  - **Blocked By**: None

  **References** (CRITICAL — be exhaustive):

  **Pattern References** (existing code to follow):
  - `crates/buff-lang-cli/src/config.rs` (1132 lines) — Existing v1 buff.toml parser using `toml::from_str` + serde derives. EXTEND this; do not rewrite. Specifically `BuffConfig`, `Package`, `ProfileOpts` structs.
  - `crates/buff-lang-cli/src/scaffold.rs` — Existing `buff new` / `buff init` scaffolding logic. Extend with template-selection dispatch.
  - `crates/buff-lang-cli/templates/desktop/` — Existing Tauri template (8 files); copy structure for new templates.
  - `crates/buff-lang-cli/src/cli.rs:Command enum` (~655 lines as of v1.9; was 499 in earlier versions — verify in preflight) — Where `buff gen`, `buff doc`, `buff release` subcommands plug in.
  - `crates/buff-lang-cli/src/commands/{build,run,new,init,fmt,check,add}.rs` — Existing command implementations; new commands follow same structure.
  - `AGENTS.md` (repo root) — Existing per-crate AGENTS.md convention; formalize as part of E1.

  **API/Type References**:
  - `crates/buff-lang-ast/src/decl.rs:Attribute parsing` — Existing `@test` attribute parser; extend for `@internal`, `@deprecated`, `@bench`, `@property`, `@should_panic`, `@ignore`.
  - `crates/buff-lang-types/src/prelude.rs:PreludeFn` — Existing prelude registration; `[prelude]` project config weaves into this.

  **External References**:
  - .NET project model: https://learn.microsoft.com/dotnet/core/project-sdk
  - Cargo manifest format: https://doc.rust-lang.org/cargo/reference/manifest.html
  - Rust editions: https://doc.rust-lang.org/edition-guide/
  - SemVer 2.0: https://semver.org/
  - dotenvy: https://docs.rs/dotenvy/latest/dotenvy/

  **WHY Each Reference Matters**:
  - config.rs: this is THE file being extended for buff.toml v2. Existing parser is robust; do not rewrite.
  - scaffold.rs: existing scaffold logic to plug templates into.
  - desktop template: existing 8-file template structure; copy for new templates.
  - cli.rs Command enum: where new subcommands register (clap derive).
  - Attribute parsing: existing `@test` shows the pattern; extend for 6 new attributes.

  **Acceptance Criteria**:

  **Per-sub-deliverable** (each MUST have its own QA scenario below):
  - [ ] A1: buff.toml v2 parses successfully (workspace/features/lints/multi-profile/prelude/edition)
  - [ ] A2: Conventions spec committed at `.sisyphus/decisions/sdk-conventions-v1x.md`
  - [ ] A3: Workspace support — `buff build` in workspace root builds all member crates
  - [ ] B1: `index.buff` barrel files work in T1's import resolver
  - [ ] C1: All 7 templates scaffold via `buff new --template <name> <project>`
  - [ ] E1: Spec document covers all 10 categories with examples
  - [ ] SHOULD-ship items each have their own acceptance scenario
  - [ ] NICE-to-ship items either shipped or explicitly marked "deferred to v1.18+" in spec

  **QA Scenarios** (MANDATORY):

  ```
  Scenario: buff.toml v2 parses with all new sections
    Tool: Bash (cargo)
    Preconditions: T0 merged
    Steps:
      1. Write test buff.toml containing:
         [package]
         name = "test"
         version = "1.0.0"
         edition = "2026"
         stability = "experimental"
         [workspace]
         members = ["crates/*"]
         [features]
         default = ["logging"]
         [lints]
         clippy = "deny"
         [profile.dev]
         opt-level = 0
         [profile.release]
         lto = true
         [prelude]
         modules = ["./src/prelude.buff"]
      2. Run: cargo run -p buff-lang-cli -- check <file using this manifest>
      3. Assert exit 0 (manifest parses without error)
      4. Assert no "unknown field" warnings (v2 schema accepts all sections)
    Expected Result: v2 manifest parses cleanly
    Failure Indicators: serde error on unknown field, missing required field
    Evidence: .sisyphus/evidence/task-0-buff-toml-v2/output.txt

  Scenario: v1 manifest still parses (backward compat)
    Tool: Bash
    Steps:
      1. Use existing examples/ola.buff-style project with v1 manifest (just [package] + name + version + edition)
      2. Run: cargo run -p buff-lang-cli -- check <file>
      3. Assert exit 0
    Expected Result: v1 manifest unchanged behavior
    Failure Indicators: v1 project breaks
    Evidence: .sisyphus/evidence/task-0-backward-compat/output.txt

  Scenario: Each of 7 templates scaffolds correctly
    Tool: Bash
    Steps:
      1. For each template in [console, lib, web, ml, game, pipeline, workspace]:
         a. Run: cargo run -p buff-lang-cli -- new test_<template> --template <template>
         b. Assert test_<template>/ directory created
         c. Assert buff.toml exists with template-appropriate config
         d. Assert src/main.buff (or src/lib.buff for lib template) exists
         e. Assert tests/test_hello.buff exists
         f. Assert examples/hello.buff exists
         g. Assert README.md exists
         h. Assert .github/workflows/ci.yml exists
      2. Cleanup test directories
    Expected Result: All 7 templates produce complete project skeletons
    Failure Indicators: Missing files, wrong template contents
    Evidence: .sisyphus/evidence/task-0-templates/{template-name}-listing.txt

  Scenario: Scaffolded console template builds and runs
    Tool: Bash
    Steps:
      1. cargo run -p buff-lang-cli -- new test_console --template console
      2. cd test_console
      3. cargo run -p buff-lang-cli -- run src/main.buff
      4. Assert stdout contains expected hello-world message
    Expected Result: Scaffolded project works out of the box
    Evidence: .sisyphus/evidence/task-0-console-runs/output.txt

  Scenario: Conventions spec exists and covers all categories
    Tool: Bash
    Steps:
      1. Assert file exists: .sisyphus/decisions/sdk-conventions-v1x.md
      2. Assert file contains headers for all 10 categories (A-J):
         - "Project & Build Conventions"
         - "Modules & Visibility"
         - "Scaffolding & Templates"
         - "Configuration & Environment"
         - "Documentation"
         - "Testing"
         - "Versioning & Compatibility"
         - "Editor & DX"
         - "CI/CD"
         - "Buff SDK Bundle"
      3. Assert each section has at least 200 words of content with examples
    Expected Result: Comprehensive spec document
    Failure Indicators: Missing sections, thin content
    Evidence: .sisyphus/evidence/task-0-spec/spec-toc.txt

  Scenario: Workspace support builds all members
    Tool: Bash
    Preconditions: A3 shipped
    Steps:
      1. cargo run -p buff-lang-cli -- new test_workspace --template workspace
      2. cd test_workspace
      3. Add two member crates via cargo run -p buff-lang-cli -- new crates/foo --template lib (repeat for bar)
      4. Run: cargo run -p buff-lang-cli -- build
      5. Assert exit 0; all member crates compiled
    Expected Result: Workspace mode builds all members
    Evidence: .sisyphus/evidence/task-0-workspace/output.txt

  Scenario: @deprecated attribute emits warning
    Tool: Bash
    Preconditions: G3 shipped
    Steps:
      1. Write test file with: @deprecated(since = "2.0", replacement = "new_fn")\nexport func old_fn() { ... }
      2. Write another file calling old_fn()
      3. Run: cargo run -p buff-lang-cli -- check <caller>
      4. Assert stderr contains: "warning: call to deprecated function 'old_fn' (since 2.0, use 'new_fn')"
    Expected Result: Deprecation warnings surface
    Evidence: .sisyphus/evidence/task-0-deprecated/warning.txt

  Scenario: buff gen module creates files
    Tool: Bash
    Preconditions: C2 shipped
    Steps:
      1. In a Buff project: cargo run -p buff-lang-cli -- gen module user
      2. Assert src/modules/user.buff created with module declaration stub
      3. Assert tests/unit/test_user.buff created with test stub
    Expected Result: Generator reduces boilerplate
    Evidence: .sisyphus/evidence/task-0-gen/output.txt
  ```

  **Commit**: YES (multiple atomic commits per sub-deliverable)
  - Suggested commit sequence:
    1. `docs(spec): Buff SDK 2.0 conventions specification` — spec doc only
    2. `feat(config): buff.toml v2 schema (workspace, features, lints, profiles, prelude, edition)`
    3. `feat(cli): workspace support + workspace.dependencies inheritance (buff.workspace.toml)` 
    4. `feat(cli): 7 built-in templates for buff new --template`
    5. `feat(cli): buff gen subcommand (module/test/example generators)`
    6. `feat(ast): @internal, @deprecated, @bench, @property, @should_panic, @ignore, @feature attributes`
    7. `feat(cli): @deprecated + @feature warning/gating in buff check`
    8. `feat(cli): stability badge in buff publish + registry`
    9. `feat(cli): buff doc scaffold command`
    10. `feat(cli): buff release command`
    11. `feat(vscode): .buff/vscode/settings.json convention + format-on-save`
    12. `feat(templates): CI workflow + Dockerfile templates`
  - Files: `crates/buff-lang-cli/{src/config.rs, src/cli.rs, src/scaffold.rs, src/commands/{gen,doc,release}.rs, templates/*}`, `crates/buff-lang-ast/src/decl.rs`, `crates/buff-lang-codegen-rust/src/rust_codegen.rs`, `.sisyphus/decisions/sdk-conventions-v1x.md`, `AGENTS.md` (cross-reference)
  - Pre-commit: `cargo test -p buff-lang-cli && cargo test -p buff-lang-ast`

---

- [x] 24. Stack Traces with Buff Spans (Source Map + Panic Hook)

  **What to do** *(added post-comparative-analysis — every scripting language solves this)*:
  - Create `crates/buff-lang-debug-info/` (new crate).
  - **Source map emission**: during codegen, emit a sidecar `.buffmap` file alongside each compiled binary. Maps Rust source spans (file:line:col) → Buff source spans (file:line:col). Includes both generated code locations and the original Buff identifiers.
  - **Panic hook interceptor**: register a `std::panic::panic_hook` automatically when Buff runtime initializes (transparent to user). On panic, the hook reads the `.buffmap`, walks the Rust panic backtrace, and remaps each frame to its Buff source location. Output is a Buff-stack-trace, not a Rust one.
  - **`buff backtrace` subcommand** — given a core dump / panic log, post-processes the Rust trace into a Buff trace (for offline debugging).
  - **Integration with v1.10 debugger (DAP)** — the DAP server (planned in tooling plan) consumes `.buffmap` so users step through Buff source, not Rust.
  - 3 examples demonstrating: simple panic shows Buff trace, async task panic shows Buff trace, nested calls show full call stack.
  - 10+ unit tests for span mapping correctness.
  - AGENTS.md + README.md.

  **Must NOT do**:
  - Do NOT modify the existing codegen output structure — source map is a SIDE-CAR file, not embedded in binary.
  - Do NOT require debug builds — release builds also get source maps (small file size, separate from binary).
  - Do NOT strip Rust traces entirely — always available via `RUST_BACKTRACE=1` env var as escape hatch.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Cross-cutting (codegen + runtime + CLI + DAP integration). Careful span tracking required.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T1, T2, T3, T4, T6 — independent foundation work)
  - **Parallel Group**: Wave 1
  - **Blocks**: F3 (manual QA uses Buff traces for evidence), v1.10 debugger integration (out of scope here, just provides the source map)
  - **Blocked By**: None (works with existing codegen; doesn't require multi-file linking)

  **References**:
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs` — Where source map emission hooks in (capture spans during codegen).
  - `crates/buff-lang-error/src/span.rs` — Existing Span type to use.
  - `crates/buff-lang-runtime/src/lib.rs` — Where panic hook registers at startup.

  **External References**:
  - Source-map spec (JS/TS): https://sourcemaps.info/spec.html
  - Python traceback module: https://docs.python.org/3/library/traceback.html
  - Rust panic hook: https://doc.rust-lang.org/std/panic/fn.set_hook.html

  **Acceptance Criteria**:

  ```
  Scenario: Panic produces Buff-source stack trace
    Tool: Bash (cargo)
    Steps:
      1. Write examples/debug/panic_demo.buff:
         "func helper() { panic_at_runtime() }\nfunc main() { helper() }"
         (where panic_at_runtime divides by zero or similar)
      2. Run: cargo run -p buff-lang-cli -- run examples/debug/panic_demo.buff
      3. Assert stderr contains:
         "thread 'main' panicked at examples/debug/panic_demo.buff:1"
         (NOT rust source file like "<__buff_generated_main>:3")
      4. Assert backtrace shows function name `helper` and `main` (not mangled)
    Expected Result: User sees Buff source locations, not Rust internals
    Failure Indicators: Rust source paths, mangled names, no span remapping
    Evidence: .sisyphus/evidence/task-24-panic-trace/output.txt

  Scenario: .buffmap sidecar file generated
    Tool: Bash (ls)
    Steps:
      1. cargo run -p buff-lang-cli -- build examples/debug/panic_demo.buff
      2. Assert file exists alongside binary: examples/debug/panic_demo.buffmap
      3. Assert file is valid JSON with span mappings
    Expected Result: Sidecar source map ships with binary
    Evidence: .sisyphus/evidence/task-24-buffmap/listing.txt

  Scenario: RUST_BACKTRACE=1 still shows Rust trace as escape hatch
    Tool: Bash
    Steps:
      1. RUST_BACKTRACE=1 cargo run -p buff-lang-cli -- run examples/debug/panic_demo.buff
      2. Assert FULL Rust backtrace also printed (after Buff trace)
    Expected Result: Escape hatch preserved for advanced debugging
    Evidence: .sisyphus/evidence/task-24-escape-hatch/output.txt
  ```

  **Commit**: YES
  - Message: `feat(debug): Buff-span stack traces via source map + panic hook`
  - Files: `crates/buff-lang-debug-info/**`, `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (sidecar emission), `crates/buff-lang-runtime/src/lib.rs` (panic hook), `crates/buff-lang-cli/src/commands/backtrace.rs`

---

- [x] 25. `buff-mock` — Mocking Framework for Testing

  **What to do** *(added post-comparative-analysis — every major language has one)*:
  - Create `crates/buff-mock/`.
  - Implement `Mock<Trait>` generic type — generates a mock impl of any trait for testing.
  - Implement API: `Mock.new()`, `mock.expect(method: "name").returning(value)`, `mock.expect(method: "name").times(n)`, `mock.verify()` (asserts all expected calls happened).
  - Implement `@mock` attribute on `let` bindings: `@mock let m: MyTrait = Mock.new()` — automatically generates mock impl.
  - Implement spy patterns: `mock.spy(method: "name")` records all calls + arguments for later inspection.
  - Codegen lowering: at test compile time, expand `Mock<MyTrait>` into a struct with `HashMap<CallSignature, ReturnValue>` + `Mutex<Vec<CallRecord>>` for thread-safe call recording.
  - 3 examples: hello mock, verify interaction, spy on calls.
  - 15+ tests (must test the mocking framework thoroughly — it's used by other tests).
  - AGENTS.md + README.md.

  **Must NOT do**:
  - Do NOT exceed 2500 LOC or 25 public functions.
  - Do NOT support mocking non-trait types (structs, enums) — only trait impls.
  - Do NOT require procedural macros (use codegen-time expansion only; consistent with T3 spike outcome of likely defer).
  - Do NOT support mocking `extern` functions (out of scope; mock at the Buff trait boundary instead).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Metaprogramming-like behavior (mock generation); careful design needed for ergonomics.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7-T12 — Wave 2 sibling)
  - **Parallel Group**: Wave 2
  - **Blocks**: T22 (API compat spike uses mocks), T23 (flagship tests use mocks)
  - **Blocked By**: T0 (conventions + @mock attribute), T1 (multi-file — test files are separate)

  **References**:
  - `crates/buff-lang-ast/src/decl.rs:TraitDecl` — Trait declarations that can be mocked.
  - Existing `@test` attribute pattern — `@mock` follows same parser path.

  **External References**:
  - mockall (Rust): https://docs.rs/mockall/latest/mockall/
  - unittest.mock (Python): https://docs.python.org/3/library/unittest.mock.html
  - Moq (C#): https://github.com/moq/moq

  **Acceptance Criteria**:

  ```
  Scenario: Mock a trait and verify interaction
    Tool: Bash
    Steps:
      1. Define trait: trait Greeter { required func greet(name: String) -> String; }
      2. In test: @mock let mock_greeter: Greeter = Mock.new()
         mock_greeter.expect(method: "greet").returning("hello world")
      3. Call mock_greeter.greet("buff")
      4. mock_greeter.verify()
      5. Assert verify passes
    Expected Result: Mock returns expected value, verify passes
    Evidence: .sisyphus/evidence/task-25-mock-verify/output.txt

  Scenario: Verify detects unmet expectations
    Tool: Bash
    Steps:
      1. Setup mock with expect times(2)
      2. Call method only once
      3. verify() should fail with "expected 2 calls, got 1"
    Expected Result: Verify correctly fails
    Evidence: .sisyphus/evidence/task-25-verify-fail/output.txt

  Scenario: Spy records call arguments
    Tool: Bash
    Steps:
      1. mock.spy(method: "greet")
      2. Call greet("alice"), greet("bob")
      3. Assert spy.calls() has 2 entries with correct args
    Expected Result: Spy captures all interactions
    Evidence: .sisyphus/evidence/task-25-spy/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-mock): MVP mocking framework with expect/verify/spy`
  - Files: `crates/buff-mock/**`, `crates/buff-lang-ast/src/decl.rs` (@mock attribute), `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (mock expansion)

---

- [x] 1. Multi-file Linking + Cargo Project Generation

  **What to do**:
  - Extend `buff-lang-cli` to compile multi-file Buff projects end-to-end: parse `import { x } from "./path.buff"` and `export` declarations across files, build a module graph, type-check across files, emit a single Cargo project (multi-file `src/`) and invoke `cargo build`/`cargo run`.
  - Add `buff build --project` (or auto-detect `buff.toml`) mode that walks the project, resolves imports relative to `buff.toml` location, handles circular imports (error with span), and emits Rust files preserving module structure.
  - Add cross-file type inference: a struct/trait/func declared in file A and `import`-ed in file B must be visible to type inference in B.
  - Add lexer/parser impact assessment substep: verify new keyword reservations (`stream`/`channel`/`select`/`macro` planned for T2/T3) don't conflict with existing identifiers. Reserve them in the lexer with backward-compat warnings (T5 handles the migration lint).
  - Update `buff new --lib` / `buff new --server` / `buff new --gpu` scaffolds to generate multi-file projects (lib + entry + at least one imported module).
  - Update `crates/buff-lang-cli/src/commands/build.rs` and `run.rs` to dispatch between single-file rustc mode (existing, for examples/) and project mode (new).
  - Add tests: circular import detection, missing import error with span, cross-file type inference, multi-file project builds and runs.
  - **Cross-compilation support** *(added post-comparative-analysis)* — `buff build --target <triple>` (e.g., `arm-unknown-linux-gnueabihf`, `x86_64-pc-windows-gnu`, `wasm32-wasi`). Reuses rustc's `--target` flag; documents which targets are "Buff-supported" (subset of Rust's tier 1/2 targets). Critical for embedded use case (user-mentioned). Initial supported set: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `wasm32-wasi`. Add `buff build --target list` to print available targets.

  **Must NOT do**:
  - Do NOT add a package manager (that's v1.6 territory, already shipped).
  - Do NOT change single-file rustc pipeline behavior (existing examples/ must still work via `buff run file.buff`).
  - Do NOT implement full Rust-style module visibility (`pub(crate)` etc.) — Buff uses `export` for public, everything else is private to the file.
  - Do NOT introduce new Cargo dependencies outside root `Cargo.toml [workspace.dependencies]`.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Multi-file linking is the critical-path foundation; requires deep understanding of lexer/parser/codegen interactions and careful design. High-difficulty architectural work.
  - **Skills**: [`/git-master`]
    - `/git-master`: Atomic commits per milestone; multi-file work touches many crates simultaneously.
  - **Skills Evaluated but Omitted**:
    - `/dotnet-clean-arch`, `/blazor-mudblazor`: Wrong stack (Buff is Rust).
    - `/postgresql-efcore`: No DB in this task.

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T2, T3, T4, T5, T6 — they're independent)
  - **Parallel Group**: Wave 1
  - **Blocks**: T7, T8, T9, T10, T11, T12, T14, T17-T23 (everything that needs multi-file)
  - **Blocked By**: None

  **References**:

  **Pattern References** (existing code to follow):
  - `crates/buff-lang-ast/src/decl.rs:ImportDecl, ExportDecl, ReexportDecl, ExternCrateDecl` — The AST nodes that already parse but don't link. Use these as-is; add resolution.
  - `crates/buff-lang-cli/src/pipeline.rs:compile_to_rust, compile_rust_to_exe` — Existing pipeline split; add `compile_project_to_cargo` alongside.
  - `crates/buff-lang-cli/src/commands/new.rs` — Existing scaffold logic for `buff new`; extend for multi-file templates.

  **API/Type References**:
  - `crates/buff-lang-parser/src/lib.rs:parse` — Single-file parse entry; wrap with `parse_project` that returns module graph.
  - `crates/buff-lang-types/src/lib.rs:TypeInferencer` — Extend to accept module graph for cross-file symbol resolution.

  **External References**:
  - Rust modules reference: https://doc.rust-lang.org/cargo/reference/workspaces.html — For generated Cargo workspace structure.

  **WHY Each Reference Matters**:
  - `decl.rs` ImportDecl: existing AST is the contract — DO NOT redefine, just resolve.
  - `pipeline.rs`: existing compile split lets you insert cargo-build step cleanly without breaking single-file mode.
  - `commands/new.rs`: scaffolds need updating so `buff new` produces multi-file projects by default (single-file is for examples/).

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Multi-file project builds and runs end-to-end
    Tool: Bash (cargo)
    Preconditions: Clean repo, v1.13-v1.17 branch checked out
    Steps:
      1. Run: cargo run -p buff-lang-cli -- new calc_app --lib
         (creates calc_app/ with buff.toml, src/main.buff, src/lib/math.buff)
      2. Manually edit src/lib/math.buff to contain:
         "export func add(a: Int, b: Int) -> Int { return a + b }"
      3. Manually edit src/main.buff to contain:
         "import { add } from "./lib/math.buff"\nfunc main() { print(add(2, 3)) }"
      4. Run: cargo run -p buff-lang-cli -- run src/main.buff
         (from calc_app/ directory)
      5. Assert stdout contains exactly: "5"
    Expected Result: Exit code 0, stdout "5"
    Failure Indicators: "unresolved import" error, link errors, missing add() symbol
    Evidence: .sisyphus/evidence/task-1-multifile-build/run-output.txt

  Scenario: Circular import detected with span
    Tool: Bash (cargo)
    Preconditions: Multi-file project from previous scenario
    Steps:
      1. Edit src/lib/math.buff to add: "import { something } from "../main.buff""
      2. Run: cargo run -p buff-lang-cli -- check src/main.buff
      3. Assert stderr contains "circular import" with file paths and line numbers
    Expected Result: Exit code non-zero, error message names both files + lines
    Failure Indicators: Stack overflow, silent success, missing span info
    Evidence: .sisyphus/evidence/task-1-circular-import/error.txt

  Scenario: Missing import gives helpful error
    Tool: Bash (cargo)
    Preconditions: Multi-file project
    Steps:
      1. Edit src/main.buff: "import { nonexistent } from "./lib/math.buff""
      2. Run: cargo run -p buff-lang-cli -- check src/main.buff
      3. Assert error message: "no symbol 'nonexistent' exported from ./lib/math.buff"
    Expected Result: Non-zero exit, clear error with file:line
    Failure Indicators: Generic "compilation failed", missing span
    Evidence: .sisyphus/evidence/task-1-missing-import/error.txt

  Scenario: Backward compat - single-file examples still work
    Tool: Bash (cargo)
    Preconditions: Clean repo
    Steps:
      1. Run: cargo run -p buff-lang-cli -- run examples/ola.buff
      2. Assert stdout contains "Olá, Mundo!"
      3. Run: cargo run -p buff-lang-cli -- run examples/fibonacci.buff
      4. Assert stdout contains "55"
    Expected Result: All v1.x examples still work unchanged
    Failure Indicators: Any example regression
    Evidence: .sisyphus/evidence/task-1-backward-compat/output.txt
  ```

  **Commit**: YES
  - Message: `feat(cli): multi-file project linking end-to-end`
  - Files: `crates/buff-lang-cli/src/commands/{build,run,new}.rs`, `crates/buff-lang-cli/src/pipeline.rs`, `crates/buff-lang-types/src/lib.rs`, `crates/buff-lang-parser/src/lib.rs`
  - Pre-commit: `cargo test -p buff-lang-cli`

---

- [x] 2. `Channel<T>` MPSC Primitive (REDUCED SCOPE — Stream/select deferred to v1.18+)

  **Scope reduction rationale** (post-user-feedback re-assessment):
  Buff already has comprehensive async coverage that eliminates the need for a general `Stream<T>` type or `select` expression in v1.13-v1.17:
  - `async func` + auto-propagation (no `await` keyword) — production
  - `spawn expr` → `Task<T>`, `task.result()` — production
  - TCP/UDP/WebSocket `.recv()` async network streams — production (covers external stream sources)
  - Sync `Vector<T>` + `for x in` iteration — covers batch streaming within process
  - Callbacks (e.g., `Effect.new(fn)`) — cover reactive patterns without streams
  The ONLY genuinely missing primitive is in-process `Channel<T>` MPSC for producer/consumer patterns (used by buff-pipeline T14 for inter-stage queues).

  **What to do**:
  - Add `Channel<T>` type to Buff as a prelude namespace module (NO new keyword — exposed as `Channel.new(buf_size)` method-style, consistent with `DateTime.now()` and `Regex.compile()` patterns from v1.4 stdlib).
  - Implement `Channel<T>` as `buff_lang_runtime::Channel<T>` wrapping `tokio::sync::mpsc::{Sender, Receiver}` pair. Hide tokio behind the runtime abstraction per Metis G6.
  - Implement API: `Channel.new(buf_size: Int) -> (Sender<T>, Receiver<T>)`, `sender.send(value: T) -> Result<(), Error>`, `receiver.recv() -> Option<T>` (async via auto-await), `receiver.close()`.
  - Type inference: register `Channel<T>` as generic type in `crates/buff-lang-types/src/prelude.rs` alongside existing prelude types.
  - Codegen lowering: emit-on-demand detection in `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (mirror existing v1.4 module pattern at line ~3129).
  - Add 2 examples in `examples/channels/`: `hello.buff` (basic send/recv), `producer_consumer.buff` (spawn producer + main consumer).
  - Add 8+ unit tests in `crates/buff-lang-runtime/tests/` covering: send/recv roundtrip, bounded backpressure, channel close returns None, multi-producer single-consumer ordering.
  - Add 3+ snapshot tests for codegen output.
  - Document memory model: Channel<T> is `Send + 'static` (matches tokio mpsc requirements).

  **Must NOT do** (v1.13-v1.17 scope lock):
  - Do NOT add `Stream<T>` general async iterable type — defer to v1.18+.
  - Do NOT add `select` expression — defer to v1.18+.
  - Do NOT add `stream`/`channel`/`select` as keywords — Channel is exposed as method-style (`Channel.new()`), no migration tool needed (T5 REMOVED).
  - Do NOT expose raw `tokio::sync::mpsc::*` paths to Buff users — wrap in `buff_lang_runtime::Channel` per Metis G6.
  - Do NOT implement async-aware locks (`tokio::sync::Mutex`) in MVP (sync `std::sync::Mutex` only).
  - Do NOT implement broadcast channels (single-consumer MPSC only for MVP; broadcast defer to v1.18+).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Still a language feature addition (runtime + types + codegen), but reduced from original scope. Careful design needed to ensure the API composes well with existing async primitives.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T1, T3, T4, T6)
  - **Parallel Group**: Wave 1
  - **Blocks**: T14 (buff-pipeline needs Channel for inter-stage queues), T23 (flagship uses pipeline)
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_spawn` (currently at line ~6860 — verify in preflight; was ~2647 in earlier versions, drift tracked by T28) — Existing `spawn expr` → `tokio::spawn(async move { expr })` lowering. Channel<T> follows similar wrapping pattern.
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` (currently at line ~3070) — DateTime/Log/Regex/etc. emit-on-demand detection pattern. Channel follows same template.
  - `crates/buff-lang-types/src/prelude.rs` — Where Channel<T> registers as prelude type.
  - `crates/buff-lang-runtime/src/lib.rs` — Where `pub struct Channel<T>` wrapper lives.

  **API/Type References**:
  - `crates/buff-lang-types/src/prelude.rs:PreludeType` — Prelude type registration; add Channel<T> as generic type.
  - `crates/buff-lang-ast/src/ty.rs:TypeRef` — Type reference for generic param T.

  **External References**:
  - Tokio mpsc docs: https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html — Underlying primitive being wrapped.

  **WHY Each Reference Matters**:
  - lower_spawn: shows existing pattern for hiding tokio behind Buff syntax — copy this approach for Channel.
  - v1.4 stdlib pattern: emit-on-demand detection is the template for all new prelude modules.
  - prelude.rs: registration point for Channel<T> as a Buff-visible type.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Channel send/recv basic roundtrip
    Tool: Bash (cargo)
    Preconditions: T1 merged (multi-file) OR single-file test
    Steps:
      1. Write examples/channels/hello.buff:
         "func main() {\n  let (sender, receiver) = Channel.new(10)\n  spawn { sender.send(42) }\n  let x = receiver.recv()\n  print(x)\n}"
      2. Run: cargo run -p buff-lang-cli -- run examples/channels/hello.buff
      3. Assert stdout: "Some(42)"
    Expected Result: Channel MPSC works end-to-end via auto-await
    Failure Indicators: Compile error, runtime panic, "None" output (channel closed early)
    Evidence: .sisyphus/evidence/task-2-channel/hello-output.txt

  Scenario: Bounded backpressure blocks sender when full
    Tool: Bash (cargo)
    Steps:
      1. Create Channel.new(2) (buffer = 2)
      2. Spawn a slow consumer that reads one item per 100ms
      3. Producer sends 5 items rapidly
      4. Assert producer blocks (test via timing marker or via spawn+result pattern)
      5. Assert all 5 items eventually arrive in consumer
    Expected Result: Bounded channel applies backpressure correctly
    Failure Indicators: Producer never blocks (unbounded behavior), items lost
    Evidence: .sisyphus/evidence/task-2-channel/backpressure.txt

  Scenario: recv returns None on closed channel
    Tool: Bash (cargo)
    Steps:
      1. Create channel
      2. Drop sender (or call close)
      3. Call receiver.recv() — should return None
      4. Assert output: "None"
    Expected Result: Channel close semantics work
    Evidence: .sisyphus/evidence/task-2-channel/close.txt

  Scenario: Multi-producer single-consumer preserves ordering per producer
    Tool: Bash (cargo)
    Steps:
      1. Create channel
      2. Spawn 3 producers each sending items 1, 2, 3
      3. Consumer collects 9 items
      4. Assert: items from each producer arrive in their send order (interleaving across producers is allowed)
    Expected Result: MPSC ordering guarantee holds per-producer
    Evidence: .sisyphus/evidence/task-2-channel/mpsc-order.txt

  Scenario: Backward compat — existing async still works
    Tool: Bash (cargo)
    Steps:
      1. Run: cargo run -p buff-lang-cli -- run examples/async_demo.buff
      2. Assert existing spawn + task.result() behavior unchanged
    Expected Result: Existing async example still passes (no regression)
    Evidence: .sisyphus/evidence/task-2-channel/backward-compat.txt
  ```

  **Commit**: YES
  - Message: `feat(runtime): add Channel<T> MPSC primitive (Stream/select deferred to v1.18+)`
  - Files: `crates/buff-lang-runtime/src/{lib.rs, channel.rs}`, `crates/buff-lang-types/src/prelude.rs`, `crates/buff-lang-codegen-rust/src/rust_codegen.rs`, `examples/channels/*.buff`
  - Pre-commit: `cargo test --workspace`

---

- [x] 3. Macro System Spike (Timeboxed Decision)

  **What to do**:
  - Conduct a 5-day timeboxed investigation to DECIDE whether Buff needs a macro system in v1.13-v1.17, or if frameworks can ship without one.
  - Investigate use cases: ORM compile-time SQL validation (buff-db), routing table generation (buff-web), automatic Serialize/Deserialize derives, JSON schema derivation.
  - For each use case, document the NON-MACRO WORKAROUND: runtime route registration (Vec<Route> + match), runtime query building (sqlx::query without macro), manual `impl Serialize for Foo`.
  - Prototype ONE macro design (recommend declarative `macro_rules!`-style over procedural macros — simpler implementation in hand-rolled parser).
  - Estimate implementation cost: LOC, weeks, lexer/parser impact.
  - Write decision document to `.sisyphus/decisions/macro-system-v1x.md` with verdict: SHIP-IN-ROADMAP | DEFER-POST-v1.17.
  - Decision rule (LOCK THIS): DEFER if (implementation > 1500 LOC) OR (< 2 frameworks genuinely require it for MVP) OR (spike exceeds 5 days).

  **Must NOT do**:
  - Do NOT implement the full macro system in this task — spike only.
  - Do NOT change Buff syntax based on this spike — that's follow-on work if verdict is SHIP.
  - Do NOT exceed 5-day timebox. If can't decide in 5 days, default is DEFER.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Spike with bounded scope; output is a decision doc, not production code.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with all Wave 1)
  - **Parallel Group**: Wave 1
  - **Blocks**: None directly (decision unblocks future macro work, but v1.13-v1.17 frameworks proceed either way using workarounds)
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `crates/buff-lang-parser/src/lib.rs` — Existing hand-rolled parser; assess extensibility for macro syntax.
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:extern_*_handling` — How extern is wired today; macros would extend this pattern.

  **API/Type References**: N/A — decision output.

  **External References**:
  - Rust macro_rules! reference: https://doc.rust-lang.org/reference/macros-by-example.html
  - Rust procedural macros: https://doc.rust-lang.org/reference/procedural-macros.html

  **WHY Each Reference Matters**:
  - Parser internals: spike must assess whether declarative macros can fit the existing recursive-descent approach.
  - extern handling: macros are conceptually similar to extern FFI (compile-time code generation) — assess if pattern extends.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Decision document committed with verdict
    Tool: Bash (ls + cat)
    Preconditions: 5-day spike complete
    Steps:
      1. Assert file exists: .sisyphus/decisions/macro-system-v1x.md
      2. Cat the file and assert it contains:
         - "Verdict:" line followed by "SHIP-IN-ROADMAP" or "DEFER-POST-v1.17"
         - "Use Cases Analyzed:" section with ≥3 use cases
         - "Per-Use-Case Workaround" section
         - "Cost Estimate" with LOC and weeks
         - "Decision Rule Applied" section
      3. Assert verdict aligns with decision rule (DEFER if LOC >1500 OR frameworks <2)
    Expected Result: Decision document exists with all required sections
    Failure Indicators: Missing file, missing verdict, missing sections
    Evidence: .sisyphus/evidence/task-3-decision/decision-doc.md (copy of source)

  Scenario: Workaround documented per framework
    Tool: Bash
    Steps:
      1. Grep the decision doc for each framework name (buff-web, buff-db, buff-reactive, buff-template, buff-ml)
      2. Assert each has a "Without Macros:" workaround paragraph
    Expected Result: Every framework has a documented non-macro path
    Evidence: .sisyphus/evidence/task-3-workaround-coverage.txt
  ```

  **Commit**: YES
  - Message: `docs(spike): macro system v1.13-v1.17 decision (verdict: <SHIP|DEFER>)`
  - Files: `.sisyphus/decisions/macro-system-v1x.md`

---

- [x] 4. FFI Safety Guide + Conventions Document

  **What to do**:
  - Write `crates/buff-lang-ffi-guide/` as a documentation-only crate (or `docs/ffi-guide.md` — pick one consistent with existing convention; verify `crates/` is preferred location per AGENTS.md).
  - Define hard rules for `extern` FFI usage in Buff frameworks:
    1. **No raw pointer exposure** to Buff users. All `*const T`/`*mut T` must be wrapped in safe Buff types.
    2. **Ownership boundary**: clearly document who owns memory crossing the Buff/Rust boundary. Recommend: Rust owns all heap memory; Buff sees only borrowed views.
    3. **Error mapping**: Rust `Result<T, E>` must lower to Buff `Result<T, BuffError>` with span-aware error messages.
    4. **Thread safety**: which Rust types can cross `spawn` boundaries (must be `Send + 'static`).
    5. **Lifetime hiding**: Rust lifetimes must NOT appear in Buff types. Use owned types (`String`, `Vec<T>`) or `'static` references only.
    6. **Panic boundary**: Rust panics in extern functions must be caught (`std::panic::catch_unwind`) and converted to Buff errors, NEVER propagate to Buff code.
  - Provide 3+1 reference implementations of safe FFI patterns:
    - Example 1: Wrapping a simple function (e.g., `url::Url::parse`)
    - Example 2: Wrapping a stateful struct (e.g., `regex::Regex`)
    - Example 3: Wrapping an async function (e.g., `reqwest::get`)
    - Example 4: Anti-pattern — what NOT to do (raw pointer exposure)
  - Document the wrapper-crate pattern that Wave 4 production wrappers (T17-T21) should follow.

  **Must NOT do**:
  - Do NOT write any production framework code — guide only.
  - Do NOT mandate a specific FFI pattern without justification (rules must have rationale + example).
  - Do NOT skip anti-patterns (negative examples are critical).

  **Recommended Agent Profile**:
  - **Category**: `writing`
    - Reason: Documentation task with technical depth. Writing category handles prose-heavy work well.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with all Wave 1)
  - **Parallel Group**: Wave 1
  - **Blocks**: T17-T21 (wrappers reference guide) — soft block; wrappers can proceed without but should follow guide
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `examples/extern_serde_json.buff`, `examples/extern_tokio.buff`, `examples/extern_reqwest.buff` — Existing extern FFI patterns to document formally.
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:extern_func handling` — How extern is lowered today.

  **External References**:
  - Rust FFI guide: https://doc.rust-lang.org/nomicon/ffi.html
  - Rust unsafe guidelines: https://rust-lang.github.io/unsafe-code-guidelines/

  **WHY Each Reference Matters**:
  - extern_*.buff examples: ground the guide in real existing usage.
  - rust_codegen extern handling: shows current unsafe-wrapping convention (compiler auto-inserts `unsafe { }`).

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: FFI guide exists with all required sections
    Tool: Bash (cat + grep)
    Preconditions: T4 merged
    Steps:
      1. Assert file exists at crates/buff-lang-ffi-guide/GUIDE.md (or docs/ffi-guide.md)
      2. Assert it contains headers for all 6 hard rules:
         - "No Raw Pointer Exposure"
         - "Ownership Boundary"
         - "Error Mapping"
         - "Thread Safety"
         - "Lifetime Hiding"
         - "Panic Boundary"
      3. Assert 4 reference examples present (3 safe + 1 anti-pattern)
    Expected Result: All sections and examples present
    Failure Indicators: Missing rule, missing example, no anti-pattern
    Evidence: .sisyphus/evidence/task-4-ffi-guide/guide-toc.txt

  Scenario: Guide is referenced from root AGENTS.md
    Tool: Bash (grep)
    Steps:
      1. Grep AGENTS.md for "ffi-guide" or "FFI Safety"
      2. Assert a link/reference exists
    Expected Result: AGENTS.md cross-references the FFI guide
    Evidence: .sisyphus/evidence/task-4-ffi-guide/agents-md-ref.txt
  ```

  **Commit**: YES
  - Message: `docs(ffi): buff-lang-ffi-guide with safety rules and examples`
  - Files: `crates/buff-lang-ffi-guide/GUIDE.md`, `crates/buff-lang-ffi-guide/Cargo.toml`, `AGENTS.md` (cross-reference)

---

- [x] ~~5. Backward-Compat Lint + `buff fix --v1-to-v1x` Migration Tool~~ — **REMOVED**

  **Rationale for removal**: This task existed to migrate v1.x Buff source files that used identifiers conflicting with newly-reserved keywords (`stream`, `channel`, `select`, `macro`). After user-feedback-driven re-assessment:
  - T2 was reduced to expose `Channel<T>` as a method-style prelude module (`Channel.new(buf_size)`) consistent with `DateTime.now()` / `Regex.compile()` from v1.4 stdlib. **No new keyword `channel` is reserved.**
  - `stream` and `select` keywords are NOT being added in v1.13-v1.17 (Stream<T> and select expression deferred to v1.18+).
  - `macro` keyword addition is contingent on T3 spike outcome — IF T3 returns SHIP-IN-ROADMAP and that requires a new keyword, this task should be re-instated at that point with scope limited to the single keyword.
  
  **Conclusion**: With no new keywords being added in Wave 1, there is no identifier-migration work to do. v1.x Buff source code remains 100% compatible with v1.13-v1.17 at the syntactic level. Backward compatibility is therefore preserved by *not changing the syntax* rather than by providing a migration tool.
  
  **No action required**. (Strikethrough preserved to maintain task numbering T1-T23 + F1-F4 stable.)

---

- [x] 6. WGSL Extensibility Assessment (Decision Document)

  **What to do**:
  - Assess whether existing `crates/buff-lang-codegen-wgsl/` can be extended to support tensor operations needed by `buff-tensor` (T8), `buff-science` (T13), and `buff-ml` (T15).
  - Document current WGSL codegen capabilities: single-param numeric map lambdas, fixed binding layout, workgroup size 64, no f64.
  - Identify GAPS for tensor ops: matrix multiplication (needs 2D indexing), reductions (needs workgroup shared memory), convolutions, autodiff graph (probably CPU-only for MVP).
  - For each gap, document: (a) feasibility of WGSL implementation, (b) estimated LOC, (c) CPU fallback path if GPU not viable.
  - Write decision document to `.sisyphus/decisions/wgsl-extensibility-v1x.md` with per-framework GPU strategy:
    - buff-tensor: GPU yes/no, what ops
    - buff-science: GPU yes/no, what ops
    - buff-ml: GPU yes/no, what ops
    - buff-image: CPU-only (per G7 lock)
    - buff-dsp: CPU-only (per G7 lock)

  **Must NOT do**:
  - Do NOT implement any WGSL extensions — assessment only.
  - Do NOT block Wave 2 start — T8 (tensor) can proceed with CPU-only MVP if assessment says GPU is hard.
  - Do NOT exceed 3-day timebox.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Focused assessment of one crate's capabilities; output is decision doc.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with all Wave 1)
  - **Parallel Group**: Wave 1
  - **Blocks**: T8 (tensor), T13 (science), T15 (ML) — they consume decision
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `crates/buff-lang-codegen-wgsl/src/lib.rs` — All ~478 lines (was ~446 in earlier versions — verify in preflight); the WGSL codegen under assessment.
  - `crates/buff-lang-runtime/src/gpu.rs, gpu_pipeline.rs` — GPU dispatch host; how WGSL is invoked.
  - `crates/buff-lang-ast/src/ir.rs` — IR types feeding WGSL codegen.

  **External References**:
  - WGSL spec: https://www.w3.org/TR/WGSL/
  - wgpu examples: https://github.com/gfx-rs/wgpu/tree/trunk/wgpu/examples

  **WHY Each Reference Matters**:
  - codegen-wgsl lib.rs: this is THE artifact under assessment.
  - gpu*.rs: how WGSL is invoked; need to understand dispatch flow to assess extensions.
  - ir.rs: what input shape WGSL codegen expects; tensor ops may need richer IR.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: WGSL assessment decision document exists
    Tool: Bash (ls + cat)
    Steps:
      1. Assert file exists: .sisyphus/decisions/wgsl-extensibility-v1x.md
      2. Assert sections present:
         - "Current WGSL Capabilities"
         - "Tensor Op Gaps" (matrix mul, reduction, convolution, autodiff)
         - "Per-Framework GPU Strategy" (tensor, science, ml, image, dsp)
         - "Per-Gap Cost Estimate" (LOC + days)
         - "CPU Fallback Paths"
    Expected Result: All sections present, decisions per framework
    Failure Indicators: Missing file, missing sections, vague conclusions
    Evidence: .sisyphus/evidence/task-6-wgsl-assessment/decision-doc.md

  Scenario: Decision feeds T8 tensor task
    Tool: Bash
    Steps:
      1. Grep decision doc for "buff-tensor" 
      2. Assert explicit "GPU: YES" or "GPU: NO" verdict with rationale
    Expected Result: Tensor GPU strategy is unambiguous
    Evidence: .sisyphus/evidence/task-6-tensor-strategy.txt
  ```

  **Commit**: YES
  - Message: `docs(spike): WGSL extensibility assessment for v1.13-v1.17 frameworks`
  - Files: `.sisyphus/decisions/wgsl-extensibility-v1x.md`

---

- [x] 7. `buff-dataframe` — Columnar DataFrames + Lazy Execution

  **What to do**:
  - Create `crates/buff-dataframe/` with Cargo.toml (workspace deps, edition 2021, MIT OR Apache-2.0, version 2.0.0).
  - Implement `DataFrame` type: schema-aware columnar storage (Vec<Series>), supports `Int`/`Float`/`String`/`Bool` columns.
  - Implement core ops: `DataFrame.from_csv(path)`, `DataFrame.from_json(path)`, `df.select(cols)`, `df.filter(predicate)`, `df.group_by(col).agg(...)`, `df.join(other, on)`, `df.sort(col)`, `df.head(n)`, `df.len()`.
  - Implement lazy execution: build query plan, execute on `df.collect()`. (Defer to v1.18+ if LOC budget exceeded.)
  - Leverage v1.4 stdlib: use `Path.exists()`, existing CSV module where possible.
  - Codegen lowering in `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (emit-on-demand detection like v1.4 stdlib).
  - Extern FFI to `polars` crate (lazy, via feature-gated dependency) for高性能 ops if needed; otherwise pure Buff+Rust implementation.
  - Add 3 examples in `examples/dataframe/`: `hello.buff` (load CSV, print head), `analysis.buff` (group_by + agg), `join.buff` (two CSVs joined).
  - Add 10+ unit tests in `crates/buff-dataframe/tests/`.
  - Add 5+ snapshot tests for codegen output.
  - Write `crates/buff-dataframe/AGENTS.md` per per-crate convention.
  - Write `crates/buff-dataframe/README.md` with install + hello-world.
  - Register on Buff registry with `"experimental"` badge.

  **Must NOT do**:
  - Do NOT exceed 2500 LOC or 25 public functions.
  - Do NOT implement Parquet reader (defer to v1.18+ — CSV/JSON only for MVP).
  - Do NOT implement streaming/chunked DataFrames (load entire CSV into memory).
  - Do NOT use GPU (CPU-only per Metis G7).
  - Do NOT add `polars` as direct dependency if it requires complex build setup — wrap simple parts only.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Foundational framework that flagship depends on; needs careful API design + codegen integration. Many moving parts (schema, ops, codegen, FFI).
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T8-T12)
  - **Parallel Group**: Wave 2
  - **Blocks**: T22 (API spike), T23 (flagship)
  - **Blocked By**: T1 (multi-file linking)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:v1.4 stdlib modules` (e.g., DateTime lowering at line 3129) — Copy this codegen pattern: emit-on-demand detection, fully-qualified Rust paths.
  - `examples/collections.buff` — Existing Vector<T>/Map usage; DataFrame extends this idiom.
  - `crates/buff-lang-types/src/prelude.rs` — How prelude types register; DataFrame may or may not be prelude (decide: implicit like DateTime vs `import`-only).

  **API/Type References**:
  - Polars API (if used): https://docs.rs/polars/latest/polars/
  - Pandas API (conceptual): group_by, filter, select patterns

  **External References**:
  - Polars: https://crates.io/crates/polars
  - CSV crate: https://docs.rs/csv/latest/csv/

  **WHY Each Reference Matters**:
  - rust_codegen v1.4 pattern: existing emit-on-demand pattern is the template for all v1.13-v1.17 frameworks. Copy it.
  - collections.buff: idiomatic collection usage to extend.
  - prelude.rs: registration pattern (decide implicit vs import).

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Load CSV and print head
    Tool: Bash (cargo)
    Steps:
      1. Create test CSV at examples/dataframe/sample.csv with columns: name,age,city
      2. Write examples/dataframe/hello.buff:
         "func main() {\n  let df = DataFrame.from_csv(\"examples/dataframe/sample.csv\")\n  print(df.head(5))\n}"
      3. Run: cargo run -p buff-lang-cli -- run examples/dataframe/hello.buff
      4. Assert stdout contains header + 5 rows
    Expected Result: CSV loads, head() returns 5 rows
    Failure Indicators: Compile error, runtime panic, missing rows
    Evidence: .sisyphus/evidence/task-7-csv-load/output.txt

  Scenario: Group by + aggregate
    Tool: Bash
    Steps:
      1. Write examples/dataframe/analysis.buff using df.group_by("city").agg({"age": "mean"})
      2. Run it
      3. Assert output shows mean age per city
    Expected Result: Group-by aggregation produces correct result
    Evidence: .sisyphus/evidence/task-7-groupby/output.txt

  Scenario: Two CSVs join correctly
    Tool: Bash
    Steps:
      1. Create users.csv (id, name) and orders.csv (user_id, total)
      2. Join on id == user_id
      3. Assert result has all columns from both, correct row count
    Expected Result: Inner join works
    Evidence: .sisyphus/evidence/task-7-join/output.txt

  Scenario: Error on missing CSV
    Tool: Bash
    Steps:
      1. Call DataFrame.from_csv("nonexistent.csv")
      2. Assert buff check produces span-aware error
    Expected Result: Graceful error, no panic
    Evidence: .sisyphus/evidence/task-7-error/error.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-dataframe): MVP columnar DataFrames with CSV/JSON load + ops`
  - Files: `crates/buff-dataframe/**`, `crates/buff-lang-codegen-rust/src/rust_codegen.rs` (DataFrame lowering), `examples/dataframe/*.buff`, root `Cargo.toml` (workspace deps)
  - Pre-commit: `cargo test -p buff-dataframe && cargo test -p buff-lang-codegen-rust`

---

- [x] 8. `buff-tensor` — N-dim Arrays + GPU Dispatch

  **What to do**:
  - Create `crates/buff-tensor/` with standard Cargo.toml.
  - Implement `Tensor<T>` type: N-dimensional array (rank ≤ 4 for MVP), dtype f32 only (defer f64/i64 to v1.18+).
  - Implement core ops: `Tensor.zeros(shape)`, `Tensor.from_vec(data, shape)`, `t.shape()`, `t.get(indices)`, `t.set(indices, val)`, `t.reshape(shape)`, `t.transpose()`.
  - Implement math ops: elementwise (`+`, `-`, `*`, `/`), matmul (2D × 2D), reduce (sum, mean, max along axis).
  - GPU dispatch: per T6 decision. If YES, extend `crates/buff-lang-codegen-wgsl` for matmul/reduce; if NO, CPU-only via rayon.
  - Codegen lowering: emit-on-demand pattern (like v1.4 stdlib).
  - Add 3 examples: `hello.buff` (create + print), `matmul.buff` (matrix multiply), `reduce.buff` (sum along axis).
  - Add 15+ unit tests (math-heavy → proptest required for numeric ops).
  - Add 5+ snapshot tests for codegen.
  - Write AGENTS.md + README.md.
  - Register with "experimental" badge.

  **Must NOT do**:
  - Do NOT exceed 2500 LOC or 25 public functions.
  - Do NOT implement autodiff (that's T15 buff-ml).
  - Do NOT support rank > 4 or dtype other than f32 (defer to v1.18+).
  - Do NOT implement sparse tensors (defer to v1.18+).
  - Do NOT implement distributed tensors (defer to v1.19+).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Math-heavy foundational framework; careful API design + GPU integration. T15 (ML) builds on this.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7, T9-T12)
  - **Parallel Group**: Wave 2
  - **Blocks**: T13 (science), T15 (ML), T22, T23
  - **Blocked By**: T1 (multi-file), T6 (WGSL decision)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-runtime/src/cpu.rs:CpuDispatcher` — Existing parallel dispatch for par_map etc.; reuse for tensor elementwise ops.
  - `crates/buff-lang-runtime/src/gpu.rs, gpu_pipeline.rs` — Existing wgpu dispatch flow.
  - `crates/buff-lang-codegen-wgsl/src/lib.rs` — WGSL codegen to extend (if T6 says GPU YES).

  **API/Type References**:
  - `crates/buff-lang-types/src/prelude.rs` — Tensor may register here as prelude namespace.

  **External References**:
  - Candle (HuggingFace): https://github.com/huggingface/candle — reference Rust tensor lib
  - NDArray: https://docs.rs/ndarray/latest/ndarray/
  - wgpu compute examples: https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/examples/

  **WHY Each Reference Matters**:
  - cpu.rs CpuDispatcher: existing rayon wrapper; reuse for tensor parallelism.
  - gpu*.rs: how GPU dispatch works today; tensor ops follow same flow.
  - codegen-wgsl lib.rs: extension point for matmul/reduce shaders.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Create and inspect tensor
    Tool: Bash
    Steps:
      1. Write examples/tensor/hello.buff: "let t = Tensor.zeros([3, 4]); print(t.shape())"
      2. Run it
      3. Assert stdout: "[3, 4]"
    Expected Result: Tensor creation + shape query work
    Evidence: .sisyphus/evidence/task-8-hello/output.txt

  Scenario: Matrix multiplication produces correct result
    Tool: Bash
    Steps:
      1. Create two 2x2 tensors: a = [[1,2],[3,4]], b = [[5,6],[7,8]]
      2. Compute c = a.matmul(b)
      3. Assert c == [[19, 22], [43, 50]]
    Expected Result: Matmul correctness verified
    Evidence: .sisyphus/evidence/task-8-matmul/output.txt

  Scenario: Reduction along axis
    Tool: Bash
    Steps:
      1. Create tensor [[1,2,3],[4,5,6]]
      2. Reduce sum along axis 0: result = [5, 7, 9]
      3. Reduce sum along axis 1: result = [6, 15]
    Expected Result: Axis-aware reduction works
    Evidence: .sisyphus/evidence/task-8-reduce/output.txt

  Scenario: GPU dispatch (if T6 says YES)
    Tool: Bash
    Steps:
      1. Create large tensor (1M elements)
      2. Elementwise multiply
      3. Assert GPU path taken (check log) and result correct
    Expected Result: GPU dispatch works for large tensors
    Failure Indicators: Silent CPU fallback without log
    Evidence: .sisyphus/evidence/task-8-gpu/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-tensor): MVP N-dim arrays with matmul + reduce + GPU dispatch`

---

- [x] 9. `buff-image` — Image Codecs + Pixel Ops (CPU-only MVP)

  **What to do**:
  - Create `crates/buff-image/` with standard Cargo.toml.
  - Implement `Image` type: 2D pixel array, supports RGB/RGBA, u8 channel.
  - Implement core ops: `Image.from_path(path)`, `Image.from_bytes(bytes)`, `img.width()`, `img.height()`, `img.get_pixel(x, y)`, `img.set_pixel(x, y, color)`, `img.save(path)`.
  - Implement filters: `img.grayscale()`, `img.invert()`, `img.resize(w, h)`, `img.crop(x, y, w, h)`, `img.blur(sigma)`.
  - Extern FFI to `image` crate (Rust) for codecs; safe wrapper per T4 FFI guide.
  - Codegen lowering: emit-on-demand.
  - 3 examples: `hello.buff` (load + info), `filter.buff` (grayscale pipeline), `resize.buff` (resize image).
  - 10+ unit tests; 5+ snapshots.
  - AGENTS.md + README.md; register with "experimental" badge.

  **Must NOT do**:
  - Do NOT exceed 2500 LOC or 25 public functions.
  - Do NOT use GPU (CPU-only per Metis G7 lock).
  - Do NOT implement exotic formats (DICOM, RAW, etc.) — PNG/JPEG/GIF/BMP/WebP only via `image` crate.
  - Do NOT implement custom codecs (use extern).

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Well-scoped framework with extern FFI wrapper pattern; high effort but not architecturally novel.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7, T8, T10-T12)
  - **Parallel Group**: Wave 2
  - **Blocks**: None directly (image is standalone)
  - **Blocked By**: T1 (multi-file), T4 (FFI guide — soft)

  **References**:

  **Pattern References**:
  - `examples/extern_serde_json.buff` — Extern FFI pattern for wrapping Rust crates; same pattern for `image` crate.
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:Process module` — Wrap stateful Rust type (Regex pattern); copy for Image.

  **External References**:
  - image crate: https://docs.rs/image/latest/image/
  - imageproc crate (filters): https://docs.rs/imageproc/latest/imageproc/

  **WHY Each Reference Matters**:
  - extern_serde_json.buff: existing pattern for extern FFI; replicate for image crate.
  - rust_codegen Process module: shows how to wrap stateful Rust struct (Command pattern); Image is similar.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Load PNG and print dimensions
    Tool: Bash
    Steps:
      1. Place test PNG at examples/image/test.png (small 4x4 image)
      2. Write examples/image/hello.buff: "let img = Image.from_path(\"examples/image/test.png\"); print(img.width(), img.height())"
      3. Run it; assert stdout "4 4"
    Expected Result: PNG loads, dimensions correct
    Evidence: .sisyphus/evidence/task-9-load/output.txt

  Scenario: Grayscale filter changes pixel data
    Tool: Bash
    Steps:
      1. Load color test image
      2. Apply grayscale filter
      3. Save to /tmp/output.png
      4. Reload and assert pixel R==G==B for sample pixel
    Expected Result: Grayscale conversion verified
    Evidence: .sisyphus/evidence/task-9-grayscale/{input,output}.png

  Scenario: Resize produces smaller image
    Tool: Bash
    Steps:
      1. Load 100x100 image
      2. Resize to 50x50
      3. Assert new dimensions are 50x50
    Expected Result: Resize works correctly
    Evidence: .sisyphus/evidence/task-9-resize/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-image): MVP image codecs + filters via extern image crate`

---

- [x] 10. `buff-audio` — Audio Codecs + Sample Ops (CPU-only MVP)

  **What to do**:
  - Create `crates/buff-audio/`.
  - Implement `AudioBuffer` type: samples (interleaved f32), sample_rate, channels.
  - Implement ops: `AudioBuffer.from_path(path)`, `AudioBuffer.from_samples(samples, sample_rate, channels)`, `a.samples()`, `a.sample_rate()`, `a.channels()`, `a.duration_secs()`, `a.save(path)`.
  - Implement simple ops: `a.normalize()`, `a.amplify(factor)`, `a.mix(other)`, `a.slice(start_sec, end_sec)`.
  - Extern FFI to `rodio` or `symphonia` crate for WAV/MP3/FLAC codecs; safe wrapper per T4.
  - 3 examples, 10+ tests, 5+ snapshots, AGENTS.md, README.md, registry entry.

  **Must NOT do**:
  - Do NOT exceed 2500 LOC or 25 public functions.
  - Do NOT use GPU (CPU-only per G7).
  - Do NOT implement real-time playback (defer to v1.18+).
  - Do NOT implement synthesis (sine/square/noise generators — those go in buff-dsp T11).

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7-T9, T11, T12)
  - **Parallel Group**: Wave 2
  - **Blocks**: None directly
  - **Blocked By**: T1, T4 (soft)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:Path module` — File-loading pattern.
  - `examples/extern_*.buff` — Extern FFI pattern for wrapping Rust crates.

  **External References**:
  - rodio: https://docs.rs/rodio/latest/rodio/
  - symphonia: https://docs.rs/symphonia/latest/symphonia/
  - hound (WAV): https://docs.rs/hound/latest/hound/

  **WHY Each Reference Matters**:
  - Path module: file I/O pattern for `from_path` constructors.
  - extern pattern: how to wrap stateful codec library.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Load WAV and report duration
    Tool: Bash
    Steps:
      1. Place test.wav (2 seconds, 44100 Hz, mono) at examples/audio/test.wav
      2. Load and print duration_secs()
      3. Assert output: "2.0"
    Expected Result: WAV loads, duration correct
    Evidence: .sisyphus/evidence/task-10-load/output.txt

  Scenario: Amplify changes peak sample
    Tool: Bash
    Steps:
      1. Load audio, find peak sample value
      2. Amplify by 2.0
      3. Assert new peak ≈ 2x old peak (within tolerance)
    Expected Result: Amplification mathematically correct
    Evidence: .sisyphus/evidence/task-10-amplify/output.txt

  Scenario: Mix two buffers correctly
    Tool: Bash
    Steps:
      1. Create buffer A (1 second of 0.5 amplitude)
      2. Create buffer B (1 second of 0.3 amplitude)
      3. Mix; assert samples ≈ 0.8
    Expected Result: Mix is sample-wise addition
    Evidence: .sisyphus/evidence/task-10-mix/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-audio): MVP audio codecs + sample ops via extern rodio`

---

- [x] 11. `buff-dsp` — Signal Processing: FFT, Filters, Windows (CPU-only MVP)

  **What to do**:
  - Create `crates/buff-dsp/`.
  - Implement signal primitives: `Signal<T>` (Vec<T> + sample_rate), `Signal.from_vec(data, sample_rate)`.
  - Implement FFT: `s.fft()` (forward), `s.ifft()` (inverse) — extern to `rustfft` crate.
  - Implement filters: `s.lowpass(cutoff_hz)`, `s.highpass(cutoff_hz)`, `s.bandpass(low, high)`.
  - Implement windows: `Window.hann(n)`, `Window.hamming(n)`, `Window.blackman(n)` — apply via `s.apply_window(window)`.
  - Implement spectral ops: `s.spectrogram(window_size)`, `s.magnitude()`, `s.phase()`.
  - Extern FFI to `rustfft`, `apodization` crates per T4 FFI guide.
  - 3 examples, 10+ tests (proptest required for signal math), 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 2500 LOC or 25 public functions.
  - Do NOT use GPU (CPU-only per G7).
  - Do NOT implement real-time streaming (defer to v1.18+; Signal is Vec-backed not Stream-backed).
  - Do NOT implement adaptive filters (LMS, RLS — defer to v1.18+).

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7-T10, T12)
  - **Parallel Group**: Wave 2
  - **Blocks**: None directly
  - **Blocked By**: T1, T4 (soft)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:Math/Random module` — Math functions lowering; similar for DSP ops.

  **External References**:
  - rustfft: https://docs.rs/rustfft/latest/rustfft/
  - realfft: https://docs.rs/realfft/latest/realfft/
  - apodization: https://docs.rs/apodization/latest/apodization/

  **WHY Each Reference Matters**:
  - rustfft: industry-standard FFT in Rust, our extern target.
  - Math module pattern: lowering approach for math fns.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: FFT of pure sine produces peaked spectrum
    Tool: Bash
    Steps:
      1. Generate 1 second of 440Hz sine wave at 44100 Hz
      2. Compute FFT
      3. Assert peak magnitude occurs near bin 440 (within tolerance)
    Expected Result: FFT identifies the 440Hz component
    Evidence: .sisyphus/evidence/task-11-fft/output.txt

  Scenario: Hann window has expected shape
    Tool: Bash
    Steps:
      1. Generate hann(8)
      2. Assert values: [0.0, 0.146, 0.5, 0.854, 1.0, 0.854, 0.5, 0.146] (within tolerance)
    Expected Result: Hann coefficients correct
    Evidence: .sisyphus/evidence/task-11-hann/output.txt

  Scenario: Lowpass filter attenuates high frequencies
    Tool: Bash
    Steps:
      1. Mix 100Hz + 5000Hz sines
      2. Apply lowpass(500 Hz)
      3. FFT; assert 5000Hz bin is attenuated > 100Hz bin
    Expected Result: Lowpass removes high frequencies
    Evidence: .sisyphus/evidence/task-11-lowpass/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-dsp): MVP FFT, filters, windows via extern rustfft`

---

- [x] 12. `buff-ecs` — Entity-Component-System Architecture

  **What to do**:
  - Create `crates/buff-ecs/`.
  - Implement `World` type: entity storage + component storage + system registry.
  - Implement core API: `World.new()`, `world.spawn(components...)` returns Entity id, `world.insert(entity, component)`, `world.remove(entity, ComponentType)`, `world.query<ComponentTypes...>()` returns iterator.
  - Implement systems: `world.add_system(system_fn)` where system_fn is `func(World, Query<Q>, Resources) -> void`. Run via `world.tick()`.
  - Implement resources: `world.insert_resource(value)`, `world.get_resource<T>()`.
  - Leverage Buff's existing traits + generics for component typing.
  - Extern FFI to `bevy_ecs` or `hecs` crate as backing implementation (recommend hecs for simpler API).
  - 3 examples, 10+ tests, 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 2500 LOC or 25 public functions.
  - Do NOT implement rendering pipeline (T16 buff-game uses existing WGSL).
  - Do NOT implement asset loading (T16 buff-game).
  - Do NOT implement parallel system scheduling (defer to v1.18+; sequential tick() for MVP).
  - Do NOT implement change detection or events (defer to v1.18+).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Foundational architecture for buff-game; API design decisions affect everything downstream.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7-T11)
  - **Parallel Group**: Wave 2
  - **Blocks**: T16 (buff-game)
  - **Blocked By**: T1

  **References**:

  **Pattern References**:
  - `crates/buff-lang-types/src/prelude.rs:PreludeType` — How types register for codegen; ECS components follow same pattern.
  - `crates/buff-lang-ast/src/decl.rs:TraitDecl` — Traits model component contracts; ECS uses traits for component bounds.

  **External References**:
  - hecs: https://docs.rs/hecs/latest/hecs/
  - bevy_ecs: https://docs.rs/bevy_ecs/latest/bevy_ecs/
  - Entity Component System pattern: https://github.com/SanderMertens/ecs-faq

  **WHY Each Reference Matters**:
  - hecs: simpler API than bevy_ecs; good extern target for MVP.
  - TraitDecl: traits are how Buff expresses component contracts.
  - ECS FAQ: industry consensus on ECS design.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Spawn entity and query back
    Tool: Bash
    Steps:
      1. Define structs Position { x: Float, y: Float } and Velocity { dx: Float, dy: Float }
      2. let world = World.new()
      3. let e = world.spawn(Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 0.0 })
      4. let query = world.query<Position, Velocity>()
      5. Assert query has 1 item with matching values
    Expected Result: Spawn + query roundtrip works
    Evidence: .sisyphus/evidence/task-12-spawn/output.txt

  Scenario: System modifies components on tick
    Tool: Bash
    Steps:
      1. Spawn entity with Position(0, 0) and Velocity(1, 0)
      2. Add system: position.x += velocity.dx
      3. world.tick(); assert position.x == 1.0
      4. world.tick(); assert position.x == 2.0
    Expected Result: System runs and updates state
    Evidence: .sisyphus/evidence/task-12-system/output.txt

  Scenario: Resource insertion and retrieval
    Tool: Bash
    Steps:
      1. world.insert_resource(GameState { score: 0 })
      2. let state = world.get_resource<GameState>()
      3. Assert state.score == 0
    Expected Result: Resources work
    Evidence: .sisyphus/evidence/task-12-resource/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-ecs): MVP World, spawn, query, systems via extern hecs`

---

- [ ] 13. `buff-science` — Linear Algebra + FFT + Stats (depends T8)

  **What to do**:
  - Create `crates/buff-science/`.
  - Implement linear algebra on `Tensor<T>` (from T8): `linalg.matmul(a, b)`, `linalg.transpose(t)`, `linalg.inverse(m)`, `linalg.determinant(m)`, `linalg.solve(a, b)`.
  - Implement numerical methods: `ode.rk4(f, initial, t_start, t_end, step)`, `interp.linear(xs, ys, x)`, `optimize.gradient_descent(f, initial, lr, steps)`.
  - Implement statistics: `stats.mean(data)`, `stats.variance(data)`, `stats.stddev(data)`, `stats.correlation(x, y)`, `stats.histogram(data, bins)`.
  - Reuse buff-dsp's FFT where applicable (don't reimplement).
  - 3 examples, 15+ tests (proptest required for numerical stability), 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 4000 LOC or 40 public functions.
  - Do NOT implement symbolic math (defer to v1.18+).
  - Do NOT implement PDE solvers (defer to v1.18+).
  - Do NOT reimplement FFT (reuse T11 buff-dsp).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Math-heavy + depends on T8; numerical stability requires careful API design.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T14, T15, T16 — Wave 3 siblings)
  - **Parallel Group**: Wave 3
  - **Blocks**: T22, T23
  - **Blocked By**: T8 (tensor)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-types/src/prelude.rs:Math functions (abs, sqrt, etc.)` — Math prelude registration pattern.
  - buff-tensor (T8) — Tensor type being consumed.

  **External References**:
  - nalgebra: https://docs.rs/nalgebra/latest/nalgebra/
  - ndarray: https://docs.rs/ndarray/latest/ndarray/
  - argmin (optimization): https://docs.rs/argmin/latest/argmin/

  **WHY Each Reference Matters**:
  - nalgebra: industry-standard Rust linear algebra; extern target.
  - Math prelude: extension pattern for new math functions.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Matrix inverse roundtrip
    Tool: Bash
    Steps:
      1. Create invertible 3x3 matrix m
      2. Compute m_inv = linalg.inverse(m)
      3. Compute product = linalg.matmul(m, m_inv)
      4. Assert product ≈ identity (diagonal ≈ 1.0, off-diagonal ≈ 0.0, within 1e-6)
    Expected Result: Inverse is mathematically correct
    Evidence: .sisyphus/evidence/task-13-inverse/output.txt

  Scenario: RK4 integrates ODE correctly
    Tool: Bash
    Steps:
      1. Define f(t, y) = y (exponential growth, solution y(t) = e^t)
      2. Integrate from t=0 to t=1 with step 0.01
      3. Assert result ≈ e^1 ≈ 2.71828 (within 1e-4)
    Expected Result: ODE solver is accurate
    Evidence: .sisyphus/evidence/task-13-rk4/output.txt

  Scenario: Stats compute on dataset
    Tool: Bash
    Steps:
      1. data = [1.0, 2.0, 3.0, 4.0, 5.0]
      2. mean == 3.0
      3. variance == 2.0
      4. stddev == sqrt(2.0)
    Expected Result: Statistics correct
    Evidence: .sisyphus/evidence/task-13-stats/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-science): MVP linalg, ODE, stats via extern nalgebra`

---

- [ ] 14. `buff-pipeline` — DAG-based ETL + Bounded Channel Queues (depends T2 Channel)

  **What to do** (revised post-T2 scope reduction):
  - Create `crates/buff-pipeline/`.
  - Implement `Pipeline` type: DAG of stages, each stage is a Buff closure taking input → output (or `Option<Output>` to indicate "skip").
  - Implement core API: `Pipeline.new()`, `p.stage(name, fn)`, `p.source(data: Vector<T>)` (batch source — NOT async streaming source; defer Kafka/Redis Streams to v1.18+), `p.sink(fn)`, `p.run()`.
  - **Inter-stage queues via T2 Channel<T>**: each stage receives input via `Channel<T>`, sends output via `Channel<U>` downstream. Bounded buffers provide backpressure naturally. Producer/consumer pattern is exactly what Channel<T> was designed for.
  - Implement common stages: `p.map(fn)`, `p.filter(pred)`, `p.batch(size)` (groups N items into Vector<T>), `p.window(size, fn)`, `p.parallel(workers, fn)` (spawns N workers each pulling from input Channel via `spawn`).
  - Implement sources: `Source.from_csv(path, chunk_size)` reads CSV in chunks, pushes rows into Channel<T> for downstream consumption (loads bounded memory window, not entire CSV at once — uses Channel backpressure to throttle file I/O).
  - Implement sinks: `Sink.to_csv(path)`, `Sink.to_json(path)`, `Sink.collect()` returns Vector<T> (terminal drain).
  - 3 examples (one real ETL pipeline), 10+ tests, 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 4000 LOC or 40 public functions.
  - Do NOT implement `Stream<T>` consumption (T2 deferred this to v1.18+) — use `Channel<T>` + sync `Vector<T>` batches.
  - Do NOT implement Kafka/Redis Streams sources (defer to v1.18+).
  - Do NOT implement exactly-once delivery guarantees (defer to v1.18+).
  - Do NOT implement pipeline orchestration (scheduling, retries) — that's a separate concern.
  - Do NOT use `select` expression (deferred to v1.18+) — workers consume from a single Channel each, no multi-source select needed.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Integration-heavy (uses Channel<T> from T2, spawn, rayon, csv/json stdlib); many moving parts but each is simple.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T13, T15, T16)
  - **Parallel Group**: Wave 3
  - **Blocks**: T22, T23
  - **Blocked By**: T1 (multi-file), T2 (Channel<T> primitive)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-runtime/src/cpu.rs:CpuDispatcher par_map` — Existing parallel pattern to extend.
  - buff-dataframe (T7) — Reuse DataFrame as source/sink where useful.
  - buff-lang-runtime `Channel<T>` (T2) — The inter-stage queue primitive.
  - `crates/buff-lang-ast/src/expr.rs:Spawn variant` — Existing `spawn expr` for parallel workers.

  **External References**:
  - Apache Arrow (conceptual): https://arrow.apache.org/
  - Rayon: https://docs.rs/rayon/latest/rayon/
  - Tokio mpsc patterns: https://tokio.rs/tokio/tutorial/channels

  **WHY Each Reference Matters**:
  - CpuDispatcher par_map: existing parallel pattern to build on.
  - T7 dataframe: pipeline should interop with DataFrame for batch ops.
  - T2 Channel: the bounded queue primitive connecting stages.
  - Spawn expr: how to spawn parallel workers consuming from a Channel.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Simple map-filter pipeline with batch source
    Tool: Bash
    Steps:
      1. let p = Pipeline.new()
      2. p.source([1, 2, 3, 4, 5])
      3. p.map({ x => x * 2 })
      4. p.filter({ x => x > 4 })
      5. let result = p.run().collect()
      6. Assert result == [6, 8, 10]
    Expected Result: Pipeline applies stages in order via Channel<T> queues
    Evidence: .sisyphus/evidence/task-14-simple/output.txt

  Scenario: Streaming CSV through pipeline with bounded memory
    Tool: Bash
    Steps:
      1. CSV with 10K rows
      2. Pipeline: source.from_csv(chunk_size: 100) → filter → map → sink.to_csv
      3. Assert output CSV has correct filtered/transformed rows
      4. Assert memory usage stayed bounded (didn't load all rows at once — verify via Channel backpressure: source blocks when downstream is slow)
    Expected Result: Channel-based streaming works without OOM
    Evidence: .sisyphus/evidence/task-14-stream/{input,output}.csv

  Scenario: Parallel stage spawns N workers per Channel
    Tool: Bash
    Steps:
      1. Pipeline with p.parallel(workers: 4, expensive_fn)
      2. Each worker pulls from shared input Channel<T>, sends to output Channel<U>
      3. Time the run
      4. Assert parallel < sequential time (within reason; workers actually parallelize)
    Expected Result: Parallel stage uses spawn + Channel MPSC correctly
    Evidence: .sisyphus/evidence/task-14-parallel/timing.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-pipeline): MVP DAG pipelines with Channel-based inter-stage queues + parallel workers`

---

- [ ] 15. `buff-ml` — Neural Network Layers + Autodiff (depends T8)

  **What to do**:
  - Create `crates/buff-ml/`.
  - Implement autodiff: `Tensor` extension that tracks operations for reverse-mode autodiff. `t.requires_grad()`, `t.backward()`, `t.grad()`.
  - Implement NN layers: `Linear(input_dim, output_dim)`, `ReLU`, `Sigmoid`, `Softmax`, `Dropout(rate)`.
  - Implement loss functions: `mse_loss(pred, target)`, `cross_entropy(pred, target)`.
  - Implement optimizers: `SGD(lr)`, `Adam(lr)`.
  - Implement training loop: `Model.sequential([layers])`, `model.forward(x)`, `model.backward(loss_grad)`, `optimizer.step(model)`.
  - Implement save/load: `model.save(path)` (JSON), `Model.load(path)`.
  - 3 examples (linear regression on synthetic data, MLP on MNIST subset if feasible, toy classification).
  - 15+ tests (proptest required for gradient checks), 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 4000 LOC or 40 public functions.
  - Do NOT implement CNNs/RNNs/Transformers (defer to v1.18+).
  - Do NOT implement distributed training (defer to v1.19+).
  - Do NOT support f64 (autodiff on f32 only for MVP).
  - Do NOT implement ONNX/safetensors serialization (JSON only for MVP).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Autodiff is hard to get right; flagship depends on this.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T13, T14, T16)
  - **Parallel Group**: Wave 3
  - **Blocks**: T22, T23
  - **Blocked By**: T8 (tensor)

  **References**:

  **Pattern References**:
  - buff-tensor (T8) — Tensor type being extended.
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:Math module` — Math fn registration.

  **External References**:
  - Candle (HuggingFace): https://github.com/huggingface/candle — reference for autodiff
  - tch-rs (PyTorch bindings): https://docs.rs/tch/latest/tch/
  - Micrograd (Karpathy): https://github.com/karpathy/micrograd — autodiff reference

  **WHY Each Reference Matters**:
  - Candle: production-quality Rust autodiff; study the design.
  - Micrograd: minimal autodiff reference; great for MVP scope calibration.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Gradient check on linear layer
    Tool: Bash
    Steps:
      1. Create Linear(3, 2) with known weights
      2. Forward pass on input
      3. Compute MSE loss vs target
      4. Backward pass
      5. Numerically check gradients via finite differences (epsilon=1e-5)
      6. Assert analytic grad ≈ numeric grad (within 1e-4)
    Expected Result: Autodiff is mathematically correct
    Evidence: .sisyphus/evidence/task-15-gradcheck/output.txt

  Scenario: Train linear regression to convergence
    Tool: Bash
    Steps:
      1. Generate synthetic data: y = 2x + 3 + noise
      2. Build model: Linear(1, 1)
      3. Train 1000 steps with SGD(lr=0.01)
      4. Assert learned weights ≈ 2.0 (± 0.1) and bias ≈ 3.0 (± 0.1)
    Expected Result: Training converges to correct params
    Evidence: .sisyphus/evidence/task-15-linear/output.txt

  Scenario: Save and reload model
    Tool: Bash
    Steps:
      1. Train model
      2. Save to /tmp/model.json
      3. Reload as new model
      4. Assert predictions match original model
    Expected Result: Serialization roundtrip preserves behavior
    Evidence: .sisyphus/evidence/task-15-save-load/{model,output}.json
  ```

  **Commit**: YES
  - Message: `feat(buff-ml): MVP autodiff, layers, optimizers, training loop`

---

- [ ] 16. `buff-game` — Game Loop + Asset Pipeline + Rendering (depends T12)

  **What to do**:
  - Create `crates/buff-game/`.
  - Implement `Game` type: window setup (via existing buff-ui-dioxus or extern `winit`), game loop with fixed timestep, ECS world integration (T12).
  - Implement core API: `Game.new(config)`, `game.add_scene(scene)`, `game.run()`, `game.quit()`.
  - Implement asset pipeline: `Asset.load_texture(path)`, `Asset.load_audio(path)` (reuse T9/T10), `Asset.cache`.
  - Implement rendering abstraction: leverage existing `crates/buff-lang-codegen-wgsl` for shaders; provide `Renderer.draw_sprite(texture, transform)`, `Renderer.draw_text(text, pos)`.
  - Implement input handling: `Input.is_key_pressed(key)`, `Input.mouse_position()`.
  - 3 examples: `hello.buff` (open window, render quad), `sprite.buff` (load + draw sprite), `input.buff` (move sprite with arrow keys).
  - 10+ tests, 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 4000 LOC or 40 public functions.
  - Do NOT implement physics engine (defer to v1.18+ — wrap rapier if needed then).
  - Do NOT implement 3D model loading (defer to v1.18+).
  - Do NOT implement audio playback mixing (use buff-audio for loading only).
  - Do NOT implement networking multiplayer (defer to v1.19+).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Game engine is complex integration work; touches many crates (ECS, image, audio, wgpu).
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T13, T14, T15)
  - **Parallel Group**: Wave 3
  - **Blocks**: None (game is optional for flagship)
  - **Blocked By**: T12 (ECS), T1

  **References**:

  **Pattern References**:
  - `crates/buff-lang-runtime/src/gpu.rs` — Existing wgpu context; reuse for rendering.
  - `crates/buff-ui-dioxus/` — Existing window setup pattern (may or may not fit game loop; assess).

  **External References**:
  - Bevy: https://bevyengine.org/ — reference architecture
  - winit: https://docs.rs/winit/latest/winit/
  - wgpu: https://docs.rs/wgpu/latest/wgpu/

  **WHY Each Reference Matters**:
  - gpu.rs: existing wgpu setup; don't duplicate.
  - Bevy: industry reference for ECS-based game architecture.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Game opens window and renders quad
    Tool: Bash (with --headless test mode if possible)
    Steps:
      1. Write examples/game/hello.buff that opens window + draws colored quad
      2. Run with timeout 5s
      3. Capture screenshot if headless test mode available; assert quad pixels present
      4. If no headless mode: run for 2s, then assert no panic / graceful exit
    Expected Result: Window opens, renders without crash
    Evidence: .sisyphus/evidence/task-16-window/{output,screenshot.png}

  Scenario: Asset loading works
    Tool: Bash
    Steps:
      1. let tex = Asset.load_texture("examples/game/test.png")
      2. Assert tex.width() > 0
    Expected Result: Asset pipeline loads resources
    Evidence: .sisyphus/evidence/task-16-asset/output.txt

  Scenario: Input handler responds to keystroke
    Tool: Bash (simulated input if possible)
    Steps:
      1. Game registers handler: on ArrowUp, move sprite up by 10px
      2. Simulate ArrowUp press (headless test mode)
      3. Assert sprite position changed by -10 in y
    Expected Result: Input system delivers events to handlers
    Failure Indicators: If no headless input simulation, mark as DEFERRED-TO-MANUAL-QA and document
    Evidence: .sisyphus/evidence/task-16-input/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-game): MVP game loop, asset pipeline, rendering`

---

- [x] 17. `buff-web` — Production Web Framework (wraps axum)

  **What to do**:
  - Create `crates/buff-web/`.
  - Implement `Server` type: HTTP server config, route table, middleware chain.
  - Implement routing: `Server.new()`, `server.get(path, handler)`, `server.post(path, handler)`, etc. Routes registered at runtime (NOT compile-time macros per T3 macro spike outcome).
  - Implement middleware: `server.use(middleware)` where middleware is `func(Request, Next) -> Response`. Built-in: `Logger`, `Cors`, `JsonParser`.
  - Implement Request/Response types: `Request.method()`, `Request.path()`, `Request.header(name)`, `Request.body()` (returns `Result<String, Error>`), `Request.json()` (returns `Result<T, Error>` via extern serde_json).
  - Implement Response builder: `Response.json(value)`, `Response.text(s)`, `Response.status(code)`, `Response.header(name, value)`.
  - Extern FFI to `axum` + `tokio` (existing runtime) + `serde_json` (existing) per T4 FFI guide.
  - 3 examples: `hello.buff` (single GET route), `json_api.buff` (POST + JSON), `middleware.buff` (custom middleware chain).
  - 10+ tests, 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 1500 LOC or 20 public functions (Wave 4 wrapper budget).
  - Do NOT implement WebSocket (use existing stdlib if needed; out of scope here).
  - Do NOT implement template rendering (T19 buff-template handles that).
  - Do NOT implement ORM/database integration (T18 buff-db handles that).
  - Do NOT use macros for routing (T3 decision pending — use runtime registration).

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Wrapper work following established FFI patterns; high effort but bounded scope.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T18-T21 — Wave 4)
  - **Parallel Group**: Wave 4
  - **Blocks**: T22, T23
  - **Blocked By**: T1 (multi-file), T4 (FFI guide — soft)

  **References**:

  **Pattern References**:
  - `examples/extern_reqwest.buff` — Existing HTTP client extern pattern; mirror for server side.
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:Process module` — Wrap stateful Rust type pattern.

  **External References**:
  - axum: https://docs.rs/axum/latest/axum/
  - tower (middleware): https://docs.rs/tower/latest/tower/
  - hyper: https://docs.rs/hyper/latest/hyper/

  **WHY Each Reference Matters**:
  - axum: industry-standard Rust web framework; our extern target.
  - extern_reqwest.buff: existing pattern for HTTP extern; replicate style for server.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: GET route returns text
    Tool: Bash (cargo + curl)
    Steps:
      1. Write examples/web/hello.buff:
         "func main() {\n  let s = Server.new()\n  s.get(\"/\", { req => Response.text(\"hello\") })\n  s.listen(port: 8080)\n}"
      2. Run server in background: cargo run -p buff-lang-cli -- run examples/web/hello.buff &
      3. Wait 2s for startup
      4. curl http://localhost:8080/
      5. Assert response body: "hello"
      6. Kill server
    Expected Result: HTTP server responds to GET
    Evidence: .sisyphus/evidence/task-17-get/curl-output.txt

  Scenario: POST with JSON body parsed
    Tool: Bash (curl)
    Steps:
      1. Server has POST /echo that returns received JSON
      2. curl -X POST -d '{"name":"buff"}' http://localhost:8080/echo
      3. Assert response: {"name":"buff"}
    Expected Result: JSON body parsing works
    Evidence: .sisyphus/evidence/task-17-post/curl-output.txt

  Scenario: Middleware modifies request
    Tool: Bash (curl)
    Steps:
      1. Logger middleware logs each request method+path
      2. GET /test
      3. Assert server log contains "GET /test"
    Expected Result: Middleware pipeline executes
    Evidence: .sisyphus/evidence/task-17-middleware/{server-log,curl-output}.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-web): MVP HTTP server with routing + middleware via extern axum`

---

- [x] 18. `buff-db` — Database Access (wraps sqlx)

  **What to do**:
  - Create `crates/buff-db/`.
  - Implement `Pool` type: connection pool, query execution.
  - Implement core API: `Pool.connect(url)`, `pool.query<T>(sql, params)` returns `Vector<T>`, `pool.execute(sql, params)` returns rows affected.
  - Implement query builder (runtime, NOT compile-time macros per T3): `Query.new(table)`, `q.select(cols)`, `q.where(pred)`, `q.join(...)`, `q.sql()` returns String.
  - Support SQLite + PostgreSQL for MVP (extern to sqlx).
  - Implement transaction: `pool.begin()` returns Transaction, `tx.commit()`, `tx.rollback()`.
  - 3 examples: `hello.buff` (connect + simple query), `crud.buff` (insert/select/update/delete), `transaction.buff` (begin/commit/rollback).
  - 10+ tests (use in-memory SQLite for tests), 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 1500 LOC or 20 public functions.
  - Do NOT implement compile-time SQL validation (defer to v1.19+macro work).
  - Do NOT implement migrations (defer to v1.18+).
  - Do NOT implement MySQL/MSSQL/Oracle (SQLite + PostgreSQL only for MVP).

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T17, T19-T21)
  - **Parallel Group**: Wave 4
  - **Blocks**: None directly
  - **Blocked By**: T1, T4 (soft)

  **References**:

  **Pattern References**:
  - `examples/extern_serde_json.buff` — Extern FFI to stateless function; similar for sqlx.

  **External References**:
  - sqlx: https://docs.rs/sqlx/latest/sqlx/
  - SeaORM (query builder reference): https://docs.rs/sea-orm/latest/sea_orm/

  **WHY Each Reference Matters**:
  - sqlx: industry-standard async Rust DB lib; our extern target.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Connect to in-memory SQLite and query
    Tool: Bash
    Steps:
      1. Write examples/db/hello.buff:
         "let pool = Pool.connect(\"sqlite::memory:\")
          pool.execute(\"CREATE TABLE users (id INTEGER, name TEXT)\", [])
          pool.execute(\"INSERT INTO users VALUES (1, 'Alice')\", [])
          let rows = pool.query(\"SELECT * FROM users\", [])
          print(rows[0])"
      2. Run it
      3. Assert stdout contains "Alice"
    Expected Result: SQLite in-memory works end-to-end
    Evidence: .sisyphus/evidence/task-18-sqlite/output.txt

  Scenario: Transaction commit and rollback
    Tool: Bash
    Steps:
      1. Begin tx, insert row, rollback — assert row NOT present
      2. Begin tx, insert row, commit — assert row IS present
    Expected Result: Transactions behave correctly
    Evidence: .sisyphus/evidence/task-18-tx/output.txt

  Scenario: Query builder generates valid SQL
    Tool: Bash
    Steps:
      1. q = Query.new("users").select(["id", "name"]).where("age > 18")
      2. Assert q.sql() == "SELECT id, name FROM users WHERE age > 18"
    Expected Result: Query builder output is valid SQL
    Evidence: .sisyphus/evidence/task-18-query-builder/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-db): MVP connection pool + query builder via extern sqlx`

---

- [x] 19. `buff-template` — HTML Templating (wraps handlebars)

  **What to do**:
  - Create `crates/buff-template/`.
  - Implement `Template` type: loads `.html` template files, renders with context.
  - Implement core API: `Template.from_path(path)`, `Template.from_string(s)`, `template.render(context)` returns String.
  - Support template syntax: `{{ variable }}`, `{% if cond %}...{% endif %}`, `{% for item in list %}...{% endfor %}`.
  - Context: pass Buff Map<String, Any> or struct (via reflection if available, otherwise require Map).
  - Extern FFI to `askama` (compile-time) OR `handlebars` (runtime) crate. Recommend handlebars for runtime simplicity (no macro dependence).
  - 3 examples: `hello.buff` (simple var substitution), `loop.buff` (list iteration), `conditionals.buff` (if/else).
  - 10+ tests, 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 1500 LOC or 20 public functions.
  - Do NOT implement template inheritance (defer to v1.18+).
  - Do NOT implement custom template syntax (use handlebars' syntax).
  - Do NOT compile templates at build time (runtime only for MVP).

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Thin wrapper around existing templating crate; well-bounded scope.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T17, T18, T20, T21)
  - **Parallel Group**: Wave 4
  - **Blocks**: None directly
  - **Blocked By**: T1, T4 (soft)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:Log module` — Stateless function wrap pattern.

  **External References**:
  - handlebars: https://docs.rs/handlebars/latest/handlebars/
  - askama: https://docs.rs/askama/latest/askama/

  **WHY Each Reference Matters**:
  - handlebars: runtime templating; no macros required (aligns with T3 spike defer outcome likely).

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Variable substitution
    Tool: Bash
    Steps:
      1. Template: "Hello {{name}}!"
      2. Render with context {"name": "Buff"}
      3. Assert output: "Hello Buff!"
    Expected Result: Variable substitution works
    Evidence: .sisyphus/evidence/task-19-var/output.txt

  Scenario: Loop renders list
    Tool: Bash
    Steps:
      1. Template: "{% for item in items %}{{item}} {% endfor %}"
      2. Context: {"items": ["a", "b", "c"]}
      3. Assert output: "a b c "
    Expected Result: Loop works
    Evidence: .sisyphus/evidence/task-19-loop/output.txt

  Scenario: Conditional renders correct branch
    Tool: Bash
    Steps:
      1. Template: "{% if ok %}yes{% else %}no{% endif %}"
      2. Context {"ok": true} → "yes"
      3. Context {"ok": false} → "no"
    Expected Result: Conditionals work
    Evidence: .sisyphus/evidence/task-19-cond/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-template): MVP HTML templating via extern handlebars`

---

- [x] 20. `buff-reactive` — Signals + Computed + Effect Callbacks (depends T1; no Stream dependency)

  **What to do** (revised post-T2 scope reduction):
  - Create `crates/buff-reactive/`.
  - Implement `Signal<T>` type: mutable reactive cell, notifies subscribers on change.
  - Implement core API: `Signal.new(initial)`, `s.get()`, `s.set(value)`, `s.update(fn)` (read-modify-write).
  - Implement computed: `Computed.new(fn)` — derives from signals, recomputes lazily, caches.
  - Implement effects: `Effect.new(fn)` — runs when dependencies change. Uses **callback pattern** (fn invoked on dep change), NOT async streams. This is the Vue/Solid.js reactive model — it composes cleanly without Stream<T>.
  - Implement batching: `batch(fn)` — defers notifications until block exits (prevents cascade).
  - Memory model: signals are `Rc<RefCell<...>>` internally (single-threaded for MVP). Document threading limits.
  - 3 examples, 10+ tests, 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do** (v1.13-v1.17 scope lock):
  - Do NOT exceed 1500 LOC or 20 public functions.
  - Do NOT use `Stream<T>` (deferred to v1.18+) — callbacks via `Effect.new(fn)` are sufficient.
  - Do NOT implement multi-threaded signals (single-threaded `Rc<RefCell>` only for MVP).
  - Do NOT integrate with v1.9 RSX directly (provide primitives; integration is separate task).
  - Do NOT implement time-travel debugging (defer to v1.18+).

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Reactive systems have subtle correctness issues (glitches, diamond dependencies); careful API design needed.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T17-T19, T21)
  - **Parallel Group**: Wave 4
  - **Blocks**: T22, T23
  - **Blocked By**: T1 (NO LONGER blocked by T2 — callbacks don't need Channel/Stream)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-ast/src/expr.rs:Closure variant` — Closures for computed/effects fns.

  **External References**:
  - Solid.js signals (conceptual): https://www.solidjs.com/docs/latest#createsignal
  - Vue refs: https://vuejs.org/guide/essentials/reactivity-fundamentals.html

  **WHY Each Reference Matters**:
  - Solid.js: clean signal API reference (callback-based, no streams).
  - Vue refs: industry-standard reactive pattern (callback-based).
  - Closure AST: existing closure syntax for computed/effects.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Signal set/get roundtrip
    Tool: Bash
    Steps:
      1. let s = Signal.new(10)
      2. Assert s.get() == 10
      3. s.set(20)
      4. Assert s.get() == 20
    Expected Result: Signal holds value
    Evidence: .sisyphus/evidence/task-20-signal/output.txt

  Scenario: Effect callback runs when signal changes
    Tool: Bash
    Steps:
      1. let s = Signal.new(0)
      2. let counter = Signal.new(0)
      3. Effect.new({ || counter.set(counter.get() + 1) })  // tracks s via callback invocation
      4. s.set(1) → assert counter == 1
      5. s.set(2) → assert counter == 2
    Expected Result: Effect callback fires on dependency change
    Evidence: .sisyphus/evidence/task-20-effect/output.txt

  Scenario: Computed caches until deps change
    Tool: Bash
    Steps:
      1. let a = Signal.new(2)
      2. let b = Signal.new(3)
      3. let call_count = Signal.new(0)
      4. let sum = Computed.new({ || call_count.set(call_count.get() + 1); return a.get() + b.get() })
      5. Assert sum.get() == 5 and call_count == 1
      6. Assert sum.get() == 5 and call_count == 1 (cached)
      7. a.set(10) → sum.get() == 13 and call_count == 2
    Expected Result: Computed memoizes correctly via dependency tracking
    Evidence: .sisyphus/evidence/task-20-computed/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-reactive): MVP signals, computed, effect callbacks`

---

- [x] 21. `buff-observe` — Structured Observability (wraps tracing + opentelemetry)

  **What to do**:
  - Create `crates/buff-observe/`.
  - Extend existing v1.4 `Log` module with structured logging: spans, fields, levels.
  - Implement `Span` type: `Span.new(name)`, `span.field(name, value)`, `span.enter()` returns guard, exits on drop.
  - Implement metrics: `Counter.new(name)`, `c.inc()`, `c.inc_by(n)`, `Histogram.new(name)`, `h.observe(value)`, `Gauge.new(name)`, `g.set(value)`.
  - Implement exporters: console (default), OTLP (extern opentelemetry-otlp).
  - 3 examples, 10+ tests, 5+ snapshots, AGENTS.md, README.md, registry.

  **Must NOT do**:
  - Do NOT exceed 1500 LOC or 20 public functions.
  - Do NOT implement distributed tracing propagation (defer to v1.18+).
  - Do NOT implement custom dashboards (use existing OTLP backends).
  - Do NOT replace v1.4 Log (extend it).

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Thin wrapper around existing tracing/opentelemetry crates.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T17-T20)
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: T1, T4 (soft)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs:Log module` (T124c) — Existing Log to extend.

  **External References**:
  - tracing: https://docs.rs/tracing/latest/tracing/
  - opentelemetry: https://docs.rs/opentelemetry/latest/opentelemetry/

  **WHY Each Reference Matters**:
  - Existing Log module: extend rather than replace.
  - tracing: industry standard Rust observability.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Span emits structured event
    Tool: Bash
    Steps:
      1. let span = Span.new("request")
      2. span.field("user_id", 42)
      3. let _guard = span.enter()
      4. Log.info("processing")
      5. Assert stderr contains "request" span + user_id=42 + "processing"
    Expected Result: Structured span logging works
    Evidence: .sisyphus/evidence/task-21-span/output.txt

  Scenario: Counter increments correctly
    Tool: Bash
    Steps:
      1. let c = Counter.new("requests")
      2. c.inc() x 3
      3. c.inc_by(2)
      4. Assert counter value == 5 (via metrics export)
    Expected Result: Counter accumulates correctly
    Evidence: .sisyphus/evidence/task-21-counter/output.txt

  Scenario: OTLP exporter sends telemetry
    Tool: Bash (with mock OTLP collector)
    Steps:
      1. Configure buff-observe to export via OTLP to localhost:4317
      2. Start mock collector (e.g., jaeger all-in-one)
      3. Run example that emits spans
      4. Assert mock collector received spans
    Expected Result: OTLP export works
    Failure Indicators: If mock collector hard to set up, mark DEFERRED-TO-MANUAL
    Evidence: .sisyphus/evidence/task-21-otlp/collector-log.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-observe): MVP structured spans + metrics via extern tracing+OTLP`

---

- [ ] 22. API Compatibility Spike — Multi-Framework Integration Examples

  **What to do**:
  - BEFORE writing the flagship, write small integration examples that exercise 2-3 frameworks together to validate API compatibility.
  - Write `examples/integration/dataframe_to_json.buff`: load CSV via buff-dataframe → serialize subset to JSON → save via existing stdlib.
  - Write `examples/integration/tensor_to_web.buff`: compute tensor result → expose via buff-web endpoint.
  - Write `examples/integration/pipeline_with_dataframe.buff`: stream CSV via buff-pipeline → batch into buff-dataframe → analyze.
  - Write `examples/integration/reactive_to_web.buff`: signal-driven counter → buff-web endpoint that returns current count.
  - For each example, document any API mismatches discovered.
  - If mismatches found that require API changes: file follow-up tasks (do NOT fix in this task — that's separate work).
  - Output: integration examples + report at `.sisyphus/decisions/api-compat-v20.md`.

  **Must NOT do**:
  - Do NOT write the flagship here (T23).
  - Do NOT fix API mismatches in this task — just document and file follow-ups.
  - Do NOT exceed 1000 LOC total across all integration examples.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Integration analysis requires understanding multiple framework APIs + ability to spot mismatches.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on most Wave 2-4 tasks)
  - **Parallel Group**: Wave 5 (solo before flagship)
  - **Blocks**: T23 (flagship)
  - **Blocked By**: T7, T8, T14, T15, T17, T20 (the frameworks being integrated)

  **References**:

  **Pattern References**:
  - All framework crates from Waves 2-4.
  - `examples/extern_*.buff` — Multi-module example pattern.

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: All integration examples run successfully
    Tool: Bash
    Steps:
      1. For each .buff file in examples/integration/:
         - Run: cargo run -p buff-lang-cli -- run <file>
         - Assert exit code 0
         - Capture stdout to evidence
      2. Assert all 4+ integration examples pass
    Expected Result: Frameworks compose without errors
    Failure Indicators: Compile errors (API mismatch), runtime errors (semantic mismatch)
    Evidence: .sisyphus/evidence/task-22-integration/{example-name}.txt

  Scenario: API mismatch report exists
    Tool: Bash
    Steps:
      1. Assert .sisyphus/decisions/api-compat-v20.md exists
      2. Assert it lists any mismatches found (or "No mismatches found" if clean)
      3. For each mismatch, assert follow-up task ID is referenced
    Expected Result: Report is comprehensive
    Evidence: .sisyphus/evidence/task-22-report/report.md
  ```

  **Commit**: YES
  - Message: `docs(spike): API compatibility integration examples + mismatch report`
  - Files: `examples/integration/*.buff`, `.sisyphus/decisions/api-compat-v20.md`

---

- [ ] 23. Flagship: Data Science Workbench

  **What to do**:
  - Build a complete data science workbench application in Buff that exercises 5 frameworks end-to-end:
    1. **Load**: CSV via `buff-dataframe` (T7) — synthetic Iris-like dataset
    2. **Analyze**: dataframe ops (filter, group_by, agg)
    3. **Train**: linear regression via `buff-ml` (T15) — predict target from features
    4. **Pipeline**: orchestrate via `buff-pipeline` (T14) — streaming data + batched training
    5. **Visualize**: web UI via `buff-web` (T17) — endpoint returns trained model params as JSON; use buff-reactive (T20) for live updates if model retrains
  - Application structure: `examples/data-science-workbench/` with:
    - `main.buff` — entry point
    - `data_loader.buff` — CSV → DataFrame
    - `model.buff` — training logic
    - `pipeline.buff` — orchestration
    - `server.buff` — HTTP endpoints
  - Endpoints:
    - `GET /` — landing page with model summary
    - `GET /predict?features=...` — returns prediction
    - `POST /retrain` — triggers retraining via pipeline
  - Use `buff-observe` (T21) for span-traced request flow.
  - Test data: synthetic Iris-like dataset (or small public-domain dataset) committed to `examples/data-science-workbench/data.csv`.
  - Demo: `cargo run -p buff-lang-cli -- run examples/data-science-workbench/main.buff` starts server, prints URL.
  - Verify end-to-end with curl + expected JSON response.

  **Must NOT do**:
  - Do NOT exceed 3000 LOC across all flagship files.
  - Do NOT add new abstractions to underlying frameworks — if framework APIs are insufficient, file follow-up tasks.
  - Do NOT implement authentication (out of scope for MVP demo).
  - Do NOT implement production-ready UI (basic JSON endpoints + maybe simple HTML template via buff-template T19).
  - Do NOT skip the API compat spike (T22) — must run first.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Integration of 5 frameworks; flagship quality sets perception of v1.13-v1.17.
  - **Skills**: [`/git-master`]

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on most things)
  - **Parallel Group**: Wave 5 (after T22)
  - **Blocks**: F1-F4
  - **Blocked By**: T7, T8, T13, T14, T15, T17, T20, T21, T22

  **References**:

  **Pattern References**:
  - `examples/dataframe/*.buff`, `examples/tensor/*.buff`, etc. — Smaller demos from each framework.
  - T22 integration examples — Validate composition before flagship.

  **External References**:
  - Iris dataset (classic): https://archive.ics.uci.edu/ml/datasets/iris

  **Acceptance Criteria**:

  **QA Scenarios**:

  ```
  Scenario: Flagship starts and serves HTTP
    Tool: Bash (cargo + curl)
    Steps:
      1. cargo run -p buff-lang-cli -- run examples/data-science-workbench/main.buff &
      2. Wait 3s for startup + model training
      3. curl http://localhost:8080/
      4. Assert response contains "model_summary", "trained", "samples"
      5. Kill server
    Expected Result: Flagship runs end-to-end
    Failure Indicators: Crash during startup, no HTTP response, missing model
    Evidence: .sisyphus/evidence/task-23-flagship/curl-output.txt

  Scenario: Predict endpoint returns prediction
    Tool: Bash (curl)
    Steps:
      1. Server running
      2. curl "http://localhost:8080/predict?sepal_length=5.1&sepal_width=3.5&petal_length=1.4&petal_width=0.2"
      3. Assert JSON response contains "prediction" field with non-null value
    Expected Result: Trained model produces predictions
    Evidence: .sisyphus/evidence/task-23-predict/curl-output.txt

  Scenario: Retrain endpoint updates model
    Tool: Bash (curl)
    Steps:
      1. Server running, capture initial model params via GET /
      2. curl -X POST http://localhost:8080/retrain
      3. Wait for response (training completes)
      4. GET / again, assert model params CHANGED (retrained on different data slice)
    Expected Result: Retraining flow works via pipeline
    Evidence: .sisyphus/evidence/task-23-retrain/{before,after}.json

  Scenario: Pipeline streams data for training
    Tool: Bash (logs)
    Steps:
      1. Run flagship with BUFF_LOG=debug
      2. Assert logs show pipeline stages executing (source → filter → map → batch → train)
      3. Assert no errors in pipeline stage transitions
    Expected Result: Pipeline orchestration visible in logs
    Evidence: .sisyphus/evidence/task-23-pipeline/server-log.txt

  Scenario: Observability spans present
    Tool: Bash
    Steps:
      1. Run with buff-observe console exporter
      2. Make a request
      3. Assert logs contain span: "request" → "predict" → "model_forward"
    Expected Result: End-to-end tracing works
    Evidence: .sisyphus/evidence/task-23-spans/server-log.txt
  ```

  **Commit**: YES
  - Message: `feat(flagship): data science workbench integrating dataframe + ML + pipeline + web + reactive`
  - Files: `examples/data-science-workbench/**`, `examples/data-science-workbench/data.csv`

---

- [x] 26. `buff-audit` Security Scanning + Code Signing

  **What to do** *(added post-comparative-analysis — security baseline for any production ecosystem)*:
  - Create `crates/buff-audit/` crate.
  - **`buff audit` subcommand** — reads `buff.lock`, queries RustSec advisory database (https://rustsec.org/advisories/) for `extern` deps via the `rustsec` crate. For Buff-registry deps, queries Buff advisory DB (new — initially seeded empty, framework authors file advisories via PR). Reports vulnerabilities with: advisory ID, affected versions, patched versions, recommended upgrade.
  - **`buff audit --fix`** — automatically updates affected deps to patched versions in `buff.toml`.
  - **Code signing** — `buff publish --sign` uses sigstore (cosign) to sign the tarball. Stores signature in registry alongside package. `buff add` verifies signature by default; `--no-verify` bypasses.
  - **`buff audit` in CI** — exit code 1 if any vulnerability found, suitable for CI gating.
  - 3 examples: audit clean project, audit finds vulnerable dep, audit --fix upgrades.
  - 10+ tests with mock advisory DB.
  - AGENTS.md + README.md.

  **Must NOT do**:
  - Do NOT exceed 2000 LOC or 15 public functions.
  - Do NOT auto-upgrade across major versions (only patch/minor for security fixes).
  - Do NOT require internet for `buff build` (only `buff audit` and `buff publish` need network).
  - Do NOT enforce code signing yet — make it opt-in for v1.13-v1.17, mandatory in v1.18+.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Integration-heavy (network calls, sigstore, registry API); well-scoped.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T17-T21 — Wave 4 sibling)
  - **Parallel Group**: Wave 4
  - **Blocks**: None directly
  - **Blocked By**: T0 (uses buff.toml + buff.lock), T1 (workspace resolution)

  **References**:
  - Existing `buff outdated` command (v1.x) — `crates/buff-lang-cli/src/commands/outdated.rs` (if exists) or similar; mirror the registry-query pattern.
  - Existing `buff publish` (v1.6) — extend with `--sign` flag.

  **External References**:
  - RustSec advisory DB: https://github.com/rustsec/advisory-db
  - rustsec crate: https://docs.rs/rustsec/latest/rustsec/
  - sigstore/cosign: https://docs.sigstore.dev/
  - npm audit: https://docs.npmjs.com/cli/audit

  **Acceptance Criteria**:

  ```
  Scenario: Audit detects vulnerable dependency
    Tool: Bash (cargo)
    Steps:
      1. Create project with buff.toml depending on a known-vulnerable version (e.g., old chrono with CVE)
      2. Run: cargo run -p buff-lang-cli -- audit
      3. Assert exit code 1
      4. Assert stderr contains advisory ID, affected version, patched version
    Expected Result: Vulnerability detected with clear remediation
    Evidence: .sisyphus/evidence/task-26-audit-detects/output.txt

  Scenario: Audit --fix upgrades vulnerable dep
    Tool: Bash
    Steps:
      1. Same setup as above
      2. Run: cargo run -p buff-lang-cli -- audit --fix
      3. Assert buff.toml now references patched version
      4. Re-run audit; assert exit 0
    Expected Result: Auto-fix works
    Evidence: .sisyphus/evidence/task-26-audit-fix/{before,after}.toml

  Scenario: Code signing roundtrip
    Tool: Bash
    Preconditions: sigstore/cosign installed
    Steps:
      1. buff publish --sign (uses ephemeral sigstore cert)
      2. Assert registry tarball has .sig sidecar
      3. buff add <package>@<version>
      4. Assert signature verified message in stdout
    Expected Result: Signing + verification work end-to-end
    Failure Indicators: If sigstore setup hard, mark DEFERRED-TO-MANUAL
    Evidence: .sisyphus/evidence/task-26-signing/output.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-audit): MVP CVE scanning + sigstore code signing`
  - Files: `crates/buff-audit/**`, `crates/buff-lang-cli/src/commands/{audit,publish}.rs`, `crates/buff-registry/src/handlers.rs` (signature verification)

---

- [x] 27. `buff fuzz` — Fuzzing Support via libFuzzer

  **What to do** *(added post-comparative-analysis — security-critical frameworks need fuzzing)*:
  - Create `crates/buff-fuzz/` crate (thin wrapper around libFuzzer via extern).
  - **`buff fuzz <file>` subcommand** — compiles the target with sanitizers + libFuzzer instrumentation, runs fuzzing loop until crash or timeout.
  - **`@fuzz` attribute on functions** — marks a function as a fuzz target. Function must accept arbitrary input (Bytes or String) and either crash, panic, or return normally. Codegen lowers to libFuzzer entry point.
  - **Corpus management** — fuzzing corpus stored in `.buff/fuzz-corpus/<target-name>/`. Crashes saved to `.buff/fuzz-crashes/`. Add `buff fuzz --repro <crash-file>` to reproduce a specific crash.
  - **Integration with buff-mock** — fuzz targets can use mocks to isolate external dependencies.
  - 2 examples: fuzz a parser (string → AST, look for panics), fuzz a crypto function (look for edge cases).
  - 8+ tests.
  - AGENTS.md + README.md.

  **Must NOT do**:
  - Do NOT exceed 1500 LOC or 10 public functions.
  - Do NOT implement custom mutators (use libFuzzer defaults).
  - Do NOT require fuzzing in CI (opt-in only; fuzzing is dev-time).
  - Do NOT fuzz frameworks that have no parsing/crypto surface (most compute frameworks don't need it).

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T22, T23 — Wave 5 sibling; fuzzing needs frameworks to exist first)
  - **Parallel Group**: Wave 5
  - **Blocks**: None
  - **Blocked By**: T0 (uses buff.toml + .buff/ dir convention), Wave 2 frameworks (need something to fuzz)

  **References**:
  - Existing `@test` attribute pattern — `@fuzz` follows same parser path.

  **External References**:
  - libFuzzer: https://llvm.org/docs/LibFuzzer.html
  - cargo-fuzz: https://github.com/rust-fuzz/cargo-fuzz
  - AFL.rs: https://github.com/rust-fuzz/afl.rs

  **Acceptance Criteria**:

  ```
  Scenario: Fuzz target compiles and runs briefly
    Tool: Bash (cargo)
    Steps:
      1. Write examples/fuzz/parse_buff.buff with @fuzz attribute on a parser function
      2. Run: cargo run -p buff-lang-cli -- fuzz examples/fuzz/parse_buff.buff --max-time 10
      3. Assert fuzzer ran for ~10 seconds
      4. Assert exit code 0 (no crash found in 10s)
    Expected Result: Fuzzing infrastructure works
    Evidence: .sisyphus/evidence/task-27-fuzz-runs/output.txt

  Scenario: Crash is captured and reproducible
    Tool: Bash
    Steps:
      1. Write a fuzz target with a known crash trigger (e.g., divide by zero on input "CRASH")
      2. Run fuzzer briefly
      3. Assert crash file in .buff/fuzz-crashes/
      4. Run: cargo run -p buff-lang-cli -- fuzz --repro <crash-file>
      5. Assert same crash reproduced
    Expected Result: Crashes captured + reproducible
    Evidence: .sisyphus/evidence/task-27-fuzz-crash/{crash-file,repro-output}.txt
  ```

  **Commit**: YES
  - Message: `feat(buff-fuzz): MVP fuzzing via libFuzzer with @fuzz attribute`
  - Files: `crates/buff-fuzz/**`, `crates/buff-lang-ast/src/decl.rs` (@fuzz attribute), `crates/buff-lang-cli/src/commands/fuzz.rs`

---

- [ ] 28. v1.24 Documentation & Codebase Refinement (Iterative Audit Until Convergence)

  **Goal** *(added per user request — post-v1.23 refinement pass)*: After v1.23.0 ships, run a comprehensive audit pass that finds and fixes everything outdated/incorrect/missing across the entire Buff project. Trivial issues are fixed in-place; non-trivial issues are documented for future iteration planning. **Iterate passes until a full pass finds zero new issues.** This task owns v1.24.0 release.

  **What to do** (iterative loop):

  **Phase 1 — Discovery scan** (each pass):
  Run all of the following scans; collect findings into a temporary audit log:

  - **Documentation scan**:
    - `AGENTS.md` (root + every per-crate `crates/*/AGENTS.md`) — verify each reflects current state. Per-crate AGENTS.md MUST exist for new v1.13-v1.24 crates: buff-{dataframe, tensor, image, audio, dsp, ecs, science, pipeline, ml, game, mock, audit, fuzz, web, db, template, reactive, observe}, buff-lang-debug-info. Missing → log. Stale → log.
    - `README.md` (root) — verify the status table includes v1.3-v1.24 (currently only shows through v1.2). Verify examples table includes new framework examples. Verify quickstart works.
    - `CHANGELOG.md` — if missing, CREATE with comprehensive entries for v1.0-v1.24. If exists, verify entries for v1.13-v1.24 are present and accurate.
    - `CONTRIBUTING.md` — verify references current workflow, conventions doc, registry, etc.
    - `LICENSE` — verify date range includes 2026.
    - Per-crate `README.md` files (each `crates/*/README.md`) — must exist and reflect current API.

  - **Examples scan**:
    - Every `examples/*.buff` file — actually run via `buff run` and verify it works. Log failures.
    - Every `examples/rust-vs-buff/*.buff` — verify still runs.
    - Every example mentioned in `README.md` exists and is correctly described.
    - Every framework example (added in v1.14-v1.24) runs correctly.

  - **Code scan**:
    - `grep -rn 'TODO\|FIXME\|XXX\|HACK\|unimplemented!\|todo!' crates/` — every match is a finding. Classify trivial (fix now) vs non-trivial (log for future).
    - `grep -rn 'unwrap()\|expect()\|panic!' crates/` in non-test code — AGENTS.md hard rule violation. Fix or log.
    - `grep -rn 'v0\.1\|v0\.5\|v1\.0\b' crates/` — outdated version references in doc comments or strings.
    - All `Cargo.toml` files — verify workspace consistency (no version pinned in crate Cargo.toml; all deps at root `[workspace.dependencies]`).
    - All rustdoc comments — verify accuracy (function signatures, return types, examples compile).

  - **Plan & decision scan**:
    - `.sisyphus/plans/*.md` — verify accuracy (some plans reference "v1.x shipped" statuses that may be outdated).
    - `.sisyphus/decisions/*.md` — verify decision docs reflect actual implementation.
    - Cross-reference deferred items: search for "deferred to v1.24+" or "deferred to v1.25+" or "post-v1.23" across the entire codebase + plans. Each deferred item is a finding.

  - **Structural scan**:
    - Root `Cargo.toml [workspace.members]` — verify every `crates/*/` directory is listed (or matches the glob).
    - Root `Cargo.toml [workspace.dependencies]` — verify every dependency used anywhere is declared here.
    - All `.github/workflows/*.yml` — verify CI runs correct commands on correct matrix.
    - `rust-toolchain.toml` — verify version is current.

  - **Tooling integration scan**:
    - Verify `buff-lsp` understands all v1.13-v1.24 language additions (Channel<T>, @feature, @deprecated, @mock, etc.).
    - Verify `buff fmt` correctly formats all new syntax.
    - Verify `buff check` catches violations of new attributes.
    - Verify `buff-vsc` (VSCode extension) highlights new keywords/attributes correctly.

  **Phase 2 — Triage** (after each scan):
  For each finding, classify:
  - **TRIVIAL** (fix in this task): typo, outdated version number, missing AGENTS.md, dead link, stale comment, missing CHANGELOG entry, etc. Estimated <50 LOC change per fix.
  - **NON-TRIVIAL** (defer to future): new feature work, architectural change, large refactor, anything >50 LOC. Log to `.sisyphus/decisions/v1.24-followup.md` with: finding, recommended action, rough estimate, suggested target version (v1.25, v1.26, etc.).

  **Phase 3 — Fix TRIVIAL items**:
  Apply fixes as atomic commits (one per logical fix or group of related fixes).
  - Update `AGENTS.md` (root + per-crate) to reflect v1.24 current state.
  - Update `README.md` (status table, examples table, quickstart, version references).
  - Update or create `CHANGELOG.md` with comprehensive v1.0-v1.24 entries.
  - Update `CONTRIBUTING.md` if needed.
  - Fix outdated version references throughout.
  - Add missing per-crate `AGENTS.md` for new crates.
  - Add missing per-crate `README.md` for new crates.
  - Fix dead links, typos, stale TODOs where trivial.

  **Phase 4 — Iterate**:
  Repeat Phases 1-3 until a complete Discovery scan finds ZERO new TRIVIAL issues. May take 3-5 passes for a project this size. **Do NOT stop early** — the user explicitly wants convergence, not "good enough".

  **Phase 5 — Final report**:
  Write `.sisyphus/decisions/v1.24-audit-report.md` summarizing:
  - Pass count (how many iterations until convergence)
  - Trivial issues found vs fixed (counts + categories)
  - Non-trivial issues logged for future (list with recommended target versions)
  - Files touched (summary count)
  - Cross-references to `.sisyphus/decisions/v1.24-followup.md` for non-trivial items

  **Specific known issues to address** *(seed list — Phase 1 will find more)*:
  - Root `README.md` status table stops at v1.2; needs v1.3-v1.24 entries.
  - Root `README.md` examples table omits v1.13-v1.24 framework examples.
  - `CHANGELOG.md` likely doesn't exist — CREATE covering v1.0-v1.24.
  - `crates/buff-lang-codegen-rust/src/rust_codegen.rs` is 12,777 lines (Momus confirmed); some plan/docs may reference it as ~3000 lines (from earlier v1.0-era references). Hunt and fix.
  - Earlier `Momus` reviews caught minor reference drift (e.g., `lower_spawn` line numbers shifted from ~2647 to ~6860 as the file grew). Hunt for similar drift in all docs.
  - Per-crate AGENTS.md for v1.13-v1.24 new crates may be missing or templated-only.
  - Deferred items scattered throughout plan: search "v1.24+", "v1.25+", "post-v1.23", "defer" to find them all.

  **Must NOT do**:
  - Do NOT exceed 5000 LOC of changes in this task. If more issues exist, log them — don't bloat v1.24.
  - Do NOT implement non-trivial deferred items in this task — log them for v1.25+ planning.
  - Do NOT break v1.23 behavior — all examples that worked in v1.23 must still work after v1.24.
  - Do NOT skip the iteration loop — convergence is the success criterion, not "did one pass".
  - Do NOT touch `.sisyphus/plans/buff-v1x-frameworks.md` (this file) — it's a historical artifact of the planning conversation; corrections to plan accuracy go in the audit report.
  - Do NOT rewrite docs from scratch — extend and update existing docs in place.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Self-iterating audit requires deep project understanding, judgment about trivial vs non-trivial, broad codebase knowledge. Not a quick task.
  - **Skills**: [`/git-master`]
    - `/git-master`: Atomic commits per logical fix; many small commits across many files.

  **Parallelization**:
  - **Can Run In Parallel**: NO — sequential after Wave 5 + F1-F4 verification of v1.23.
  - **Parallel Group**: Wave 6 (solo — owns v1.24.0 release)
  - **Blocks**: None (this is the last task before final user approval)
  - **Blocked By**: T23 (flagship complete), F1-F4 (v1.23 verification approved) — T28 starts only after v1.23 is fully shipped

  **References** (CRITICAL — seed list for Phase 1 scan):

  **Pattern References**:
  - `AGENTS.md` (repo root) — primary doc to update.
  - `crates/*/AGENTS.md` (existing 6 per-crate AGENTS.md files) — model for new crate AGENTS.md.
  - `README.md` (repo root) — primary user-facing doc.
  - `.sisyphus/plans/buff-master.md` — orchestrator plan; verify accuracy.
  - `.sisyphus/plans/buff-post-v10-tooling.md` — existing v1.1-v1.12 work; verify status.
  - `.sisyphus/plans/buff-conventions.md` — Buff language conventions; verify still accurate.

  **External References**:
  - Keep a Changelog format: https://keepachangelog.com/
  - SemVer 2.0: https://semver.org/

  **WHY Each Reference Matters**:
  - AGENTS.md (root): the canonical project knowledge base; must reflect v1.24 state.
  - Per-crate AGENTS.md: each new crate (buff-ml, buff-dataframe, etc.) needs one for future contributors.
  - README.md: the first thing users see; status table is the version source-of-truth.
  - CHANGELOG.md: every release platform needs one; Buff doesn't have one yet.

  **Acceptance Criteria**:

  **Per-phase deliverables** (each MUST have QA scenario below):
  - [ ] Phase 1 Discovery scan produces a findings log per pass
  - [ ] Phase 2 Triage classifies every finding as TRIVIAL or NON-TRIVIAL
  - [ ] Phase 3 trivial fixes applied as atomic commits
  - [ ] Phase 4 iteration converges (a final full scan finds zero new TRIVIAL issues)
  - [ ] Phase 5 audit report committed at `.sisyphus/decisions/v1.24-audit-report.md`
  - [ ] Non-trivial followup doc committed at `.sisyphus/decisions/v1.24-followup.md`
  - [ ] All new crates have AGENTS.md and README.md
  - [ ] Root README.md status table covers v1.0-v1.24
  - [ ] CHANGELOG.md exists with comprehensive v1.0-v1.24 entries

  **QA Scenarios** (MANDATORY):

  ```
  Scenario: Iteration converges (no infinite loop)
    Tool: Bash (cargo)
    Preconditions: T28 in progress
    Steps:
      1. Run discovery scan (Phase 1) — produces findings.log
      2. Count findings
      3. If findings == 0: pass and exit loop
      4. If findings > 0: triage, fix trivial, log non-trivial, repeat
      5. Assert: after some N passes (target: 3-5), a complete scan finds zero new TRIVIAL findings
      6. Assert: total passes did not exceed 10 (safety — if exceeded, force-stop and report)
    Expected Result: Iteration converges naturally; total passes ≤ 10
    Failure Indicators: Pass count exceeds 10 without convergence (indicates a stuck loop)
    Evidence: .sisyphus/evidence/task-28-convergence/pass-log.txt

  Scenario: All new crates have AGENTS.md and README.md
    Tool: Bash (ls + cat)
    Steps:
      1. For each crate in crates/buff-{dataframe, tensor, image, audio, dsp, ecs, science, pipeline, ml, game, mock, audit, fuzz, web, db, template, reactive, observe}, buff-lang-debug-info:
         - Assert crates/<crate>/AGENTS.md exists
         - Assert crates/<crate>/README.md exists
         - Assert AGENTS.md contains STRUCTURE, WHERE TO LOOK, CONVENTIONS sections
         - Assert README.md contains install/usage sections
      2. Report any missing files
    Expected Result: Every v1.13-v1.24 crate has both files
    Failure Indicators: Missing AGENTS.md or README.md in any new crate
    Evidence: .sisyphus/evidence/task-28-crate-docs/listing.txt

  Scenario: Root README.md reflects v1.24 status
    Tool: Bash (cat + grep)
    Steps:
      1. Read README.md
      2. Assert status table contains rows for: v1.0, v1.5, v1.7, v1.8, v1.9, v1.10, v1.11, v1.12, v1.13, v1.14, v1.15, v1.16, v1.17, v1.18, v1.19, v1.20, v1.21, v1.22, v1.23, v1.24
      3. Assert examples table includes framework examples (buff-ml/hello.buff, buff-dataframe/hello.buff, etc.)
      4. Assert quickstart commands run successfully:
         - cargo run -p buff-lang-cli -- run examples/ola.buff
         - cargo run -p buff-lang-cli -- new test_proj --template ml
    Expected Result: README is current
    Failure Indicators: Status table missing versions, examples don't run
    Evidence: .sisyphus/evidence/task-28-readme/{status-table,quickstart-output}.txt

  Scenario: CHANGELOG.md exists and covers all releases
    Tool: Bash (cat + grep)
    Steps:
      1. Assert CHANGELOG.md exists at repo root
      2. Assert it contains section headers for: v1.0.0, v1.5.0, v1.7.0, v1.8.0, v1.9.0, v1.10.0, v1.11.0, v1.12.0, v1.13.0, v1.14.0, v1.15.0, v1.16.0, v1.17.0, v1.18.0, v1.19.0, v1.20.0, v1.21.0, v1.22.0, v1.23.0, v1.24.0
      3. Each section has at least 3 bullet points describing what shipped
      4. Format follows Keep a Changelog convention
    Expected Result: Comprehensive CHANGELOG
    Failure Indicators: Missing file, missing version sections
    Evidence: .sisyphus/evidence/task-28-changelog/listing.txt

  Scenario: All examples run without error
    Tool: Bash (cargo)
    Steps:
      1. Find all .buff files under examples/
      2. For each (or a representative sample if 100+):
         - cargo run -p buff-lang-cli -- run <file>
         - Assert exit 0
      3. Log any failures
    Expected Result: All examples work
    Failure Indicators: Any example fails to compile or run
    Evidence: .sisyphus/evidence/task-28-examples/results.txt

  Scenario: No AGENTS.md hard-rule violations in non-test code
    Tool: Bash (grep)
    Steps:
      1. grep -rn 'unwrap()\|expect()\|panic!\|unimplemented!\|todo!' crates/ --include='*.rs'
      2. Filter out lines containing '#[cfg(test)]' or '/tests/' or '#[test]'
      3. Assert: zero violations in remaining matches
    Expected Result: All AGENTS.md hard rules honored
    Failure Indicators: unwrap/panic in production code
    Evidence: .sisyphus/evidence/task-28-no-panic/violations.txt

  Scenario: Audit report exists with required sections
    Tool: Bash (cat)
    Steps:
      1. Assert .sisyphus/decisions/v1.24-audit-report.md exists
      2. Assert sections present:
         - "Pass Count" (number of iterations to convergence)
         - "Trivial Findings Summary" (counts by category)
         - "Non-Trivial Findings" (deferred to future)
         - "Files Touched" (count + categories)
         - "Convergence Statement" (final pass found zero new trivial issues)
      3. Assert .sisyphus/decisions/v1.24-followup.md exists if any non-trivial items deferred
    Expected Result: Audit deliverables complete
    Failure Indicators: Missing audit report, missing convergence statement
    Evidence: .sisyphus/evidence/task-28-report/audit-report.md

  Scenario: All deferred items from plan are accounted for
    Tool: Bash (grep + cross-reference)
    Steps:
      1. grep -rn 'v1\.24+\|v1\.25+\|post-v1\.23\|deferred to' .sisyphus/plans/ crates/ examples/
      2. For each match: verify it appears in either:
         - The v1.24 audit report as FIXED, OR
         - The v1.24 followup doc as DEFERRED with target version
      3. Assert: zero unaccounted deferred items
    Expected Result: No forgotten deferred work
    Failure Indicators: Deferred item exists but not in either report
    Evidence: .sisyphus/evidence/task-28-deferred-accounting/cross-ref.txt
  ```

  **Commit**: YES (many small atomic commits per logical fix)
  - Suggested commit sequence (one per category of fix):
    1. `docs(readme): update status table for v1.0-v1.24`
    2. `docs(changelog): create CHANGELOG.md covering v1.0-v1.24`
    3. `docs(agents): refresh root AGENTS.md to v1.24 state`
    4. `docs(agents): add per-crate AGENTS.md for new v1.13-v1.24 crates` (one commit per crate if substantial)
    5. `docs(readme): add per-crate README.md for new crates`
    6. `fix(examples): update outdated version references in examples/`
    7. `docs(contributing): refresh CONTRIBUTING.md`
    8. `chore(deps): verify workspace dependency completeness`
    9. `fix(docs): correct stale line/file references caught by audit`
    10. `docs(audit): commit v1.24 audit report + followup`
  - Files: `AGENTS.md`, `crates/*/AGENTS.md`, `crates/*/README.md`, `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `LICENSE`, `examples/*.buff`, `.sisyphus/decisions/v1.24-{audit-report,followup}.md`, plus any specific files where stale references are fixed
  - Pre-commit: `cargo check --workspace && cargo test --workspace`

---

---

## Tasks T29-T36 — Tier 1 Common Frameworks (Release v1.16.0)

> Universal-pain-point frameworks identified by cross-language analysis (each problem has popular libraries in 6+ languages). All wrap mature Rust crates via safe `extern` FFI per T4 guide. All ship at MVP quality: working happy-path, 3 examples, 10+ tests, README, registry entry with "experimental" badge.

- [x] 29. `buff-validate` — Declarative Schema Validation (pydantic-equivalent)

  **What to do**: Wrap Rust `validator` crate (or build derive-based validation on serde). Provide `@validate` attribute on struct fields with rules (email, url, length, range, regex, custom). `Validator.validate(instance) -> Result<(), ValidationErrors>`. Auto-generate JSON Schema from validated structs. Cross-language prevalence: 6/6 (pydantic/Zod/Joi/validator/Hibernate Validator/FluentValidation).
  - **LOC budget**: ≤2000, ≤20 public fns. **External deps**: `validator`, `serde_json` (for JSON Schema).
  - **Must NOT do**: No compile-time macro validation (defer with T3 outcome); no async validators (v1.22+).
  - **References**: T0 (conventions), T1 (multi-file), T4 (FFI guide). Rust `validator` crate: https://docs.rs/validator/latest/validator/
  - **Acceptance**: Validation passes/fails correctly on test structs. JSON Schema export works. 3 examples + 10 tests.
  - **Commit**: `feat(buff-validate): MVP schema validation + JSON Schema export`

- [x] 30. `buff-config` — Layered Configuration (viper-equivalent)

  **What to do**: Wrap Rust `figment` crate (or `config` crate). Provide layered config: defaults → file (TOML/YAML/JSON) → env vars → CLI args. Type-safe config structs via existing struct decl. Hot reload via `config.watch(callback)`. Cross-language prevalence: 6/6 (dynaconf/convict/viper/figment/typesafe/Microsoft.Extensions.Configuration).
  - **LOC budget**: ≤2000, ≤20 fns. **External deps**: `figment`, `notify` (hot reload).
  - **Must NOT do**: No remote config servers (etcd/consul) in MVP; no secret-service integration (v1.22+).
  - **References**: T0 (.env auto-load extension), T1. Rust `figment`: https://docs.rs/figment/latest/figment/
  - **Acceptance**: Layered precedence works. Hot reload callback fires on file change. 3 examples + 10 tests.
  - **Commit**: `feat(buff-config): MVP layered config with hot reload`

- [x] 31. `buff-cache` — In-Memory + Distributed Cache

  **What to do**: Wrap Rust `moka` crate (in-memory) + `redis` crate (distributed). Provide `Cache.new(max_capacity)`, `cache.get(key)`, `cache.set(key, value, ttl)`, `cache.delete(key)`. TTL eviction. LRU semantics. Redis backend optional via feature flag. Cross-language prevalence: 6/6 (cachetools/lru-cache/go-cache/moka/Caffeine/MemoryCache).
  - **LOC budget**: ≤2000, ≤15 fns. **External deps**: `moka`, `redis` (optional).
  - **Must NOT do**: No multi-tier cache orchestration in MVP; no cache invalidation pub/sub (v1.22+).
  - **References**: T0, T1. Rust `moka`: https://docs.rs/moka/latest/moka/
  - **Acceptance**: In-memory LRU works. TTL evicts correctly. Redis path verified with mock. 3 examples + 10 tests.
  - **Commit**: `feat(buff-cache): MVP in-memory LRU + Redis backend`

- [x] 32. `buff-cli` — CLI Framework for User Programs

  **What to do**: Wrap Rust `clap` crate with Buff-idiomatic API. Note: clap already powers the `buff` compiler CLI; this task exposes clap to USER programs. Provide `App.new(name)`, `app.command(name, handler)`, `app.flag(name, short, description)`, `app.parse(args) -> ParsedArgs`. Auto-generated help. Subcommand nesting. Cross-language prevalence: 6/6 (Click/Commander/Cobra/clap/Picocli/System.CommandLine).
  - **LOC budget**: ≤2000, ≤20 fns. **External deps**: `clap` (already in workspace).
  - **Must NOT do**: No interactive prompts (Inquirer-style); no shell completion generation (v1.22+).
  - **References**: T0, T1. Existing `crates/buff-lang-cli/src/cli.rs` for clap pattern reference.
  - **Acceptance**: Subcommands parse correctly. Help text generated. 3 examples (hello, subcommands, flags) + 10 tests.
  - **Commit**: `feat(buff-cli): MVP clap-equivalent for user programs`

- [x] 33. `buff-http-client` — Idiomatic HTTP Client

  **What to do**: Wrap Rust `reqwest` crate (already used via extern). Provide Buff-idiomatic fluent API: `HttpClient.new()`, `client.get(url)`, `client.post(url).json(body).header(name, val).send() -> Response`. Response: `.status()`, `.json<T>()`, `.text()`, `.headers()`. Built-in retry via T36 buff-resilience. Cross-language prevalence: 6/6 (requests/axios/resty/reqwest/OkHttp/RestSharp).
  - **LOC budget**: ≤2000, ≤20 fns. **External deps**: `reqwest` (existing).
  - **Must NOT do**: No HTTP/2 push promises; no streaming uploads in MVP (v1.22+).
  - **References**: T0, T1, T4 (FFI guide), existing `examples/extern_reqwest.buff`.
  - **Acceptance**: GET/POST/PUT/DELETE work. JSON body serialization works. Timeout + retry composes with T36. 3 examples + 10 tests.
  - **Commit**: `feat(buff-http-client): MVP reqwest wrapper with fluent API`

- [x] 34. `buff-auth` — JWT + OAuth2 + Password Hashing + RBAC

  **What to do**: Wrap Rust crates: `jsonwebtoken` (JWT), `oauth2` (OAuth2 client flows), `argon2` (password hashing), `casbin` or custom (RBAC). Provide: `JWT.encode(claims, secret)`, `JWT.decode(token, secret)`, `OAuth2Client.authorization_url()`, `OAuth2Client.exchange_code(code)`, `Password.hash(plain)`, `Password.verify(plain, hash)`, `Rbac.enforce(user, resource, action)`. Cross-language prevalence: 6/6 (Authlib/Passport/golang-jwt/oauth2/Spring Security/IdentityServer4).
  - **LOC budget**: ≤3000, ≤25 fns. **External deps**: `jsonwebtoken`, `oauth2`, `argon2`, optionally `casbin`.
  - **Must NOT do**: No WebAuthn / passkey support (v1.22+); no SAML (v1.22+); no multi-factor auth in MVP.
  - **References**: T0, T1, T4, existing T124k Hash/HMAC. Rust `jsonwebtoken`: https://docs.rs/jsonwebtoken/latest/jsonwebtoken/
  - **Acceptance**: JWT roundtrip works. OAuth2 auth-code flow works against test provider. Password hash/verify works. RBAC enforces policy. 4 examples + 15 tests.
  - **Commit**: `feat(buff-auth): MVP JWT + OAuth2 + password hashing + RBAC`

- [x] 35. `buff-jobs` — Background Job Queue + Scheduler

  **What to do**: Wrap Rust `apalis` crate (or `tokio-cron-scheduler`). Provide: `Queue.new(backend)`, `queue.enqueue(job)`, `queue.worker(handler)`, `Scheduler.cron(expr, job)`, `Scheduler.interval(duration, job)`. Backends: in-memory (MVP), Redis (defer). Job priorities, retries via T36, dead-letter queue. Cross-language prevalence: 6/6 (Celery/Bull/asynq/apalis/Quartz/Hangfire).
  - **LOC budget**: ≤3000, ≤25 fns. **External deps**: `apalis` or `tokio-cron-scheduler`, `cron`.
  - **Must NOT do**: No Redis/Postgres backend in MVP (in-memory only); no workflow DAG dependencies (v1.22+).
  - **References**: T0, T1, T2 (Channel<T> for internal queue), T36 (resilience for retries). Rust `apalis`: https://docs.rs/apalis/latest/apalis/
  - **Acceptance**: Enqueue/dequeue works. Cron schedule fires correctly. Retry policy honored. 3 examples (queue, cron, retry) + 12 tests.
  - **Commit**: `feat(buff-jobs): MVP background queue + cron scheduler`

- [x] 36. `buff-resilience` — Retry + Circuit Breaker + Rate Limiter + Timeout

  **What to do**: Wrap Rust `tower` middleware crate (or build standalone). Provide: `Retry.policy(max_attempts, backoff)`, `CircuitBreaker.new(failure_threshold, reset_timeout)`, `RateLimiter.new(requests_per_second)`, `Timeout.duration(secs)`. Composable: `pipeline = Retry → CircuitBreaker → RateLimiter → handler`. Cross-language prevalence: 6/6 (tenacity/cockatiel/failsafe-go/tower/Resilience4j/Polly).
  - **LOC budget**: ≤2500, ≤20 fns. **External deps**: `tower` (or standalone), `governor` (rate limiting).
  - **Must NOT do**: No bulkhead pattern (v1.22+); no distributed rate limiting (Redis-based) in MVP.
  - **References**: T0, T1. Rust `tower`: https://docs.rs/tower/latest/tower/
  - **Acceptance**: Retry with exponential backoff works. Circuit breaker opens/closes correctly. Rate limiter throttles. Timeout aborts. Composition works. 4 examples + 15 tests.
  - **Commit**: `feat(buff-resilience): MVP retry + circuit breaker + rate limiter + timeout`

---

## Tasks T37-T44 — Tier 2 Common Frameworks (Release v1.17.0)

> Testing + I/O + state frameworks identified by cross-language analysis (4-5 languages prevalence). All wrap mature Rust crates via safe extern FFI per T4 guide. MVP quality.

- [x] 37. `buff-fake` — Fake Data Generation (Faker-equivalent)

  **What to do**: Wrap Rust `fake` crate. Provide: `Fake.name()`, `Fake.email()`, `Fake.address()`, `Fake.phone()`, `Fake.uuid()`, `Fake.lorem(words)`, `Fake.int(min, max)`, `Fake.datetime(range)`. Locales: en-US, pt-BR (matches existing PT-BR example convention). Cross-language prevalence: 6/6 (Faker/Faker.js/gofakeit/fake/EasyRandom/Bogus).
  - **LOC budget**: ≤1500, ≤15 fns. **External deps**: `fake`.
  - **References**: T0, T1, T25 (buff-mock integration for test data). Rust `fake`: https://docs.rs/fake/latest/fake/
  - **Acceptance**: Generates plausible values per category. Locale switching works. Seeded RNG reproduces values. 3 examples + 10 tests.
  - **Commit**: `feat(buff-fake): MVP fake data generation with locales`

- [x] 38. `buff-assertions` — Fluent Test Assertions (assertThat-style)

  **What to do**: Wrap Rust `claim` crate (or build on existing `assert_eq`). Provide: `assertThat(value).isEqualTo(expected)`, `.isGreaterThan(n)`, `.isInstanceOf(Type)`, `.startsWith(s)`, `.contains(item)`, `.throws(fn)`. Readable failure messages. Cross-language prevalence: 6/6 (Hamcrest/Chai/testify/claim/AssertJ/FluentAssertions).
  - **LOC budget**: ≤2000, ≤30 fns (fluent API). **External deps**: `claim` (optional).
  - **References**: T0, T1, existing `assert_eq` prelude function. Rust `claim`: https://docs.rs/claim/latest/claim/
  - **Acceptance**: Fluent chains read naturally. Failure messages descriptive. 5 examples + 15 tests.
  - **Commit**: `feat(buff-assertions): MVP fluent test assertions`

- [x] 39. `buff-archive` — Zip/Tar/Gz/Zstd Compression

  **What to do**: Wrap Rust crates: `zip` (read/write zip), `tar` (tarballs), `flate2` (gzip), `zstd` (Zstandard). Provide unified API: `Archive.compress_dir(input_dir, output_path, format)`, `Archive.extract(archive_path, output_dir)`, `Format.{Zip,Tar,Gz,Zstd}` enum. Cross-language prevalence: 6/6 (zipfile/zlib/compress/flate2-tar-zstd/java.util.zip/System.IO.Compression).
  - **LOC budget**: ≤2000, ≤15 fns. **External deps**: `zip`, `tar`, `flate2`, `zstd`.
  - **Must NOT do**: No 7z, RAR, BZip2 (v1.22+); no encryption-at-rest (combine with T34 if needed).
  - **References**: T0, T1. Rust `zip`: https://docs.rs/zip/latest/zip/
  - **Acceptance**: Roundtrip compress → extract preserves files. All 4 formats work. 4 examples + 12 tests.
  - **Commit**: `feat(buff-archive): MVP zip/tar/gz/zstd compression`

- [x] 40. `buff-fsm` — State Machine Library

  **What to do**: Wrap Rust `sfsm` crate (or build standalone). Provide: `Machine.new(initial_state)`, `machine.add_transition(from, event, to, guard, action)`, `machine.fire(event)`, `machine.current_state()`. States as enums, events as enums. Codegen-time transition table validation. Cross-language prevalence: 5/6 (transitions/xstate/looplab-fsm/sfsm/squirrel-foundation/Stateless).
  - **LOC budget**: ≤2000, ≤15 fns. **External deps**: `sfsm` or standalone.
  - **References**: T0, T1. Rust `sfsm`: https://docs.rs/sfsm/latest/sfsm/
  - **Acceptance**: State transitions correctly. Invalid transitions error. Guards block transitions. Actions fire. 3 examples (traffic light, order status, turnstile) + 12 tests.
  - **Commit**: `feat(buff-fsm): MVP state machine library`

- [x] 41. `buff-pubsub` — In-Process Event Bus

  **What to do**: Build standalone on T2 Channel<T> (no extern needed). Provide: `EventBus.new()`, `bus.subscribe(topic, handler)`, `bus.publish(topic, event)`. Topics as strings. Handlers as closures. Optional: typed events via generics. Cross-language prevalence: 5/6 (blinker/EventEmitter/eventbus/EventBus — Go uses channels natively).
  - **LOC budget**: ≤1500, ≤10 fns. **External deps**: none (uses T2 Channel<T>).
  - **References**: T0, T1, T2 (Channel<T>).
  - **Acceptance**: Subscribe/publish delivers events to all subscribers. Multiple subscribers receive same event. 3 examples + 10 tests.
  - **Commit**: `feat(buff-pubsub): MVP in-process event bus`

- [x] 42. `buff-email` — SMTP + Templated Email

  **What to do**: Wrap Rust `lettre` crate. Provide: `Email.new(from, to, subject)`, `email.body(text)`, `email.html(template, context)`, `email.attach(path)`, `SmtpClient.send(email)`. Templating via T19 buff-template (handlebars). Cross-language prevalence: 5/6 (emails/nodemailer/gomail/lettre/JavaMail/MailKit).
  - **LOC budget**: ≤2000, ≤15 fns. **External deps**: `lettre`, `handlebars` (existing).
  - **Must NOT do**: No IMAP/POP3 receiving (v1.22+); no calendar integration.
  - **References**: T0, T1, T4, T19 (buff-template). Rust `lettre`: https://docs.rs/lettre/latest/lettre/
  - **Acceptance**: Send plain-text email via SMTP. HTML template renders. Attachments work. 3 examples + 10 tests (use mailtrap or mock SMTP).
  - **Commit**: `feat(buff-email): MVP SMTP client with templates`

- [x] 43. `buff-scrape` — HTML Parsing + Crawling

  **What to do**: Wrap Rust `scraper` crate (HTML parsing) + `fantoccini` (headless browser via WebDriver). Provide: `Document.from_html(html)`, `doc.select(css) -> Vector<Element>`, `el.text()`, `el.attr(name)`, `Crawler.new(seed_url)`, `crawler.follow_links(policy)`. Cross-language prevalence: 6/6 (BeautifulSoup/cheerio/colly/scraper/Jsoup/HtmlAgilityPack).
  - **LOC budget**: ≤2500, ≤20 fns. **External deps**: `scraper`, `fantoccini` (optional), `reqwest` (existing via T33).
  - **Must NOT do**: No JS rendering by default (defer to fantoccini optional path); no distributed crawling.
  - **References**: T0, T1, T4, T33 (buff-http-client). Rust `scraper`: https://docs.rs/scraper/latest/scraper/
  - **Acceptance**: Parse static HTML. CSS selector queries work. Crawler respects robots.txt. 3 examples + 12 tests.
  - **Commit**: `feat(buff-scrape): MVP HTML parsing + crawling`

- [x] 44. `buff-i18n` — Internationalization

  **What to do**: Wrap Rust `fluent` crate (Mozilla's i18n system) + `rust-i18n` for simpler workflows. Provide: `t!(key, locale: "en")` macro-like function, `I18n.load(locale)`, `I18n.available_locales()`. Message catalogs in `locales/<locale>.ftl` (Fluent) or `.toml`. Pluralization, gender, ICU MessageFormat. Cross-language prevalence: 6/6 (babel/i18next/gotext/fluent/ICU4J/IStringLocalizer).
  - **LOC budget**: ≤2500, ≤15 fns. **External deps**: `fluent`, `rust-i18n`, `unic-langid`.
  - **Must NOT do**: No machine translation; no RTL layout helpers (UI concern).
  - **References**: T0, T1. Rust `fluent`: https://docs.rs/fluent/latest/fluent/
  - **Acceptance**: Translate keys per locale. Pluralization rules work. Missing key warning. 3 examples (en, pt-BR, es) + 10 tests.
  - **Commit**: `feat(buff-i18n): MVP internationalization with Fluent`

---

## Tasks T45-T52 — Tier 3 Specialized Frameworks (Release v1.18.0)

> Rust-ecosystem leverage: specialized frameworks where Buff wraps a mature Rust crate to provide idiomatic API. These address 1-3 language ecosystems but unlock valuable use cases.

- [x] 45. `buff-geo` — Geospatial / GIS

  **What to do**: Wrap Rust `geo` crate + `geo-types`. Provide: `Point.new(lat, lon)`, `Polygon.from_coords(...)`, `LineString`, distance/area/buffer/intersect ops, `Projection.wgs84_to_web_mercator()`. Cross-language: 4/6 (shapely/turf/geo/JTS — Java/C# have libraries too).
  - **LOC budget**: ≤2500, ≤25 fns. **External deps**: `geo`, `geo-types`, `proj`.
  - **References**: T0, T1, T4. Rust `geo`: https://docs.rs/geo/latest/geo/
  - **Acceptance**: Distance/area calculations correct. Intersection detection works. 3 examples + 12 tests.
  - **Commit**: `feat(buff-geo): MVP geospatial operations`

- [x] 46. `buff-nlp` — Text Processing / NLP

  **What to do**: Wrap Rust `whatlang` (language detection), `rust-stemmers` (Porter stemmer), `unicode-segmentation` (work/char segmentation). Provide: `Text.detect_language(text)`, `Text.stem(word)`, `Text.tokenize(text)`, `Text.sentences(text)`. Cross-language: 3/6 (spaCy/natural/whatlang — Python dominant).
  - **LOC budget**: ≤2000, ≤15 fns. **External deps**: `whatlang`, `rust-stemmers`, `unicode-segmentation`.
  - **References**: T0, T1, T4. Rust `whatlang`: https://docs.rs/whatlang/latest/whatlang/
  - **Acceptance**: Language detection accuracy >90% on test corpus. Stemmer produces correct stems. 3 examples + 10 tests.
  - **Commit**: `feat(buff-nlp): MVP text processing (detection, stemmer, tokenizer)`

- [x] 47. `buff-chat` — Discord/Telegram Bots

  **What to do**: Wrap Rust `serenity` (Discord) + `teloxide` (Telegram). Provide unified `Bot.new(platform, token)`, `bot.command(name, handler)`, `bot.on_message(handler)`, `bot.start()`. Cross-platform handler abstraction. Cross-language: 5/6 (discord.py/discord.js/discordgo/serenity/JDA).
  - **LOC budget**: ≤3000, ≤20 fns. **External deps**: `serenity`, `teloxide`.
  - **References**: T0, T1, T4. Rust `serenity`: https://docs.rs/serenity/latest/serenity/
  - **Acceptance**: Bot connects to test server (Discord test mode + Telegram bot API). Command dispatch works. 3 examples + 10 tests (mock API).
  - **Commit**: `feat(buff-chat): MVP Discord + Telegram bot framework`

- [x] 48. `buff-web3` — Blockchain RPC + Smart Contracts

  **What to do**: Wrap Rust `ethers-rs` crate. Provide: `Provider.new(rpc_url)`, `Wallet.from_private_key(key)`, `Contract.new(address, abi, wallet)`, `contract.method(name).call()`, `contract.method(name).send()`. Read/write blockchain. Cross-language: 4/6 (web3.py/ethers.js/ethers-rs/Web3j).
  - **LOC budget**: ≤3000, ≤25 fns. **External deps**: `ethers`, `ethers-contract`, `ethers-signers`.
  - **References**: T0, T1, T4, T34 (auth - signed transactions). Rust `ethers`: https://docs.rs/ethers/latest/ethers/
  - **Acceptance**: Connect to public RPC. Read contract state. Sign and send transaction (testnet). 3 examples + 12 tests (use local testnet).
  - **Commit**: `feat(buff-web3): MVP Ethereum client + contract interaction`

- [x] 49. `buff-crypto-extras` — AES/RSA/ECC/argon2 (beyond Hash/HMAC)

  **What to do**: Extend T124k Hash/HMAC with symmetric/asymmetric encryption. Wrap Rust `aes-gcm` (AES), `rsa`, `p256`/`p384` (ECC), `argon2` (already used in T34). Provide: `AES.encrypt(plaintext, key)`, `AES.decrypt(ciphertext, key)`, `RSA.generate_keypair()`, `RSA.sign(data, private)`, `ECDH.derive_shared(private, public)`. Cross-language: 6/6 (cryptography/pycryptodome/crypto/ring/BouncyCastle/System.Security.Cryptography).
  - **LOC budget**: ≤2500, ≤20 fns. **External deps**: `aes-gcm`, `rsa`, `p256`, `argon2` (shared with T34).
  - **Must NOT do**: No homebrew crypto; no TLS implementation (use rustls).
  - **References**: T0, T1, T4, T34, existing T124k. Rust `aes-gcm`: https://docs.rs/aes-gcm/latest/aes_gcm/
  - **Acceptance**: AES roundtrip works. RSA sign/verify works. ECDH key agreement works. 4 examples + 15 tests (NIST test vectors).
  - **Commit**: `feat(buff-crypto-extras): MVP AES/RSA/ECC/argon2 beyond Hash/HMAC`

- [x] 50. `buff-xml` — XML Parsing

  **What to do**: Wrap Rust `quick-xml` crate. Provide: `Xml.from_str(xml) -> Document`, `doc.root()`, `doc.find(xpath)`, `el.children()`, `el.attr(name)`, `Xml.to_string(doc)`. Streaming for large XML. Cross-language: 6/6 (lxml/quick-xml/jsoup/SAX — all stdlib or popular).
  - **LOC budget**: ≤2000, ≤15 fns. **External deps**: `quick-xml`.
  - **References**: T0, T1, T4. Rust `quick-xml`: https://docs.rs/quick-xml/latest/quick_xml/
  - **Acceptance**: Parse well-formed XML. XPath queries work. Streaming doesn't OOM on large files. 3 examples + 10 tests.
  - **Commit**: `feat(buff-xml): MVP XML parsing via quick-xml`

- [x] 51. `buff-msgpack` — MessagePack Binary Format

  **What to do**: Wrap Rust `rmp-serde` crate. Provide: `MsgPack.serialize(value) -> Bytes`, `MsgPack.deserialize(bytes) -> Value`. Auto-derive for structs (post-T3 macro). Cross-language: 6/6 (msgpack/python-msgpack/msgpack-lite/rmp-serde/MsgPack-Jackson/MessagePack-CSharp).
  - **LOC budget**: ≤1500, ≤10 fns. **External deps**: `rmp-serde`, `serde` (existing).
  - **References**: T0, T1, T4. Rust `rmp-serde`: https://docs.rs/rmp-serde/latest/rmp_serde/
  - **Acceptance**: Roundtrip serialize/deserialize preserves values. Cross-compatible with Python msgpack. 3 examples + 10 tests.
  - **Commit**: `feat(buff-msgpack): MVP MessagePack binary format`

- [x] 52. `buff-protobuf` — Protocol Buffers

  **What to do**: Wrap Rust `prost` crate (Rust protobuf compiler). Provide: buff CLI command `buff proto <proto-file> <output-dir>` generates Buff types from `.proto` definitions. Runtime: `Message.encode()`, `Message.decode(bytes)`. gRPC client/server scaffolding (combine with T17 buff-web). Cross-language: 6/6 (protobuf in all 6 languages).
  - **LOC budget**: ≤3000, ≤20 fns. **External deps**: `prost`, `prost-build`, `prost-types`, `tonic` (gRPC).
  - **Must NOT do**: No gRPC streaming in MVP (unary only); no protobuf reflection API.
  - **References**: T0, T1, T4, T17 (buff-web for gRPC transport). Rust `prost`: https://docs.rs/prost/latest/prost/
  - **Acceptance**: Generate Buff types from .proto. Encode/decode roundtrip. gRPC unary call works. 3 examples + 12 tests.
  - **Commit**: `feat(buff-protobuf): MVP Protocol Buffers + gRPC unary`

---

## Tasks T54-T56 — Language Inspirations (Release v1.19.0)

> Features inspired by analysis of Zig, Mojo, V/Go, and Swift. Language-level enhancements that make Buff faster (compile + runtime) and more ergonomic.

- [x] 53. `comptime` — Compile-Time Code Execution (Zig-inspired)

  **What to do**: Add Zig-style `comptime` blocks and function parameters. `comptime { ... }` runs at compile time, producing values or generating code. `fn foo(comptime T: Type, x: T)` runs specialized per type. Use cases: lookup table generation, config validation, type-driven codegen, generic specialization without monomorphization bloat.
  - **Why comptime INSTEAD of just macros**: Simpler mental model (it's just code that runs early), same language (no separate macro syntax), better error messages (no macro expansion confusion), composable (comptime fns call other comptime fns). Investigated alongside T3 macro spike (both kept in parallel per user decision).
  - **LOC budget**: ≤4000, ≤30 fns. **External deps**: none (compiler-internal).
  - **Must NOT do**: No I/O at comptime (no file reads, network); no reflection beyond type info; comptime errors must be Buff-source-mapped (use T24 source maps).
  - **References**: T0 (conventions), T1 (compiler integration), T24 (source maps for comptime errors), T3 (parallel macro spike — compare approaches). Zig reference: https://ziglang.org/documentation/master/#comptime
  - **Acceptance**: Compile-time fibonacci(10) precomputes to constant. Generic `comptime max(Type, T, T)` specializes per type. Lookup table builds at compile time. Comptime errors show Buff source spans. 5 examples + 20 tests.
  - **Commit**: `feat(ast): comptime compile-time execution engine (Zig-inspired)`

- [x] 54. `Simd<T, N>` — First-Class SIMD Types (Mojo-inspired)

  **What to do**: Add `Simd<T, N>` type to buff-tensor or new buff-simd crate. Represents N values of type T in a SIMD register (e.g., `Simd<Float, 4>` = 4 floats in 128-bit register). Operations: `Simd.splat(x)`, `simd.add(other)`, `simd.mul(other)`, `simd.sum()`, `simd.min()`/`simd.max()`. Auto-vectorization remains for non-explicit code; `Simd<T,N>` for hand-optimized hot loops. Wraps Rust's `std::simd` (stable since 1.51) or `packed_simd2`.
  - **Why**: 4-8x speedup for compute frameworks (buff-tensor T8, buff-science T13, buff-ml T15, buff-image T9, buff-dsp T11) in hot loops where auto-vectorizer misses.
  - **LOC budget**: ≤2500, ≤20 fns. **External deps**: `std::simd` (in Rust stdlib) or `packed_simd2`.
  - **Must NOT do**: No runtime SIMD detection (use compile-time target features); no GPU SIMD (that's WGSL's job).
  - **References**: T0, T1, T6 (WGSL assessment — SIMD is CPU-side complement), T8 (buff-tensor). Rust `std::simd`: https://doc.rust-lang.org/std/simd/index.html
  - **Acceptance**: `Simd<Float, 4>` operations produce correct results vs scalar equivalents. Benchmark shows ≥3x speedup on dot product vs scalar loop. 4 examples + 15 tests.
  - **Commit**: `feat(buff-simd): MVP Simd<T,N> first-class SIMD types (Mojo-inspired)`

- [x] 55. Compile-Speed Optimization Program (V/Go-inspired)

  **What to do**: Buff's #1 DX risk is inheriting Rust's slow compiles (30-90s for medium projects). This task owns a multi-pronged compile-speed program:
  1. **Incremental compilation audit** — verify Cargo incremental is on by default in `buff build`; measure improvement.
  2. **Generated-Rust caching** — skip re-codegen if `.buff` source unchanged (cache hash → generated `.rs` content). Save 30-50% on repeat builds.
  3. **`buff check` as fast preview** — type-check + lint in <2s; document as the inner-loop command.
  4. **Linker selection** — auto-detect and use `mold` (Linux) or `lld` (Windows/macOS) when available; 2-5x link speedup.
  5. **sccache integration** — wrap rustc invocation with sccache for cross-project crate caching.
  6. **`buff build --fast` mode** — skip LTO, skip optimization, use debug profile. For dev inner loop.
  7. **Benchmark suite** — `cargo run -p buff-lang-cli -- bench-compile` measures and records compile times across project sizes; publishes comparison vs Go/Rust baseline.
  - **LOC budget**: ≤3000 across CLI changes + benchmark harness. **External deps**: `sccache` (optional client), `mold` (system tool).
  - **Must NOT do**: No custom linker; no LLVM replacement; no JIT compilation (Buff is AOT-first).
  - **References**: T0 (profiles), T1 (build pipeline). Go build speed: https://docs.google.com/document/d/1Sk2TQ3Pr8aT78Po7... V build speed: https://github.com/vlang/v
  - **Acceptance**: Repeat `buff build` is ≥40% faster than baseline (cache hit). `buff check` completes in <2s on medium project. Benchmark report published. 5 examples + 10 tests.
  - **Commit**: `feat(cli): compile-speed optimization program (caching + mold + sccache + bench)`

- [x] 56. Property Wrappers — `@State`, `@Published`, `@Cached` (Swift-inspired)

  **What to do**: Add Swift-style property wrappers as attribute-driven codegen. `@State var count = 0` desugars to `let count = Signal.new(0)` + accessors. `@Published var score` desugars to observable signal. `@Cached(compute_fn)` desugars to memoized lazy value. Reduces boilerplate for reactive patterns. Enhances T20 buff-reactive.
  - **Why**: Swift's SwiftUI adoption exploded because property wrappers made reactive state ergonomic. Same opportunity for Buff with buff-reactive + v1.9 RSX UI.
  - **LOC budget**: ≤2000, ≤15 attribute kinds. **External deps**: depends on T53 comptime OR T3 macro spike (must ship one first).
  - **Must NOT do**: No custom user-defined wrappers in MVP (built-in set only); no Objective-C interop (n/a).
  - **References**: T0, T1, T20 (buff-reactive — direct enhancement), T53 (comptime — codegen engine) OR T3 (macros — alternative). Swift property wrappers: https://docs.swift.org/swift-book/LanguageGuide/Properties.html
  - **Acceptance**: `@State`/`@Published`/`@Cached` desugar correctly. Reactive UI updates fire. Existing `Signal.new()` API still works (additive). 4 examples + 12 tests.
  - **Commit**: `feat(ast): property wrappers @State/@Published/@Cached (Swift-inspired)`

---

## Tasks T57-T58 — Julia-Inspired Changes (Release v1.19.0)

> Opt-in language extensions inspired by Julia, targeted at numerical/scientific users. Both features are edition-gated (require `edition = "scientific"` in buff.toml).

- [x] 57. Mathematical Syntax Edition (Julia-inspired)

  **What to do**: Add opt-in mathematical syntax activated by `edition = "scientific"` in buff.toml. Features:
  1. **Implicit multiplication**: `2x` parses as `2 * x`; `2(x+y)` parses as `2 * (x+y)`. Lexer change to insert implicit `*` between number and identifier/lparen.
  2. **Unicode operators**: `∑` (sum), `∏` (product), `∫` (integral — for future), `√` (sqrt), `∈` (in), `∉` (not in), `⊂` (subset), `≤`/`≥` (already common), `≠` (not equal), `≈` (approx equal), `→` (function arrow alternative).
  3. **Matrix literals**: `[1 2 3]` (row vector), `[1; 2; 3]` (column vector), `[1 2; 3 4]` (2x2 matrix). Currently Buff uses `[[1, 2], [3, 4]]` (Vec of Vecs); matrix literals are more concise.
  4. **Adjoint operator**: `A'` for matrix transpose (postfix `'`).
  - **Why**: Julia/MATLAB/R users expect this syntax. buff-tensor (T8), buff-science (T13), buff-ml (T15) become more ergonomic.
  - **LOC budget**: ≤3500 (lexer + parser changes; type system unchanged). **External deps**: none.
  - **Must NOT do**: No breaking changes to default edition; no new types (just syntax sugar); Unicode operators must have ASCII alternatives (don't force users to type ∑).
  - **References**: T0 (edition concept), T1 (compiler integration). Julia manual: https://docs.julialang.org/en/v1/manual/mathematical-operations/
  - **Acceptance**: Scientific edition parses `2x` correctly. Unicode operators work in editor (LSP completion). Matrix literals produce correct tensors. Default edition unchanged. 6 examples + 20 tests.
  - **Commit**: `feat(ast): mathematical syntax edition (Julia-inspired, opt-in)`

- [x] 58. Multiple Dispatch for Numerical APIs (Julia-inspired)

  **What to do**: Extend Buff's trait system to support multiple dispatch — function dispatch on ALL arguments, not just receiver. `func matmul(a: Matrix, b: Vector)` and `func matmul(a: Vector, b: Matrix)` are different methods, dispatched by both argument types. Currently Buff uses single-dispatch (`a.matmul(b)` dispatches on `a` only).
  - **Why**: Numerical operations are often symmetric (`a + b` vs `b + a` have different impls for different type pairs). Multiple dispatch makes `+(Int, Float)`, `+(Matrix, Vector)`, `+(Vector, Matrix)` all natural top-level functions. Julia's entire stdlib is built on this.
  - **How**: Extend trait/impl system with multi-argument dispatch tables. Codegen lowers to Rust trait impls with explicit type matching. Backward compatible (single dispatch is a special case).
  - **LOC budget**: ≤4000 (type system + codegen changes). **External deps**: none.
  - **Must NOT do**: No breaking changes to single-dispatch trait system; no runtime dispatch (compile-time only); no pattern matching on values (only types).
  - **References**: T0, T1, T8 (buff-tensor — primary consumer), T13 (buff-science). Julia methods: https://docs.julialang.org/en/v1/manual/methods/
  - **Acceptance**: `func combine(a: T1, b: T2)` dispatches correctly for all (T1, T2) combinations. Existing single-dispatch methods unchanged. Numerical APIs read naturally as top-level functions. 5 examples + 18 tests.
  - **Commit**: `feat(ast): multiple dispatch for numerical APIs (Julia-inspired)`

---

## Task T59 — Actor Model (Release v1.19.0)

> Actor model and fault-tolerance patterns inspired by Erlang/Elixir/Gleam. Built on top of existing Channel<T> (T2) and spawn primitives.

- [x] 59. `buff-actors` — Actor Model + Supervisor Trees (Gleam/Erlang-inspired)

  **What to do**: Build actor abstraction on top of T2 Channel<T> + spawn. Provide:
  1. **Actor trait**: `trait Actor { required func handle(message: Message) -> Action; }`. Each actor runs in its own task, receives messages via Channel, processes via `handle`.
  2. **`ActorSystem.new()`**: manages actor lifecycle. `system.spawn(MyActor.new())` returns `ActorRef`.
  3. **`ActorRef.send(message)`:** type-safe message send (wraps Channel.send).
  4. **Supervisor trees**: `Supervisor.start_child(spec)` with restart strategies (`:permanent`, `:temporary`, `:transient`). "Let it crash" philosophy — actors crash, supervisors restart them. Mirrors Erlang OTP.
  5. **Named actors**: `system.register("logger", actor_ref)` + `system.lookup("logger")` for service discovery.
  6. **Graceful shutdown**: `system.shutdown()` sends termination messages, waits for actors to finish.
  - **Why**: Long-running services (buff-web T17, buff-jobs T35) benefit from fault-tolerant actor patterns. Erlang's legendary 99.9999999% uptime comes from this model.
  - **LOC budget**: ≤3500, ≤25 fns. **External deps**: T2 (Channel<T>), T20 (buff-reactive for state).
  - **Must NOT do**: No distributed actors (single-process only); no hot code swap (defer to v2.0+); no actor persistence.
  - **References**: T0, T1, T2 (Channel<T>), T20 (buff-reactive). Gleam: https://gleam.run/ Erlang OTP: https://www.erlang.org/doc/design_principles/des_princ.html
  - **Acceptance**: Actor system spawns actors. Messages delivered correctly. Supervisor restarts crashed actor. Named lookup works. Graceful shutdown completes. 5 examples + 18 tests.
  - **Commit**: `feat(buff-actors): MVP actor model + supervisor trees (Gleam/Erlang-inspired)`

---

## Tasks T60-T72 — Quality, DX & Community (Releases v1.19.0-v1.21.0)

> 13 tasks addressing performance, productivity, DX, and ecosystem topics the user explicitly selected in earlier discussion rounds but were not yet added as tasks. Per user directive: "don't defer anything we can actually do in this plan unless specified by me."

- [x] 60. Binary Size Minimization
  **What**: Add `[profile.minimal]` with `panic = "abort"`, `strip = true`, `opt-level = "z"`. Add `buff build --minimal` flag. Feature-gate tokio/rayon/wgpu (don't link if unused). Document size budget per template. Target: <5MB for console apps.
  **LOC budget**: ≤1500. **Deps**: T0 (profiles). **Acceptance**: Console template builds <5MB with --minimal. 3 examples + 10 tests.

- [x] 61. Cold-Start Benchmarks
  **What**: Build benchmark suite comparing Buff vs Go/Rust/Java/Python cold-start on AWS Lambda + Cloudflare Workers. Publish results to `benchmarks/` directory. Create `buff bench-cold-start` subcommand.
  **LOC budget**: ≤1000. **Deps**: T0, T1. **Acceptance**: Benchmark report published. Buff cold-start <50ms (matching Rust). 2 examples + 5 tests.

- [ ] 62. PGO (Profile-Guided Optimization) Support
  **What**: Add `[profile.pgo]` to buff.toml. Automate 3-step PGO flow: instrument → run representative workload → rebuild with profile. `buff build --pgo` flag. Wraps rustc PGO flags.
  **LOC budget**: ≤1500. **Deps**: T0. **Acceptance**: PGO build shows 10%+ speedup on benchmark suite vs regular release. 2 examples + 8 tests.

- [ ] 63. Error Message Quality Enhancement
  **What**: Add suggestion engine to buff-lang-error ("did you mean `print`?"). Build transpilation error mapping (when rustc rejects generated Rust, map back to Buff source spans). Create `errors.buff-lang.org/E12xx` error docs website. Add common-mistake linter patterns to buff check.
  **LOC budget**: ≤3000. **Deps**: T0, T1, T24 (source maps for error mapping). **Acceptance**: Typo suggestions appear. Rust errors mapped to Buff spans. 5 error doc pages live. 15 tests.

- [ ] 64. Hot Reload Beyond UI (Server + Game)
  **What**: Extend T17 buff-web with route hot-swap (change handler → reload without restart). Extend T16 buff-game with ECS system hot-swap. Build on existing v1.8 ui_dev WebSocket infrastructure. `buff watch` subcommand for non-UI code.
  **LOC budget**: ≤2500. **Deps**: T0, T1, T16, T17. **Acceptance**: Server route change live-reloads. Game system hot-swaps. `buff watch` detects file changes. 3 examples + 12 tests.

- [x] 65. AI Assistant Integration (`buff ai`)
  **What**: Add `buff ai` subcommand that generates "AI context pack" — types, signatures, available APIs, idioms, current project structure — as a single file users paste into Copilot/Claude. `buff ai --verify <file>` runs buff check on AI-generated code. Playground integration for immediate feedback.
  **LOC budget**: ≤2000. **Deps**: T0, T1. **Acceptance**: Context pack generated for test project. AI-verify catches syntax errors. 3 examples + 10 tests.

- [ ] 66. Refactoring Tools (`buff refactor`)
  **What**: Add `buff refactor` subcommand with non-interactive refactoring: `buff refactor rename <old> <new>`, `buff refactor extract-function <range> <name>`, `buff refactor inline-variable <name>`. Leverages existing buff-lang-ast for AST manipulation. LSP gains code actions for interactive versions.
  **LOC budget**: ≤3000. **Deps**: T0, T1, T24 (source maps for preserving spans). **Acceptance**: Rename propagates across files. Extract function produces valid code. Inline variable works. 5 examples + 18 tests.

- [x] 67. Documentation Site (`docs.buff-lang.org`)
  **What**: Build static documentation site with: language reference, API docs per crate (auto-generated by T0 `buff doc`), tutorial/getting started, cookbook recipes (from T68), migration guides. Search across all docs. Built with a static site generator (Zola or mdBook).
  **LOC budget**: ≤3000 (site + content seed). **Deps**: T0 (buff doc command). **Acceptance**: Site deployed. API docs render for 5+ crates. Search works. Tutorial complete. 10 pages minimum.

- [ ] 68. Cookbook / Patterns Guide
  **What**: Create recipe-style documentation at `docs.buff-lang.org/cookbook/` covering common patterns: HTTP request, file I/O, JSON parsing, database query, parallel map, async task, error handling, testing patterns, etc. Each recipe: problem → solution → explanation. Per-framework cookbooks (buff-ml, buff-web, buff-dataframe).
  **LOC budget**: ≤2000 (content). **Deps**: T67 (docs site). **Acceptance**: 50+ recipes published. Each recipe tested (code blocks compile). Cross-referenced with API docs. 5 tests.

- [ ] 69. Onboarding Paths by Background
  **What**: Create tailored guides: "Buff for Python developers" (show async without await, type hints comparison, DataFrame vs pandas), "Buff for Rust developers" (show borrow-checker-free code, extern FFI, trait system), "Buff for Go developers" (spawn vs goroutines, Channel vs channels, interfaces vs traits), "Buff for JavaScript developers" (async model, callback patterns, web frameworks).
  **LOC budget**: ≤2500 (content). **Deps**: T67 (docs site). **Acceptance**: 4 guides published. Each covers syntax, tooling, ecosystem mapping. Code examples compile. 4 tests.

- [ ] 70. Package Quality Signals
  **What**: Extend registry (v1.6 buff-registry) with quality badges beyond T0 stability: "verified publisher" (authenticated author), "maintained" (commits in 6 months), "tested" (coverage %), "documented" (doc comment coverage). Surface in `buff search` results and registry web UI.
  **LOC budget**: ≤2000. **Deps**: T0, T26 (buff-audit for security badge). **Acceptance**: Badges computed per package. Search results show badges. 4 badge types implemented. 10 tests.

- [x] 71. Stability Promise Document
  **What**: Write formal stability contract at `.sisyphus/decisions/stability-promise.md` and `docs.buff-lang.org/stability/`. Defines: what's guaranteed not to break (public APIs, language syntax), exceptions (security fixes), deprecation policy (T0 @deprecated → 1 minor version warning → removal), edition contract (opt-in breaking changes via edition field). Inspired by Rust 1.0 stability promise.
  **LOC budget**: ≤1000 (document). **Deps**: T0 (editions, stability badges). **Acceptance**: Document published. Covers all stability dimensions. Referenced from README. 3 tests (doc validation).

- [ ] 72. Plugin Architecture (Compiler + LSP + Runtime)
  **What**: Add plugin extension points: (1) compiler plugins (custom lints, custom codegen passes — via comptime T53), (2) LSP plugins (custom code actions, hover providers), (3) runtime plugins (custom tracing collectors, metric exporters). Define plugin manifest format (`buff-plugin.toml`). Plugin loading via dynamic dispatch (not dlopen — use trait objects).
  **LOC budget**: ≤3000. **Deps**: T0, T1, T53 (comptime for compiler plugins). **Acceptance**: One example plugin per type (lint, code action, tracing). Plugin manifest documented. 6 examples + 15 tests.

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.
>
> **Do NOT auto-proceed after verification. Wait for user's explicit approval before marking work complete.**
> **Never mark F1-F4 as checked before getting user's okay.** Rejection or user feedback -> fix -> re-run -> present again -> wait for okay.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, cargo run example, cargo test). For each "Must NOT Have": search codebase for forbidden patterns (`unwrap(`, `panic!`, raw-string codegen in codegen-rust) — reject with file:line if found. Check evidence files exist in `.sisyphus/evidence/`. Compare deliverables against plan (3 foundations + 3 decision docs + 10 frameworks + 1 flagship).
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo check --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --check` + `cargo test --workspace`. Review all new crate files for: `as any`/`@ts-ignore` (n/a, Rust — look for `unwrap`/`expect`/`panic!`), empty catches, `println!` in lib code (only allowed in CLI bins), commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction, generic names (data/result/item/temp).
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high` (+ `playwright` skill if UI)
  Start from clean state (`cargo clean`). Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Test cross-task integration: load CSV via buff-dataframe → train via buff-ml → visualize via buff-web (flagship flow). Test edge cases: empty state, invalid input, GPU-unavailable host. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git log/diff). Verify 1:1 — everything in spec was built (no missing), nothing beyond spec was built (no creep). Check LOC budgets per framework (Wave 2 ≤2500, Wave 3 ≤4000, Wave 4 ≤1500). Check "Must NOT do" compliance. Detect cross-task contamination: Task N touching Task M's files. Flag unaccounted changes. Verify no work leaked into v1.9-v1.12 scope (RSX/debugger/Bufflings/distribution).
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Per-task commits**: Each task produces 1+ atomic commits using Conventional Commits format.
- **Commit message style**: Per AGENTS.md conventions: `feat(<scope>): <desc>` ≤50 chars subject, body only when "why" isn't obvious.
- **Scope naming**: `feat(buff-ml): add tensor autodiff backward pass` (use crate name as scope).
- **Pre-commit hooks**: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p <crate>` must pass.
- **Per-release tags**: When all tasks in a coherent release group complete, tag the release. See Version Mapping section for the 12-release breakdown (v1.13.0 → v1.24.0):
  - v1.13.0 = Foundations (T0, T1, T2, T24, T3, T4, T6, T53)
  - v1.14.0 = Compute + Mocking (T7-T12, T25)
  - v1.15.0 = Wrappers + Security (T17-T21, T26, T27)
  - v1.16.0 = Tier 1 Common (T29-T36)
  - v1.17.0 = Tier 2 Common (T37-T44)
  - v1.18.0 = Tier 3 Specialized (T45-T52)
  - v1.19.0 = Language Evolution (T54-T60)
  - v1.20.0 = Developer Experience (T61-T67)
  - v1.21.0 = Community & Quality (T68-T72)
  - v1.22.0 = Domain Frameworks (T13-T16)
  - v1.23.0 = Integration + Flagship (T22, T23)
  - v1.24.0 = Audit & Polish (T28)
- **Branch strategy**: Long-lived `v1x-frameworks` branch off master. Each task = commit(s) to `v1x-frameworks`. Release tag on `v1x-frameworks`. Master merge per-release tag.
- **CHANGELOG**: Updated per-task commit; release summary entry added per-tag.

---

## Success Criteria

### Verification Commands

```bash
# Workspace builds clean
cargo check --workspace                                        # Expected: PASS, 0 errors
cargo clippy --workspace --all-targets -- -D warnings          # Expected: PASS, 0 warnings
cargo fmt --check                                              # Expected: PASS
cargo test --workspace                                         # Expected: ALL PASS

# Backward compat: v1.x examples still work
cargo run -p buff-lang-cli -- run examples/ola.buff            # Expected: "Olá, Mundo!"
cargo run -p buff-lang-cli -- run examples/fibonacci.buff      # Expected: 55
cargo run -p buff-lang-cli -- run examples/calculadora.buff    # Expected: functional calculator

# Multi-file linking works
cargo run -p buff-lang-cli -- new test_multifile --lib
cd test_multifile && cargo run -p buff-lang-cli -- run src/main.buff    # Expected: works end-to-end

# Flagship works
cargo run -p buff-lang-cli -- run examples/data-science-workbench/main.buff \
    -- data.csv --model linear-regression                       # Expected: trained model + visualization

# Each framework has runnable example
for fw in dataframe tensor science image audio dsp pipeline ecs ml game web db template reactive observe; do
    cargo run -p buff-lang-cli -- run examples/$fw/hello.buff   # Expected: works
done

# Buff SDK 2.0 conventions work
cargo run -p buff-lang-cli -- new test_sdk --template console   # Expected: scaffolds full project
cd test_sdk && cargo run -p buff-lang-cli -- run src/main.buff  # Expected: works out of the box
cargo run -p buff-lang-cli -- doc --scaffold                    # Expected: emits placeholder HTML
# (buff fix --v1-to-v1x NOT listed — T5 was removed because no new keywords are added in v1.13-v1.17)
```

### Final Checklist

- [ ] All "Must Have" present (1 SDK + 4 language foundations + 7 language inspirations + 3 decision docs + 10 domain + 5 wrappers + 3 security + 24 common/specialized + 1 actors + 1 flagship + 1 audit)
- [ ] All "Must NOT Have" absent (no panics, no raw strings, no scope creep, no breaking v1.x)
- [ ] All v1.x examples still compile (backward compat verified)
- [ ] All v1.x snapshot tests still pass (no regression)
- [ ] All v1.x buff.toml manifests still parse (v2 schema is additive)
- [ ] Flagship Data Science Workbench runs end-to-end
- [ ] All 7 SDK templates scaffold correctly (console/lib/web/ml/game/pipeline/workspace)
- [ ] Conventions spec at `.sisyphus/decisions/sdk-conventions-v1x.md` covers all 10 categories
- [ ] `buff audit` detects a known-vulnerable dependency
- [ ] `buff mock` allows trait mocking in tests
- [ ] Panics produce Buff-span stack traces (not raw Rust)
- [ ] `buff build --target <triple>` cross-compiles to at least one non-host target
- [ ] `buff fuzz` runs a fuzz target for 10 seconds without crash
- [ ] Code signing roundtrip works (publish --sign, add verifies)
- [ ] **v1.16 frameworks all ship MVP** (validate/config/cache/cli/http-client/auth/jobs/resilience)
- [ ] **v1.17 frameworks all ship MVP** (fake/assertions/archive/fsm/pubsub/email/scrape/i18n)
- [ ] **v1.18 frameworks all ship MVP** (geo/nlp/chat/web3/crypto-extras/xml/msgpack/protobuf)
- [ ] **v1.19 language inspirations all ship**: comptime + SIMD + compile-speed + property wrappers
- [ ] **v1.19 Julia-inspired features ship**: mathematical syntax edition + multiple dispatch
- [x] **v1.19 actor framework ships**: buff-actors with supervisor trees
- [ ] **T28 audit converged**: a full discovery scan finds zero new trivial issues
- [ ] **All new crates have AGENTS.md and README.md** (created by T28 if missing)
- [ ] **CHANGELOG.md exists** covering v1.0-v1.24 (created/updated by T28)
- [ ] **Root README.md status table covers v1.0-v1.24** (updated by T28)
- [ ] **Audit report + followup doc committed** at `.sisyphus/decisions/v1.24-{audit-report,followup}.md`
- [ ] F1-F4 final verification wave returns APPROVE verdicts
- [ ] User gives explicit "okay" after final verification
