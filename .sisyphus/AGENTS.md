# .sisyphus/

Project orchestration: hierarchical markdown plans, decision records, per-phase
scratchpads, and active session state. Named after Sisyphus — the boulder never
reaches the top, but the pushing is the point.

## STRUCTURE

```
.sisyphus/
├── boulder.json                  # Active session state ("the boulder"). Tracked in git.
├── plans/                        # 13 plan files — task breakdowns, conventions, references
│   ├── buff-master.md            # Master orchestrator (entry point for plan hierarchy)
│   ├── buff-conventions.md       # 18+ Buff-language coding conventions (NOT Rust rules)
│   ├── buff-v01-mvp.md           # Phase 1: "Ola Buff" MVP
│   ├── buff-v05-language.md      # Phase 2: "Real Language"
│   ├── buff-v10-production.md    # Phase 3: "Production"
│   ├── buff-post-v10-tooling.md  # Post-v1.0 tooling roadmap
│   ├── buff-v1x-frameworks.md    # v1.13-v1.23 framework crate waves
│   ├── buff-launch-readiness.md  # v1.25-v1.38 launch readiness
│   ├── v1.26-real-use-cases-launch.md  # Current: v1.26 real-use-cases + launch infra
│   ├── buff-v2-mlir-selfhost.md  # SUPERSEDED — MLIR/self-host deferred
│   ├── buff-project-structure.md # Project layout standard (buff.toml, templates)
│   ├── buff-numeric-system.md    # Numeric type specification
│   └── rename-to-buff.md         # Rename wave history
├── decisions/                    # ADR-style decision records (10 files)
│   ├── stability-promise.md      # Formal stability contract (Rust-style)
│   ├── dioxus-feasibility.md     # T121b Dioxus 0.7 feasibility spike
│   ├── api-compat-v20.md         # T22 API compatibility spike + mismatch report
│   ├── buff-direction-speed-moat-selfhost.md  # v1.25+ strategic direction
│   ├── sdk-conventions-v1x.md    # SDK 2.0 conventions for framework crates
│   ├── wgsl-extensibility-v1x.md # WGSL codegen extensibility bounds
│   ├── macro-system-v1x.md       # Macro system design decisions
│   ├── rsx-syntax-feasibility.md # RSX syntax feasibility analysis
│   ├── v1.24-audit-report.md     # v1.24 audit findings
│   └── v1.24-followup.md         # v1.24 audit follow-up actions
├── notepads/                     # Per-phase working scratchpads (may be messy)
│   ├── buff-master/              # Orchestration notes, readiness checks
│   ├── buff-v05-language/        # v0.5 phase issues/learnings/problems
│   ├── buff-post-v10-tooling/    # Post-v1.0 tooling phase notes
│   └── rename-to-buff/           # Rename wave execution notes
├── audits/                       # Audit reports
│   └── code-hygiene-v1.25.md     # v1.25 code hygiene audit
├── drafts/                       # Empty (reserved for plan proposals)
└── evidence/                     # Gitignored — task verification artifacts
    └── task-{N}-{slug}.{ext}     # QA outputs, build logs, test results
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| Plan hierarchy entry point | `plans/buff-master.md` |
| Buff-language conventions | `plans/buff-conventions.md` |
| Per-phase task breakdowns | `plans/buff-v{01,05,10}-*.md` |
| Current active plan | `plans/v1.26-real-use-cases-launch.md` |
| A decision record | `decisions/<topic>.md` |
| Phase-specific issues/learnings | `notepads/<phase-name>/` |
| Audit reports | `audits/` |
| T-number cross-references | Every plan uses T{N} IDs referenced across plans, evidence, root AGENTS.md, per-crate AGENTS.md |

## CONVENTIONS (this dir only)

- **Plan hierarchy**: `buff-master.md` is the top-level orchestrator. It links to per-phase plans (v01/v05/v10), post-v10 roadmap, v1x frameworks, launch readiness, and milestone plans. Read master first.
- **T-numbered task IDs**: Every task gets a T-number (T1, T44, T124, etc). Stable cross-references used across plans, evidence files, root AGENTS.md, and per-crate AGENTS.md files. Never reassign a T-number.
- **buff-conventions.md governs Buff language**, not Rust. Rust crate conventions live in root AGENTS.md and CONTRIBUTING.md.
- **decisions/ are ADR-style**: each file records a decision, alternatives considered, rationale, and consequences. Append-only; superseded decisions note their replacement.
- **notepads/ are working scratchpads**: per-phase with standard sub-files (issues.md, decisions.md, learnings.md, problems.md). May be rough or incomplete.
- **evidence/ naming**: `task-{T-number}-{scenario-slug}.{ext}` (e.g. `task-44-wgsl-codegen.txt`).
- **v2-mlir-selfhost.md is SUPERSEDED**: MLIR and self-hosting are deferred. File exists as historical record only.

## NOTES

- **evidence/ is gitignored** (`.gitignore` line: `.sisyphus/evidence/`). Verification artifacts live locally but are never committed.
- **boulder.json is tracked in git** (NOT gitignored). Represents active session state — may be stale between sessions but persists across them.
- **drafts/ is empty**: reserved for future plan proposals before promotion to plans/.
- **notepads/ mirrors plan phases**: each plan file has a matching notepad dir where working notes accumulate during execution. Not all plans have notepads yet.
- **10 decision records** span stability-promise, Dioxus feasibility (T121b), API compatibility (T22), v1.25+ strategic direction, SDK conventions, WGSL extensibility, macro system, RSX syntax feasibility, and v1.24 audit.
