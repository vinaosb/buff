# Buff Post-v1.0 Tooling Roadmap — Adoption & Expansion

> **Phase 4+ of the Buff roadmap.** Depends on [Phase 3 (v1.0)](./buff-v10-production.md) completion.
> Shared context: [Master Plan](./buff-master.md) | [Conventions](./buff-conventions.md) | [Project Structure](./buff-project-structure.md) | [Numeric System](./buff-numeric-system.md)

---

## TL;DR

> **Goal**: Deliver the bare minimum tooling for massive adoption by the Rust community (devs who find Rust too hard), then expand into markets where Rust is weak.
>
> **Strategy**: Leverage existing ecosystems aggressively. No reinvention. Each tool wraps or builds on proven foundations. Bare minimum per release.
>
> **Two phases**:
> - **Phase 1 (Adoption)**: v1.1–v1.3 — Playground + tree-sitter + LSP + VSCode + extern/bindgen. After v1.3, a Rust dev can discover, try, install, edit, use their crates, and ship.
> - **Phase 2 (Expansion)**: v1.4–v1.12 — Attack markets where Rust is weak: web/frontend, data science, scripting, education, ecosystem, production hardening.
>
> **Deliverables**:
> - Web playground (transpile-only, `.buff`→`.rs` side-by-side)
> - tree-sitter grammar (universal editor highlighting)
> - LSP server (diagnostics, hover, completion, goto-def)
> - VSCode extension (bundles tree-sitter + LSP)
> - Minimal extern/bindgen (call Rust crates from Buff)
> - Stdlib expansion: DateTime, Log, Regex, Toml (wrapping chrono/tracing/regex/toml)
> - REPL, Jupyter kernel, package registry, UI framework, debugger, coverage, Bufflings, buffup
>
> **Estimated Effort**: Phase 1 = Medium (3 lean releases). Phase 2 = Large (9 releases, expand into Rust-weak markets).
> **Parallel Execution**: YES — Phase 2 releases v1.5/v1.7/v1.8/v1.10 are independent and can overlap.
> **Critical Path**: v1.3 (bindgen) → v1.8 (UI foundations) → v1.9 (UI framework)

---

## Context

### Strategic Reframe (THE guiding principle)

**Target audience**: Rust devs who find Rust too hard (borrow checker, lifetimes, async coloring).
**Goal**: Bare minimum tooling for massive adoption from THAT audience.
**Constraint**: LEVERAGE EXISTING THINGS AGGRESSIVELY. No reinvention. Bare minimum per release.
**Follow-on**: Once Rust-curious devs are captured, expand to markets where Rust is weak.

### The Adoption Journey

```
Rust dev hears about Buff
    → visits playground
    → sees "painful Rust → clean Buff → same performance"  ← THE PITCH
    → tries in browser (no install)                         ← PLAYGROUND (transpile-only)
    → installs: cargo install buff-cli                      ← FAMILIAR
    → opens .buff → highlighting works                      ← TREE-SITTER
    → LSP gives hover/completion/diagnostics                ← LSP
    → calls serde/tokio/reqwest from Buff                   ← EXTERN/BINDGEN
    → buff build --release → binary                         ← CARGO (v1.0 T56)
    → tells colleagues                                      ← ADOPTION
```

### Research Findings (from planning session)

- **LSP**: `lsp-server` crate (rust-analyzer's). Compiler ~80% ready. MVP: diagnostics+hover+completion+goto-def+doc-symbols+formatting. References: TexLab, ruff-analyzer.
- **tree-sitter**: 6-12 weeks. External C scanner mandatory for offside-rule. Reference: tree-sitter-nickel (layout-sensitive, transpiled — Buff's twin).
- **Registry**: Build minimal (~2000 LOC, axum+diesel+semver), NOT fork crates.io. Git deps first.
- **UI**: Wrap Dioxus (MIT, mature, cross-platform). Tauri 2.0 for desktop/mobile. RSX needs Rust syntax → v1.9 builds RSX-for-Buff.
- **Playground**: Client-side transpile-only is PERFECT for Rust-curious audience. No server, no ops. The `.buff`→`.rs` output IS the demo.

### Metis Review (Incorporated)

**Critical landmine resolved**: v1.9 RSX-for-Buff is a language change (parser/lexer/AST extension), NOT pure tooling. Explicitly flagged as the one release that touches the compiler. Guardrail: all other releases (v1.1–v1.8, v1.10–v1.12) build ON the frozen v1.0 compiler.

**Key guardrails added** (see Must NOT Have section):
- No Buff language/AST/keyword changes in v1.1–v1.8, v1.10–v1.12
- Hand-rolled parser (crates/buff-lang-parser) is authoritative; tree-sitter is derived approximation
- Dioxus is vendored upstream dependency, never a fork
- Two registries (Buff + cargo), hard boundary
- Registry is the sole hosted service — explicit ops boundary
- Tooling lives in NEW crates, never in buff-lang-* compiler crates

---

## Work Objectives

### Core Objective

Deliver tooling that makes Buff the obvious choice for Rust devs who want Rust's performance without Rust's complexity, then expand into markets where Rust is weak.

### Concrete Deliverables

**Phase 1 (Adoption — v1.1–v1.3)**:
- Web playground showing `.buff` → `.rs` transpilation in browser
- tree-sitter grammar for universal editor highlighting
- LSP server with diagnostics, hover, completion, goto-def
- VSCode extension bundling tree-sitter + LSP
- Minimal extern/bindgen for calling Rust crates
- Side-by-side "Rust pain → Buff relief" example library

**Phase 2 (Expansion — v1.4–v1.12)**:
- Package registry (minimal, git deps + API)
- REPL for scripting/automation
- Jupyter kernel for data science/ML
- UI foundations (wrap Dioxus) + full UI framework (RSX-for-Buff)
- Debugger (DAP), coverage tooling
- Bufflings (education), buffup + CI + Docker (distribution)

### Definition of Done

- [ ] Phase 1: A Rust dev can discover, try (playground), install (cargo), edit (tree-sitter+LSP+VSCode), use Rust crates (extern), and ship (buff build) — all within 30 minutes of first hearing about Buff.
- [ ] Phase 2: Buff has tooling to attack at least 3 markets where Rust is weak (web, data science, scripting).

### Must Have

- Every release leverages existing ecosystems — no reinvention where leverage exists
- Every release ships with: semver bump, changelog, **README update** (status table, examples, roadmap links), git tag, backward-compat regression test, clippy-clean
- Tooling-appropriate TDD per tool-type (protocol-conformance for LSP/DAP, corpus for tree-sitter, Playwright for playground, publish/install roundtrip for registry)
- Each tool lives in its own crate (mirror v1.0's 9-crate discipline)
- Bare minimum scope per release — no gold-plating

### Must NOT Have (Guardrails — from Metis review)

- **NO Buff language/AST/keyword changes in v1.1–v1.8, v1.10–v1.12** (v1.9 RSX is the documented exception)
- **NO breaking changes to consumed compiler APIs after v1.2 ships** — once LSP/bindgen depend on tokenize/parse/TypeInferencer/SourceMap, those become semi-public
- **NO tooling code in buff-lang-* compiler crates** — new crates only (buff-lsp, buff-tree-sitter, buff-bindgen, buff-registry-*, buff-ui-*, buff-dap, etc.)
- **NO forking of Dioxus** — vendored upstream dependency, wrap only
- **NO forking of crates.io** — build minimal registry from scratch
- **NO auto-pulling between registries** — Buff packages and Rust crates are separate; bindgen is the only bridge
- **NO compiler-TDD-everywhere** — use protocol-conformance tests for LSP/DAP, corpus for tree-sitter, Playwright for playground
- **NO `buff audit` with Rust CVE database** — Buff-advisories only (defer RustSec integration to v2.0)
- **NO premature optimization** — LSP starts with full reparse (incremental is v2.0 non-goal for Phase 1-2; aligns with v1.0 T91 deferral)
- **NO bindgen for arbitrary generic/trait-heavy Rust crates** — extern-C + concrete types only in v1.3
- **NO server-side playground execution** — transpile-only (shows `.buff`→`.rs`)
- **NO accounts/auth in playground** — anonymous, ephemeral, share-by-URL
- **NO GPU kernel debugging/coverage** — documented out of scope
- **NO Dioxus integration via extern-C/FFI** — Dioxus is macro-driven (`rsx!{}` proc macro); integrates via codegen-rust emitting Rust source with macro calls, NOT via bindgen (G1)
- **NO introducing bareword `state`/`component`/`signal` statement keywords** in v1.9 — use attribute system (`@component`) + stdlib functions (`signal()`), preserving the 25-keyword freeze (G4)

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision

- **Infrastructure exists**: YES (cargo test, insta, proptest — from v1.0)
- **Automated tests**: YES (tests-after for tooling; TDD where unit-testable)
- **Framework**: Rust `#[test]` + insta + proptest (compiler crates); tooling-specific frameworks per type
- **Tooling-specific test strategies**:
  - **LSP/DAP**: Protocol-conformance tests (`lsp-test`, debug-adapter test harness)
  - **tree-sitter**: Corpus tests (`tree-sitter test`)
  - **Playground**: Playwright (DOM assertions, not visual)
  - **VSCode extension**: `@vscode/test-electron`
  - **Registry**: Integration tests (publish→install roundtrip)
  - **UI**: Playwright on headless browser

### QA Policy

Every task includes agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Frontend/UI**: Playwright — navigate, interact, assert DOM, screenshot
- **CLI/TUI**: Bash/tmux — run command, validate output, check exit code
- **API/Backend**: curl — send requests, assert status + response fields
- **Protocol (LSP/DAP)**: Send JSON-RPC, assert response shape
- **Editor integration**: `@vscode/test-electron` or `lsp-test`

### Backward Compatibility Regression (EVERY release)

Every release MUST pass: compile a frozen v1.0 fixture program (`tests/fixtures/v10-compat.buff`) → exit 0, output unchanged. This is the backward-compat promise.

> **Prerequisite (do FIRST, before v1.1)**: Create `tests/fixtures/v10-compat.buff` — a representative program exercising the full v1.0 feature surface (types, functions, structs, enums, pattern matching, modules, async, collections). Snapshot its transpiled `.rs` output and runtime output. This fixture + snapshot IS the backward-compat contract that every subsequent release regresses against. Owner: whoever ships v1.0 → v1.1 transition. Without this fixture, the "backward-compat regression" acceptance criterion on every task is unverifiable.

---

## Execution Strategy

### Release Sequence

```
Phase 1: ADOPTION (capture Rust-curious — BARE MINIMUM)

  v1.1 "Try Buff" [Start immediately after v1.0]
  ├── Playground (Wasm transpile-only)           [visual-engineering]
  ├── tree-sitter grammar                        [deep]
  └── Website + side-by-side examples            [writing]

  v1.2 "Use Buff" (depends: v1.1 tree-sitter)
  ├── LSP server                                 [deep]
  └── VSCode extension                           [visual-engineering]

  v1.3 "Rust interop" (depends: v1.0)
  ├── extern/bindgen minimal                     [deep]
  ├── Cargo polish                               [quick]
  ├── Example library (Rust→Buff side-by-side)   [writing]
  └── Dioxus feasibility spike (UI go/no-go)     [deep]  ← de-risks v1.8/v1.9 early


Phase 2: EXPANSION (markets where Rust is weak — AFTER adoption)

  v1.4 "Stdlib + Ecosystem foundations" (v1.0 defers File/HTTP/JSON to v1.4 — see T124+ below)
  ├── DateTime (chrono)                          [deep]
  ├── Log (tracing)                              [unspecified-high]
  ├── Regex (regex)                              [unspecified-high]
  ├── Toml (toml)                                [quick]
  ├── Math, Random, Sort, Strings                 [unspecified-high]
  ├── Args, Env, input(), sleep()                [unspecified-high]
  ├── URL, Base64, Hex, URLEncode, UUID          [unspecified-high]
  ├── YAML, CSV                                  [quick]
  ├── Path, Dir, Tempfile                        [unspecified-high]
  ├── Hash (SHA/MD5), HMAC                       [quick]
  ├── Process, OS info                           [quick]
  ├── TCP, UDP, WebSocket                        [deep]
  ├── Git deps support                           [deep]
  ├── Workspace support                          [unspecified-high]
  └── Error code catalog                         [quick]

  v1.5 "Scripting" (PARALLEL with v1.4)
  ├── buff-eval shared core (T125-prep)           [deep]    ← consumed by v1.7, v1.11
  ├── REPL core (read-eval-print, T125a)          [unspecified-high]
  ├── Session state + :type (T125b)              [unspecified-high]
  └── Commands + file load + multiline (T125c)   [unspecified-high]

  v1.6 "Package registry" (depends: v1.4 git deps)
  ├── Minimal registry server                    [deep]
  ├── buff publish/install/add                   [unspecified-high]
  └── buff deps/outdated (audit deferred)        [quick]

  v1.7 "Data science" (NOT parallel with v1.5 — T129b needs T125-prep/a/b)
  ├── Jupyter kernel protocol + install          [deep]
  ├── Execution engine + cross-cell state        [deep]
  └── Rich display + introspection               [deep]

  v1.8 "Web/frontend foundations" (depends: v1.3 bindgen + T121b PoC, T114 Wasm)
  ├── Wrap Dioxus core (builds on v1.3 PoC)      [deep]
  ├── buff ui dev (hot reload server)            [unspecified-high]
  └── Tauri scaffolding                          [quick]

  v1.9 "Full UI framework" (depends: v1.8) ⚠️ LANGUAGE CHANGE
  ├── RSX-for-Buff parser/lexer extension        [ultrabrain]
  ├── Component model + data binding             [deep]
  └── SSR + mobile                               [unspecified-high]

  v1.10 "Production hardening" (PARALLEL)
  ├── Debugger (DAP)                             [deep]
  └── Coverage tooling                           [unspecified-high]

  v1.11 "Education"
  ├── Bufflings CLI + runner                     [writing]
  ├── 25 exercises / 12 topics                   [writing]
  └── Verification engine + CI gate              [writing]

  v1.12 "Distribution scale"
  ├── buffup version manager                     [unspecified-high]
  ├── setup-buff GitHub Action                   [quick]
  └── Docker images                              [quick]

  FINAL (after ALL releases)
  ├── F1: Plan compliance audit                  [oracle]
  ├── F2: Code quality review                    [unspecified-high]
  ├── F3: Real manual QA                         [unspecified-high]
  └── F4: Scope fidelity check                   [deep]
```

### Dependency Matrix

| Release | Depends On | Blocks | Parallelizable With |
|---|---|---|---|
| v1.1 | v1.0 (T114 playground prerequisite: Wasm target) | v1.2 (tree-sitter) | — |
| v1.2 | v1.1 (tree-sitter), v1.0 (compiler APIs) | v1.6 (VSCode pattern) | v1.3 |
| v1.3 | v1.0 (extern keyword, Cargo) | v1.8 (bindgen + T121b Dioxus PoC gate) | v1.2 |
| v1.4 | v1.0 (buff.toml) | v1.6 (registry) | v1.5, v1.7, v1.10 |
| v1.5 | v1.0 | — | v1.4, v1.7, v1.10 |
| v1.6 | v1.4 (git deps) | — | v1.5, v1.7, v1.8, v1.10 |
| v1.7 | v1.0; **T129b depends on T125a/b (shared eval core)** | — | v1.4, v1.6, v1.10 (NOT v1.5 — T129b blocked on T125a/b) |
| v1.8 | v1.3 (bindgen + T121b PoC verdict PASS), T114 (playground prerequisite: Wasm target) | v1.9 | v1.6, v1.7, v1.10 |
| v1.9 | v1.8 (Dioxus wrap) ⚠️ | — | — (language change, sequential) |
| v1.10 | T60 (source maps) | — | v1.4-v1.8, v1.11 |
| v1.11 | v1.0 (compiler pipeline for T138c verification), v1.5 (T125c REPL file eval, reused by T138a) | — | v1.10 |
| v1.12 | v1.0 | — | anything |

**Critical path**: v1.3 (bindgen) → v1.8 (UI foundations) → v1.9 (UI framework)
**Adoption path**: v1.1 (try) → v1.2 (use) → v1.3 (interop)

---

## TODOs

> Tasks numbered T114+ (continuing from v1.0's T113). T113b is the v1.0→v1.1 bridge.
> Every task: What to do + References + Acceptance Criteria + QA Scenarios + Commit.
> **Leverage mandate**: Every task MUST specify what existing work it leverages.

### Prerequisite (v1.0 → v1.1 bridge)

- [x] **T113b: Backward-compat fixture + snapshot (MUST complete before any v1.1+ task)** [quick]

  **What to do**:
  - Create `tests/fixtures/v10-compat.buff` — a representative program exercising the full v1.0 feature surface (types, functions, structs, enums, pattern matching, modules, async, collections, error handling)
  - Snapshot its transpiled `.rs` output to `tests/fixtures/v10-compat.snapshot.rs`
  - Snapshot its runtime output
  - Add a CI job: `buff build tests/fixtures/v10-compat.buff` → diff against snapshot → exit 0 if identical
  - This fixture + snapshot IS the backward-compat contract every subsequent release regresses against

  **Leverages**: v1.0 compiler (the thing being frozen). Existing test patterns from v0.1.

  **Must NOT do**: Change this fixture after creation (it's frozen — that's the point). Update only if a deliberate breaking change is intended (major version bump).

  **References**: `tests/` (existing test directory), `crates/buff-lang-cli/src/pipeline.rs:46 compile_to_rust()`

  **Acceptance Criteria**:
  - [x] `tests/fixtures/v10-compat.buff` exists and exercises all v1.0 features
  - [x] `tests/fixtures/v10-compat.snapshot.rs` exists (transpiled output snapshot)
  - [x] CI job runs: build fixture → diff snapshot → pass if identical
  - [x] Runtime output snapshot exists

  **QA Scenarios**:
  ```
  Scenario: Fixture builds and matches snapshot
    Tool: Bash
    Steps:
      1. buff build tests/fixtures/v10-compat.buff
      2. diff output tests/fixtures/v10-compat.snapshot.rs
      3. Assert exit 0 (identical)
    Expected Result: Backward-compat contract established
    Evidence: .sisyphus/evidence/task-113b-fixture.txt
  ```

  **Commit**: `test(fixtures): v1.0 backward-compat fixture and snapshot for regression testing`

  **Status**: ✅ Done at commit `a70a250` (2026-07-19). Fixture: `tests/fixtures/v10-compat.buff` (137 lines, expanded post-cleanup to cover all arithmetic/comparison/logical operators + else if/else + integer-literal match). Snapshot: `tests/fixtures/v10-compat.snapshot.rs` (145 lines). Test: `crates/buff-lang-cli/tests/v10_compat.rs` (byte-identical assertion + `#[ignored]` regen helper). Excludes async/modules/user-enum (codegen-only — documented in fixture header).

- [x] **T57b: Integrate LosslessTree into `buff fmt` (comment preservation)** [deep]

  > **WHY THIS EXISTS**: T57 (v1.0) shipped the `LosslessTree` data structure at `crates/buff-lang-ast/src/lossless.rs` (743 lines, 39 passing tests, byte-exact roundtrip proven). However, `crates/buff-lang-cli/src/fmt.rs:format_source()` still strips comments because `tokenize()` drops them at `lexer.rs:130-167`. An attempt to integrate during v1.0 cleanup was reverted (architectural issue: comment draining happened only at top-level indent, not recursively at every block level — 14/15 Phase 1 tests failed). This task finishes the integration.

  **What to do**:
  - Phase 1 (the easy 80%): comment positions on their own line
    - File-header comments
    - Comment above top-level decl (func/struct/enum/trait/import)
    - Comment above struct/enum field
    - Comment above stmt in block body
    - Comment above match-arm body stmt
    - Orphan comments between top-level decls (blank-line-separated)
    - Multi-line block comments (re-indented to canonical form)
    - File-end / last-stmt-in-body comments
    - Trailing comments on `let`/`return`/simple expr stmts
  - Phase 2 (the harder 15%): trailing on type headers, single-line match arm trailers, comments between attrs — DEFER to v2.0
  - Phase 3 (explicit unsupported, ≤5%): drop with `tracing::warn!`

  **Design**: Follow the Oracle design spec from v1.0 cleanup (Walk-With-Trivia approach). Key insight missed in v1.0 attempt: `drain_comments_in` must be called RECURSIVELY at every block level (write_func body, write_struct body, write_match arms), not just from `write_decls`. The previous attempt called drain only from `write_decls` which is why indent was wrong.

  **Leverages**:
  - `crates/buff-lang-ast/src/lossless.rs` (T57 v1.0 — the data structure)
  - Oracle design spec for Walk-With-Trivia pattern
  - rustfmt's missed-spans approach (reference)
  - Prettier's attachComments pattern (reference)

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - **Skills**: []

  **Parallelization**: Blocks T117 LSP (which assumes `buff fmt` preserves comments for formatting-on-save). Otherwise independent.

  **References**:
  - `crates/buff-lang-cli/src/fmt.rs:84` (`format_source` entry — where LosslessTree plugs in)
  - `crates/buff-lang-ast/src/lossless.rs:295` (`LosslessTree` API: `comments()`, `piece_at()`, `Piece`, `TriviaKind`)
  - `crates/buff-lang-lexer/src/lexer.rs:130-167` (where comments are currently stripped)

  **Inline design summary** (Walk-With-Trivia, derived from Oracle spec):
  - Add `Option<&LosslessTree>` field to `Formatter` struct; thread it through `format_source()` → `format_decls_with_comments()`.
  - Pre-extract `Vec<&Piece>` of comment pieces (sorted by start byte) at construction; iterate via `next_comment_idx`.
  - For each AST node `N` with span `[s_N, e_N]`, drain comments in window `(last_emitted_byte, s_N)`:
    - No newline between `last_emitted_byte` and comment.start → TRAILING (emit inline with space prefix)
    - Newline but not blank-line-separated → LEADING (emit at current indent before node)
    - Blank-line-separated both sides → ORPHAN (emit at current indent with forced blank line after)
  - **Critical insight missed in v1.0 attempt**: `drain_comments_in` MUST be called recursively at every block level (`write_func` body, `write_struct` body, `write_match` arms), NOT just from top-level `write_decls`. The v1.0 attempt failed because drain happened only at indent_level=0.
  - Idempotency: emit comments at canonical indent (`indent_level × 4 spaces`); re-indent multi-line block comments by stripping original indent and re-applying canonical.

  **Acceptance Criteria**:
  - [ ] All 39 existing lossless tests stay green
  - [ ] All 30 existing fmt snapshot tests stay green (comment-free input → byte-identical output)
  - [ ] 10+ new fmt_comment_* tests pass (file header, above func, above struct field, trailing on let, leading in match arm, trailing in match arm, multi-line block, after last stmt, multiple consecutive, orphan between funcs)
  - [ ] Idempotency: `format_source(format_source(src)) == format_source(src)` for all v1.0 example files
  - [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
  - [ ] No new external dependencies

  **QA Scenarios**:
  ```
  Scenario: Comment above func preserved
    Tool: Bash
    Steps:
      1. echo "// hello\nfunc main():\n    print(\"hi\")\n" > /tmp/test.buff
      2. buff fmt /tmp/test.buff
      3. cat /tmp/test.buff
      4. Assert output contains "// hello" line above "func main():"
  ```

  **Commit**: `feat(fmt): preserve comments via LosslessTree integration (T57b)`

### v1.1 "Try Buff" — Playground + tree-sitter + Website

- [x] **T114: Web playground (Wasm transpile-only)** [visual-engineering]

  **What to do**:
  - Build a Wasm target for the Buff compiler (lexer + parser + codegen-rust only — no runtime/GPU)
  - Create a web UI: left pane = `.buff` editor (Monaco/CodeMirror), right pane = generated `.rs` (read-only)
  - On keystroke (debounced 300ms): compile `.buff` → show `.rs` in right pane
  - Share-by-URL: encode `.buff` source in URL fragment (base64), reload restores state
  - Show errors inline (red underline in editor, message below)
  - Deploy as static site (GitHub Pages / Netlify / Vercel — no server needed)

  **Leverages**:
  - T114 prerequisite (Wasm target co-delivered with playground, post-v1.0) — compiler already compiles to wasm32
  - Existing `compile_to_rust()` pipeline (buff-lang-cli/src/pipeline.rs) — call from Wasm
  - Monaco Editor or CodeMirror — proven web code editors
  - Rust playground architecture (open source) — reference for URL-sharing pattern

  **Must NOT do**:
  - Server-side compilation/execution (transpile-only, no rustc in browser)
  - User accounts, auth, persistence beyond URL
  - Multiple files (single-file transpile only)

  **Recommended Agent Profile**:
  - **Category**: `visual-engineering` — web UI + Wasm integration
  - **Skills**: [`frontend-ui-ux`] — web UI polish

  **Parallelization**: Can start immediately after v1.0. Blocks nothing critical but is adoption-critical.

  **References**:
  - `crates/buff-lang-cli/src/pipeline.rs:compile_to_rust()` — the pipeline to expose via Wasm
  - `crates/buff-lang-codegen-rust/src/lib.rs:generate_rust()` — the codegen entry point
  - `crates/buff-lang-error/src/lib.rs:Diagnostic` — error format to display
  - Rust playground: `https://github.com/rust-lang/rust-playpen` — URL-sharing reference

  **Acceptance Criteria**:
  - [ ] Playground deployed at a public URL
  - [ ] Type `.buff` → `.rs` appears in right pane within 300ms
  - [ ] Errors show with red underline + message
  - [ ] URL sharing works (encode → reload → restore)
  - [ ] Loads in <3s on a fresh cache
  - [ ] Works in Chrome, Firefox, Safari, Edge

  **QA Scenarios**:
  ```
  Scenario: Transpile fibonacci.buff
    Tool: Playwright
    Preconditions: Playground deployed and accessible
    Steps:
      1. Navigate to playground URL
      2. Clear editor, paste contents of examples/fibonacci.buff
      3. Wait 500ms (debounce)
      4. Assert right pane contains "fn " (Rust function generated)
      5. Assert right pane contains "fibonacci" (function name preserved)
    Expected Result: Right pane shows valid Rust code
    Evidence: .sisyphus/evidence/task-114-transpile-fib.png

  Scenario: Error display
    Tool: Playwright
    Steps:
      1. Type invalid Buff: "func ( broken"
      2. Wait 500ms
      3. Assert error message visible below editor
      4. Assert editor shows red underline/squiggle
    Expected Result: Parse error shown with message
    Evidence: .sisyphus/evidence/task-114-error-display.png

  Scenario: URL sharing
    Tool: Playwright
    Steps:
      1. Type "func main(): print(\"hello\")" in editor
      2. Copy current URL
      3. Open new browser tab, paste URL
      4. Assert editor contains the same code
    Expected Result: Code restored from URL
    Evidence: .sisyphus/evidence/task-114-url-share.txt
  ```

  **Commit**: `feat(playground): Wasm transpile-only web playground with URL sharing`

- [x] **T115: tree-sitter grammar for Buff** [deep]

  **What to do**:
  - Write `grammar.js` defining Buff syntax (25 keywords, offside rule, braces-for-data)
  - Write `src/scanner.c` external scanner for indentation (offside rule — stack-based indent tracking, bracket balancing, error recovery, state serialization)
  - Write `corpus/` test cases (one per language feature: functions, if/else, for, match, struct, etc.)
  - Write `queries/` (highlights.scm, folds.scm, locals.scm) for editor integration
  - Publish to npm as `tree-sitter-buff` and to the tree-sitter org if accepted

  **Leverages**:
  - tree-sitter-nickel (BEST reference — layout-sensitive, transpiled-to-Nix, Buff's twin)
  - tree-sitter-python scanner.c — indentation tracking pattern
  - tree-sitter-haskell scanner — advanced layout handling
  - Buff's hand-rolled parser grammar (crates/buff-lang-parser) — the authoritative syntax spec to port

  **Must NOT do**:
  - Don't make tree-sitter authoritative — hand-rolled parser (in compiler) is source of truth; tree-sitter is derived approximation
  - Don't try to auto-generate from hand-rolled parser (no bridge exists — manual port)

  **Recommended Agent Profile**:
  - **Category**: `deep` — grammar + external C scanner is complex
  - **Skills**: []

  **Parallelization**: Independent. Can run parallel with T114 (playground).

  **References**:
  - `crates/buff-lang-parser/src/` — hand-rolled parser grammar to port (THE source of truth)
  - `crates/buff-lang-lexer/src/lib.rs:TokenKind` — all tokens to recognize
  - `https://github.com/nickel-lang/tree-sitter-nickel` — best reference grammar
  - `https://github.com/tree-sitter/tree-sitter-python/blob/master/src/scanner.c` — indent scanner pattern

  **Acceptance Criteria**:
  - [ ] `tree-sitter test` passes all corpus cases
  - [ ] Grammar highlights correctly in Neovim (screenshot evidence)
  - [ ] Grammar highlights correctly on GitHub code view (or documented how to add)
  - [ ] Published to npm as `tree-sitter-buff`
  - [ ] Offside rule handled (indentation creates/removes blocks)

  **QA Scenarios**:
  ```
  Scenario: Corpus tests pass
    Tool: Bash
    Steps:
      1. cd tree-sitter-buff/
      2. tree-sitter test
    Expected Result: All corpus tests pass (0 failures)
    Evidence: .sisyphus/evidence/task-115-corpus-test.txt

  Scenario: Highlights in Neovim
    Tool: Bash (with Neovim installed)
    Steps:
      1. Install tree-sitter-buff grammar in Neovim
      2. Open examples/fibonacci.buff
      3. Assert keywords (func, if, else, return) are highlighted
      4. Assert strings and numbers are highlighted
    Expected Result: Syntax highlighting renders correctly
    Evidence: .sisyphus/evidence/task-115-neovim-highlight.png

  Scenario: Offside rule — nested indentation
    Tool: Bash
    Steps:
      1. Parse a file with nested if-blocks (3 levels deep)
      2. Assert tree-sitter produces correct nesting in parse tree
    Expected Result: Indentation-based blocks correctly nested
    Evidence: .sisyphus/evidence/task-115-offside-nesting.txt
  ```

  **Commit**: `feat(tree-sitter): Buff grammar with offside-rule external scanner`

- [x] **T116: Website + side-by-side example library** [writing]

  **What to do**:
  - Build a landing page for Buff with the core pitch: "Rust performance with Go productivity"
  - Create 5-10 side-by-side examples: painful Rust → clean Buff → explanation
    - Examples: borrow checker pain, lifetime annotations, async/await coloring, error handling, pattern matching, structs/traits
  - Each example: 3 columns (Rust code | Buff code | "Why this is easier")
  - Link to playground for each example ("try this")
  - Quick start: `cargo install buff-cli` + 3-line "run your first program"
  - Deploy as static site alongside playground

  **Leverages**:
  - T114 playground — embed/link for "try this" buttons
  - Rust website (rust-lang.org) — layout/structure reference
  - Go tour (tour.golang.org) — side-by-side example pattern
  - Existing examples/ directory — starting material

  **Must NOT do**:
  - Full language reference (that's T62 v1.0 territory, link to it)
  - Interactive tutorial (that's Bufflings, v1.11)
  - Blog/CMS — static site only

  **Recommended Agent Profile**:
  - **Category**: `writing` — content-heavy, marketing copy + code examples
  - **Skills**: [`frontend-ui-ux`] — landing page polish

  **Parallelization**: Depends on T114 (playground) for "try this" links. Can draft content in parallel.

  **References**:
  - `examples/*.buff` — existing example programs
  - `README.md` — existing pitch and language description
  - `.sisyphus/plans/buff-master.md` — language keywords, architecture

  **Acceptance Criteria**:
  - [ ] Website deployed at buff-lang.org (or similar)
  - [ ] ≥5 side-by-side Rust-vs-Buff examples
  - [ ] Each example has "Try this" → links to playground with code pre-loaded
  - [ ] Quick start shows `cargo install buff-cli` + first program in <5 lines
  - [ ] Mobile-responsive

  **QA Scenarios**:
  ```
  Scenario: Landing page loads
    Tool: Playwright
    Steps:
      1. Navigate to website URL
      2. Assert page title contains "Buff"
      3. Assert pitch text visible ("Rust performance" or similar)
      4. Assert ≥5 example sections exist
    Expected Result: Landing page renders with pitch and examples
    Evidence: .sisyphus/evidence/task-116-landing.png

  Scenario: Side-by-side example renders
    Tool: Playwright
    Steps:
      1. Navigate to website
      2. Find first side-by-side example
      3. Assert 3 columns exist (Rust | Buff | explanation)
      4. Assert Rust column contains Rust syntax (fn, let, ::)
      5. Assert Buff column contains Buff syntax (func, indentation)
      6. Click "Try this" button
      7. Assert navigation to playground with code loaded
    Expected Result: Example shows comparison, "try" loads playground
    Evidence: .sisyphus/evidence/task-116-side-by-side.png
  ```

  **Commit**: `docs(website): landing page with side-by-side Rust-vs-Buff examples`

---

### v1.2 "Use Buff" — LSP Server + VSCode Extension

- [x] **T117: LSP server (buff-lsp crate)** [deep]

  **What to do**:
  - Create `crates/buff-lsp/` crate
  - Use `lsp-server` crate (rust-analyzer's) for JSON-RPC scaffolding
  - Implement document tracking (open, change, close) with full-reparse on change (no incremental — that's v2.0)
  - Implement MVP capabilities (in priority order):
    1. **Diagnostics**: tokenize → parse → typecheck → publish errors/warnings
    2. **Hover**: show type info + doc comments for symbol under cursor
    3. **Completion**: local scope + imports (no fuzzy, no auto-import)
    4. **Goto definition**: resolve symbol → find span → return location
    5. **Document symbols**: outline view (functions, structs, enums)
    6. **Formatting**: call existing `buff fmt` (T54, v1.0)
  - Add typecheck-only mode (don't generate Rust, just run type inference)
  - Debounce diagnostics (300ms idle before publishing)

  **Leverages**:
  - `lsp-server` + `lsp-types` crates — JSON-RPC + LSP types
  - Compiler APIs (80% ready): `tokenize()`, `parse()`, `TypeInferencer::infer_expr/infer_stmt`, `SourceMap::lookup()`
  - T57 lossless AST (v1.0) — preserves whitespace for accurate positions
  - T54 `buff fmt` (v1.0) — reuse for formatting capability
  - TexLab (reference LSP server, pure Rust) — architecture pattern
  - ruff-analyzer — offside-rule handling patterns

  **Must NOT do**:
  - Incremental parsing (full reparse only — v2.0 goal)
  - Rename, find references, workspace symbols, semantic tokens, inlay hints, code actions (all v2.0)
  - Cross-file goto-def (single-file only for v1.2; multi-file needs v0.5 module system matured)

  **Recommended Agent Profile**:
  - **Category**: `deep` — complex protocol + compiler integration
  - **Skills**: []

  **Parallelization**: Depends on T115 (tree-sitter) for VSCode highlighting. LSP itself can start immediately.

  **References** (module paths verified against codebase):
  - `crates/buff-lang-lexer/src/lexer.rs:39 tokenize()` (re-exported via lib.rs) — lex entry point
  - `crates/buff-lang-parser/src/parser.rs:31 parse()` (re-exported via lib.rs) — parse entry point
  - `crates/buff-lang-types/src/infer.rs:20 TypeInferencer` (re-exported via lib.rs) — type inference
  - `crates/buff-lang-error/src/source_map.rs:87 SourceMap` (has `lookup()`) — byte→line/col conversion
  - `crates/buff-lang-error/src/` `Diagnostic` — error format (check diagnostic.rs)
  - `https://github.com/rust-lang/rust-analyzer/tree/master/crates/lsp-server` — framework
  - `https://github.com/latex-lsp/texlab` — reference implementation (similar scale)

  **Acceptance Criteria**:
  - [ ] `cargo test -p buff-lsp` passes (protocol conformance + unit tests)
  - [ ] Diagnostics publish on file open + change
  - [ ] Hover shows type info for identifiers
  - [ ] Completion offers local scope symbols
  - [ ] Goto-def navigates within single file
  - [ ] Document symbols outline works
  - [ ] Formatting via buff fmt integration
  - [ ] Hover latency <100ms on 1k-line file
  - [ ] Passes `lsp-test` conformance suite for implemented capabilities

  **QA Scenarios**:
  ```
  Scenario: Diagnostics on type error
    Tool: Bash (lsp-test harness)
    Steps:
      1. Start buff-lsp server
      2. Send textDocument/didOpen with: "let x: Int = \"hello\""
      3. Wait for server to push textDocument/publishDiagnostics notification
      4. Assert notification contains a diagnostic with severity Error
      5. Assert diagnostic span covers the type mismatch
    Expected Result: Type error pushed as LSP diagnostic notification
    Evidence: .sisyphus/evidence/task-117-diagnostics.txt

  Scenario: Hover shows type
    Tool: Bash (lsp-test harness)
    Steps:
      1. Open file with: "let x = 42"
      2. Send textDocument/hover at position of "x"
      3. Assert response contains "Int" (inferred type)
    Expected Result: Hover returns type information
    Evidence: .sisyphus/evidence/task-117-hover.txt

  Scenario: Completion offers locals
    Tool: Bash (lsp-test harness)
    Steps:
      1. Open file with: "func add(a, b): return a + \n"
      2. Send textDocument/completion at end of line
      3. Assert response includes "a", "b" (local scope)
    Expected Result: Completion returns local variables
    Evidence: .sisyphus/evidence/task-117-completion.txt

  Scenario: Performance — hover latency
    Tool: Bash
    Steps:
      1. Generate a 1000-line .buff file
      2. Open in LSP
      3. Time 10 hover requests
      4. Assert average <100ms
    Expected Result: Hover responds within 100ms
    Evidence: .sisyphus/evidence/task-117-perf.txt
  ```

  **Commit**: `feat(lsp): buff-lsp server with diagnostics, hover, completion, goto-def`

- [x] **T118: VSCode extension** [visual-engineering]

  **What to do**:
  - Create `editors/vscode/` directory with extension scaffold
  - Bundle tree-sitter-buff (T115) for syntax highlighting
  - Bundle buff-lsp (T117) as the language server
  - Register file association: `.buff` → Buff language
  - Add commands: `buff.run`, `buff.build`, `buff.check` (call CLI)
  - Add snippets: function, struct, if/else, for, match templates
  - Add configuration: buff binary path, formatting on save
  - Package as `.vsix`, publish to VSCode Marketplace

  **Leverages**:
  - T115 tree-sitter grammar — highlighting
  - T117 buff-lsp — intelligence
  - VSCode extension generator (`yo code` or manual)
  - rust-analyzer VSCode extension — reference for LSP client setup

  **Must NOT do**:
  - Custom editor themes (use VSCode defaults)
  - Debug integration (that's v1.10 DAP)
  - Visual designer / GUI builders

  **Recommended Agent Profile**:
  - **Category**: `visual-engineering` — VSCode extension UI + integration
  - **Skills**: [`frontend-ui-ux`]

  **Parallelization**: Depends on T115 (tree-sitter) + T117 (LSP).

  **References**:
  - `editors/vscode/` — new directory for extension
  - T115 tree-sitter grammar output — to bundle
  - T117 buff-lsp binary — to bundle/configure
  - `https://github.com/rust-lang/rust-analyzer/tree/master/editors/code` — reference extension

  **Acceptance Criteria**:
  - [ ] Extension installs from `.vsix` in CI
  - [ ] Opening `.buff` file activates highlighting
  - [ ] LSP starts automatically (diagnostics appear)
  - [ ] Hover, completion, goto-def work in editor
  - [ ] `buff.run` command executes current file
  - [ ] Formatting on save works (if configured)
  - [ ] Published to VSCode Marketplace (or documented install path)

  **QA Scenarios**:
  ```
  Scenario: Extension activates on .buff file
    Tool: @vscode/test-electron
    Steps:
      1. Install buff-vscode.vsix in test VSCode
      2. Open examples/fibonacci.buff
      3. Assert syntax highlighting is active (tree-sitter)
      4. Assert diagnostics appear (LSP connected)
    Expected Result: Extension activates and provides highlighting + diagnostics
    Evidence: .sisyphus/evidence/task-118-activate.png

  Scenario: Hover and goto-def work
    Tool: @vscode/test-electron
    Steps:
      1. Open a .buff file with a function definition
      2. Hover over a call site
      3. Assert hover popup shows type/signature
      4. Ctrl+click on function name
      5. Assert cursor jumps to definition
    Expected Result: Hover shows info, goto-def navigates
    Evidence: .sisyphus/evidence/task-118-hover-goto.png

  Scenario: buff.run command
    Tool: @vscode/test-electron
    Steps:
      1. Open examples/ola.buff
      2. Execute command "Buff: Run"
      3. Assert output panel shows "Olá, Buff!"
    Expected Result: Run command executes and shows output
    Evidence: .sisyphus/evidence/task-118-run.txt
  ```

  **Commit**: `feat(vscode): Buff extension with tree-sitter highlighting and LSP integration`

---

### v1.3 "Rust interop" — extern/bindgen + Cargo polish + Examples

- [x] **T119: Minimal extern/bindgen (call Rust crates from Buff)** [deep]

  **What to do**:
  - Make Buff's existing `extern` keyword functional for declaring Rust functions
  - Syntax: `extern "C" fn serde_json_parse(input: String) -> Value` → generates `extern "C"` linkage (using `"C"` ABI for stability and cross-language compatibility)
  - Auto-generate `[rust-deps]` entries in buff.toml based on extern declarations
  - Create wrapper examples for 3 popular Rust crates: `serde_json`, `reqwest`, `tokio`
  - Handle type marshalling: Buff String ↔ Rust String, Buff Int ↔ Rust i64, Buff Vector ↔ Rust Vec
  - Document the extern pattern in a guide

  **Leverages**:
  - Buff's `extern` keyword (already in 25 reserved keywords!)
  - Rust's FFI (`extern "C"`, `extern "Rust"`)
  - Cargo's dependency system (already the backend)
  - wasm-bindgen pattern — reference for type marshalling

  **Must NOT do**:
  - Auto-generate bindings for entire crates (that's a future `buff bindgen` tool)
  - Handle generic/trait-heavy Rust APIs (extern-C + concrete types only)
  - Handle unsafe Rust (wrap in safe abstraction or reject)

  **Recommended Agent Profile**:
  - **Category**: `deep` — FFI + type system integration
  - **Skills**: []

  **Parallelization**: Independent of v1.1/v1.2. Can start immediately after v1.0.

  **References**:
  - `crates/buff-lang-ast/src/` — extern declaration AST node (if exists, or add)
  - `crates/buff-lang-codegen-rust/src/` — where to emit Rust extern FFI
  - `Cargo.toml [workspace.dependencies]` — how Rust deps are declared
  - `https://doc.rust-lang.org/std/keyword.extern.html` — Rust extern semantics

  **Acceptance Criteria**:
  - [ ] Can declare `extern` function in Buff and call it
  - [ ] `serde_json::parse` callable from Buff (example works)
  - [ ] `reqwest::get` callable from Buff (example works)
  - [ ] Type marshalling works for String, Int, Float, Bool, Vector
  - [ ] `[rust-deps]` auto-populated from extern declarations
  - [ ] Documentation guide written

  **QA Scenarios**:
  ```
  Scenario: Call serde_json from Buff
    Tool: Bash
    Steps:
      1. Write a .buff file that declares extern serde_json_parse
      2. Call it with a JSON string
      3. buff run the file
      4. Assert output shows parsed value
    Expected Result: Rust crate function callable from Buff
    Evidence: .sisyphus/evidence/task-119-serde-json.txt

  Scenario: Type marshalling — String roundtrip
    Tool: Bash
    Steps:
      1. Declare extern fn that takes String and returns String
      2. Pass Buff String "hello"
      3. Assert returned value is "hello"
    Expected Result: String marshals correctly both directions
    Evidence: .sisyphus/evidence/task-119-string-marshal.txt

  Scenario: Failure — generic API rejected
    Tool: Bash
    Steps:
      1. Try to extern a generic Rust function
      2. Assert buff check shows error (generics not supported in v1.3)
    Expected Result: Clear error message about generic limitation
    Evidence: .sisyphus/evidence/task-119-generic-reject.txt
  ```

  **Commit**: `feat(bindgen): minimal extern for calling Rust crates from Buff`

- [x] **T120: Cargo polish + buff.toml manifest** [quick]

  **What to do**:
  - Ensure `buff build` cleanly shells out to `cargo build` with correct flags
  - Polish `buff.toml` manifest format: `[package]`, `[dependencies]`, `[rust-deps]`
  - Ensure `buff.toml` → `Cargo.toml` generation is correct and idempotent
  - Add `buff clean` command (wraps `cargo clean`)
  - Add `buff update` command (wraps `cargo update`)

  **Leverages**:
  - T56 (buff build --release, v1.0) — already exists
  - Cargo — the backend
  - Existing buff.toml work in v1.0

  **Must NOT do**:
  - Custom build system (cargo is the build system)
  - Custom dependency resolver (use cargo's)

  **References**:
  - `crates/buff-lang-cli/src/` — CLI command structure
  - `.sisyphus/plans/buff-project-structure.md` — buff.toml format

  **Acceptance Criteria**:
  - [ ] `buff build` produces working binary
  - [ ] `buff clean` removes target/
  - [ ] `buff update` updates dependencies
  - [ ] buff.toml correctly generates Cargo.toml
  - [ ] `[rust-deps]` section produces correct Cargo [dependencies]

  **QA Scenarios**:
  ```
  Scenario: buff build end-to-end
    Tool: Bash
    Steps:
      1. buff new test_project
      2. cd test_project
      3. buff build
      4. Assert target/release/test_project exists
      5. Run binary, assert output
    Expected Result: Build produces working binary
    Evidence: .sisyphus/evidence/task-120-build.txt
  ```

  **Commit**: `feat(cli): polish cargo integration with buff.toml manifest`

- [x] **T121: Side-by-side example library** [writing]

  **What to do**:
  - Create `examples/rust-vs-buff/` directory
  - Write 10-15 examples, each as a pair: `example.rs` (painful Rust) + `example.buff` (clean Buff)
  - Topics: borrow checker, lifetimes, async/await, error handling, pattern matching, iterators, structs, traits, generics, concurrency
  - Each pair has a `README.md` explaining what Rust pain is avoided
  - All `.buff` examples must compile and run via `buff run`

  > **Boundary vs T116**: T116 (v1.1) creates the *website presentation* (a curated marketing subset, may inline example snippets). T121 (v1.3) is the *canonical CI-verified repo source of truth* in `examples/rust-vs-buff/`. T116's website should draw from these files once T121 lands. If executing v1.1 before v1.3, T116 authors provisional snippets; T121 formalizes + expands them into the tested directory. No duplicated maintenance: repo files are authoritative.

  **Leverages**:
  - T116 (website examples) — can share content
  - Existing `examples/` directory

  **References**:
  - `examples/*.buff` — existing examples
  - Rust by Example — pattern reference for structuring examples

  **Acceptance Criteria**:
  - [ ] ≥10 example pairs in `examples/rust-vs-buff/`
  - [ ] All `.buff` examples pass `buff run`
  - [ ] Each pair has README explaining the simplification
  - [ ] CI verifies all examples compile

  **QA Scenarios**:
  ```
  Scenario: All examples compile
    Tool: Bash
    Steps:
      1. For each .buff file in examples/rust-vs-buff/
      2. Run: buff run <file>
      3. Assert exit code 0
    Expected Result: All examples compile and run
    Evidence: .sisyphus/evidence/task-121-examples-compile.txt
  ```

  **Commit**: `docs(examples): add 10+ side-by-side Rust-vs-Buff example pairs`

- [x] **T121b: Dioxus codegen feasibility spike (UI go/no-go gate)** [deep]

  > **WHY THIS IS IN v1.3, NOT v1.8**: This spike front-loads the biggest technical risk: whether Buff's codegen-rust can emit valid Rust source containing Dioxus macro invocations that compile and render. Proving/disproving this 5 releases early means the web/frontend strategy is de-risked before Phase 2 investment.
  >
  > **⚠️ MECHANISM UNDER TEST — CODEGEN, NOT FFI**: Dioxus is macro-driven (`rsx!{}` is a compile-time proc macro). It does NOT integrate via extern-C FFI — you cannot extern-C a proc macro. The correct integration path is: Buff's codegen-rust emits Rust *source text* containing `rsx!{}` / `#[component]` macro calls → rustc + dioxus-rsx proc macro expands them → compiles to Wasm → renders in browser. This spike tests whether `syn`/`quote`/`prettyplease` can emit Dioxus-compatible macro AST nodes. (T119's extern/bindgen is for calling Rust *functions* — orthogonal to UI.)

  **What to do**:
  - Extend codegen-rust to emit a Dioxus **counter component** (not just "Hello World" — a counter exercises signals + event handlers + reactive re-render, which is where macro ergonomics actually break):
    - Rust output contains `#[component]` attribute + `rsx! { button { onclick: move |_| ... } }` macro
  - Build the generated Rust for wasm32, render in a headless browser
  - Pin the Dioxus version tested (e.g., `0.7.x`) and record in the decision document
  - Assess error-message quality: if generated Rust has errors, can they map back to Buff source lines? (This is the #1 real-world risk — generated-code errors leaking to users.)
  - Document outcome in `.sisyphus/decisions/dioxus-feasibility.md`:
    - **PASS** (codegen emits valid macro calls, counter renders AND reacts to clicks) → v1.8 proceeds as planned
    - **PARTIAL** (renders but error messages are poor / some macros don't survive prettyplease) → document what's needed, decide whether to invest in error-mapping before v1.8
    - **FAIL** (codegen cannot emit valid Dioxus macros) → trigger replan of v1.8/v1.9 BEFORE Phase 2 begins
  - Note: no public transpilation precedent exists (no language emits Dioxus from non-Rust source). Buff is breaking ground — document this risk honestly.

  **Leverages**: `syn`/`quote`/`prettyplease` (codegen stack, already in workspace). T114 prerequisite (Wasm target, post-v1.0). Dioxus 0.7.x (pin). `web-sys`.

  **Must NOT do**: Build the full UI foundation (that's v1.8's T130). Build a dev server or Tauri scaffold. Use extern-C/FFI to wrap Dioxus (wrong mechanism). This is a THROWAWAY spike — output is a decision record + a proof, not production code.

  **References**: `crates/buff-lang-codegen-rust/src/lib.rs:53 generate_rust()` (where to emit Dioxus macros), `Cargo.toml [workspace.dependencies] syn/quote/prettyplease`, `https://github.com/dioxuslabs/dioxus` (Dioxus 0.7 source)

  **Acceptance Criteria**:
  - [ ] Decision record written to `.sisyphus/decisions/dioxus-feasibility.md` with PASS/PARTIAL/FAIL verdict
  - [ ] Dioxus version pinned and recorded in the decision
  - [ ] If PASS/PARTIAL: counter component renders in headless browser AND reacts to button click (signal updates DOM)
  - [ ] Generated `.rs` file contains `rsx!` token (verifiable via grep)
  - [ ] Error-message quality assessed (can rustc errors on generated code map to Buff source?)
  - [ ] If FAIL/PARTIAL: concrete replan notes for v1.8/v1.9 recorded
  - [ ] "No transpilation precedent" risk documented

  **QA Scenarios**:
  ```
  Scenario: Codegen emits valid Dioxus counter (happy path)
    Tool: Playwright (headless browser) + Bash
    Preconditions: codegen-rust extended to emit Dioxus macros
    Steps:
      1. Write a .buff file that triggers Dioxus counter codegen
      2. buff build --target wasm32 → assert exit 0
      3. Assert generated .rs contains "rsx!" (grep)
      4. Serve in headless Chrome
      5. Assert page body shows initial counter value "0"
      6. Click the increment button
      7. Assert page body now shows "1" (reactive update works)
      8. Assert .sisyphus/decisions/dioxus-feasibility.md exists with verdict line
    Expected Result: Codegen path works end-to-end, signal+event+reactivity proven
    Evidence: .sisyphus/evidence/task-121b-dioxus-counter.png

  Scenario: Feasibility blocked (failure path is valid)
    Tool: Bash
    Steps:
      1. If codegen cannot emit valid Dioxus macros, assert decision record documents the specific blocker (prettyplease mangling? macro expansion? quote limitation?)
      2. Assert replan notes for v1.8/v1.9 are present
    Expected Result: Failure is documented, not hidden — gates Phase 2 UI investment
    Evidence: .sisyphus/evidence/task-121b-decision.txt
  ```

  **Commit**: `spike(codegen): Dioxus feasibility gate — emit rsx!{} via codegen-rust, render counter in browser`

### v1.4 "Stdlib + Ecosystem foundations" — DateTime + Log + Regex + Toml + Git deps + Workspace + Error codes

> **PREREQUISITE for all stdlib tasks**: Add wrapped crates to `Cargo.toml [workspace.dependencies]` FIRST. Crates needed: `chrono`, `tracing`, `tracing-subscriber`, `regex`, `toml`, `rand`, `url`, `base64`, `hex`, `percent-encoding`, `uuid`, `serde_yml` (NOT `serde_yaml` which is deprecated), `csv`, `tempfile`, `walkdir`, `sha2`, `md5`, `hmac`, `num_cpus` (or `sysinfo`), `tokio-tungstenite`. Follow Convention: workspace deps only, never pin in crate Cargo.toml.
>
> **ASYNC dependency**: Tasks involving `sleep()`, `TCP`, `UDP`, `WebSocket` depend on v1.0's async call-graph propagation being complete. If async isn't fully working by v1.4, these tasks are blocked.
>
> **Prelude shadowing**: All modules are prelude-implicit. User-defined names shadow prelude (e.g., user can define their own `URL` type). Document this behavior — it's intentional (matches Buff's simplicity goal).
>
> **Method syntax on prelude types**: These modules use `Type.method()` syntax (e.g., `DateTime.now()`, `Math.sqrt()`, `Regex.match()`). The existing prelude only has `print` (a free function). **T124+ (v1.4 stdlib, replacing deferred v1.0 T61) MUST establish the pattern for types-with-methods in the prelude.** If T124b's DateTime module doesn't use method syntax, the remaining v1.4 tasks need to verify/establish it first.

> **WHY**: Buff's pitch is "Go productivity." Go ships a rich stdlib (datetime, regexp, log, encoding in-box). Rust's stdlib is thin — devs hunt for chrono/regex/tracing crates. Buff should absorb the most common dependencies so users get zero-frustrure productivity. Each module wraps a proven Rust crate (leverage mandate). These are prelude-implicit (no `import` needed, like `print` today).

- [x] **T124b: DateTime module** [deep]

  **What to do**:
  - Add `DateTime`, `Date`, `Time`, `Duration`, `Instant` types to Buff prelude
  - Wrap `chrono` crate (or `time` crate — pick one, document why)
  - API: `DateTime.now()`, `DateTime.parse(iso_string)`, `dt.format("%Y-%m-%d")`, `dt + Duration.days(7)`, comparison operators
  - Type signatures in `buff-lang-types/src/prelude.rs`; codegen lowering in `buff-lang-codegen-rust`

  **Leverages**: `chrono` crate (mature, de facto Rust datetime). Buff's existing prelude pattern (`print`).

  **Must NOT do**: Timezone database bundling (use system tz). Calendar arithmetic beyond Gregorian. Custom formatting syntax (use chrono's strftime).

  **References**: `crates/buff-lang-types/src/prelude.rs` (where prelude types live), `crates/buff-lang-codegen-rust/src/` (lowering), `https://docs.rs/chrono`

  **Acceptance Criteria**:
  - [ ] `DateTime.now()` returns current time
  - [ ] `DateTime.parse("2026-07-16T12:00:00Z")` works
  - [ ] `dt.format("%Y-%m-%d")` returns formatted string
  - [ ] `Duration.days(7)` + DateTime arithmetic works
  - [ ] Comparison: `dt1 < dt2` works
  - [ ] All available without `import` (prelude-implicit)

  **QA Scenarios**:
  ```
  Scenario: DateTime operations
    Tool: Bash
    Steps:
      1. Write .buff: let now = DateTime.now(); print(now.format("%Y"))
      2. buff run
      3. Assert output contains "2026" (or current year)
    Expected Result: DateTime works without import
    Evidence: .sisyphus/evidence/task-124b-datetime.txt
  ```

  **Commit**: `feat(stdlib): DateTime module wrapping chrono`

- [x] **T124c: Log module** [unspecified-high]

  **What to do**:
  - Add `Log` module to prelude: `Log.debug(msg)`, `Log.info(msg)`, `Log.warn(msg)`, `Log.error(msg)`
  - Wrap `tracing` crate (structured logging, async-aware — fits Buff's hidden-async model)
  - Support structured fields: `Log.info("user logged in", user_id: 42, ip: "10.0.0.1")`
  - Log level configuration via env var (`BUFF_LOG=debug`) or buff.toml
  - Default: pretty-printed to stderr in dev, JSON to stdout in release

  **Leverages**: `tracing` + `tracing-subscriber` crates. Buff's async model (tracing is async-native).

  **Must NOT do**: Custom log filtering DSL (use tracing's env filter). File/network log sinks (future stdlib expansion). Custom formatters beyond pretty/JSON.

  **References**: `crates/buff-lang-types/src/prelude.rs`, `https://docs.rs/tracing`

  **Acceptance Criteria**:
  - [ ] `Log.info("hello")` prints to stderr
  - [ ] Structured fields work: `Log.info("msg", key: value)`
  - [ ] Log level controllable via `BUFF_LOG` env var
  - [ ] JSON output in release mode
  - [ ] Available without `import`

  **QA Scenarios**:
  ```
  Scenario: Logging with levels
    Tool: Bash
    Steps:
      1. Write .buff: Log.info("hello", count: 42)
      2. BUFF_LOG=info buff run
      3. Assert stderr contains "hello" and "count" and "42"
    Expected Result: Structured logging works
    Evidence: .sisyphus/evidence/task-124c-log.txt
  ```

  **Commit**: `feat(stdlib): Log module wrapping tracing`

- [x] **T124d: Regex module** [unspecified-high]

  **What to do**:
  - Add `Regex` type to prelude: `Regex.compile(pattern)`, `regex.match(text)`, `regex.find(text)`, `regex.replace(text, replacement)`
  - Wrap `regex` crate (Rust's standard regex library)
  - Capture groups: `regex.captures(text) -> Map` (named groups as map keys)
  - Return `Option` for match results (Buff's no-null principle)

  **Leverages**: `regex` crate (mature, fast, Unicode-aware).

  **Must NOT do**: Custom regex syntax (use Rust regex syntax). PCRE compatibility. Regex building DSL.

  **References**: `crates/buff-lang-types/src/prelude.rs`, `https://docs.rs/regex`

  **Acceptance Criteria**:
  - [ ] `Regex.compile("\\d+")` works
  - [ ] `regex.match("123abc")` returns `Some`
  - [ ] `regex.match("no digits")` returns `None`
  - [ ] Capture groups accessible by name
  - [ ] `regex.replace("a1b2", "\\d", "X")` returns `"aXbX"`
  - [ ] Available without `import`

  **QA Scenarios**:
  ```
  Scenario: Regex matching and replacement
    Tool: Bash
    Steps:
      1. Write .buff: let r = Regex.compile("(\\w+)@(\\w+)"); let m = r.captures("user@host"); print(m["1"])
      2. buff run
      3. Assert output: "user"
    Expected Result: Regex with captures works
    Evidence: .sisyphus/evidence/task-124d-regex.txt
  ```

  **Commit**: `feat(stdlib): Regex module wrapping regex crate`

- [ ] **T124e: Toml module** [quick]

  **What to do**:
  - Add `Toml` module to prelude: `Toml.parse(string) -> Map`, `Toml.stringify(value) -> String`
  - Wrap `toml` crate
  - Critical because Buff's OWN project config (`buff.toml`) is TOML — users need to parse it natively
  - Return Buff `Map` type (interop with existing collection types)

  **Leverages**: `toml` crate. Buff's existing `Map` type.

  **Must NOT do**: TOML schema validation. TOML editing/preserving-format round-trip. Custom serialization attributes.

  **References**: `crates/buff-lang-types/src/prelude.rs`, `https://docs.rs/toml`

  **Acceptance Criteria**:
  - [ ] `Toml.parse("key = \"value\"")` returns Map with key "key"
  - [ ] `Toml.stringify(map)` produces valid TOML
  - [ ] Round-trip: parse → stringify → parse produces same data
  - [ ] Available without `import`

  **QA Scenarios**:
  ```
  Scenario: TOML round-trip
    Tool: Bash
    Steps:
      1. Write .buff: let config = Toml.parse(File.read("buff.toml")); print(config["package"]["name"])
      2. buff run (in a Buff project)
      3. Assert output: project name from buff.toml
    Expected Result: TOML parsing works on real buff.toml
    Evidence: .sisyphus/evidence/task-124e-toml.txt
  ```

  **Commit**: `feat(stdlib): Toml module wrapping toml crate`

- [ ] **T124f: Utility modules — Math, Random, Sort, Strings** [unspecified-high]

  **What to do**:
  - `Math`: `Math.sqrt(x)`, `Math.sin/cos/tan(x)`, `Math.PI`, `Math.E`, `Math.abs(x)`, `Math.floor/ceil/round(x)`, `Math.pow(base, exp)`, `Math.min/max(a, b)` — wrap Rust's `std::f64` methods + consts
  - `Random`: `Random.int(min, max)`, `Random.float()`, `Random.choice(vector)`, `Random.shuffle(vector)` — wrap `rand` crate
  - `Sort`: `vector.sort()`, `vector.sort_by(comparator)` — wrap Rust's `sort_by` on slices (method on Vector/Collection)
  - `Strings`: `Strings.split(text, sep)`, `Strings.join(vector, sep)`, `Strings.trim(text)`, `Strings.replace(text, from, to)`, `Strings.contains(text, substr)`, `Strings.starts_with(text, prefix)`, `Strings.to_uppercase(text)`, `Strings.to_lowercase(text)` — wrap Rust's `str`/`String` methods as a module (some may already be methods on Buff's String type; expose as module for functional-style use)
  - All prelude-implicit (no import)

  **Leverages**: `std::f64` (Rust built-in math), `rand` crate, Rust slice sort methods. Buff's existing Vector type.

  **Must NOT do**: Custom math DSL. Matrix operations (that's GPU territory). Cryptographically secure random (that's Hash/Crypto module).

  **References**: `crates/buff-lang-types/src/prelude.rs`, `https://docs.rs/rand`, Rust `std::f64` methods

  **Acceptance Criteria**:
  - [ ] `Math.sqrt(16)` → `4.0`; `Math.PI` available; `Math.floor(3.7)` → `3.0`
  - [ ] `Random.int(1, 10)` returns int in [1, 10]; `Random.choice([1,2,3])` returns element
  - [ ] `[3, 1, 2].sort()` → `[1, 2, 3]`; `sort_by` works with custom comparator
  - [ ] All available without import

  **QA Scenarios**:
  ```
  Scenario: Math and Random
    Tool: Bash
    Steps:
      1. Write: print(Math.sqrt(16)); print(Math.PI); print(Random.int(1, 100) >= 1)
      2. buff run
      3. Assert: "4.0", "3.14...", and "true"
    Expected Result: Math and Random work prelude-implicit
    Evidence: .sisyphus/evidence/task-124f-math-random.txt
  ```

  **Commit**: `feat(stdlib): Math, Random, and Sort utility modules`

- [ ] **T124g: System modules — Args, Env, input(), sleep()** [unspecified-high]

  **What to do**:
  - `Args`: `Args.list() -> Vector<String>` (command-line arguments), `Args.get(index) -> String` — wrap `std::env::args`
  - `Env`: `Env.get("KEY") -> Option<String>`, `Env.set("KEY", "value")`, `Env.has("KEY") -> Bool` — wrap `std::env::var`
  - `input()`: `input() -> String` (read line from stdin), `input(prompt: String) -> String` (print prompt then read) — wrap `std::io::stdin`
  - `sleep()`: `sleep(Duration.seconds(2))` — async-transparent (Buff's hidden async); wrap `tokio::time::sleep`
  - All prelude-implicit

  **Leverages**: `std::env`, `std::io::stdin`, `tokio::time::sleep`. Buff's async model (sleep must be async-transparent).

  **Must NOT do**: Argument parsing DSL (that's a future `buff parse-args` or clap wrapper). Password input (getpass — future).

  **References**: `crates/buff-lang-types/src/prelude.rs`, `std::env` docs, `tokio::time` docs

  **Acceptance Criteria**:
  - [ ] `Args.list()` returns program name + args
  - [ ] `Env.get("HOME")` returns Some(path) on most systems
  - [ ] `input("Name: ")` prints prompt, reads line, returns string
  - [ ] `sleep(Duration.seconds(1))` blocks ~1 second (async-transparent)
  - [ ] All available without import

  **QA Scenarios**:
  ```
  Scenario: Args and Env
    Tool: Bash
    Steps:
      1. Write: print(Args.list()); print(Env.get("HOME"))
      2. buff run -- arg1 arg2
      3. Assert: args list includes "arg1", "arg2"; HOME path printed
    Expected Result: System modules work
    Evidence: .sisyphus/evidence/task-124g-system.txt
  ```

  **Commit**: `feat(stdlib): Args, Env, input(), and sleep() system modules`

- [ ] **T124h: Web modules — URL, Base64, Hex, URLEncode, UUID** [unspecified-high]

  **What to do**:
  - `URL`: `URL.parse("https://a.com/b?q=1") -> URL`, `.scheme`, `.host`, `.path`, `.query(key) -> Option<String>` — wrap `url` crate
  - `Base64`: `Base64.encode(bytes) -> String`, `Base64.decode(string) -> Vector<Byte>` — wrap `base64` crate
  - `Hex`: `Hex.encode(bytes) -> String`, `Hex.decode(string) -> Vector<Byte>` — wrap `hex` crate
  - `URLEncode`: `URLEncode.encode(string) -> String`, `URLEncode.decode(string) -> String` — wrap `percent-encoding` crate
  - `UUID`: `UUID.v4() -> String`, `UUID.v7() -> String`, `UUID.parse(string) -> Bool` — wrap `uuid` crate
  - All prelude-implicit

  **Leverages**: `url`, `base64`, `hex`, `percent-encoding`, `uuid` crates.

  **Must NOT do**: Custom URL builder DSL. UUID v1-v3 (time-based, niche). URL routing/matching (that's a web framework concern).

  **References**: `crates/buff-lang-types/src/prelude.rs`, respective crate docs

  **Acceptance Criteria**:
  - [ ] URL parsing extracts scheme, host, path, query params
  - [ ] Base64 round-trip works
  - [ ] Hex round-trip works
  - [ ] URLEncode handles spaces, special chars
  - [ ] UUID.v4() generates valid UUID
  - [ ] All available without import

  **QA Scenarios**:
  ```
  Scenario: URL parsing
    Tool: Bash
    Steps:
      1. Write: let u = URL.parse("https://example.com/path?key=val"); print(u.host); print(u.query("key"))
      2. buff run
      3. Assert: "example.com" and "val"
    Expected Result: URL module works
    Evidence: .sisyphus/evidence/task-124h-web.txt
  ```

  **Commit**: `feat(stdlib): URL, Base64, Hex, URLEncode, UUID web utility modules`

- [ ] **T124i: Data format modules — YAML, CSV** [quick]

  **What to do**:
  - `Yaml`: `Yaml.parse(string) -> Map`, `Yaml.stringify(value) -> String` — wrap `serde_yml` crate (NOT `serde_yaml` which is deprecated)
  - `Csv`: `Csv.parse(string) -> Vector<Vector<String>>`, `Csv.stringify(rows) -> String` — wrap `csv` crate
  - Both prelude-implicit

  **Leverages**: `serde_yml` (active fork; `serde_yaml` is deprecated), `csv` crates. Same pattern as Toml (T124e).

  **Must NOT do**: Schema validation. Streaming parsing (for huge files). Custom serialization attributes.

  **References**: `crates/buff-lang-types/src/prelude.rs`, `serde_yml` docs, `csv` docs

  **Acceptance Criteria**:
  - [ ] YAML parse → Map; stringify → valid YAML
  - [ ] CSV parse → Vector of rows; stringify → valid CSV
  - [ ] Both round-trip correctly
  - [ ] Available without import

  **QA Scenarios**:
  ```
  Scenario: YAML and CSV
    Tool: Bash
    Steps:
      1. Write: let y = Yaml.parse("a: 1\nb: 2"); print(y["a"])
      2. buff run; Assert: "1"
    Expected Result: Data format modules work
    Evidence: .sisyphus/evidence/task-124i-formats.txt
  ```

  **Commit**: `feat(stdlib): YAML and CSV data format modules`

- [ ] **T124j: Filesystem modules — Path, Dir, Tempfile** [unspecified-high]

  **What to do**:
  - `Path`: `Path.join(a, b)`, `.parent() -> Option<Path>`, `.extension() -> Option<String>`, `.basename() -> String`, `.exists() -> Bool` — wrap `std::path::Path`
  - `Dir`: `Dir.list(path) -> Vector<String>`, `Dir.create(path)`, `Dir.remove(path)`, `Dir.walk(path) -> Vector<Path>` — wrap `std::fs::read_dir`, `walkdir` crate
  - `Tempfile`: `Tempfile.create() -> Path` (temp file in system temp dir), `Tempfile.dir() -> Path` — wrap `tempfile` crate
  - All prelude-implicit. Establishes File I/O (replaces deferred v1.0 T61).

  **Leverages**: `std::path::Path`, `std::fs`, `tempfile` crate, `walkdir` crate.

  **Must NOT do**: File watching (that's `buff watch` T80). Permission management. Symlink creation (niche, unsafe-adjacent).

  **References**: `crates/buff-lang-types/src/prelude.rs`, `std::path` docs, `tempfile` docs

  **Acceptance Criteria**:
  - [ ] Path.join, parent, extension work correctly
  - [ ] Dir.list returns directory contents
  - [ ] Dir.walk recursively lists files
  - [ ] Tempfile.create returns valid temp file path
  - [ ] All available without import

  **QA Scenarios**:
  ```
  Scenario: Path and Dir
    Tool: Bash
    Steps:
      1. Write: let p = Path.join("a", "b", "c.txt"); print(p.extension())
      2. buff run; Assert: "txt"
      3. Write: for f in Dir.list("."): print(f)
      4. buff run; Assert: files listed
    Expected Result: Filesystem modules work
    Evidence: .sisyphus/evidence/task-124j-filesystem.txt
  ```

  **Commit**: `feat(stdlib): Path, Dir, and Tempfile filesystem modules`

- [ ] **T124k: Crypto modules — Hash, HMAC** [quick]

  **What to do**:
  - `Hash`: `Hash.sha256(data) -> String` (hex digest), `Hash.sha512(data) -> String`, `Hash.md5(data) -> String` — wrap `sha2` + `md5` crates. Note: MD5 is cryptographically broken — include for checksum compatibility only, document "not for security use."
  - `HMAC`: `HMAC.sha256(key, data) -> String` — wrap `hmac` crate
  - Input: String or Vector<Byte>. Output: hex string.
  - Both prelude-implicit

  **Leverages**: `sha2`, `md5`, `hmac` crates.

  **Must NOT do**: Symmetric/asymmetric encryption (AES, RSA — too specialized). Custom crypto implementations (always wrap vetted crates). Password hashing (argon2/bcrypt — separate concern).

  **References**: `crates/buff-lang-types/src/prelude.rs`, `sha2` docs, `hmac` docs

  **Acceptance Criteria**:
  - [ ] `Hash.sha256("hello")` returns known SHA-256 hex digest
  - [ ] `Hash.md5("hello")` returns known MD5 hex digest
  - [ ] `HMAC.sha256("key", "data")` returns correct HMAC
  - [ ] Available without import

  **QA Scenarios**:
  ```
  Scenario: Hash known values
    Tool: Bash
    Steps:
      1. Write: print(Hash.sha256("hello"))
      2. buff run
      3. Assert: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    Expected Result: SHA-256 matches known value
    Evidence: .sisyphus/evidence/task-124k-crypto.txt
  ```

  **Commit**: `feat(stdlib): Hash (SHA-256/512, MD5) and HMAC crypto modules`

- [ ] **T124l: Process & OS modules — Process, OS info** [quick]

  **What to do**:
  - `Process`: `Process.spawn(command, args) -> Process` (NOTE: wraps `std::process::Command::new(cmd).args(args)` — does NOT shell out; command and args are separate parameters to prevent shell injection). `.wait() -> Int` (exit code), `Process.exit(code)`, `Process.id() -> Int` — wrap `std::process::Command`
  - `OS`: `OS.name() -> String` (linux/macos/windows), `OS.arch() -> String` (x86_64/aarch64), `OS.hostname() -> String`, `OS.cpus() -> Int` — wrap `std::env::consts` + `num_cpus`/`sysinfo`
  - Both prelude-implicit

  **Leverages**: `std::process::Command`, `std::env::consts`, `num_cpus` or `sysinfo` crate.

  **Must NOT do**: Signal handling (SIGINT — niche). Shell expansion. Privilege management.

  **References**: `crates/buff-lang-types/src/prelude.rs`, `std::process` docs

  **Acceptance Criteria**:
  - [ ] `Process.spawn("echo hello").wait()` returns exit code 0
  - [ ] `OS.name()` returns "linux"/"macos"/"windows"
  - [ ] `OS.cpus()` returns positive int
  - [ ] Available without import

  **QA Scenarios**:
  ```
  Scenario: Process spawn
    Tool: Bash
    Steps:
      1. Write: let p = Process.spawn("echo hello"); print(p.wait())
      2. buff run
      3. Assert: "hello" printed, exit code "0"
    Expected Result: Process module works
    Evidence: .sisyphus/evidence/task-124l-process.txt
  ```

  **Commit**: `feat(stdlib): Process and OS system info modules`

- [ ] **T124m: Networking modules — TCP, UDP, WebSocket** [deep]

  **What to do**:
  - `TCP`: `TCP.connect(host, port) -> Connection`, `.send(data)`, `.recv() -> Vector<Byte>`, `.close()` — wrap `std::net::TcpStream` (async via tokio)
  - `UDP`: `UDP.bind(host, port) -> Socket`, `.send_to(data, addr)`, `.recv_from() -> (data, addr)` — wrap `std::net::UdpSocket`
  - `WebSocket`: `WebSocket.connect(url) -> WsConnection`, `.send(text)`, `.recv() -> String`, `.close()` — wrap `tokio-tungstenite`
  - All prelude-implicit. Async-transparent (Buff's hidden async model).

  **Leverages**: `std::net` (TCP/UDP), `tokio-tungstenite` (WebSocket). Buff's async propagation.

  **Must NOT do**: TLS/SSL directly (wrap behind a flag or separate module — note: WebSocket without TLS/wss:// is of limited use for real apps; document that TLS support comes via a future `TLS` module or `native-tls`/`rustls` integration). Custom protocol implementations. Raw socket options (SO_REUSEADDR etc. — advanced).

  **References**: `crates/buff-lang-types/src/prelude.rs`, `std::net` docs, `tokio-tungstenite` docs

  **Acceptance Criteria**:
  - [ ] TCP connect → send → recv → close works against a test server
  - [ ] UDP bind → send_to → recv_from works
  - [ ] WebSocket connect → send → recv → close works against a test WS server
  - [ ] All async-transparent (no `await` needed in Buff)
  - [ ] Available without import

  **QA Scenarios**:
  ```
  Scenario: TCP echo
    Tool: Bash
    Preconditions: test TCP echo server running (can be spawned in test setup)
    Steps:
      1. Write: let conn = TCP.connect("localhost", 8080); conn.send("hello"); print(conn.recv()); conn.close()
      2. buff run (with echo server running)
      3. Assert: "hello" received back
    Expected Result: TCP networking works
    Evidence: .sisyphus/evidence/task-124m-networking.txt
  ```

  **Commit**: `feat(stdlib): TCP, UDP, and WebSocket networking modules`

### (continued v1.4) — Git deps + Workspace + Error codes

- [ ] **T122: Git dependency support** [deep]

  **What to do**:
  - Implement `buff add git+https://github.com/user/lib.buff` syntax
  - Clone git repo to `~/.buff/git/<hash>/`, parse its `buff.toml` for transitive deps
  - Support `branch`, `tag`, `rev` qualifiers (like Cargo git deps)
  - Add `[git-dependencies]` section to buff.toml
  - Generate correct Cargo.toml entries (or local path deps for the git checkout)

  **Leverages**: Cargo git deps pattern (clone + parse + path dep). Go modules pre-proxy pattern.

  **Must NOT do**: Custom git hosting, registry API (that's v1.6).

  **References**: `crates/buff-lang-cli/src/` (CLI), Cargo git deps docs, `.sisyphus/plans/buff-project-structure.md`

  **Acceptance Criteria**:
  - [ ] `buff add git+https://...` clones and adds dep
  - [ ] Transitive deps resolved
  - [ ] `branch`/`tag`/`rev` qualifiers work
  - [ ] Consumed library's functions callable

  **QA Scenarios**:
  ```
  Scenario: Add and use git dep
    Tool: Bash
    Steps:
      1. Create new buff project
      2. buff add git+https://github.com/buff-lang/example-lib.buff
      3. Import and call function from lib
      4. buff build && ./target/release/app
      5. Assert output correct
    Expected Result: Git dep consumed successfully
    Evidence: .sisyphus/evidence/task-122-git-dep.txt
  ```

  **Commit**: `feat(cli): git-based dependency support`

- [ ] **T123: Workspace support** [unspecified-high]

  **What to do**:
  - Support `[workspace]` section in root buff.toml
  - Member projects share target/ directory and dependencies
  - `buff build` from workspace root builds all members
  - `buff test` runs all member tests
  - Passthrough to Cargo workspaces (don't reinvent)

  **Leverages**: Cargo workspaces (the backend). Don't reinvent.

  **Must NOT do**: Custom workspace concept (cargo workspaces passthrough only).

  **References**: Cargo workspaces docs, `crates/buff-lang-cli/src/`

  **Acceptance Criteria**:
  - [ ] Workspace buff.toml with members recognized
  - [ ] Shared target/ directory works
  - [ ] `buff build` from root builds all members
  - [ ] Shared dependencies deduplicated

  **QA Scenarios**:
  ```
  Scenario: Multi-package workspace builds
    Tool: Bash
    Steps:
      1. Create workspace with 2 member packages
      2. buff build from workspace root
      3. Assert both member binaries/libs built
    Expected Result: Workspace builds all members
    Evidence: .sisyphus/evidence/task-123-workspace.txt
  ```

  **Commit**: `feat(cli): workspace support via cargo passthrough`

- [ ] **T124: Error code catalog** [quick]

  **What to do**:
  - Assign stable codes to each Buff error (E1001, E1002, ... like Rust's E0XXX)
  - Update buff-lang-error to emit codes alongside messages
  - Build a static site (`errors.buff-lang.org/E1001`) with detailed explanations + examples for each code
  - Error messages reference the code: `error[E1001]: type mismatch...`

  **Leverages**: Rust error index (rust-lang.org/error-index). T59 ariadne diagnostics (deferred to v2.0 — re-enable if pursuing rich error rendering when T59 lands).

  **References**: `crates/buff-lang-error/src/`, Rust error index format

  **Acceptance Criteria**:
  - [ ] Every error type has a stable code
  - [ ] Error messages include `[EXXXX]`
  - [ ] Static site documents each code with example
  - [ ] Codes are stable across releases (documented in conventions)

  **QA Scenarios**:
  ```
  Scenario: Error includes code
    Tool: Bash
    Steps:
      1. Trigger a type error
      2. Assert error message contains "[E1" prefix
      3. Fetch errors.buff-lang.org/E1XXX
      4. Assert page documents the error
    Expected Result: Errors are coded and documented online
    Evidence: .sisyphus/evidence/task-124-error-codes.txt
  ```

  **Commit**: `feat(error): stable error codes with online catalog`

---

### v1.5 "Scripting" — REPL

- [ ] **T125-prep: Extract shared `buff-eval` evaluation core** [deep]

  > **WHY THIS IS FIRST**: T129b (v1.7 Jupyter) and T138a (v1.11 Bufflings) both "reuse the REPL evaluator." Without a shared crate, they'd have to refactor buff-repl internals (scope creep into a "done" release). This task extracts the evaluation engine into a reusable crate BEFORE any consumer depends on it.

  **What to do**:
  - Create `crates/buff-eval/` crate with a clean evaluation API:
    - `Evaluator::new() -> Evaluator`
    - `eval(&mut self, source: &str) -> EvalResult` (returns value or error diagnostic)
    - `eval_line(&mut self, line: &str) -> EvalResult` (incremental, accumulates declarations)
    - State accumulation (variables persist across calls)
    - Type introspection: `type_of(&self, expr: &str) -> Option<Type>`
    - **Output capture**: `eval()` returns `EvalResult { value, stdout: String, stderr: String }` — captures print output so Jupyter (T129b) can route it to iopub, and Bufflings (T138c) can compare program output. Critical: Buff's `print` must write to a capturable buffer, not directly to process stdout.
  - T125a/b/c (REPL), T129b (Jupyter), and T138c (Bufflings verification) all depend on this crate

  **Leverages**: Compiler APIs (`tokenize`, `parse`, `TypeInferencer`). This is a *thin orchestration layer* over existing compiler primitives, not new compilation logic.

  **Must NOT do**: UI/IO concerns (rustyline, stdin/stdout, WebSocket — those belong in consumers). File loading (T125c's job). Rich display (T129c's job).

  **References**: `crates/buff-lang-lexer/src/lexer.rs:39 tokenize()`, `crates/buff-lang-parser/src/parser.rs:31 parse()`, `crates/buff-lang-types/src/infer.rs:20 TypeInferencer`

  **Acceptance Criteria**:
  - [ ] `crates/buff-eval/` crate exists with clean public API
  - [ ] `eval("2 + 3")` returns `5`
  - [ ] `eval_line("let x = 42")` then `eval_line("x + 8")` returns `50` (state persists)
  - [ ] `type_of("x")` returns `Int` (after above)
  - [ ] `cargo test -p buff-eval` passes

  **QA Scenarios**:
  ```
  Scenario: Evaluate expression
    Tool: Bash
    Steps:
      1. cargo test -p buff-eval -- eval_expression
      2. Assert pass
    Expected Result: Evaluation core works standalone
    Evidence: .sisyphus/evidence/task-125prep-eval-core.txt
  ```

  **Commit**: `feat(eval): extract shared buff-eval crate for REPL/Jupyter/Bufflings`

- [ ] **T125a: REPL core (read-eval-print loop)** [unspecified-high]

  **What to do**:
  - Create `crates/buff-repl/` crate + `buff repl` CLI command
  - Integrate `rustyline` for line editing + prompt
  - Read one expression/statement → tokenize+parse+typecheck+evaluate → print result
  - Handle parse/type errors gracefully (print diagnostic, stay in loop)

  **Leverages**: `rustyline` crate. Compiler APIs (tokenize+parse+typecheck). Evcxr pattern (Rust REPL reference).

  **Must NOT do**: State persistence (T125b), commands/file-load (T125c), notebook output (Jupyter v1.7).

  **References**: `rustyline` crate, `crates/buff-lang-lexer/src/lexer.rs:39`, `crates/buff-lang-parser/src/parser.rs:31`, `crates/buff-lang-types/src/infer.rs:20`

  **Acceptance Criteria**:
  - [ ] `buff repl` launches interactive shell with prompt
  - [ ] Single expression evaluates and prints (e.g. `2 + 3` → `5`)
  - [ ] Parse/type error prints diagnostic and continues loop
  - [ ] `cargo test -p buff-repl` passes

  **QA Scenarios**:
  ```
  Scenario: Evaluate expression
    Tool: tmux/interactive_bash
    Steps:
      1. Launch buff repl
      2. Type: 2 + 3
      3. Assert output: 5
      4. Type: broken(
      5. Assert error diagnostic printed, prompt returns
    Expected Result: REPL evaluates and survives errors
    Evidence: .sisyphus/evidence/task-125a-repl-core.txt
  ```

  **Commit**: `feat(repl): REPL core read-eval-print loop with rustyline`

- [ ] **T125b: Session state + type introspection** [unspecified-high]

  **What to do**:
  - Maintain session environment: `let` bindings persist across inputs
  - Re-use accumulated declarations when evaluating new input
  - `:type <expr>` command shows inferred type
  - Show inferred type alongside results (optional, toggle)

  **Leverages**: T125a (REPL core). `TypeInferencer` (persistent env across evals).

  **Must NOT do**: File loading (T125c). Persistence to disk between sessions.

  **References**: T125a, `crates/buff-lang-types/src/infer.rs:20 TypeInferencer` (env accumulation)

  **Acceptance Criteria**:
  - [ ] `let x = 42` then `x + 8` → `50` (state persists)
  - [ ] `:type x` → `Int`
  - [ ] Redefining a variable shadows correctly

  **QA Scenarios**:
  ```
  Scenario: State persists across lines
    Tool: tmux
    Steps:
      1. Launch buff repl
      2. Type: let x = 42
      3. Type: x + 8
      4. Assert output: 50
      5. Type: :type x
      6. Assert output: Int
    Expected Result: Variables persist, types introspectable
    Evidence: .sisyphus/evidence/task-125b-state.txt
  ```

  **Commit**: `feat(repl): session state persistence and :type introspection`

- [ ] **T125c: REPL commands, file loading, multi-line, history** [unspecified-high]

  **What to do**:
  - Commands: `:help`, `:load <file>`, `:quit`
  - `:load` reads a `.buff` file, evaluates its declarations into session
  - Multi-line input: detect incomplete input (open block via indentation) → continue prompt
  - History: save/restore to `~/.buff_history`

  **Leverages**: T125a (core), T125b (state). `rustyline` history API.

  **Must NOT do**: Notebook rich output. Auto-complete (LSP territory).

  **References**: T125a, T125b, `rustyline` history docs

  **Acceptance Criteria**:
  - [ ] `:help` lists commands
  - [ ] `:load examples/fibonacci.buff` then `fibonacci(10)` → `55`
  - [ ] Multi-line function definition works (indentation-aware continuation)
  - [ ] History saved to `~/.buff_history` and restored next session

  **QA Scenarios**:
  ```
  Scenario: Load file and call function
    Tool: tmux
    Steps:
      1. Launch buff repl
      2. Type: :load examples/fibonacci.buff
      3. Type: fibonacci(10)
      4. Assert output: 55
    Expected Result: File loaded, functions callable
    Evidence: .sisyphus/evidence/task-125c-load.txt

  Scenario: Multi-line input
    Tool: tmux
    Steps:
      1. Type a function header ending with ':' (opens block)
      2. Assert continuation prompt appears
      3. Type indented body
      4. Assert function defined and callable
    Expected Result: Multi-line indentation-aware input works
    Evidence: .sisyphus/evidence/task-125c-multiline.txt
  ```

  **Commit**: `feat(repl): commands, file loading, multi-line input, history`

### v1.6 "Package registry" — Minimal registry + CLI

- [ ] **T126: Minimal Buff registry server** [deep]

  **What to do**:
  - Build registry server (axum + diesel + postgres + S3/MinIO storage)
  - Endpoints: `POST /api/v1/publish` (upload .buff package tarball), `GET /api/v1/package/<name>` (metadata), `GET /api/v1/download/<name>/<version>` (tarball)
  - Semver version resolution (use `semver` crate — implements Cargo's exact semver)
  - Token-based auth (GitHub OAuth for publish, anonymous for download)
  - Index API for dependency resolution
  - Deploy to a VPS or Fly.io/Railway ($50-100/mo)

  **Leverages**: `axum` 0.8 (web), `diesel` 2.3 (postgres ORM), `semver` 1.0, `serde`, crates.io API patterns (familiar to Rust devs). DO NOT fork crates.io — build minimal ~2000 LOC.

  **Must NOT do**: Fork crates.io (too complex). Search UI, docs hosting, download stats, webhooks, teams (all v2.0). RustSec audit integration (Buff-advisories only).

  **References**: `axum`/`diesel`/`semver` crate docs. crates.io API as pattern reference. Key research finding: Cargo's resolver is NOT reusable as a library — use `semver` crate directly and build a simpler Pubgrub-based resolver.

  **Acceptance Criteria**:
  - [ ] Publish endpoint accepts .buff tarball
  - [ ] Download endpoint serves tarball
  - [ ] Semver resolution returns correct version for requirements
  - [ ] Auth required for publish, anonymous download
  - [ ] Dependency cycle detection (reject A→B→A)
  - [ ] Published and accessible at registry.buff-lang.org (or similar)
  - [ ] Rate limiting on publish endpoint (prevent abuse)
  - [ ] Package name validation (reject `../`, profanity, squatting patterns)
  - [ ] Anonymous download, auth required for publish
  - [ ] Ops runbook: backup/restore procedure documented

  **QA Scenarios**:
  ```
  Scenario: Publish + download roundtrip
    Tool: Bash (curl)
    Steps:
      1. POST a .buff package tarball to /api/v1/publish with auth
      2. Assert response: 201 Created with package metadata
      3. GET /api/v1/package/test-pkg
      4. Assert response includes version 1.0.0
      5. GET /api/v1/download/test-pkg/1.0.0
      6. Assert tarball downloaded matches uploaded
    Expected Result: Publish and download work end-to-end
    Evidence: .sisyphus/evidence/task-126-publish-download.txt

  Scenario: Semver resolution
    Tool: Bash (curl)
    Steps:
      1. Publish versions 1.0.0, 1.1.0, 2.0.0 of a package
      2. Query resolution for "^1.0.0"
      3. Assert returns 1.1.0 (highest compatible)
    Expected Result: Semver resolution correct
    Evidence: .sisyphus/evidence/task-126-semver.txt

  Scenario: Dependency cycle rejected
    Tool: Bash (curl)
    Steps:
      1. Publish package A depending on B
      2. Try to publish package B depending on A
      3. Assert error: cycle detected
    Expected Result: Cycles rejected
    Evidence: .sisyphus/evidence/task-126-cycle-reject.txt

  Scenario: Security — unauthenticated publish rejected, name validation
    Tool: Bash (curl)
    Steps:
      1. POST to /api/v1/publish WITHOUT auth token
      2. Assert response: 401 Unauthorized
      3. POST with auth but package name "../evil"
      4. Assert response: 400 Invalid name
      5. Rapid-fire 100 publishes → assert rate-limited (429) after threshold
    Expected Result: Abuse prevention works
    Evidence: .sisyphus/evidence/task-126-security.txt
  ```

  **Commit**: `feat(registry): minimal Buff package registry server`

- [ ] **T127: buff CLI package commands (publish/install/add)** [unspecified-high]

  **What to do**:
  - `buff add <name>[@version]` — fetch from registry, update buff.toml, resolve deps
  - `buff publish` — pack .buff source into tarball, upload to registry
  - `buff install <name>` — install a binary package (like cargo install)
  - `buff login` — authenticate with registry (store token in ~/.buff/credentials)
  - Fall back to git deps (T122) if registry unavailable

  **Leverages**: Cargo CLI patterns (`cargo add`, `cargo publish`). T122 git deps (fallback).

  **Must NOT do**: `buff audit` with Rust CVEs (Buff-advisories only, v2.0).

  **References**: `crates/buff-lang-cli/src/`, T122 (git deps), T126 (registry API)

  **Acceptance Criteria**:
  - [ ] `buff login` stores credentials
  - [ ] `buff add <pkg>` fetches and adds to buff.toml
  - [ ] `buff publish` uploads package to registry
  - [ ] `buff install <binary>` installs CLI tool
  - [ ] Consumed package's functions callable

  **QA Scenarios**:
  ```
  Scenario: Publish then consume
    Tool: Bash
    Steps:
      1. buff login (with test credentials)
      2. buff publish (from a test package)
      3. Create new project, buff add test-pkg
      4. Import and call function
      5. buff build && run
      6. Assert correct output
    Expected Result: Full publish-consume cycle works
    Evidence: .sisyphus/evidence/task-127-publish-consume.txt
  ```

  **Commit**: `feat(cli): buff add/publish/install for registry integration`

- [ ] **T128: buff deps + buff outdated** [quick]

  **What to do**:
  - `buff deps` — print dependency tree (like `cargo tree`)
  - `buff outdated` — check for newer versions of dependencies
  - `buff deps --why <pkg>` — show why a dependency is included

  **Leverages**: `cargo tree`, `cargo-outdated` patterns. T126/T127 registry API.

  **Must NOT do**: `buff audit` (security) — defer to v2.0 (needs advisory database).

  **Acceptance Criteria**:
  - [ ] `buff deps` prints tree
  - [ ] `buff outdated` reports newer versions
  - [ ] `--why` shows dependency chain

  **QA Scenarios**:
  ```
  Scenario: Dependency tree
    Tool: Bash
    Steps:
      1. In a project with 2+ deps
      2. buff deps
      3. Assert output shows tree structure with package names and versions
    Expected Result: Tree printed correctly
    Evidence: .sisyphus/evidence/task-128-deps-tree.txt
  ```

  **Commit**: `feat(cli): buff deps and buff outdated commands`

---

### v1.7 "Data science" — Jupyter kernel

- [ ] **T129a: Jupyter kernel protocol + install** [deep]

  **What to do**:
  - Create `crates/buff-jupyter/` crate implementing the Jupyter wire protocol (ZMQ, 5 sockets: shell, iopub, stdin, control, heartbeat)
  - Handle `kernel_info_request`, `execute_request` (stub execution), `shutdown_request`
  - `buff jupyter install` writes kernelspec (`kernel.json`) via `jupyter kernelspec install`
  - HMAC message signing

  **Leverages**: `evcxr_jupyter` (reference impl). `zeromq` crate. Jupyter messaging protocol spec.

  **Must NOT do**: Real execution (T129b), rich display (T129c).

  **References**: `evcxr_jupyter` source, Jupyter messaging protocol spec (`jupyter-client.readthedocs.io`)

  **Acceptance Criteria**:
  - [ ] `buff jupyter install` registers kernel (appears in `jupyter kernelspec list`)
  - [ ] Kernel handshake works (`kernel_info_request` → reply)
  - [ ] `jupyter console --kernel buff` connects without error

  **QA Scenarios**:
  ```
  Scenario: Kernel registers and connects
    Tool: Bash
    Steps:
      1. buff jupyter install
      2. jupyter kernelspec list
      3. Assert output contains "buff"
      4. Run: jupyter kernelspec list --json | assert buff entry
    Expected Result: Kernel registered and discoverable
    Evidence: .sisyphus/evidence/task-129a-kernel-install.txt
  ```

  **Commit**: `feat(jupyter): kernel protocol scaffold and kernelspec install`

- [ ] **T129b: Execution engine + state persistence** [deep]

  **What to do**:
  - Wire `execute_request` to the evaluation engine (reuse T125 REPL evaluator)
  - Return text output via `iopub` stream messages
  - Persist kernel state across cells (variables/functions accumulate)
  - Report execution errors as `error` messages with traceback

  **Leverages**: T129a (protocol), T125a/T125b (REPL evaluation engine + state — shared core).

  **Must NOT do**: Rich/image display (T129c). Widgets.

  **References**: T129a, T125a/b (evaluation + state core)

  **Acceptance Criteria**:
  - [ ] Cell executes Buff code, text output returned
  - [ ] Variables persist across cells
  - [ ] Errors returned as traceback, kernel survives

  **QA Scenarios**:
  ```
  Scenario: Execute + state persists across cells
    Tool: Bash (nbconvert)
    Steps:
      1. Notebook cell 1: let x = 42
      2. Cell 2: print(x)
      3. jupyter nbconvert --execute --to html test.ipynb
      4. Assert cell 2 output contains "42"
    Expected Result: Execution + cross-cell state works
    Evidence: .sisyphus/evidence/task-129b-exec-state.html
  ```

  **Commit**: `feat(jupyter): execution engine with cross-cell state persistence`

- [ ] **T129c: Rich display + introspection** [deep]

  **What to do**:
  - Render Buff matrices/vectors as HTML tables or images (GPU data → PNG via `display_data`)
  - `?<name>` help and `??<name>` source inspection
  - MIME bundle output (text/plain + text/html + image/png where applicable)

  **Leverages**: T129b (execution). Buff's GPU compute output. Jupyter `display_data` protocol.

  **Must NOT do**: Interactive widgets (ipywidgets). Long-running kernel timeout (document limitation).

  **References**: T129b, Jupyter `display_data` MIME spec

  **Acceptance Criteria**:
  - [ ] Matrix renders as HTML table or image
  - [ ] `?name` shows help, `??name` shows source
  - [ ] MIME bundle includes text fallback

  **QA Scenarios**:
  ```
  Scenario: Matrix displays as rich output
    Tool: Bash (nbconvert)
    Steps:
      1. Cell: create a 3x3 matrix and display it
      2. nbconvert --execute --to html
      3. Assert output HTML contains a table or img tag
    Expected Result: Rich matrix display
    Evidence: .sisyphus/evidence/task-129c-rich-display.html
  ```

  **Commit**: `feat(jupyter): rich matrix display and ?/?? introspection`

### v1.8 "Web/frontend foundations" — Wrap Dioxus + Dev server + Tauri

> **De-risking already done in v1.3 (T121b)**: The Dioxus feasibility spike ran back in v1.3 as a bindgen-capability test. Before starting T130, READ `.sisyphus/decisions/dioxus-feasibility.md`. If that verdict was FAIL/PARTIAL and not yet resolved, STOP and replan — do not build on an unproven foundation.

- [ ] **T130: Wrap Dioxus core (production, builds on v1.3 PoC)** [deep]

  **What to do**:
  - Precondition: T121b (v1.3) verdict is PASS (or PARTIAL-resolved). Build on that proof, not from scratch.
  - Build the production Dioxus wrapper crate `crates/buff-ui-dioxus/`
  - Expose: component definition, RSX-equivalent (string template or function call — full RSX-for-Buff syntax is v1.9/T133), state (signals), event handlers
  - Harden the "Hello World" PoC into a maintained example + test

  **Leverages**: **T121b PoC (v1.3) — the proven feasibility base**. Dioxus (MIT). T119 extern/bindgen. T114 prerequisite (Wasm, post-v1.0). `web-sys`.

  **Must NOT do**: Fork Dioxus. Full component library. Theming/design system. RSX-for-Buff syntax (that's v1.9). Re-run the feasibility spike (done in T121b).

  **References**: `.sisyphus/decisions/dioxus-feasibility.md` (T121b verdict), `https://github.com/dioxuslabs/dioxus`, T119 bindgen output, `web-sys` docs.

  **Acceptance Criteria**:
  - [ ] `crates/buff-ui-dioxus/` production wrapper crate exists
  - [ ] Component definition + signals (state) + event handlers exposed
  - [ ] "Hello World" example renders in headless browser (Playwright), kept as a regression test
  - [ ] State (signals) mutation from Buff triggers re-render

  **QA Scenarios**:
  ```
  Scenario: Production Dioxus wrapper renders + reacts
    Tool: Playwright (headless browser)
    Preconditions: T121b verdict PASS; buff-ui-dioxus crate built
    Steps:
      1. Build buff-ui-dioxus example for wasm32
      2. Serve in headless browser
      3. Assert page contains "Hello World"
      4. Trigger a signal update from a button; assert DOM reflects new value
    Expected Result: Buff drives Dioxus rendering AND reactive updates
    Evidence: .sisyphus/evidence/task-130-dioxus-render.png

  Scenario: Guard against unproven foundation
    Tool: Bash
    Steps:
      1. Read .sisyphus/decisions/dioxus-feasibility.md
      2. Assert verdict is PASS or PARTIAL-resolved before proceeding
    Expected Result: T130 only proceeds on a proven base
    Evidence: .sisyphus/evidence/task-130-precondition.txt
  ```

  **Commit**: `feat(ui): wrap Dioxus core for Buff with browser-rendering example`

- [ ] **T131: buff ui dev (hot reload server)** [unspecified-high]

  **What to do**:
  - `buff ui dev` starts a dev server (like Vite/trunk/cargo-leptos)
  - Watches `.buff` files, recompiles Wasm on change (debounced 200ms)
  - WebSocket HMR: pushes updates to browser without full reload
  - Serves static assets + Wasm bundle
  - Source maps for debugging

  **Leverages**: `notify` crate (file watching). `tokio-tungstenite` (WebSocket). `wasm-pack` (Wasm build). trunk/cargo-leptos architecture as reference.

  **Must NOT do**: Production bundler (that's `buff ui build`, future). CSS preprocessor. JS bundler.

  **References**: `trunk` crate source, `cargo-leptos` source (hot reload patterns)

  **Acceptance Criteria**:
  - [ ] `buff ui dev` starts server on localhost:8080
  - [ ] Saving .buff file triggers recompile within 300ms
  - [ ] Browser updates without full page reload
  - [ ] Errors shown in browser overlay

  **QA Scenarios**:
  ```
  Scenario: Hot reload on save
    Tool: Playwright + file watch
    Steps:
      1. Start buff ui dev
      2. Open browser to localhost:8080
      3. Modify .buff source (change "Hello" to "World")
      4. Wait 500ms
      5. Assert browser content updated to "World" (no manual refresh)
    Expected Result: Hot reload works
    Evidence: .sisyphus/evidence/task-131-hot-reload.png
  ```

  **Commit**: `feat(ui): buff ui dev hot-reload server`

- [ ] **T132: Tauri scaffolding** [quick]

  **What to do**:
  - `buff ui new --desktop` scaffolds a Tauri app project
  - Buff backend logic + Buff-Wasm-Dioxus frontend in Tauri webview
  - Cross-platform: builds for Win/macOS/Linux via Tauri's tooling
  - Template includes: window config, IPC bridge, buff.toml for Tauri

  **Leverages**: Tauri 2.0 (production-ready, 80k+ stars, mobile since Oct 2024). T130 Dioxus wrap. T114 prerequisite (Wasm, post-v1.0).

  **Must NOT do**: Mobile build (iOS/Android) in v1.8 — that's v1.9. Custom windowing.

  **References**: Tauri 2.0 docs, T130 output

  **Acceptance Criteria**:
  - [ ] `buff ui new --desktop my-app` scaffolds Tauri project
  - [ ] `buff ui build --desktop` produces native desktop binary
  - [ ] App opens window with Buff-Wasm-Dioxus frontend
  - [ ] Works on Win/macOS/Linux

  **QA Scenarios**:
  ```
  Scenario: Desktop app builds and runs
    Tool: Bash
    Steps:
      1. buff ui new --desktop test-app
      2. cd test-app && buff ui build --desktop
      3. Run produced binary
      4. Assert window opens showing "Hello World"
    Expected Result: Native desktop window renders Buff UI
    Evidence: .sisyphus/evidence/task-132-desktop-app.png
  ```

  **Commit**: `feat(ui): Tauri desktop scaffolding`

---

### v1.9 "Full UI framework" — RSX-for-Buff + Component model ⚠️ LANGUAGE CHANGE

> **⚠️ CRITICAL**: This release TOUCHES THE COMPILER (parser/lexer/AST extension for RSX-for-Buff). This is the ONE documented exception to the "no language changes in tooling releases" guardrail. All other releases (v1.1-v1.8, v1.10-v1.12) build ON the frozen v1.0 compiler.
>
> **Design decision needed before starting**: RSX-for-Buff syntax. Options:
> - (A) Embedded HTML in Buff (like JSX/Razor): `render: <div>{count}</div>`
> - (B) Pure Buff DSL (indentation-based tags): `render: div: {count}`
> - (C) Separate template files (like Vue/Svelte)
>
> **Recommendation**: Resolve syntax via a design spike BEFORE T133. This is the load-bearing decision.

- [ ] **T133: RSX-for-Buff parser/lexer extension** [ultrabrain]

  **What to do**:
  - Extend lexer to recognize markup tokens (`<`, `>`, `/`, attributes)
  - Extend parser to parse RSX expressions embedded in Buff code
  - Add AST nodes for elements, attributes, children, expressions
  - Extend codegen-rust to lower RSX → Dioxus RSX macro calls (or direct DOM)
  - Update tree-sitter grammar (T115) to highlight RSX
  - Update LSP (T117) to provide completion/diagnostics in RSX context
  - Snapshot tests for round-trip fidelity

  **Leverages**: JSX grammar (Babel). Dioxus RSX macro. T57 lossless AST (preserves RSX trivia).

  **Must NOT do**: Change the 25 keywords. Break v1.0 code (RSX is additive only).

  **References**: `crates/buff-lang-lexer/`, `crates/buff-lang-parser/`, `crates/buff-lang-ast/`, `crates/buff-lang-codegen-rust/`. Dioxus RSX macro source.

  **Acceptance Criteria**:
  - [ ] RSX syntax parses without breaking existing Buff code
  - [ ] RSX AST nodes added, snapshot tests stable
  - [ ] Codegen produces correct Dioxus calls
  - [ ] tree-sitter highlights RSX
  - [ ] LSP provides diagnostics in RSX blocks
  - [ ] v1.0 fixture program compiles unchanged (backward compat)

  **QA Scenarios**:
  ```
  Scenario: RSX renders
    Tool: Playwright
    Steps:
      1. Write component with RSX: render: <div class="greeting">{message}</div>
      2. Build for wasm32
      3. Assert browser shows div with class "greeting" containing message value
    Expected Result: RSX compiles and renders
    Evidence: .sisyphus/evidence/task-133-rsx-render.png

  Scenario: Backward compat — v1.0 code still compiles
    Tool: Bash
    Steps:
      1. Compile tests/fixtures/v10-compat.buff
      2. Assert exit 0, output unchanged
    Expected Result: No regression from RSX addition
    Evidence: .sisyphus/evidence/task-133-backward-compat.txt
  ```

  **Commit**: `feat(ui): RSX-for-Buff parser/lexer/codegen extension`

- [ ] **T134: Component model + data binding + lifecycle** [deep]

  **What to do**:
  - `@component` **attribute** (NOT a keyword — attributes don't require reserved keywords) for declaring UI components
  - Props: typed inputs to components (function parameters)
  - State: via stdlib `signal()` function call (e.g., `let count = signal(0)`) — NOT a `state` keyword. Reactive updates via `.set()` / `.get()` patterns lowered to Dioxus signals.
  - Data binding: one-way `{expr}` and two-way `{bind:var}`
  - Event handlers: `@click`, `@input`, `@submit` (attributes, not keywords)
  - Lifecycle hooks: `on_init`, `on_render`, `on_destroy` (regular function names passed to Dioxus, not keywords)
  - Conditional rendering: `if` in RSX context (existing keyword, reused)
  - List rendering: `for` in RSX context (existing keyword, reused)

  > **⚠️ PREREQUISITE — v1.0 attribute system**: `@component`, `@click`, `@input` all depend on the attribute parsing system shipped in v1.0 (T49 `@prefer` — done at commit 24043de). T84 `@test.parametrize` was deferred to v2.0. If v1.0 attribute parsing is insufficient for UI needs, T134 is **blocked** — declare additional attribute requirements explicitly. No attribute system → no `@component` → T134 cannot proceed without introducing new keywords (which violates the guardrail).

  > **Guardrail**: Do NOT add new reserved keywords (e.g., `component`, `state`, `signal`). Use the attribute system (`@component`, `@click`) and regular stdlib function calls (`signal()`). The 25 keywords must remain unchanged even in v1.9 — RSX syntax is the only addition.

  **Leverages**: Dioxus component model. React/Blazor patterns (well-understood).

  **References**: T133 RSX AST. Dioxus component docs.

  **Acceptance Criteria**:
  - [ ] Components declared with props
  - [ ] State updates trigger re-render
  - [ ] Two-way binding works (input → state → input)
  - [ ] Event handlers fire
  - [ ] Lifecycle hooks called at correct times

  **QA Scenarios**:
  ```
  Scenario: Counter component
    Tool: Playwright
    Steps:
      1. Build counter app: state count, button @click increments, display {count}
      2. Open in browser
      3. Click button 3 times
      4. Assert display shows "3"
    Expected Result: Reactive state + event handling work
    Evidence: .sisyphus/evidence/task-134-counter.png
  ```

  **Commit**: `feat(ui): component model with state, binding, events, lifecycle`

- [ ] **T135: SSR + mobile (stretch)** [unspecified-high]

  **What to do**:
  - SSR: render Buff UI to HTML string on server (Node/Deno + Buff-Wasm)
  - Hydration: client-side Wasm attaches to SSR HTML
  - Mobile: Tauri 2.0 mobile builds (iOS/Android)
  - Document hydration mismatch handling

  **Leverages**: Tauri 2.0 mobile (stable since Oct 2024). Dioxus SSR. Leptos hydration pattern.

  **References**: T132 Tauri scaffolding. Tauri 2.0 mobile docs.

  **Acceptance Criteria**:
  - [ ] SSR produces valid HTML
  - [ ] Hydration attaches without losing state
  - [ ] iOS build produces .app
  - [ ] Android build produces .apk

  **QA Scenarios**:
  ```
  Scenario: SSR + hydration
    Tool: Playwright
    Steps:
      1. Build SSR version
      2. Fetch page, assert HTML contains rendered content (not empty div)
      3. Load in browser, assert interactive (button click works)
    Expected Result: SSR with working hydration
    Evidence: .sisyphus/evidence/task-135-ssr-hydration.png
  ```

  **Commit**: `feat(ui): SSR with hydration and Tauri mobile builds`

### v1.10 "Production hardening" — Debugger + Coverage

- [ ] **T136: Debugger (DAP)** [deep]

  **What to do**:
  - Create `crates/buff-dap/` crate implementing Debug Adapter Protocol
  - Use `dap` crate or rust-analyzer's debug adapter patterns
  - Map Buff source → Rust debuginfo → binary breakpoints (use T60 source maps, v1.0)
  - Implement: set breakpoint, step over/in/out, continue, inspect locals, stack trace
  - `buff debug` launches the DAP server, editors connect automatically
  - Document limitation: GPU shader code cannot be stepped (out of scope)

  **Leverages**: `debug-adapter` protocol spec. T60 source maps (v1.0). rustc debuginfo (already in binary). `lldb`/`gdb` work on binary already — DAP wraps the mapping.

  **Must NOT do**: GPU kernel debugging. Watch expressions (v2.0). Hot reload while debugging. Reverse debugging.

  **References**: DAP spec (`microsoft.github.io/debug-adapter-protocol/`). `crates/buff-lang-error/src/source_map.rs`. rust-analyzer's debug adapter code.

  **Acceptance Criteria**:
  - [ ] Set breakpoint on Buff line → binary pauses there
  - [ ] Step over works (next Buff statement)
  - [ ] Local variables visible with correct values
  - [ ] Stack trace shows Buff frames (not raw Rust)
  - [ ] Works with VSCode debugger UI

  **QA Scenarios**:
  ```
  Scenario: Breakpoint and inspect
    Tool: debug-adapter test harness (or VSCode test)
    Steps:
      1. Open examples/fibonacci.buff
      2. Set breakpoint on "return fibonacci(n-1) + fibonacci(n-2)"
      3. Start debugging with input fibonacci(5)
      4. Assert breakpoint hits
      5. Inspect local "n" → assert value is correct on each hit
      6. Continue → assert hits again (recursion)
    Expected Result: Breakpoint, stepping, locals all work
    Evidence: .sisyphus/evidence/task-136-debugger.txt
  ```

  **Commit**: `feat(dap): debug adapter with Buff-to-binary source mapping`

- [ ] **T137: Coverage tooling** [unspecified-high]

  **What to do**:
  - `buff coverage` wraps `llvm-cov` or `tarpaulin` (Rust coverage tools)
  - Map coverage data (Rust lines) back to Buff source lines (T60 source maps)
  - Generate LCOV/HTML coverage report for .buff files
  - `buff coverage --html` opens HTML report

  **Leverages**: `cargo-llvm-cov` or `tarpaulin`. T60 source maps.

  **Must NOT do**: Branch coverage, MC/DC. GPU shader coverage (documented out of scope).

  **References**: `cargo-llvm-cov` docs, `tarpaulin` docs, T60 source maps

  **Acceptance Criteria**:
  - [ ] `buff coverage` runs tests and collects coverage
  - [ ] Report shows line-level coverage for .buff files
  - [ ] HTML report renders correctly
  - [ ] Coverage % matches llvm-cov on equivalent Rust

  **QA Scenarios**:
  ```
  Scenario: Coverage report
    Tool: Bash
    Steps:
      1. In a project with tests
      2. buff coverage --html
      3. Open generated HTML
      4. Assert: covered lines highlighted green, uncovered red
      5. Assert coverage % reported
    Expected Result: Source-mapped coverage report
    Evidence: .sisyphus/evidence/task-137-coverage.png
  ```

  **Commit**: `feat(coverage): buff coverage with source-mapped reports`

---

### v1.11 "Education" — Bufflings

- [ ] **T138a: bufflings CLI + exercise runner** [writing]

  **What to do**:
  - Create `bufflings` CLI (clone Rustlings UX): `list`, `start <ex>`, `verify <ex>`, `progress`, `watch`
  - Exercise directory structure (`exercises/<topic>/<name>.buff` with TODO markers)
  - `watch` mode: auto-verify on file save (like Rustlings)
  - Progress tracking persisted to `~/.bufflings/`

  **Leverages**: Rustlings (open source, proven structure/UX). `notify` crate (watch mode, same as T131).

  **Must NOT do**: Exercise content (T138b), CI gate (T138c). Video/web tutorial. LLM grading.

  **References**: `https://github.com/rust-lang/rustlings` (CLI structure). T125c (file eval, reusable).

  **Acceptance Criteria**:
  - [ ] `bufflings list` shows exercises with status
  - [ ] `bufflings start <ex>` opens/prepares an exercise
  - [ ] `bufflings watch` re-verifies on save
  - [ ] Progress persisted to `~/.bufflings/`

  **QA Scenarios**:
  ```
  Scenario: CLI workflow
    Tool: Bash
    Steps:
      1. bufflings list → assert exercises listed with [pending]
      2. bufflings start basics_01 → assert exercise ready
      3. bufflings progress → assert basics_01 shows pending
    Expected Result: CLI navigation works
    Evidence: .sisyphus/evidence/task-138a-cli.txt
  ```

  **Commit**: `feat(bufflings): CLI with list/start/verify/progress/watch`

- [ ] **T138b: Exercise content (25 exercises)** [writing]

  **What to do**:
  - Write 25 exercises across 12 topics: basics, functions, types, control flow, structs, enums, traits, pattern matching, error handling, async, collections, generics/modules
  - Each: `.buff` file with TODO(s), `README.md` explaining the concept, hidden solution file
  - Progressive difficulty within each topic

  **Leverages**: T121 example library (content source). Existing `examples/`. Rustlings exercise design.

  **Must NOT do**: CLI/runner (T138a), CI gate (T138c).

  **References**: T121 (`examples/rust-vs-buff/`), Rustlings exercises

  **Acceptance Criteria**:
  - [ ] 25 exercises across 12 topics
  - [ ] Each has TODO(s) + README + hidden solution
  - [ ] Solutions produce correct output when applied

  **QA Scenarios**:
  ```
  Scenario: Solve an exercise
    Tool: Bash
    Steps:
      1. bufflings start basics_01
      2. Apply the hidden solution
      3. bufflings verify basics_01
      4. Assert: "Exercise solved!" and marked complete
    Expected Result: Exercise + solution work end-to-end
    Evidence: .sisyphus/evidence/task-138b-solve.txt
  ```

  **Commit**: `docs(bufflings): 25 exercises across 12 topics with solutions`

- [ ] **T138c: Verification engine + CI solvability gate** [writing]

  **What to do**:
  - Verification: run exercise `.buff` through compiler, check against expected output/tests
  - "Not done yet" detection (TODO markers or failing check) vs "solved"
  - CI job: apply every hidden solution, run `bufflings verify --all` → all 25 must pass
  - Guards against shipping an unsolvable exercise

  **Leverages**: T138a (CLI), T138b (content). Compiler pipeline for verification.

  **Must NOT do**: LLM grading. Partial credit.

  **References**: T138a, T138b, `crates/buff-lang-cli/src/pipeline.rs:46`

  **Acceptance Criteria**:
  - [ ] `bufflings verify <ex>` correctly detects solved vs unsolved
  - [ ] `bufflings verify --all` with solutions → all 25 pass
  - [ ] CI job fails if any exercise is unsolvable

  **QA Scenarios**:
  ```
  Scenario: CI verifies all solvable
    Tool: Bash
    Steps:
      1. Apply all hidden solutions
      2. Run: bufflings verify --all
      3. Assert all 25 pass, exit 0
      4. Corrupt one solution → assert verify --all fails
    Expected Result: CI gate catches unsolvable exercises
    Evidence: .sisyphus/evidence/task-138c-ci-gate.txt
  ```

  **Commit**: `feat(bufflings): verification engine and CI solvability gate`

---

### v1.12 "Distribution scale" — buffup + CI Action + Docker

- [ ] **T139: buffup version manager** [unspecified-high]

  **What to do**:
  - `buffup` CLI for installing/switching Buff versions
  - `buffup install <version>` — downloads pre-built binary or builds from source
  - `buffup default <version>` — sets active version
  - `buffup list` — shows installed versions
  - `buffup update` — self-updates buffup
  - Manages `~/.buff/` directory with symlinks to active version
  - Add `~/.buff/bin` to PATH instructions

  **Leverages**: `rustup` (architecture reference, NOT fork). Pre-built binary releases from GitHub Releases.

  **Must NOT do**: Per-directory overrides (`.buff-version` file). Components (rustup-style). Nightly channel (just versions). Toolchain manifests.

  **References**: rustup source (architecture only). GitHub Releases API.

  **Acceptance Criteria**:
  - [ ] `buffup install 1.0.0` downloads and installs
  - [ ] `buffup default 1.0.0` switches active version
  - [ ] `buff --version` reports correct version after switch
  - [ ] `buffup list` shows installed versions
  - [ ] `buffup update` self-updates

  **QA Scenarios**:
  ```
  Scenario: Install and switch versions
    Tool: Bash
    Steps:
      1. buffup install 1.0.0
      2. buffup install 1.1.0
      3. buffup default 1.0.0
      4. buff --version → assert "1.0.0"
      5. buffup default 1.1.0
      6. buff --version → assert "1.1.0"
    Expected Result: Version switching works
    Evidence: .sisyphus/evidence/task-139-buffup.txt
  ```

  **Commit**: `feat(buffup): version manager for installing and switching Buff versions`

- [ ] **T140: setup-buff GitHub Action** [quick]

  **What to do**:
  - Create `setup-buff` GitHub Action (TypeScript or composite)
  - Inputs: `buff-version` (default: latest), `buffup-version` (default: latest)
  - Installs buffup, then buff, caches for subsequent steps
  - Publish to GitHub Actions Marketplace
  - Example workflow in README

  **Leverages**: `actions/setup-node`, `actions-rust-lang/setup-rust-toolchain` (pattern reference). T139 buffup.

  **Must NOT do**: GPU runner setup. Custom runner images.

  **References**: `setup-buff` action repo. GitHub Actions docs.

  **Acceptance Criteria**:
  - [ ] Action published to Marketplace
  - [ ] `uses: buff-lang/setup-buff@v1` installs buff in CI
  - [ ] Subsequent `buff` commands work
  - [ ] Caching works (faster second run)

  **QA Scenarios**:
  ```
  Scenario: Action installs buff
    Tool: GitHub Actions (test workflow)
    Steps:
      1. Create workflow: uses: buff-lang/setup-buff@v1
      2. Run: buff --version
      3. Assert output contains version
    Expected Result: Action works in CI
    Evidence: .sisyphus/evidence/task-140-action.txt (workflow run URL/log)
  ```

  **Commit**: `feat(ci): setup-buff GitHub Action`

- [ ] **T141: Docker images** [quick]

  **What to do**:
  - `buff:builder` image: Rust toolchain + Buff CLI (for CI builds)
  - `buff:slim` image: minimal base for running Buff-built binaries (no compiler)
  - Multi-stage builds: builder → slim for small final images
  - Non-root user, multi-arch (amd64, arm64)
  - Publish to Docker Hub / GitHub Container Registry
  - Example Dockerfile in docs

  **Leverages**: Official Rust Docker images (base). Multi-arch builds via `docker buildx`.

  **Must NOT do**: GPU-driver images (users add their own). "Runtime VM" (Buff compiles to native, no VM).

  **References**: `rust:slim` Docker image. Docker buildx docs.

  **Acceptance Criteria**:
  - [ ] `buff:builder` image works for `buff build`
  - [ ] `buff:slim` image runs Buff-built binaries
  - [ ] Images <200MB (slim)
  - [ ] Multi-arch (amd64 + arm64)
  - [ ] Published to container registry

  **QA Scenarios**:
  ```
  Scenario: Build in Docker
    Tool: Bash
    Steps:
      1. docker run --rm -v $(pwd):/app buff:builder buff build
      2. Assert binary produced
    Expected Result: Docker build works
    Evidence: .sisyphus/evidence/task-141-docker-build.txt

  Scenario: Multi-stage slim image
    Tool: Bash
    Steps:
      1. Multi-stage Dockerfile: builder → slim
      2. docker build
      3. Assert final image <200MB
      4. docker run → assert binary executes
    Expected Result: Slim image runs the app
    Evidence: .sisyphus/evidence/task-141-docker-slim.txt
  ```

  **Commit**: `feat(docker): builder and slim images with multi-arch support`

---

## Final Verification Wave (MANDATORY — after ALL releases)

> 4 review agents in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] **F1. Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in `.sisyphus/evidence/`. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] **F2. Code Quality Review** — `unspecified-high`
  Run `cargo clippy --workspace -- -D warnings` + `cargo test --workspace`. Review all new crates for: `as any`/`@ts-ignore` equivalents, empty catches, unwrap in non-test, unused imports. Check AI slop: excessive comments, over-abstraction, generic names.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] **F3. Real Manual QA** — `unspecified-high` (+ `playwright` skill if UI)
  Start from clean state. Execute EVERY QA scenario from EVERY task. Test cross-release integration. Test edge cases: empty state, invalid input, rapid actions. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] **F4. Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — everything in spec was built, nothing beyond spec. Check "Must NOT do" compliance. Detect cross-task contamination.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | VERDICT`

---

## Commit Strategy

- **Branches**: `v1.x-dev` per release with feature branches per task
- **Pattern**: `test(scope): ...` [RED] → `feat(scope): ...` [GREEN] → `refactor(scope): ...` (optional)
- **Tags**: `v1.1.0`, `v1.2.0`, ... `v1.12.0` (after each release + its exit criteria)
- **Per-release exit**: semver bump + changelog + README update + git tag + backward-compat regression test + clippy-clean across expanded workspace

---

## Success Criteria

### Phase 1 Success (Adoption)
```bash
# A Rust dev's 30-minute journey works end-to-end:
cargo install buff-cli                                    # install
buff run examples/fibonacci.buff                         # try locally (example from v0.1)
# Open playground in browser → type .buff → see .rs      # try online
code example.buff                                         # VSCode opens, highlighting works
# Hover shows types, completion works, diagnostics show  # LSP works
buff build --release                                      # ships binary
```

### Phase 2 Success (Expansion)
- Buff has tooling to attack ≥3 Rust-weak markets (web, data science, scripting)
- Package registry operational with ≥1 third-party library published
- At least one Buff UI app running in browser

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass across expanded workspace
- [ ] Backward-compat regression passes (v1.0 program compiles unchanged)
- [ ] Every release tagged with semver + changelog
