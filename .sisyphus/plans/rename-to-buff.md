# Rename Project: Deox → Buff

## TL;DR

> **Quick Summary**: Mechanically rename the entire "Deox" programming language project to "Buff" — all crates, source identifiers, file extensions, docs, planning artifacts, binary name, and the git repo. Behavior-preserving refactor verified by the existing test suite + a hard residual-`deox` grep audit.
>
> **Deliverables**:
> - 9 crates renamed `deox-*` → `buff-lang-*` (dirs via `git mv`, Cargo.tomls, module names)
> - All source identifiers renamed (`DeoxError`→`BuffError`, `deox_to_rust`→`buff_to_rust`, `__deox_tmp_`→`__buff_tmp_`, all `use deox_*`)
> - Binary `deox` → `buff`; CLI clap attrs + help text updated
> - File extension `.deox` → `.buff` (examples, fixtures, string-literal path refs)
> - Coupled `error_mapper.rs` codegen logic + test assertions updated together
> - README.md rewritten (incl. ASCII architecture diagram manually realigned)
> - All `.sisyphus/` plan/notepad files renamed + 20+ internal links repaired + `boulder.json` updated
> - Insta snapshots regenerated; `Cargo.lock` regenerated
> - GitHub repo renamed + git remote updated + local folder renamed
>
> **Estimated Effort**: Medium
> **Parallel Execution**: YES - 3 waves (Wave 1 has 3 parallel tasks; Waves 2-4 are the sequential critical path)
> **Critical Path**: T3 (atomic code core) → T4 (extensions) → T5 (snapshots) → T6 (verify) → F1-F4 (reviews)

---

## Context

### Original Request
Rename the project from "Deox" to a new name. After brainstorming + availability verification, the chosen name is **Buff** (mascot: Buff the Crab with a buffing wheel, polishing Rust smooth — a continuation of the "Deoxidizer" concept).

### Interview Summary
**Key Discussions**:
- **Name choice**: Explored chemistry, biological, refining, and mascot-driven angles. User chose **Buff** for its approachable vibe, strong "polishing/smoothing Rust" metaphor, and mascot potential.
- **Crate prefix**: Bare `buff` is taken on crates.io ("Traits for buffer"); confirmed `buff-lang-*` prefix (→ modules `buff_lang_*`).
- **File extension**: `.buff` (NOT `.bf` — Brainfuck conflict).
- **Domain**: `buff.rs` (free, thematic).
- **Scope**: FULL rename — everything. Backward compat: NONE (pre-release, clean break).

**Research Findings**:
- Availability VERIFIED: `buff-lang`, `bufflang`, `buff-cli` free on crates.io; GitHub org `buff-lang` free (404); `buff.rs` + `buff-lang.dev` domains free; `buff.dev` parked/premium; `.bf` = Brainfuck.
- Codebase: 73 files reference "deox"; 9-crate Rust workspace; no ambiguous references; no CI/build-script/env-var/docker/mascot-image concerns.
- Special identifiers: `__deox_tmp_N` (codegen temp vars), `DeoxError` enum, `deox_to_rust` (source_map.rs field/method), coupled `error_mapper.rs` logic+assertions.

### Metis Review
**Identified Gaps** (addressed):
- `bufflang` vs `buff-lang` contradiction in draft → RESOLVED: `buff-lang-*` confirmed by user; draft fixed.
- `.sisyphus` filename rename policy → RESOLVED: user chose "rename files + repair links".
- Evidence files policy → DEFAULT APPLIED: leave `.sisyphus/evidence/*.txt` untouched (historical record).
- Repo casing → DEFAULT APPLIED: local `Buff`, GitHub `buff-lang/buff`.
- `deox_to_rust` identifier → DEFAULT APPLIED: rename to `buff_to_rust` for consistency.
- README ASCII diagram width change → DIRECTIVE: manual realignment task, not find-replace.
- Atomicity constraint → DIRECTIVE: crate-name + path-dep + `use`-statement renames are ONE atomic commit (cargo won't compile at intermediate states).
- Snapshot regeneration → DIRECTIVE: use `cargo insta test --accept`, never hand-edit `.snap`.

---

## Work Objectives

### Core Objective
Transform every "Deox" reference in the project to "Buff" (per the identifier map), preserving all behavior, such that the existing test suite passes and zero unintended "deox" references remain (excluding git history + evidence files).

### Concrete Deliverables
- Working `buff` binary: `buff run examples/ola.buff` prints `Olá, Buff!`
- All crates named `buff-lang-*`, importable as `buff_lang_*`
- Zero residual `deox` in source/configs/docs (allowlisted: git log, evidence files)

### Definition of Done
- [ ] `cargo build --workspace` exits 0
- [ ] `cargo test --workspace` exits 0 (all pass)
- [ ] `./target/debug/buff run examples/ola.buff` prints `Olá, Buff!`
- [ ] `cargo metadata --no-deps` shows all packages named `buff-lang-*`, one bin target `buff`
- [ ] `git grep -iE "deox"` (excluding target/, Cargo.lock, *.snap, .sisyphus/evidence/, git history) returns 0 matches

### Must Have
- Every crate, module, identifier, file, doc renamed per the mapping table
- `git mv` used for ALL file/directory/extension renames (history preservation)
- Crate-name + path-dep + `use`-statement renames land in ONE atomic commit
- `error_mapper.rs` codegen logic AND its test assertions updated together
- `boulder.json` updated atomically with `.sisyphus` plan-file renames
- README ASCII diagram manually realigned (width-aware)
- Snapshots regenerated via `cargo insta test --accept`

### Must NOT Have (Guardrails)
- **NO** rewriting git history or commit messages (`b18e456 chore: initialize deox...` stays forever)
- **NO** modifying `.sisyphus/evidence/*.txt` (historical timestamped records)
- **NO** adding `[package]` metadata fields (repository, homepage, keywords) — they don't exist today; separate task
- **NO** creating CHANGELOG.md / CONTRIBUTING.md / SECURITY.md / badges / mascot images
- **NO** fixing unrelated clippy warnings or fmt issues discovered during rename (note them, don't fix)
- **NO** redesigning temp-var scheme, error enum structure, or any architecture
- **NO** changing language semantics, syntax, milestone numbering, or phase structure
- **NO** hand-editing `Cargo.lock` or `.snap` files (regenerate only)
- **NO** touching `target/` (build artifacts)
- **NO** "improving" doc comments — only substitute names
- **NO** naive find-replace on the README ASCII diagram block (manual realignment)

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** - ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test workspace, insta snapshots)
- **Automated tests**: Tests-after (this is a mechanical rename/refactor — existing tests are the safety net; no new tests needed)
- **Framework**: cargo test + cargo-insta

### QA Policy
Every task includes agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Build/test**: Use Bash (cargo) — build, test, assert exit codes + output
- **Binary/CLI**: Use Bash — run `buff --help`, `buff run`, assert stdout
- **Residual audit**: Use Bash (git grep) — assert zero matches outside allowlist
- **Metadata**: Use Bash (cargo metadata + jq/grep) — assert naming

### Residual-`deox` Audit Allowlist (explicit)
These matches are ACCEPTABLE and must not fail the audit:
- `git log` output (commit history — never rewritten)
- `.sisyphus/evidence/*.txt` (historical evidence — never modified)
- The rename plan itself if it documents the old name for reference

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - 3 independent parallel tasks, no file overlap):
├── T1: .sisyphus plan/notepad file renames + link repair + boulder.json [deep]
├── T2: README.md rewrite + ASCII diagram realignment [deep]
└── T3: Atomic code core: crates + Cargo.tomls + all source identifiers [deep]
        (T3 is the critical path — the non-subdivisible compilation unit)

Wave 2 (After T3 - file extensions):
└── T4: .deox → .buff extension rename + fixture/example content + path strings [unspecified-high]

Wave 3 (After T4 - regeneration):
└── T5: Insta snapshot regeneration + Cargo.lock regen + snapshots/README.md [quick]

Wave FINAL (After T3, T4, T5 - verification):
└── T6: Full verification suite + residual audit [deep]
    Then: F1-F4 review agents run in PARALLEL

Wave EXTERNAL (Manual, after T6 - documented, requires user action):
└── T7: GitHub repo rename + git remote URL + local folder rename

Critical Path: T3 → T4 → T5 → T6 → F1-F4 → user okay
Parallel Speedup: Wave 1 runs 3 tasks concurrently (~35% faster than sequential)
Max Concurrent: 3 (Wave 1)
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| T1 | — | T6 |
| T2 | — | T6 |
| T3 | — | T4, T6 |
| T4 | T3 | T5, T6 |
| T5 | T4 | T6 |
| T6 | T1, T2, T3, T4, T5 | F1-F4 |
| T7 | T6 | — (external/manual) |
| F1-F4 | T6 | user okay |

### Agent Dispatch Summary

- **Wave 1**: T1 → `deep`, T2 → `deep`, T3 → `deep`
- **Wave 2**: T4 → `unspecified-high`
- **Wave 3**: T5 → `quick`
- **Wave FINAL**: T6 → `deep`; F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`
- **External**: T7 → user manual action (documented)

---

## TODOs

> Implementation tasks below. EVERY task has QA Scenarios (mandatory).

### ⚙️ Identifier Mapping Reference (shared across all tasks)

> **ALL tasks MUST apply this mapping consistently.** Print this and keep it handy.

| Old (Deox) | New (Buff) | Context |
|------------|-----------|---------|
| `deox` | `buff` | Binary name, language name in prose, clap `name` attr |
| `deox-*` (crate dirs) | `buff-lang-*` | Directory names under `crates/` |
| `name = "deox-*"` | `name = "buff-lang-*"` | Cargo.toml `[package]` |
| `name = "deox"` (bin) | `name = "buff"` | Cargo.toml `[[bin]]` in deox-cli |
| `deox-* = { path = ... }` | `buff-lang-* = { path = ... }` | Root Cargo.toml workspace deps |
| `deox-*.workspace = true` | `buff-lang-*.workspace = true` | Per-crate Cargo.toml deps |
| `use deox_*::` | `use buff_lang_*::` | Rust source (hyphens→underscores) |
| `deox_ast`, `deox_lexer`, etc. | `buff_lang_ast`, `buff_lang_lexer`, etc. | Module/identifiers in source |
| `DeoxError` | `BuffError` | Error enum (`deox-error/src/diagnostic.rs:149`) |
| `deox_to_rust` | `buff_to_rust` | Field/method in `source_map.rs:128,156` |
| `__deox_tmp_` | `__buff_tmp_` | Codegen temp var prefix (`context.rs:44,47`) |
| `.deox` | `.buff` | File extension (examples, fixtures, path strings) |
| `"Olá, Deox!"` | `"Olá, Buff!"` | Example/fixture/test output |
| `Olá, Deox` (milestone) | `Olá, Buff` | v0.1 codename in docs |
| `deox-master.md` etc. | `buff-master.md` etc. | `.sisyphus/plans/` filenames |
| `notepads/deox-master/` | `notepads/buff-master/` | `.sisyphus/notepads/` dir |
| `plan_name: "deox-master"` | `plan_name: "buff-master"` | `boulder.json` |

**Crate rename map (9 crates):**
| Old dir | New dir |
|---------|---------|
| `crates/deox-ast` | `crates/buff-lang-ast` |
| `crates/deox-lexer` | `crates/buff-lang-lexer` |
| `crates/deox-parser` | `crates/buff-lang-parser` |
| `crates/deox-types` | `crates/buff-lang-types` |
| `crates/deox-codegen-rust` | `crates/buff-lang-codegen-rust` |
| `crates/deox-codegen-wgsl` | `crates/buff-lang-codegen-wgsl` |
| `crates/deox-runtime` | `crates/buff-lang-runtime` |
| `crates/deox-error` | `crates/buff-lang-error` |
| `crates/deox-cli` | `crates/buff-lang-cli` |

---

- [x] 1. **Rename .sisyphus plan/notepad files + repair internal links + update boulder.json**

  **What to do**:
  - Use `git mv` to rename these files (PRESERVES HISTORY — never delete+create):
    - `.sisyphus/plans/deox-master.md` → `.sisyphus/plans/buff-master.md`
    - `.sisyphus/plans/deox-v01-mvp.md` → `.sisyphus/plans/buff-v01-mvp.md`
    - `.sisyphus/plans/deox-v05-language.md` → `.sisyphus/plans/buff-v05-language.md`
    - `.sisyphus/plans/deox-v10-production.md` → `.sisyphus/plans/buff-v10-production.md`
    - `.sisyphus/plans/deox-numeric-system.md` → `.sisyphus/plans/buff-numeric-system.md`
    - `.sisyphus/plans/deox-conventions.md` → `.sisyphus/plans/buff-conventions.md`
    - `.sisyphus/plans/deox-project-structure.md` → `.sisyphus/plans/buff-project-structure.md`
    - `.sisyphus/notepads/deox-master/` → `.sisyphus/notepads/buff-master/` (entire dir)
  - Update `boulder.json`: change `active_plan` path AND `plan_name` from `deox-master` → `buff-master` (lines ~2, ~22).
  - Repair ALL internal markdown links across `.sisyphus/` files: every `[text](./deox-*.md)` → `[text](./buff-*.md)`, and `[text](./.sisyphus/plans/deox-*.md)` → `[text](./.sisyphus/plans/buff-*.md)`. There are 20+ such links (check README.md too, and cross-refs between plan files).
  - Update content inside each renamed file: `Deox`→`Buff`, `deox-*`→`buff-lang-*`, `deox_*`→`buff_lang_*`, `.deox`→`.buff`, `Olá, Deox`→`Olá, Buff`, `deox.toml`→`buff.toml`, `deox.lock`→`buff.lock` (future config names in project-structure plan).
  - **DO NOT touch** `.sisyphus/evidence/*.txt` (historical records — leave untouched).
  - **DO NOT touch** `.sisyphus/drafts/rename-deox.md` (Prometheus will delete it after planning).

  **Must NOT do**:
  - No rewriting git commit history
  - No modifying evidence files
  - No restructuring milestone numbering or phase structure (only rename text)
  - No creating new plan files

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Requires careful coordinated file moves + 20+ link repairs + content edits; mistakes break the Sisyphus orchestrator.
  - **Skills**: []
    - (No specialized skill overlap — pure file/text operations)
  - **Skills Evaluated but Omitted**:
    - `git-master`: Useful but the `git mv` operations are simple enough; the skill is heavier than needed.

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2, T3)
  - **Blocks**: T6 (verification)
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `.sisyphus/boulder.json` — Contains `active_plan` and `plan_name` fields pointing to `deox-master`. Must update atomically with the file rename or Sisyphus breaks.
  - `.sisyphus/plans/deox-master.md` — Master orchestrator doc; contains links to all other plan files. Will become `buff-master.md`.
  - `README.md:162,175,179` — Contains links to `.sisyphus/plans/deox-*.md` that must be repaired (these are in T2's scope for README, but T1 handles the .sisyphus-internal links).

  **WHY Each Reference Matters**:
  - `boulder.json`: The Sisyphus orchestrator reads this to find the active plan. If the file is renamed but boulder.json still says `deox-master`, the next `/start-work` fails. CRITICAL coupling.
  - Plan files cross-link heavily (e.g., deox-master.md links to deox-v01-mvp.md, deox-conventions.md, etc.). Every link must be updated to the new filename or links 404.

  **Acceptance Criteria**:
  - [ ] All 7 plan files + notepad dir renamed via `git mv` (verify: `git status` shows renames, not delete+add)
  - [ ] `boulder.json` `active_plan` and `plan_name` reference `buff-master`
  - [ ] `git grep -nE "\[.*\]\(\.?\.?/?\.sisyphus/plans/deox" -- .sisyphus/ README.md` → 0 matches (all links repaired)
  - [ ] `git grep -nE "\[.*\]\(\.\/deox-" -- .sisyphus/plans/` → 0 matches
  - [ ] Every renamed file still exists and is readable: `Test-Path .sisyphus/plans/buff-master.md` → True

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Sisyphus state integrity after rename
    Tool: Bash (PowerShell + git)
    Preconditions: All file renames + boulder.json edit complete
    Steps:
      1. Run: git status --short
         Assert: output shows "R  .sisyphus/plans/deox-master.md -> .sisyphus/plans/buff-master.md" (R = renamed, not D+A)
      2. Run: Get-Content .sisyphus/boulder.json | Select-String "deox"
         Assert: ZERO matches (boulder.json fully updated)
      3. Run: Get-Content .sisyphus/boulder.json | Select-String "buff-master"
         Assert: at least one match (active_plan points to buff-master)
      4. Run: Test-Path .sisyphus/plans/buff-master.md
         Assert: True
      5. Run: Test-Path .sisyphus/plans/deox-master.md
         Assert: False (old path gone)
    Expected Result: All 5 assertions pass.
    Failure Indicators: "D  " (deleted) instead of "R  " (renamed) in git status; boulder.json still references deox-master.
    Evidence: .sisyphus/evidence/task-1-sisyphus-integrity.txt

  Scenario: No broken internal markdown links
    Tool: Bash (git grep)
    Preconditions: All link repairs complete
    Steps:
      1. Run: git grep -nE "\(\.\/deox-.*\.md\)" -- .sisyphus/
         Assert: ZERO matches (no links point to old deox-*.md filenames)
      2. Run: git grep -nE "\(\.\/\.sisyphus/plans/deox-.*\.md\)" -- README.md
         Assert: ZERO matches
      3. Run: git grep -nE "\(\.\/\.sisyphus/plans/buff-.*\.md\)" -- README.md
         Assert: matches exist (links now point to buff-*.md)
    Expected Result: Zero stale deox-* link targets; new buff-* link targets present.
    Failure Indicators: Any remaining `(./deox-*.md)` link target.
    Evidence: .sisyphus/evidence/task-1-link-audit.txt
  ```

  **Commit**: YES (C5)
  - Message: `chore(rename): rename .sisyphus plan files + repair internal links`
  - Files: all `.sisyphus/plans/buff-*.md`, `.sisyphus/notepads/buff-master/`, `boulder.json`, cross-refs in README.md
  - Pre-commit: `Test-Path .sisyphus/plans/buff-master.md` returns True; `boulder.json` references buff-master

---

- [x] 2. **Rewrite README.md for Buff (incl. ASCII architecture diagram manual realignment)**

  **What to do**:
  - Update `README.md` content per the identifier mapping:
    - Line 1: `# Deox` → `# Buff`
    - Line 3: tagline `> **Deox** (Deoxidizer) — a high-level language...` → `> **Buff** — a high-level language that transpiles to Rust.` (drop "Deoxidizer" parenthetical or rephrase; the name IS Buff now)
    - Line 20: `**Deox exists to break that trilemma.**` → `**Buff exists to break that trilemma.**`
    - Lines 25-50: all `Deox` → `Buff`, `.deox` → `.buff`, `deox run` → `buff run`
    - Line 61: milestone `*Olá, Deox*` → `*Olá, Buff*`
    - Line 69: v0.1 exit criteria `deox run ola.deox prints "Olá, Deox!"` → `buff run ola.buff prints "Olá, Buff!"`
    - Line 77,82,87: `cargo install deox` → `cargo install buff-lang-cli` (or `buff`), `deox run examples/ola.deox` → `buff run examples/ola.buff`
    - Line 90: clone URL `https://github.com/vsbb1/Deox.git` → `https://github.com/buff-lang/buff.git`
    - Line 102: `examples/ola.deox` → `examples/ola.buff`
    - Lines 140-159: **ASCII architecture diagram — MANUAL REALIGNMENT** (see below)
    - Lines 162,175,179: update `.sisyphus/plans/deox-*.md` links → `.sisyphus/plans/buff-*.md` (coordinate with T1)
    - Line 175: `deox-v01-mvp.md` → `buff-v01-mvp.md`; Line 179: `deox-master.md` → `buff-master.md`
  - **ASCII diagram realignment (CRITICAL — do NOT find-replace)**:
    - Lines 137-159 contain a box-drawing diagram with crate names. `buff-lang-lexer` (16 chars) is WIDER than `deox-lexer` (10 chars). The `──▶`, `┌──┴──┐`, `│` alignment WILL BREAK under naive replacement.
    - Recompute all column widths and box sizes for the new names. Redraw the diagram so all box-drawing characters align.
    - Example transformation: if old was `┌─deox-lexer─┐` (12 chars inside), new is `┌─buff-lang-lexer─┐` (16 chars inside) — adjust surrounding connectors.
  - Update install/quickstart section to reflect `buff` binary and `.buff` extension.

  **Must NOT do**:
  - No adding badges, logos, or mascot images
  - No creating CHANGELOG/CONTRIBUTING sections
  - No restructuring the roadmap or milestone numbering
  - No find-replace on the ASCII diagram (manual redraw only)
  - No "improving" prose beyond name substitution

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: The ASCII diagram realignment requires careful spatial reasoning + width math, not bulk text ops.
  - **Skills**: []
    - (Pure markdown editing; no specialized skill needed)

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T3)
  - **Blocks**: T6
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `README.md` (203 lines) — The sole file. Read it fully before editing.
  - `README.md:137-159` — The ASCII architecture diagram. Measure exact character widths of each crate name before redrawing.

  **WHY Each Reference Matters**:
  - ASCII diagram: The ONLY place where find-replace fails. Width changes cascade through all box-drawing chars. Must be redrawn by measuring new name lengths.

  **Acceptance Criteria**:
  - [ ] `Select-String -Path README.md -Pattern "Deox" -CaseSensitive` → 0 matches
  - [ ] `Select-String -Path README.md -Pattern "\.deox" ` → 0 matches
  - [ ] `Select-String -Path README.md -Pattern "Olá, Buff"` → ≥1 match
  - [ ] `Select-String -Path README.md -Pattern "buff-lang/"` → matches (clone URL updated)
  - [ ] ASCII diagram: every line of the diagram has consistent width (box chars `│` align vertically) — verify by visual inspection in evidence screenshot or line-length check
  - [ ] All `.sisyphus/plans/deox-*.md` links in README updated to `buff-*.md`

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: README fully renamed, no residual Deox
    Tool: Bash (PowerShell Select-String)
    Preconditions: README.md edits complete
    Steps:
      1. Run: Select-String -Path README.md -Pattern "Deox" -CaseSensitive
         Assert: ZERO matches
      2. Run: Select-String -Path README.md -Pattern "\.deox"
         Assert: ZERO matches
      3. Run: Select-String -Path README.md -Pattern "buff-lang-"
         Assert: ≥9 matches (crate refs in diagram + text)
      4. Run: Select-String -Path README.md -Pattern "Olá, Buff"
         Assert: ≥1 match
      5. Run: Select-String -Path README.md -Pattern "github.com/buff-lang/buff"
         Assert: ≥1 match (clone URL)
    Expected Result: All 5 assertions pass.
    Failure Indicators: Any "Deox" or ".deox" residual; clone URL still says vsbb1/Deox.
    Evidence: .sisyphus/evidence/task-2-readme-audit.txt

  Scenario: ASCII diagram alignment integrity
    Tool: Bash (PowerShell — line inspection)
    Preconditions: Diagram redrawn
    Steps:
      1. Run: $lines = Get-Content README.md; $diagram = $lines[136..158]; $diagram | ForEach-Object { $_.Length }
         Assert: lines using box-drawing chars (│, ┌, ┐, └, ┘, ├, ┤, ─) have consistent lengths within each visual "row" of the diagram
      2. Visually inspect: capture the diagram lines into evidence file, confirm vertical bars │ align in columns
    Expected Result: Box-drawing characters form properly aligned rectangles/connectors.
    Failure Indicators: Jagged │ bars; ─▶ connectors that don't reach their target; mismatched box widths.
    Evidence: .sisyphus/evidence/task-2-ascii-diagram.txt
  ```

  **Commit**: YES (C4)
  - Message: `docs(rename): rewrite README for Buff`
  - Files: `README.md`
  - Pre-commit: `Select-String -Path README.md -Pattern "Deox"` returns nothing

---

- [x] 3. **ATOMIC CODE CORE: Rename crates, Cargo.tomls, all source identifiers (Deox→Buff)**

  > ⚠️ **THIS IS THE NON-SUBDIVISIBLE COMPILATION UNIT.** Cargo will NOT compile at intermediate states. The crate-dir rename, Cargo.toml name/path-dep edits, AND all `use` statement / identifier renames MUST land together before `cargo build` is attempted. Do NOT split across commits.

  **What to do** (execute in this order):

  **Step 3a — Rename crate directories via `git mv`** (preserves history):
  ```
  git mv crates/deox-ast           crates/buff-lang-ast
  git mv crates/deox-lexer         crates/buff-lang-lexer
  git mv crates/deox-parser        crates/buff-lang-parser
  git mv crates/deox-types         crates/buff-lang-types
  git mv crates/deox-codegen-rust  crates/buff-lang-codegen-rust
  git mv crates/deox-codegen-wgsl  crates/buff-lang-codegen-wgsl
  git mv crates/deox-runtime       crates/buff-lang-runtime
  git mv crates/deox-error         crates/buff-lang-error
  git mv crates/deox-cli           crates/buff-lang-cli
  ```

  **Step 3b — Update root `Cargo.toml`** (workspace deps section, ~lines 23-30):
  - Every `deox-* = { path = "crates/deox-*" }` → `buff-lang-* = { path = "crates/buff-lang-*" }`
  - The `[workspace].members = ["crates/*"]` glob auto-includes renamed dirs (no change needed, but verify).

  **Step 3c — Update each crate's `Cargo.toml`** (9 files):
  - `[package] name = "deox-*"` → `name = "buff-lang-*"`
  - In `deox-cli/Cargo.toml`: `[[bin]] name = "deox"` → `name = "buff"` (the executable)
  - Every `deox-*.workspace = true` dependency line → `buff-lang-*.workspace = true`
  - Update any `path = "../deox-*"` references → `path = "../buff-lang-*"`

  **Step 3d — Update ALL source identifiers** (59+ files across crates):
  - Every `use deox_*::...` → `use buff_lang_*::...` (Rust converts hyphens to underscores in module paths)
  - `deox_ast` → `buff_lang_ast`, `deox_lexer` → `buff_lang_lexer`, etc. (all 9 module prefixes)
  - `DeoxError` → `BuffError` (enum in `buff-lang-error/src/diagnostic.rs:149` — use `lsp_rename` for safety)
  - `deox_to_rust` → `buff_to_rust` (field + methods in `buff-lang-error/src/source_map.rs:128,156` — use `lsp_rename`)
  - `__deox_tmp_` → `__buff_tmp_` (codegen temp var prefix in `buff-lang-codegen-rust/src/context.rs:44,47`; also any doc comment referencing it)
  - clap attributes in `buff-lang-cli/src/cli.rs:18,20`: `#[command(name = "deox"...)]` → `name = "buff"`; `about = "Deox language compiler — transpiles .deox to Rust..."` → `about = "Buff language compiler — transpiles .buff to Rust..."` (NOTE: `.deox`→`.buff` here is part of the help string — update it now even though extension files are T4's scope; the STRING must be consistent)
  - Module doc comments in `buff-lang-cli/src/main.rs:1`: `Deox CLI — Command-line interface for the Deox language transpiler` → `Buff CLI — ...`; `deox_cli` → `buff_lang_cli`
  - Test path strings: every `PathBuf::from("test.deox")`, `"run_ola.deox"`, `"ola_fixture.deox"` → `.buff` equivalents (these are STRING LITERALS referencing file paths — coordinate with T4 which renames the actual files; the strings must match the new filenames)

  **Step 3e — COUPLED: `error_mapper.rs` logic + assertions together**:
  - `buff-lang-cli/src/error_mapper.rs` (lines ~188-267): The codegen emits `prog.deox:LINE:COL` in panic/error strings, and tests assert output contains `prog.deox:2:15`.
  - Update BOTH the codegen string format (`prog.deox` → `prog.buff`) AND every test assertion checking for it, IN THE SAME EDIT PASS. If you update one without the other, tests fail.

  **Step 3f — Verify build compiles**:
  - Run `cargo build --workspace` — MUST exit 0.
  - Run `cargo test --workspace` — MUST pass (tests will fail if any identifier was missed; fix until green).

  **Must NOT do**:
  - No splitting this into multiple commits (atomic!)
  - No hand-editing `Cargo.lock` (it regenerates on build — commit the regenerated version)
  - No touching `target/` (build artifacts)
  - No redesigning the temp-var prefix scheme, error enum structure, or architecture
  - No fixing unrelated clippy warnings (note them in a comment, don't fix)
  - No "improving" doc comments beyond name substitution

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Large coordinated atomic change across 59+ files with a hard compilation gate. Requires systematic execution + iterative fix-until-green. Single agent owns the whole atomic unit to avoid merge conflicts.
  - **Skills**: []
    - (No specialized skill overlap; the agent uses cargo, git, grep, lsp_rename natively)
  - **Skills Evaluated but Omitted**:
    - `git-master`: `git mv` operations are simple; skill is heavier than needed.

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T1, T2 — touches only `crates/`, no overlap with `.sisyphus/` or `README.md`)
  - **Parallel Group**: Wave 1
  - **Blocks**: T4, T5, T6 (CRITICAL PATH)
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `Cargo.toml` (root, workspace) — Lines 23-30 define all path deps; the source of truth for crate wiring.
  - `crates/deox-cli/Cargo.toml` — Contains BOTH the package name (`deox-cli`) AND the binary name (`[[bin]] name = "deox"`). Two distinct renames.
  - `crates/deox-error/src/diagnostic.rs:149` — `DeoxError` enum definition. Rename via `lsp_rename` for workspace-wide safety.
  - `crates/deox-error/src/source_map.rs:128,156` — `deox_to_rust` field + method. Rename via `lsp_rename`.
  - `crates/deox-codegen-rust/src/context.rs:44,47` — `__deox_tmp_{}` temp var prefix. The doc comment explains collision-avoidance design (keep the design, change the name).
  - `crates/deox-cli/src/error_mapper.rs:188-267` — COUPLED codegen logic + assertions. Read fully before editing.

  **API/Type References**:
  - The identifier mapping table at the top of this TODOs section — print it and follow it exactly.

  **Test References**:
  - `crates/deox-cli/tests/cli_run_tests.rs`, `cli_build_tests.rs` — Reference `deox run`, `test.deox`, `run_ola.deox` paths. Update string literals.
  - `crates/deox-error/tests/span_test.rs:143-183` — Assertions on error output format.

  **WHY Each Reference Matters**:
  - `lsp_rename` for `DeoxError` and `deox_to_rust`: These symbols are referenced across many files. LSP finds ALL references workspace-wide and renames safely. Manual grep-replace risks missing call sites.
  - `error_mapper.rs` coupling: The codegen writes `prog.deox:LINE:COL` into error strings; tests assert that exact string. Updating one side without the other = failing tests. This is the #1 trap in this task.
  - Temp var prefix: The `__` prefix is a deliberate design (user identifiers never contain `__`). Preserve this invariant; only change `deox` → `buff`.

  **Acceptance Criteria**:
  - [ ] All 9 crate dirs renamed (verify: `Get-ChildItem crates/ -Directory | Select Name` shows `buff-lang-*`, no `deox-*`)
  - [ ] `cargo build --workspace` exits 0
  - [ ] `cargo test --workspace` — all tests pass, exit 0
  - [ ] `git grep -nE "use deox_" -- crates/` → 0 matches
  - [ ] `git grep -nE "DeoxError" -- crates/` → 0 matches
  - [ ] `git grep -nE "deox_to_rust" -- crates/` → 0 matches
  - [ ] `git grep -nE "__deox_tmp_" -- crates/` → 0 matches
  - [ ] `cargo metadata --no-deps --format-version 1 | Select-String '"name"'` → all show `buff-lang-*`, none `deox`
  - [ ] Binary target: `cargo metadata --no-deps` shows exactly one bin named `buff`, none named `deox`

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Workspace compiles after atomic rename
    Tool: Bash (cargo)
    Preconditions: All 6 steps (3a-3f) complete
    Steps:
      1. Run: cargo build --workspace 2>&1
         Assert: exit code 0; output contains "Finished" and NO "error["
      2. Run: cargo test --workspace 2>&1
         Assert: exit code 0; output contains "test result: ok"; zero "FAILED"
    Expected Result: Both commands exit 0; all tests pass.
    Failure Indicators: "error[E0432]: unresolved import"; "cannot find type `DeoxError`"; any "FAILED" in test output.
    Evidence: .sisyphus/evidence/task-3-build-test.txt

  Scenario: No residual deox identifiers in source
    Tool: Bash (git grep)
    Preconditions: Build passes
    Steps:
      1. Run: git grep -nE "use deox_" -- crates/
         Assert: ZERO matches
      2. Run: git grep -nE "DeoxError" -- crates/
         Assert: ZERO matches
      3. Run: git grep -nE "deox_to_rust" -- crates/
         Assert: ZERO matches
      4. Run: git grep -nE "__deox_tmp_" -- crates/
         Assert: ZERO matches
      5. Run: git grep -niE "deox" -- crates/ | Select-String -NotMatch "\.snap"
         Assert: ZERO matches (excluding snapshot files, which T5 regenerates)
    Expected Result: All 5 grep commands return nothing.
    Failure Indicators: Any `deox` match in source files.
    Evidence: .sisyphus/evidence/task-3-residual-audit.txt

  Scenario: Binary identity correct (buff, not deox)
    Tool: Bash (cargo metadata)
    Preconditions: Build passes
    Steps:
      1. Run: cargo metadata --no-deps --format-version 1 > $env:TEMP\buff-meta.json
      2. Run: Select-String -Path $env:TEMP\buff-meta.json -Pattern '"name": "deox'
         Assert: ZERO matches
      3. Run: Select-String -Path $env:TEMP\buff-meta.json -Pattern '"name": "buff-lang-'
         Assert: ≥9 matches (all crates)
      4. Run the CLI: cargo run -p buff-lang-cli -- --help 2>&1
         Assert: output contains "buff" AND "Buff language compiler"; does NOT contain "deox" or "Deox"
    Expected Result: All package names are buff-lang-*; binary help shows "buff".
    Failure Indicators: Any package still named deox-*; help text mentions deox.
    Evidence: .sisyphus/evidence/task-3-binary-identity.txt

  Scenario: error_mapper coupled change verified (NO test failures from half-update)
    Tool: Bash (cargo test)
    Preconditions: Step 3e complete
    Steps:
      1. Run: cargo test -p buff-lang-cli 2>&1
         Assert: exit 0; zero "FAILED"; specifically the error_mapper tests pass
      2. Run: git grep -nE "prog\.deox" -- crates/buff-lang-cli/
         Assert: ZERO matches (both codegen string AND assertions now use prog.buff)
      3. Run: git grep -nE "prog\.buff" -- crates/buff-lang-cli/
         Assert: matches exist (the new format is used consistently)
    Expected Result: No half-updated error_mapper; logic + assertions aligned.
    Failure Indicators: Tests fail with "expected prog.deox but got prog.buff" or vice versa.
    Evidence: .sisyphus/evidence/task-3-error-mapper-coupled.txt
  ```

  **Commit**: YES (C1) — THE atomic commit
  - Message: `refactor(rename): crates, modules, identifiers Deox→Buff`
  - Files: all `crates/buff-lang-*/**`, root `Cargo.toml`, regenerated `Cargo.lock`
  - Pre-commit: `cargo build --workspace && cargo test --workspace` both exit 0

---

- [x] 4. **Rename file extension `.deox` → `.buff` (examples, fixtures, path strings)**

  **What to do**:
  - Use `git mv` to rename all `.deox` files to `.buff` (PRESERVES HISTORY):
    - `examples/ola.deox` → `examples/ola.buff`
    - `tests/fixtures/valid/ola.deox` → `tests/fixtures/valid/ola.buff`
    - `tests/fixtures/valid/arithmetic.deox` → `tests/fixtures/valid/arithmetic.buff`
    - `tests/fixtures/invalid/bad_indent.deox` → `tests/fixtures/invalid/bad_indent.buff`
    - `tests/fixtures/invalid/missing_semicolon.deox` → `tests/fixtures/invalid/missing_semicolon.buff`
    - Any other `.deox` files found via: `Get-ChildItem -Recurse -Filter *.deox -Path . | Where-Object { $_.FullName -notmatch 'target' }`
  - Update content of renamed files:
    - `examples/ola.buff` line 1: `// Deox v0.1 MVP example` → `// Buff v0.1 MVP example`
    - `examples/ola.buff` line 3: `print("Olá, Deox!")` → `print("Olá, Buff!")`
    - `tests/fixtures/valid/ola.buff`: same two changes
    - Update comments in other fixtures referencing "Deox"
  - Update ALL string-literal path references in Rust source/test files:
    - `PathBuf::from("test.deox")` → `PathBuf::from("test.buff")`
    - `"run_ola.deox"` → `"run_ola.buff"`, `"ola_fixture.deox"` → `"ola_fixture.buff"`
    - Find them all: `git grep -nE "\.deox" -- crates/ tests/ examples/`
    - NOTE: T3 should have updated some of these (cli.rs help strings, error_mapper). This task catches any REMAINING `.deox` path strings that reference actual files. Coordinate: ensure string literals match the new `.buff` filenames.
  - Update `Cargo.lock` if the build references filenames (run `cargo build` to regenerate).

  **Must NOT do**:
  - No hand-editing `.snap` files (T5 regenerates them)
  - No deleting files (use `git mv` only)
  - No changing fixture CONTENT beyond name substitution (e.g., don't alter test logic)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Many small coordinated file moves + string updates; needs thoroughness but not deep architectural reasoning.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (sequential after T3)
  - **Parallel Group**: Wave 2 (solo)
  - **Blocks**: T5, T6
  - **Blocked By**: T3 (source identifiers must be renamed first so path strings are consistent)

  **References**:

  **Pattern References**:
  - `examples/ola.deox` — The canonical hello-world example. Will become `ola.buff`.
  - `tests/fixtures/valid/ola.deox`, `tests/fixtures/invalid/bad_indent.deox` etc. — Test fixtures consumed by the test suite. Their filenames are referenced as strings in test code; both must change together.

  **Test References**:
  - `crates/buff-lang-cli/tests/cli_run_tests.rs`, `cli_build_tests.rs` — Contain `"run_ola.deox"` and similar string literals. Update to `.buff`.

  **WHY Each Reference Matters**:
  - Fixture filename + test string coupling: If `ola.deox` is renamed to `ola.buff` but a test still does `PathBuf::from("ola.deox")`, the test fails to find the file. Both sides change together.

  **Acceptance Criteria**:
  - [ ] `Get-ChildItem -Recurse -Filter *.deox | Where-Object { $_.FullName -notmatch 'target' }` → returns nothing (all renamed)
  - [ ] `Get-ChildItem -Recurse -Filter *.buff | Where-Object { $_.FullName -notmatch 'target' }` → returns the renamed files
  - [ ] `git grep -nE "\.deox" -- crates/ tests/ examples/` → 0 matches (excluding `.snap` files)
  - [ ] `Select-String -Path examples/ola.buff -Pattern "Olá, Buff"` → match
  - [ ] `git status` shows renames (R), not delete+add

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: All .deox files renamed to .buff
    Tool: Bash (PowerShell)
    Preconditions: All git mv operations complete
    Steps:
      1. Run: Get-ChildItem -Recurse -Filter *.deox -Path . | Where-Object { $_.FullName -notmatch '\\target\\' }
         Assert: returns EMPTY (no .deox files remain outside target/)
      2. Run: Get-ChildItem -Recurse -Filter *.buff -Path . | Where-Object { $_.FullName -notmatch '\\target\\' }
         Assert: returns ≥5 files (examples + fixtures)
      3. Run: git status --short
         Assert: renamed files show "R  ...ola.deox -> ...ola.buff"
    Expected Result: Zero .deox files; .buff files present; git tracks as renames.
    Failure Indicators: Any .deox file remaining; "D" (deleted) instead of "R" (renamed).
    Evidence: .sisyphus/evidence/task-4-extension-rename.txt

  Scenario: No residual .deox path strings in source
    Tool: Bash (git grep)
    Preconditions: String literal updates complete
    Steps:
      1. Run: git grep -nE "\.deox" -- crates/ tests/ examples/
         Assert: ZERO matches (excluding *.snap files which T5 handles)
      2. Run: git grep -nE "ola\.buff" -- crates/ tests/ examples/
         Assert: matches exist (new path strings in place)
    Expected Result: Source references only .buff paths.
    Failure Indicators: Any ".deox" string literal referencing a file path.
    Evidence: .sisyphus/evidence/task-4-path-strings.txt

  Scenario: Example content updated (Olá, Buff!)
    Tool: Bash (PowerShell)
    Preconditions: Fixture/example content edited
    Steps:
      1. Run: Select-String -Path examples/ola.buff -Pattern "Olá, Buff!"
         Assert: ≥1 match
      2. Run: Select-String -Path examples/ola.buff -Pattern "Deox"
         Assert: ZERO matches
    Expected Result: Example prints "Olá, Buff!" with no Deox references.
    Failure Indicators: Still says "Olá, Deox!".
    Evidence: .sisyphus/evidence/task-4-example-content.txt
  ```

  **Commit**: YES (C2)
  - Message: `refactor(rename): file extension .deox→.buff`
  - Files: all `*.buff` files (renamed), Rust source with path string updates, regenerated `Cargo.lock`
  - Pre-commit: `cargo build --workspace && cargo test --workspace` exit 0

---

- [x] 5. **Regenerate insta snapshots + Cargo.lock + snapshots/README.md**

  **What to do**:
  - Run `cargo insta test --accept --workspace` (or `cargo test --workspace` with `INSTA_UPDATE=always` env var). This regenerates ALL `.snap` snapshot files with the new `buff`/`buff-lang-*` names.
  - Delete stale snapshot files with `deox` in their names (if insta doesn't auto-clean):
    - `crates/buff-lang-lexer/tests/snapshots/lexer_tests__snapshot_ola_deox.snap` → will be regenerated as `lexer_tests__snapshot_ola_buff.snap`
    - `crates/buff-lang-lexer/tests/snapshots/lexer_tests__snapshot_arithmetic_deox.snap` → regenerated as `..._arithmetic_buff.snap`
    - Use `git rm` for stale files if they remain after insta accept.
  - Verify `Cargo.lock` is current (run `cargo build --workspace`; commit regenerated `Cargo.lock`).
  - Update `tests/snapshots/README.md` (lines 3, 16-19): any `Deox` → `Buff`, `.deox` → `.buff` references.
  - Verify the snapshot content now references `buff` identifiers, not `deox`.

  **Must NOT do**:
  - No hand-editing `.snap` files (regenerate via insta only)
  - No hand-editing `Cargo.lock` (regenerate via cargo only)
  - No changing snapshot test logic

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Mostly running regeneration commands + verifying output; straightforward.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 (solo)
  - **Blocks**: T6
  - **Blocked By**: T4 (file extensions must be renamed first so snapshots capture the new state)

  **References**:

  **Pattern References**:
  - `crates/buff-lang-lexer/tests/snapshots/` — Contains `*_deox.snap` files that will regenerate as `*_buff.snap`.
  - `crates/buff-lang-ast/tests/snapshots/snapshot_helper__helper_example_int_42.snap` — May contain deox identifiers in content.
  - `tests/snapshots/README.md` — Documents the snapshot directory; references Deox.

  **WHY Each Reference Matters**:
  - Insta snapshots are golden-file tests. After the rename, the AST/lexer output changes (identifiers are now `buff_*`). The old `.snap` files assert the OLD output → tests fail until snapshots are accepted.

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace` exits 0 (snapshots accepted, all pass)
  - [ ] `Get-ChildItem -Recurse -Filter *_deox*.snap` → returns nothing (stale snapshots removed)
  - [ ] `git grep -niE "deox" -- "*.snap"` → 0 matches (snapshot content updated)
  - [ ] `Select-String -Path tests/snapshots/README.md -Pattern "Deox"` → 0 matches

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Snapshots regenerate cleanly with buff names
    Tool: Bash (cargo insta)
    Preconditions: T3 + T4 complete; code compiles
    Steps:
      1. Run: cargo insta test --accept --workspace 2>&1
         Assert: completes without error; prints "accepted" or similar for updated snapshots
      2. Run: cargo test --workspace 2>&1
         Assert: exit 0; "test result: ok"; zero "FAILED"
      3. Run: Get-ChildItem -Recurse -Filter "*deox*.snap" -Path crates
         Assert: returns EMPTY (no stale deox snapshots)
      4. Run: Get-ChildItem -Recurse -Filter "*buff*.snap" -Path crates
         Assert: returns ≥2 files (new buff snapshots present)
    Expected Result: All snapshots regenerated; tests green; no stale deox .snap files.
    Failure Indicators: "FAILED" in test output; stale *_deox.snap files remaining; insta says "rejected".
    Evidence: .sisyphus/evidence/task-5-snapshot-regen.txt

  Scenario: Snapshot content free of deox references
    Tool: Bash (git grep)
    Preconditions: Snapshots accepted
    Steps:
      1. Run: git grep -niE "deox" -- "*.snap"
         Assert: ZERO matches
      2. Run: git grep -niE "buff" -- "*.snap"
         Assert: matches exist (new content present)
    Expected Result: No snapshot file contains "deox".
    Failure Indicators: Any .snap file still references deox identifiers.
    Evidence: .sisyphus/evidence/task-5-snapshot-content.txt
  ```

  **Commit**: YES (C3)
  - Message: `chore(rename): regenerate insta snapshots + Cargo.lock`
  - Files: all `*.snap` (regenerated), `Cargo.lock`, `tests/snapshots/README.md`
  - Pre-commit: `cargo test --workspace` exit 0

---

- [x] 6. **FULL VERIFICATION SUITE + residual-`deox` audit (gate before review)**

  **What to do**:
  - This is the integration gate. Run the COMPLETE verification battery and capture evidence for each. Fix any issue found (loop back to the relevant task if something was missed).
  - **Verification battery** (run ALL, capture evidence):
    1. Clean build: `cargo clean; cargo build --workspace` → exit 0
    2. Full test suite: `cargo test --workspace` → all pass, exit 0
    3. End-to-end: `./target/debug/buff run examples/ola.buff` → stdout contains `Olá, Buff!`
    4. Build command: `./target/debug/buff build examples/ola.buff` → exit 0
    5. Binary help: `./target/debug/buff --help` → contains `buff` + `Buff language compiler`, NO `deox`
    6. Binary version: `./target/debug/buff --version` → exit 0
    7. Metadata check: `cargo metadata --no-deps --format-version 1` → all packages `buff-lang-*`, one bin `buff`
    8. **Residual audit**: `git grep -iE "deox" -- . ':(exclude)target' ':(exclude).sisyphus/evidence' ':(exclude)*.snap'` → must be 0 matches (the ONLY acceptable residuals are git-log history and evidence files)
    9. Edge cases: `buff run nonexistent.buff` → graceful error (not crash); `buff run examples/ola.buff extra args` → works
  - If ANY check fails, diagnose which task missed something, file a follow-up, and re-run.

  **Must NOT do**:
  - No skipping checks ("looks fine")
  - No marking pass without capturing evidence
  - No modifying code here (this is VERIFY-ONLY; if something's broken, note it and route back)

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Requires running a full battery, diagnosing failures, and routing fixes. Needs good judgment.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave FINAL (solo, then F1-F4 in parallel after)
  - **Blocks**: F1, F2, F3, F4 (review agents)
  - **Blocked By**: T1, T2, T3, T4, T5 (ALL implementation tasks)

  **References**:

  **Pattern References**:
  - The "Verification Commands" in the Success Criteria section of THIS plan — the canonical command list.
  - The residual-audit allowlist in the Verification Strategy section — defines what `deox` matches are acceptable.

  **WHY Each Reference Matters**:
  - The residual audit with an explicit allowlist is the definitive proof the rename is complete. Without it, scattered `deox` strings may persist silently.

  **Acceptance Criteria**:
  - [ ] All 9 verification battery checks pass
  - [ ] Evidence captured for each in `.sisyphus/evidence/task-6-*.txt`
  - [ ] Residual audit returns 0 matches outside allowlist

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: End-to-end buff run prints Olá, Buff!
    Tool: Bash (cargo)
    Preconditions: cargo build --workspace succeeded
    Steps:
      1. Run: .\target\debug\buff.exe run examples/ola.buff
         Assert: exit code 0; stdout contains exactly "Olá, Buff!"
      2. Run: .\target\debug\buff.exe --help
         Assert: stdout contains "buff"; contains "Buff language compiler"; does NOT contain "deox" or "Deox"
    Expected Result: Hello-world works end-to-end with new name.
    Failure Indicators: stdout says "Olá, Deox!"; help mentions deox; nonzero exit.
    Evidence: .sisyphus/evidence/task-6-e2e-buff-run.txt

  Scenario: Residual deox audit (the definitive completeness check)
    Tool: Bash (git grep with exclusions)
    Preconditions: All tasks T1-T5 complete
    Steps:
      1. Run: git grep -iE "deox" -- . ":(exclude)target" ":(exclude).sisyphus/evidence" ":(exclude)*.snap" ":(exclude).git/*"
         Assert: ZERO matches
         NOTE: git log history WILL contain "deox" in old commit messages — that is EXPECTED and EXCLUDED from this grep (git grep searches working tree, not history). If this grep returns the rename-plan file itself documenting the old name, that's acceptable — note it.
      2. Run: git log --oneline | Select-String "deox"
         Assert: matches ARE EXPECTED (historical commits). This is fine — do NOT rewrite history.
    Expected Result: Working tree has zero unintended deox references; git history retains deox (correct).
    Failure Indicators: Working-tree matches outside the allowlist (rename plan doc, evidence).
    Evidence: .sisyphus/evidence/task-6-residual-audit.txt

  Scenario: Metadata confirms all crates renamed
    Tool: Bash (cargo metadata)
    Preconditions: Build passes
    Steps:
      1. Run: cargo metadata --no-deps --format-version 1 | Select-String '"name": "deox'
         Assert: ZERO matches
      2. Run: cargo metadata --no-deps --format-version 1 | Select-String '"name": "buff-lang-'
         Assert: ≥9 matches
      3. Run: cargo metadata --no-deps --format-version 1 | Select-String '"name": "buff"'
         Assert: exactly one match (the binary target)
    Expected Result: Package registry fully migrated to buff-lang-*.
    Failure Indicators: Any deox-* package; multiple or zero buff binaries.
    Evidence: .sisyphus/evidence/task-6-metadata.txt

  Scenario: Edge cases handled gracefully
    Tool: Bash (buff CLI)
    Preconditions: Build passes
    Steps:
      1. Run: .\target\debug\buff.exe run nonexistent.buff
         Assert: exits nonzero with a clear error message (not a panic/crash); error mentions the file
      2. Run: .\target\debug\buff.exe run examples/ola.buff -- extra args
         Assert: runs (extra args passed through or ignored gracefully)
    Expected Result: Errors are graceful, not crashes.
    Failure Indicators: Panic / stack trace / unhelpful error.
    Evidence: .sisyphus/evidence/task-6-edge-cases.txt
  ```

  **Commit**: NO (verification only — no code changes; if fixes were needed, they commit under their originating task)

---

- [ ] 7. **EXTERNAL (Manual/User Action): GitHub repo rename + git remote + local folder rename**

  > ⚠️ **This task requires USER action on GitHub.** It is documented for completeness but cannot be fully automated by an agent (GitHub repo rename + local folder rename require interactive steps). The agent should provide the exact commands and guide the user.

  **What to do** (provide these instructions to the user):

  **Step 7a — Rename the GitHub repository**:
  - Via `gh` CLI: `gh repo rename buff --repo vsbb1/Deox` (or via GitHub web UI: Settings → Repository name → `buff`)
  - Optionally create the `buff-lang` org first and transfer: `gh repo transfer vsbb1/Deox buff-lang` (then repo becomes `buff-lang/Deox`, then rename to `buff-lang/buff`)
  - GitHub auto-redirects old URLs, but update all references anyway.

  **Step 7b — Update git remote origin**:
  ```
  git remote set-url origin https://github.com/buff-lang/buff.git
  git remote -v   # verify
  ```

  **Step 7c — Rename the local folder** (requires closing editors/terminals that lock it first):
  - Close all editors/terminals/IDEs using `C:\Users\vsbb1\source\repos\Deox`
  - In a separate terminal (NOT inside the repo): `Rename-Item -LiteralPath "C:\Users\vsbb1\source\repos\Deox" -NewName "Buff"`
  - Reopen terminal in the new path: `C:\Users\vsbb1\source\repos\Buff`

  **Step 7d — Verify**:
  - `git remote -v` shows `https://github.com/buff-lang/buff.git`
  - `git push` works (tests the new remote)
  - `Test-Path C:\Users\vsbb1\source\repos\Buff` → True
  - `Test-Path C:\Users\vsbb1\source\repos\Deox` → False

  **Must NOT do**:
  - No rewriting git history
  - No force-pushing
  - No deleting the old repo before confirming the new one works

  **Recommended Agent Profile**:
  - **Category**: `quick` (documentation/guidance only — the agent prepares the command list; the USER executes)
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (external, sequential, after T6)
  - **Blocks**: Nothing (terminal step)
  - **Blocked By**: T6 (all code rename verified first)

  **References**:
  - `README.md:90` — Clone URL `https://github.com/vsbb1/Deox.git` was updated to `buff-lang/buff.git` in T2; this task makes the actual repo match.
  - Current remote: `git remote -v` shows the existing origin URL to replace.

  **Acceptance Criteria**:
  - [ ] GitHub repo is named `buff-lang/buff` (or `buff` under the user's chosen org)
  - [ ] `git remote -v` shows the new URL
  - [ ] `git push` succeeds
  - [ ] Local folder is `Buff`

  **QA Scenarios (MANDATORY)**:

  ```
  Scenario: Remote URL updated and pushable
    Tool: Bash (git)
    Preconditions: GitHub repo renamed; remote updated
    Steps:
      1. Run: git remote -v
         Assert: origin shows https://github.com/buff-lang/buff.git (fetch + push)
      2. Run: git push --dry-run
         Assert: succeeds (exit 0); no auth errors
    Expected Result: Remote points to new repo; push works.
    Failure Indicators: Old URL; auth failure; "Repository not found".
    Evidence: .sisyphus/evidence/task-7-remote-verify.txt
  ```

  **Commit**: NO (external operations; the remote/folder changes are infrastructure, not commits)

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo build --workspace` + `cargo clippy --workspace` + `cargo test --workspace`. Review all changed files for: leftover `deox` identifiers, broken `use` statements, inconsistent naming. Check the identifier mapping was applied uniformly.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  Start from clean state (`cargo clean`). Execute EVERY QA scenario from EVERY task — follow exact steps, capture evidence. Run `buff run examples/ola.buff` end-to-end. Test edge cases: `buff --help`, `buff --version`, invalid file, missing file. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git log/diff). Verify 1:1 — everything in spec was built, nothing beyond spec was built. Check "Must NOT do" compliance (no metadata added, no mascot, no CHANGELOG, no unrelated fixes). Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

Commits follow Metis's recommended atomic sequence. Each commit must leave the tree compilable (except where noted).

| Commit | Task(s) | Message | Pre-commit check |
|--------|---------|---------|------------------|
| C1 | T3 | `refactor(rename): crates, modules, identifiers Deox→Buff` | `cargo build --workspace && cargo test --workspace` |
| C2 | T4 | `refactor(rename): file extension .deox→.buff` | `cargo build --workspace && cargo test --workspace` |
| C3 | T5 | `chore(rename): regenerate insta snapshots + Cargo.lock` | `cargo test --workspace` |
| C4 | T2 | `docs(rename): rewrite README for Buff` | n/a (docs only) |
| C5 | T1 | `chore(rename): rename .sisyphus plan files + repair links` | `boulder.json` active_plan path exists |
| — | T6 | (verification only, no commit) | — |
| — | T7 | (external, user manual) | `git remote -v` shows new URL |

**Note**: T1, T2, T3 run in parallel (Wave 1) but commit independently in dependency-safe order. The orchestrator should commit C1 (T3) first since it's the compilation unit, then C4 (T2) and C5 (T1) can commit in any order.

---

## Success Criteria

### Verification Commands
```bash
cargo build --workspace                    # Expected: exit 0
cargo test --workspace                     # Expected: all pass, exit 0
cargo metadata --no-deps --format-version 1 | grep '"name"'  # Expected: all buff-lang-*
./target/debug/buff run examples/ola.buff  # Expected: stdout contains "Olá, Buff!"
git grep -iE "deox" -- . ':(exclude)target' ':(exclude).sisyphus/evidence'  # Expected: 0 matches (excluding git log)
cargo run -p buff-lang-cli -- --help       # Expected: contains "buff", no "deox"
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass (`cargo test --workspace`)
- [ ] `buff run examples/ola.buff` prints `Olá, Buff!`
- [ ] Zero residual `deox` (outside allowlist)
- [ ] Git history preserved (evidence via `git log --follow`)
- [ ] User explicit "okay" after F1-F4 review
