# Decisions Log — self-host-completion-roadmap

## 2026-07-26 — Plan initialized
- v16 plan committed at dcd1fc5
- P0.24 already done (tags exist, contradictions resolved)
- Starting execution with Batch 1: P0.0, P0.0.1, P0.9, P0.17, P0.23
- Background: S3 (HashMap audit), S7 (unsafe audit)

## P0.17 — http-client timeout
- ft-001 FIXED
- 30s request timeout + 10s connect timeout added

## P0.0.1 — Extension counter + pre-commit hook
- Counter at .sisyphus/evidence/extensions-counter.json (force-added, gitignore exception)
- Pre-commit hook at .githooks/pre-commit (no jq dependency — uses grep/sed for Windows Git Bash compat)
- Git core.hooksPath set to .githooks
- Hook verified: rejects commit when used=4 > max=3
- Commit: 64ce027 ci: extension cap enforcement (P0.0.1)

## P0.23 — Homebrew SHA-256
- cicd-002 FIXED
- All <FILL-ME placeholders replaced with real hashes (linux-x64 computed from local build)
- CI check added (installer-lint job) to detect future placeholder strings
- update-sha256.sh automation script created for release workflow
- Release.yml wired to call update-sha256.sh before publishing
- 3 platform hashes (macos-arm64, macos-x64, linux-arm64) left empty — filled by release workflow
- Commit: 86b36af fix(homebrew): sha256 (P0.23)
---

## P0.0 — Porting Conventions doc created (2026-07-26)
- File: .sisyphus/decisions/porting-conventions.md
- 11 sections + appendix covering: file header, naming, comments, errors, tests, imports/modules, type system, patterns-need-special-handling, project-walls, verification-checklist, references, decision-log
- 8 convention categories required; doc has 10 (named categories with Rust→Buff before/after snippets per task spec)
- Cross-references: DR-014, self-host-completion-roadmap.md §Equivalence Contract v2, buff-conventions.md §1/§6/§7/§8/§14/§15/§19, root AGENTS.md anti-patterns + unique styles

### Key convention decisions made (in doc Appendix A):
- Standardize on `Type.new()` constructor (not struct literal) for logic ports — matches existing token.buff/stream.buff corpus
- `impl Block` translates as free functions with type-prefixed names (e.g. `stream_peek(s)`) — matches corpus
- Rust `()` (unit) → Buff `Option<String>` (None=success / Some(msg)=failure) — matches stream.buff lines 66-74
- `?` operator IS supported in Buff (CORRECTED task-spec assumption — examples/error_handling.buff line 31 confirms end-to-end)
- `while` is NOT a Buff keyword — conditional loops spell as `for cond:`
- Rust match or-patterns expand to `if/else if` chains with `==` (Buff match rejects or-patterns, qualified patterns, guards, statement bodies, `{}` block arms — documented in stream.buff lines 18-28)
- Custom error ENUM ports (data); thiserror Display impl stays in Rust host; manual `_display()` free function bridges
- HashMap portability gated on S3 spike (dep-001 audit finding — `Map` is BTreeMap-backed)

### Files consulted (per task REQUIRED TOOLS):
- .sisyphus/decisions/selfhost-feasibility.md (DR-014 crate list)
- .sisyphus/plans/buff-conventions.md (19 Buff language conventions)
- .sisyphus/plans/self-host-completion-roadmap.md lines 92-144 (Equivalence Contract v2)
- AGENTS.md root (anti-patterns, unique styles)
- examples/{ola,fibonacci,closures,collections,error_handling}.buff
- self-host/lexer/token.buff + self-host/parser/stream.buff (existing logic-port style)
- crates/buff-lang-ast/selfhost/common.buff (existing data-model port style)
- .sisyphus/notepads/self-host-completion-roadmap/learnings.md
