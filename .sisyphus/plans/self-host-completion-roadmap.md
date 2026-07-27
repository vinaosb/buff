# Self-Host Completion Roadmap (v15 — RECREATED with all review fixes applied)

**Status:** READY FOR EXECUTION — starts IMMEDIATELY. Tags v1.0.0-v1.39.0 ALL canonical (14 tags v1.26-v1.39 EXIST per v3.2 audit coh-001; prior "deleted" claim was false). AI-executed (no time estimates).
**Created:** 2026-07-26 (v15: recreated after v14 file corruption; incorporates all review findings from 5-agent review: Metis hidden-intention analysis, Explore factual verification, Explore audit-coverage verification)
**Governing authority:** T110 (`.sisyphus/decisions/buff-direction-speed-moat-selfhost.md`) — TRIAD: **Speed + MOAT + Self-host-frontend**
**Originating decision:** DR-014 (`.sisyphus/decisions/selfhost-feasibility.md`) — 10 potentially portable crates; "~5-7 realistic in focused session" (AI execution may exceed this)
**Audit baseline:** v3.2 audit (`.sisyphus/reports/buff-audit-2026-07-26-2019-v3.{json,md}`) — **31 verified findings** (17 critical · 14 high) + **1 false positive corrected** (FP-2: ethers/bzip2 claim DISPROVEN)

---

## TL;DR

> **Goal**: (1) Port 10 Rust compiler crates to Buff (.buff) with byte-identical behavior, achieving front-end bootstrap (M7). (2) Remediate ALL 31 v3.2 audit findings + 3 high-severity false negatives. (3) Ship migration guide + deprecation plan.
> **Execution model**: AI-agent executed (Sisyphus + sub-agents). No time estimates — tasks complete when acceptance criteria met.
> **Critical path**: `P0.8 (cargo-deny gate) → P0.4 (PARITY audit) → S1 (dyn Trait spike) → [P2.1 IF YELLOW] → P4.4 → P4.3 → P3.1 → P4.1 → P4.10 (monolith) → [Phase 5] → M7`
> **Abort triggers**: S1+S4 both fail; P4.1 exceeds 5 agent resume cycles; P0.4 finds <5 GREEN crates; cumulative perf regression >10%; 3rd Oracle NOT_VERIFIED

---

## Context

### Original Request
"Convert all achievable Rust crates to Buff for self-hosting with identical behavior."

### User Decisions (v15)
- **Scope**: All achievable streams (logic port + transpile fixes + framework fixes + CI gate + equivalence tests)
- **Depth**: Full behavioral parity per crate
- **Language gaps**: Extend Buff when gaps hit — **CAPPED at 3 extensions total** (scope creep guard)
- **IMPOSSIBLE crates**: Bring up prerequisites + path of least resistance
- **Approach**: Evaluate manual/transpile/hybrid via DR (defer until spikes)
- **Demo vs Production**: PRODUCTION — Rust originals become disposable after deprecation Phase B
- **Time estimates**: REMOVED — AI execution model (user directive v15)

### Research Findings (verified v15)
- `Bits<8>` exists for bytes (no Byte type needed)
- `for cond { }` exists for while loops (no while keyword needed)
- `Type::DynamicDispatch` exists at ty.rs:1192; GAP: `TypeRef::Dyn` variant missing (S1 validates)
- LOC: lexer 3,995; parser 14,204 (per DR-014, not prior drafts' inflated numbers)
- 28 unsafe blocks in codegen-rust (not 49 — verified v15 by Explore agent)
- Multi-file linking (T29) NOT IMPLEMENTED — single-file monolith workaround for M7
- 56 self-host/*.buff files exist (not 17 — verified v15 by Explore agent); bootstrap-report.md §4 categorizes 49 transpile failures into **5 categories** (LEX 4, PARSE-A 34, PARSE-B 5, PARSE-C 1, CODEGEN 5)
- Equivalence harness `scripts/equivalence-rust-vs-buff.sh` has **9 tests** (verified v15)
- 7 baseline files pass `buff check` (in `self-host/` directory — NOT `crates/buff-lang-cli/selfhost/`)
- 100 insta snapshots exist (not 95 — verified v15)
- Tags v1.26.0-v1.39.0 ALL EXIST (14 tags — verified by v3.2 audit coh-001 + Explore agent)

### Crate Scope (per DR-014 — VERIFIED v15, authoritative)

**12 IMPOSSIBLE crates** (DR-014 §🟥):
`buff-lang-codegen-rust` (58,617 LOC — THE WALL), `buff-lang-types` (27,146), `buff-lang-runtime` (11,433), `buff-lang-codegen-buffhtml` (2,559), `buff-lang-codegen-wgsl` (1,995), `buff-registry` (5,396), `buff-jupyter` (4,839), `buff-lsp` (3,722), `buff-dap` (2,214), `buff-ui-dioxus` (1,053), `buff-playground-wasm` (366), `buff-mcp` (1,603)

**Not ported** (effectively not portable, not in DR-014 impossible list):
`buff-lang-cli` (monolith HOST, not a port target), `buff-repl` (rustyline FFI)

**~40 Framework Wrappers** (DR-014 §🟧 — CATEGORY ERROR to port):
`buff-dataframe`, `buff-tensor`, `buff-image`, `buff-audio`, `buff-dsp`, `buff-ecs`, `buff-science`, `buff-pipeline`, `buff-ml`, `buff-pubsub`, `buff-fsm`, etc. — **"These crates are Buff's PRODUCT, not candidates for self-hosting"** (DR-014 line 59). **v15 REMOVED buff-pubsub/fsm/tensor from target list** (prior drafts included them erroneously).

**10 TARGET crates** (DR-014 §🟩):
1. `buff-lang-ast` (6,547 LOC — ✅ Most portable, 0 dyn-trait)
2. `buff-lang-ast-rsx` (418 LOC — ✅ Easy)
3. `buff-lang-error` (4,616 LOC — 🟡 Subset: Span port, thiserror doesn't)
4. `buff-lang-debug-info` (1,466 LOC — 🟡 Medium)
5. `buff-lang-lexer` (3,995 LOC — 🟡 Medium: byte scanner)
6. `buff-lang-parser` (14,204 LOC — 🟡 Large: recursive descent)
7. `buff-lang-buffhtml-parser` (2,748 LOC — 🟡 Medium)
8. `buff-lang-ffi-guide` (19 LOC — ✅ Trivial: docs only)
9. `buff-eval` (1,079 LOC — 🟡 Medium: thin eval)
10. `buff-template` (340 LOC — 🟡 Depends on tera/handlebars FFI)

P0.4 produces the AUTHORITATIVE verdict per crate.

---

## Work Objectives

### Core Objective
Port 10 crates to Buff with byte-identical output for every public function.

### Concrete Deliverables
- Up to 10 .buff port files (authoritative list from P0.4)
- `buff_compiler.buff` single-file monolith (M7 front-end bootstrap proof)
- CI hard gate (equivalence harness as required check + `buff check --dump-ast` flag)
- 5+ framework crate fixes (crypto-extras, fake, fuzz, jobs, web3 — Rust-side API drift)
- Decision record evaluating manual/transpile/hybrid approaches
- Migration guide + architecture doc + deprecation Phase B definition

### Definition of Done
1. P0.4-authoritative crate list at tiered coverage parity (90/85/80/75)
2. M7: `buff_compiler.buff` ingests 5+ .buff files → AST matching Rust parser output (via `buff check --dump-ast`)
3. .buff-compiled compiler ≤10% slower than Rust-compiled (per-phase gate: ≤3%)
4. Oracle signs compliance report
5. ALL 31 v3.2 audit findings + 3 FN tasks resolved (FIXED or DEFERRED with rationale)

### Equivalence Contract (v15 NEW — resolves Metis AMB-1)

| Tier | Scope | Comparison Method |
|------|-------|-------------------|
| **T1: Pure-value** | Fns returning primitives, strings, structs | Byte-identical JSON stdout |
| **T2: Collection** | Fns returning Vec, Map, Set | Sorted-then-compared (BTreeMap ordering) |
| **T3: Timestamped** | Fns emitting timestamps, UUIDs, random | Structural-equal modulo volatile fields |
| **T4: Async/stateful** | Fns with internal state, tokio | Snapshot protocol (P5.3) |

Error messages must match byte-for-byte (text + error codes). Errors sorted by `span.start` before comparison. Span values must match exactly.

### Must Have
- Every public function ported with byte-identical behavior per Equivalence Contract tiers
- Equivalence harness in CI as hard gate
- Phase 0.5 spikes (S1-S7) complete before Phase 1
- P0.4 PARITY audit complete before Phase 3
- Failure mode abort criteria enforced
- **Buff language extensions CAPPED at 3 total** (scope creep guard — Metis AI-FP-4)

### Must NOT Have (Guardrails)
- Any of the 12 IMPOSSIBLE crates ported
- Framework wrapper crates ported (category error per DR-014)
- Multi-file linking implemented (T29 — monolith workaround)
- Rust backend replaced (T110 forbids)
- Raw-string codegen (project rule)
- **Rust originals modified to make equivalence pass** (verify via git diff in F1)
- **Buff features used that Rust original lacks** (faithful port only)
- **Error MESSAGE divergence** (byte-for-byte, not just error codes)
- **More than 3 Buff language extensions** (hard cap — Metis AI-FP-4)
- **Unrelated bugs fixed in port PRs** (file separate issue)

### Allowed Buff Language Extensions (MAX 3 — v15 scope creep control)
1. `TypeRef::Dyn` + `KwDyn` token (for dyn Trait dispatch — P2.1)
2. _(reserved for spike-discovered gap)_
3. _(reserved for spike-discovered gap)_

Each extension requires a new DR (decision record) approved before implementation.

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification agent-executed.

### Test Decision
- **Infrastructure**: YES (insta 1.40, proptest 1.5, 100 snapshots)
- **Automated tests**: Tests-after (ports verified via equivalence harness)
- **Framework**: insta + proptest + custom equivalence harness

### QA Policy
- **T1 Pure fns**: JSON stdout byte-identical
- **T2 Collections**: Sorted-then-compared
- **T3 Timestamped**: Structural-equal modulo volatile
- **T4 Stateful**: Snapshot protocol (sync=byte-identical, pubsub=set-equal)
- **Property-based**: proptest for parser/lexer (1000+ random programs)
- **Differential**: EMI (1000+ mutated programs) — if budget insufficient, fall back to expanded proptest
- **Coverage**: cargo-tarpaulin (tiered: 90/85/80/75 by criticality)
- **Performance gate**: ≤3% regression per phase, ≤10% cumulative, ABORT >10%
- **AST comparison**: `buff check --dump-ast` produces deterministic BTreeMap-ordered JSON

---

## Execution Strategy

### Wave 0 — Foundation + Audit Remediation (PARALLEL tracks)
- **Track A (Audit)**: Phase 0.8 (P0.8-P0.27) — audit remediation on `main`
- **Track B (Self-host)**: Phase 0 (P0.1-P0.4) + Phase 0.5 (S1-S7) + Phase 0.6-0.7 — on `self-host/v1` branch
- **CI edit queue**: P0.8 (cargo-deny) → P0.11 (test split) → P0.1 (equivalence) — sequenced (all modify ci.yml)

### Wave 1 — Bug Fixes
Phase 1 — fix bug-class transpile failures.

### Wave 2 — Language Extensions
Phase 2 (P2.1 dyn Trait) — IF S1 spike passes AND extension cap not exceeded.

### Wave 3 — Tier 1 Ports
Phase 3 (P3.1-P3.8) — complete existing logic ports.

### Wave 4 — Tier 2 Ports
Phase 4 (P4.4→P4.3→P4.1→P4.5-9→P4.10) — data-only → logic → monolith.

### Wave 5 — Framework Fixes (parallel)
Phase 6 — Rust-side framework crate fixes.

### Wave 6 — Verification
Phase 5 — full parity verification (CI-only, Linux Docker).

### Wave 7 — Bootstrap
M7 — `buff_compiler.buff` monolith produces AST from 5+ .buff files.

---

## TODOs

### PHASE 0 — Foundation & Triage

> **Goal**: Lock regression prevention; produce complete triage of 49 transpile failures.
> **Exit criteria**: P0.1-P0.4 complete; all CI checks pass.

- [ ] **P0.1 — Promote equivalence harness to required CI check**
  **What**: Add `equivalence-check` job to ci.yml that runs `scripts/equivalence-rust-vs-buff.sh` (9 tests) inside Docker as a HARD gate.
  **Files**: `.github/workflows/ci.yml`
  **Must NOT do**: Do NOT remove `self-host-check` job; do NOT add `continue-on-error`.
  **Steps**: Add new job `equivalence-check` (ubuntu-only, Docker, builds buff-lang-cli release, runs harness, HARD gate). Test locally via Docker before pushing.
  **Acceptance**: Job appears on every PR; runs all 9 tests; exits non-zero on failure; blocks PR merge.
  **QA**: Happy path (9/9 pass) + negative (deliberate divergence blocks PR).
  **Parallelization**: YES (with P0.2, P0.3, P0.4). **Blocks**: all later port work. **Blocked By**: P0.8 (CI edit queue).
  **Commit**: `ci: equivalence-check hard gate (P0.1)`

- [ ] **P0.1.2a — Add `buff check --dump-ast` flag plumbing**
  **What**: Add `--dump-ast` CLI flag. Wire to a stub that prints "TODO: implement serializers" for now. P0.1.2b implements the actual JSON serializers.
  **Files**: `crates/buff-lang-cli/src/cli.rs` (add flag), `crates/buff-lang-cli/src/check.rs` (wire flag)
  **Acceptance**: `buff check --dump-ast examples/ola.buff` prints something (stub OK); flag appears in `--help`.
  **QA**: `buff check --help | grep dump-ast` returns match.
  **Commit**: `feat(check): --dump-ast flag plumbing (P0.1.2a)`

- [ ] **P0.1.2b — Implement AST JSON serializers (BTreeMap-ordered, deterministic)**
  **What**: Implement `to_json() -> serde_json::Value` for every AST node type (~100 enum variants across Decl/Expr/Stmt/Ty/Op/Span/Pattern). Uses BTreeMap-backed serialization for deterministic field ordering.
  **Why separate from P0.1.2a**: Metis review found original 0.5-day estimate severely underestimated — ~100 node types need manual serializers. Split for parallelization.
  **Files**: `crates/buff-lang-ast/src/{lib,decl,expr,stmt,ty,op,common,literal}.rs` (add `to_json()` per node)
  **Acceptance**: `buff check --dump-ast examples/ola.buff` produces valid JSON with all Decl nodes + spans; running twice = byte-identical (determinism); `jq .` accepts output.
  **QA**: Determinism test (run twice, diff = empty) + validity test (jq parse).
  **Parallelization**: YES — one agent per AST module (lib/decl/expr/stmt/ty = 5 parallel agents).
  **Commit**: `feat(check): deterministic AST JSON serializers (P0.1.2b)`

- [ ] **P0.2 — Convert self-host-check to hard gate; lock 7-file baseline**
  **What**: Remove `continue-on-error: true` from `self-host-check` CI job. Lock the 7 currently-passing files (in `self-host/` directory) as baseline.
  **Files**: `.github/workflows/ci.yml` (lines ~162-193)
  **7 baseline files** (in `self-host/` NOT `crates/buff-lang-cli/selfhost/`): `parser/expr_pattern.buff`, `parser/expr_postfix.buff`, `parser/parser.buff`, `parser/stmt.buff`, `parser/stream.buff`, `types/lib.buff`, `types/prelude_types.buff`
  **Acceptance**: `continue-on-error` removed; 7 files explicitly listed; breaking any FAILS CI; 49 failing files don't block.
  **Commit**: `ci: lock 7-file self-host baseline (P0.2)`

- [ ] **P0.3 — Reformat triage doc (49 failures → bug/lang-gap/unsupported matrix)**
  **What**: Create `self-host/triage.md` classifying each of 49 transpile failures. **Must independently re-run `buff check` on each file** — do NOT trust bootstrap-report categorization blindly (Metis AI-FP-3: hallucination risk).
  **Files**: `self-host/triage.md` (NEW)
  **4th category added** (v15 per Metis): "unknown — needs spike" for failures where root cause unclear without investigation. Cap unknowns at 10; if >10, escalate before Phase 1.
  **Acceptance**: All 49 failures categorized; counts: bugs + lang-gaps + unsupported + unknown = 49; Oracle reviews.
  **Commit**: `docs: self-host triage matrix (P0.3)`

- [ ] **P0.4 — PARITY audit (CRITICAL UNBLOCK)**
  **What**: Produce AUTHORITATIVE list of target crates with per-crate portability verdict + pub fn inventory. Generate machine-readable `parity-audit.json`.
  **Method improvement** (v15 per Metis AI-FP-7): Use `grep -rn "pub fn\|pub struct\|pub enum" crates/<crate>/src/` directly — SKIP `cargo doc` (slow + flaky on broken crates).
  **Files**: `.sisyphus/evidence/parity-audit.json` (NEW), `.sisyphus/evidence/parity-audit.md`
  **Per crate**: pub fn count, pub struct count, pub enum count, verdict (GREEN/YELLOW/RED), purity (pure/stateful/unknown per Equivalence Contract tiers)
  **Acceptance**: JSON has 10 crate entries; each has verdict; plan §Crate Scope updated to reflect P0.4 verdict; if scope reduced (<5 GREEN), documented with rationale.
  **Parallelization**: YES — one agent per crate (10 parallel).
  **Blocks**: P5.2, all P3.x/P4.x port tasks.
  **Commit**: `docs(parity): authoritative crate inventory (P0.4)`

---

### PHASE 0.8 — Audit Remediation (v15 — per v3.2 audit)

> **Goal**: Remediate ALL 31 v3.2 findings + 3 high-severity FNs BEFORE self-host porting.
> **Source**: `.sisyphus/reports/buff-audit-2026-07-26-2019-v3.{json,md}`
> **Strategy**: Root-cause fixes cascade. cargo-deny gate kills dep-001/002/003 + hi-001/002/003. CI split kills cicd-001. Doc reconciliation kills prd-001 + coh-001.

#### v3.2 Finding → Task Mapping

| Finding | Severity | Description | Task |
|---------|----------|-------------|------|
| dep-001 | CRITICAL | No cargo-deny gate | P0.8 |
| dep-002 | CRITICAL | buff-registry rusqlite bundled (cc-rs) | P0.8 |
| dep-003 | CRITICAL | ring 0.17.14 via jsonwebtoken 9.3.1 (buff-auth) | P0.8 |
| cq-001 | CRITICAL | God fn lower_prelude_type_assoc_fn 3340 LOC | P0.14 (deferred) |
| cq-002 | CRITICAL | God fn lower_prelude_type_instance_fn 3501 LOC | P0.14 (deferred) |
| cicd-001 | CRITICAL | CI continue-on-error + --lib only | P0.11 |
| sec-001 | CRITICAL | buffup no checksum (RCE) | P0.9 |
| tc-001 | CRITICAL | crypto-extras zero tests | P0.16 |
| obs-001 | CRITICAL | buff-observe Tracer::bootstrap() never called | P0.15 |
| ft-001 | CRITICAL | buff-http-client no timeout | P0.17 |
| arch-001 | CRITICAL | buff-jobs Scheduler.start() never executes | P0.22 |
| sec-002 | CRITICAL | setup-buff curl\|sh no checksum | P0.10 |
| cicd-002 | CRITICAL | Homebrew FILL-ME sha256 | P0.23 |
| coh-001 | CRITICAL | Plan falsely claims tags deleted | P0.24 (DONE) |
| lic-001 | CRITICAL | License 3-way split | P0.13 |
| lms-001 | CRITICAL | .sisyphus/evidence/ gitignored | P0.20 |
| prd-001 | CRITICAL | AGENTS.md 52 commits stale | P0.12 |
| FP-2 | FALSE POS | ethers/bzip2 claim DISPROVEN | P0.8 (regression prevention) |
| hi-001..003 | HIGH | ring/libsqlite3-sys paths | P0.8 (cascades) |
| hi-004/005 | HIGH | OAuth CSRF + cookie flags | P0.25 (NEW v15) |
| hi-006/007 | HIGH | Actions/Docker mutable refs | P0.10 (cascades) |
| hi-008/009 | HIGH | God fns (codegen-rust) | P0.14 (deferred) |
| hi-010/011 | HIGH | Layer violations (LSP/eval) | P0.26 (NEW v15) |
| hi-012 | HIGH | buff-lang-cli god-crate | P0.26 |
| hi-013 | HIGH | OAuth token-exchange no timeout | P0.25 |
| hi-014 | HIGH | buff-jobs Worker backoff | P0.22 (companion) |
| FN-1 | HIGH FN | WebSocket hardening (ui_dev/server.rs) | P0.27 (NEW v15) |
| FN-2 | HIGH FN | Registry input validation | P0.28 (NEW v15) |
| FN-3 | HIGH FN | buff-web3 zero test coverage | P0.29 (NEW v15) |

#### Wave 0.8A — Supply Chain

- [ ] **P0.8 — cargo-deny hard CI gate** (dep-001/002/003 + FP-2 regression prevention)
  **What**: Add `deny.toml` banning cc/gcc/cmake/bindgen/pkg-config + non-allowlisted *-sys. Add CI job as HARD gate.
  **V3.2 VERIFIED cc-rs surface** (NOT v2.0's false claim): ring 0.16 (jsonwebtoken 8.3 + rustls), ring 0.17 (jsonwebtoken 9.3.1 + buff-auth), libsqlite3-sys (rusqlite bundled + sqlx).
  **FP-2 CORRECTION**: v2.0's "ethers smuggles bzip2-sys" was DISPROVEN — `cargo tree -i ethers-solc` → "nothing to print". Do NOT re-add ethers-solc bans.
  **Steps**: Create deny.toml; add CI job; fix ring paths (jsonwebtoken default-features=false OR allowlist ring as documented exception); fix libsqlite3-sys (revert buff-registry to in-memory OR document exception); verify `cargo deny check bans` exits 0.
  **Acceptance**: deny.toml exists; CI job is HARD gate; cargo-deny exits 0; ethers-solc stays disabled (FP-2 regression test).
  **Commit**: `ci: cargo-deny hard gate (P0.8)`

- [ ] **P0.9 — buffup SHA-256 checksum verification** (sec-001)
  **What**: After downloading tarball, fetch `.sha256` sidecar, compute local SHA-256, refuse on mismatch.
  **Files**: `crates/buffup/src/commands/install.rs`, `crates/buffup/src/github.rs`
  **Acceptance**: Checksum verified on every download; mismatch → clear error; `--skip-checksum` flag with security warning.
  **Commit**: `fix(buffup): SHA-256 verification (P0.9)`

- [ ] **P0.10 — Pin GitHub Actions to SHAs + Docker by digest** (sec-002 + hi-006/007)
  **What**: Replace @v4/@master with commit SHAs; pin Docker base by digest.
  **Files**: `.github/workflows/*.yml`, `docker/builder.Dockerfile`, `docker/slim.Dockerfile`
  **Acceptance**: All Action refs SHA-pinned; all Docker bases digest-pinned.
  **Commit**: `ci: SHA + digest pinning (P0.10)`

#### Wave 0.8B — CI Hardening

- [ ] **P0.11 — Split CI test job: gating-core + advisory-framework** (cicd-001)
  **What**: Replace `cargo test --lib` with `test-core` (gating, buff-lang-* only) + `test-framework` (advisory, continue-on-error + deadline).
  **Files**: `.github/workflows/ci.yml`
  **Acceptance**: test-core is HARD gate; test-framework advisory; integration tests run (not just --lib).
  **Commit**: `ci: split test job (P0.11)`

#### Wave 0.8C — Documentation

- [ ] **P0.12 — AGENTS.md + CHANGELOG + README reconciliation** (prd-001 + coh-001)
  **What**: Update AGENTS.md commit/branch/tag refs to match actual git state. Backfill CHANGELOG v1.26-v1.39 from tag annotations. Extend README Status table. Add pre-commit hook that regenerates metadata from git.
  **Key**: Tags v1.26.0-v1.39.0 EXIST and are canonical — reconcile docs to match, do NOT delete tags.
  **Files**: `AGENTS.md`, `CHANGELOG.md`, `README.md`, `.githooks/pre-commit` (NEW)
  **Acceptance**: AGENTS.md hash matches HEAD; branch = main; README has v1.26-v1.39 rows; pre-commit hook works; CI check: tag count == README row count.
  **Commit**: `docs: reconcile AGENTS+CHANGELOG+README (P0.12)`

- [ ] **P0.13 — LICENSE-APACHE + LICENSE-MIT** (lic-001)
  **What**: Add both license files; update README; retire single LICENSE.
  **Acceptance**: Both files exist with SPDX IDs; README references dual license; `cargo deny check licenses` passes.
  **Commit**: `legal: dual MIT/Apache license (P0.13)`

#### Wave 0.8D — Code Quality (deferred)

- [ ] **P0.14 — Document codegen-rust god-functions as deferred** (cq-001/002 + hi-008/009)
  **What**: codegen-rust is IMPOSSIBLE per DR-014. God-functions are real but deferred to v2.x MLIR effort.
  **Files**: `.sisyphus/decisions/codegen-rust-god-functions-deferred.md` (NEW DR)
  **Acceptance**: DR exists documenting deferral + references cq-001/002.
  **Commit**: `docs(codegen-rust): defer god-fn split (P0.14)`

#### Wave 0.8E — Observability

- [ ] **P0.15 — buff-observe: wire OR remove OR defer** (obs-001 + hi-021/022)
  **What**: buff-observe is vaporware (Tracer::bootstrap() never called, bootstrap_otlp returns Err, no /health endpoint). Decide: implement / remove / defer.
  **Recommended**: Defer (disabled feature + DR).
  **Acceptance**: Decision in DR; if implement: /health returns 200; if remove: workspace clean; if defer: feature disabled + DR.
  **Commit**: `decision(observe): defer to v2.x (P0.15)`

#### Wave 0.8F — Test Coverage + Resilience

- [ ] **P0.16 — buff-crypto-extras test vectors** (tc-001)
  **What**: Add NIST AES-GCM + RFC 8032 ECDH + RFC 9106 Argon2id + RSA round-trip tests. Fix AGENTS.md false claim.
  **Files**: `crates/buff-crypto-extras/tests/{aes,rsa,ecc,argon2}.rs` (NEW)
  **Acceptance**: 4 test files with vectors; all pass; AGENTS.md corrected.
  **Commit**: `test(crypto-extras): NIST/RFC vectors (P0.16)`

- [ ] **P0.17 — buff-http-client default timeout** (ft-001)
  **What**: Add `.timeout(Duration::from_secs(30)).connect_timeout(Duration::from_secs(10))` to Client builder.
  **Files**: `crates/buff-http-client/src/lib.rs`
  **Acceptance**: Default 30s timeout; configurable via builder; test: mock server timeout.
  **Commit**: `fix(http-client): default timeout (P0.17)`

- [ ] **P0.18 — buff-registry /health + /ready** (obs-003)
  **What**: Add /health (200 unconditional) + /ready (probes storage, 503 on failure).
  **Files**: `crates/buff-registry/src/lib.rs`
  **Acceptance**: /health 200; /ready 200/503 based on storage; documented.
  **Commit**: `feat(registry): health endpoints (P0.18)`

#### Wave 0.8G — Strategy Alignment

- [ ] **P0.19 — Strategy-practice misalignment** (framework examples can't `buff run`)
  **What**: v1.14-v1.23 framework crates ship but examples are parse-only/codegen-deferred. Relabel in README + flip CI severity.
  **Files**: `README.md`, `.github/workflows/ci.yml`
  **Acceptance**: README marks codegen-deferred; CI severity flipped or documented.
  **Commit**: `docs: honest codegen-deferred labeling (P0.19)`

#### Wave 0.8H — Evidence + NEW v15 Tasks

- [ ] **P0.20 — Evidence persistence** (lms-001)
  **What**: CI uploads `.sisyphus/evidence/` as artifact. MANIFEST.json with SHA-256 + artifact URLs.
  **Acceptance**: CI artifact upload works; MANIFEST.json exists.
  **Commit**: `ci(evidence): automate backup (P0.20)`

- [ ] **P0.22 — Fix buff-jobs Scheduler.start()** (arch-001 — NEW v14)
  **What**: Scheduler spawns task that ONLY updates next_fire timestamp — NEVER calls job handler. Add handler dispatch + error handling + backoff.
  **Files**: `crates/buff-jobs/src/scheduler.rs`
  **Acceptance**: Scheduled jobs execute; handler errors logged; integration test proves execution.
  **Commit**: `fix(jobs): Scheduler executes handlers (P0.22)`

- [ ] **P0.23 — Fix Homebrew sha256 placeholders** (cicd-002 — NEW v14)
  **What**: `installers/homebrew/buff.rb` has 4 `<FILL-ME: sha256>` placeholders. Compute real hashes + add CI check.
  **Files**: `installers/homebrew/buff.rb`
  **Acceptance**: All 4 placeholders replaced; `brew install` works; CI check for `<FILL-ME`.
  **Commit**: `fix(homebrew): sha256 placeholders (P0.23)`

- [ ] **P0.24 — Plan reconciliation with git state** (coh-001 — DONE in v15)
  **What**: This plan IS the fix — tags acknowledged as EXISTING, v3.2 audit mapped, all contradictions resolved.
  **Acceptance**: [x] No "deleted" claims; [x] v3.2 baseline; [x] All findings mapped; [x] FP-2 documented.
  **Commit**: `docs(plan): v15 reconciled with v3.2 audit (P0.24)`

- [ ] **P0.25 — buff-auth OAuth fixes** (hi-004/005/013 — NEW v15)
  **What**: Fix CSRF (state param dead_code), cookie missing Secure flag, token echoed in body, token-exchange no timeout.
  **Files**: `crates/buff-registry/src/oauth.rs`
  **Acceptance**: State param validated; Secure flag set; token not echoed; exchange has timeout.
  **Commit**: `fix(auth): OAuth CSRF + cookie + timeout (P0.25)`

- [ ] **P0.26 — Layer violation extraction** (hi-010/011/012 — NEW v15)
  **What**: Extract buff-lang-fmt, buff-lang-check, buff-lang-pipeline sibling crates to resolve: buff-lsp depends on entire buff-lang-cli for 1 fn; buff-eval uses #[path] cross-crate include; buff-lang-cli is a god-crate (75 files).
  **Files**: `crates/buff-lang-{fmt,check,pipeline}/` (NEW crates)
  **Acceptance**: buff-lsp no longer depends on buff-lang-cli; buff-eval no longer uses #[path]; buff-lang-cli reduced.
  **Commit**: `refactor: extract fmt/check/pipeline crates (P0.26)`

- [ ] **P0.27 — WebSocket hardening** (FN-1 — NEW v15)
  **What**: buff-lang-cli/src/ui_dev/server.rs has no WebSocket-specific attack surface protection (CSWSH, origin validation, message size limits, slow-loris-WS).
  **Files**: `crates/buff-lang-cli/src/ui_dev/server.rs`
  **Acceptance**: Origin validation; message size cap; connection lifecycle timeout.
  **Commit**: `fix(ui-dev): WebSocket hardening (P0.27)`

- [ ] **P0.28 — Registry input validation** (FN-2 — NEW v15)
  **What**: buff-registry accepts arbitrary bytes on publish — no package name sanitization, no path traversal checks.
  **Files**: `crates/buff-registry/src/handlers.rs`
  **Acceptance**: Package name sanitized (alphanumeric + hyphens only); path traversal blocked; size limits enforced.
  **Commit**: `fix(registry): input validation (P0.28)`

- [ ] **P0.29 — buff-web3 test coverage** (FN-3 — NEW v15)
  **What**: buff-web3 has zero test coverage for ethers ABI bindings.
  **Files**: `crates/buff-web3/tests/` (NEW)
  **Acceptance**: ABI binding round-trip tests; integration test with mock provider.
  **Commit**: `test(web3): ABI binding coverage (P0.29)`

- [ ] **P0.21 — Comprehensive high-finding remediation tracker**
  **What**: Track all 14 high findings in `.sisyphus/evidence/audit-remediation-tracker.md`. Most cascade from P0.8/P0.10/P0.14. Non-cascading (hi-004/005/010/011/012/013) have dedicated tasks above.
  **Acceptance**: Tracker exists; re-run `/audit all` at M7: 0 critical + 0 high.
  **Commit**: Multiple per theme.

---

### PHASE 0.5 — Validation Spikes (HARD GATE for Phase 1)

> **Exit criteria**: ALL spikes S1-S7 complete. If S1 OR S4 fails, update plan. If S1 AND S4 BOTH fail: ABORT.

- [ ] **S1 — dyn Trait end-to-end POC** (CRITICAL — validates P2.1 feasibility)
  **What**: Write `examples/spike_dyn_trait.buff` using `Vector<Box<dyn Trait>>`. Test if it parses + runs.
  **Acceptance**: `buff check` result documented; `buff run` result documented; verdict: FEASIBLE / NEEDS WORK / IMPOSSIBLE.
  **Commit**: `spike: dyn Trait POC — <verdict> (S1)`

- [ ] **S2 — insta rustc drift check** (validates toolchain stability)
  **What**: Run 100 snapshots on rustc 1.95.0. Document pass/fail.

- [ ] **S3 — HashMap audit** (CRITICAL — validates byte-identical possibility)
  **What**: Grep for HashMap in 10 target crates. If any portable crate uses HashMap, byte-identical output is impossible.
  **MOVED BEFORE P0.1.2b** (v15 per Metis UA-1): If S3 finds HashMap in parser/types, P0.1.2b determinism is broken.

- [ ] **S4 — LexCallback portability** (CRITICAL — validates lexer port)
  **What**: Read `crates/buff-lang-lexer/src/string_interp.rs`. Can Buff express recursive `&mut dyn LexCallback`?

- [ ] **S5 — Executor identification**
  **What**: Confirm executor (AI agent fleet). Verify: Pratt parser + syn/quote familiarity. (v15: AI execution confirmed.)

- [ ] **S6 — Harness on all 10 targets**
  **What**: Extend equivalence harness from 9 to 10 entries.

- [ ] **S7 — Unsafe audit**
  **What**: Document 28 unsafe blocks in codegen-rust (verified count). Verify 10 PORTABLE crates have ZERO unsafe.

---

### PHASE 0.6 — Performance Baseline

- [ ] **P0.6 — Baseline benchmark**
  **What**: Record build + execution times BEFORE porting. Save to `.sisyphus/evidence/baseline-benchmark.json`.
  **Acceptance**: Baseline JSON exists with build_time_sec, cold-start ms, execution ms.

- [ ] **P0.6.1 — Per-phase re-record protocol**
  **What**: After each phase, re-record benchmark. Compare to baseline. Gate: ≤3% regression per phase.

### PHASE 0.7 — Meta-Validation

- [ ] **P0.7 — Validate equivalence harness catches divergence**
  **What**: Inject deliberate divergence. Verify harness catches it.
  **Acceptance**: Injected divergence → harness FAILS.

---

### PHASE 1 — Compiler Bug Fixes

> **Goal**: Fix all `bug`-class transpile failures from P0.3 triage.
> **Exit criteria**: All `bug`-class failures fixed; baseline ratcheted beyond 7 files.

- [ ] **P1.1-P1.5** — Fix bug-class failures (LEX, PARSE-A, PARSE-B, PARSE-C, CODEGEN categories from triage). One task per category. Each task: read failing files, identify root cause, fix in Rust compiler, verify `buff check` now passes, ratchet baseline.

---

### PHASE 2 — Language Extensions: dyn Trait (IF S1 passes + extension cap not hit)

> **Abort if**: S1 = IMPOSSIBLE, OR extension cap (3 total) already reached.

- [ ] **P2.1a — Add TypeRef::Dyn variant** (extension #1 of max 3)
- [ ] **P2.1b — Add KwDyn token + lexer support**
- [ ] **P2.1c — Autoboxing + method dispatch**
- [ ] **P2.1d — Lint: warn on unnecessary dyn**
- [ ] **P2.1e — Error codes E14xx for dyn Trait errors**

---

### PHASE 3 — Tier 1: Complete Existing Logic Ports

> **Goal**: Complete ports that were started but not finished (in `crates/*/selfhost/`).

- [ ] **P3.1 — buff-lang-lexer port** (🟡 Medium)
- [ ] **P3.2 — buff-lang-error port** (🟡 Subset: Span only)
- [ ] **P3.3 — buff-lang-debug-info port** (🟡 Medium)
- [ ] **P3.4 — buff-lang-ffi-guide port** (✅ Trivial: 19 LOC docs)
- [ ] **P3.5 — buff-lang-ast-rsx port** (✅ Easy: 418 LOC)
- [ ] **P3.6-P3.8** — Remaining Tier 1 ports per P0.4 verdict

Each port: (1) Read Rust source, (2) Write .buff equivalent, (3) Run equivalence harness, (4) Fix divergences, (5) Ratchet baseline.

---

### PHASE 4 — Tier 2: Data-Only → Logic → Monolith

> **Dependency order**: P4.4 (data-only first) → P4.3 (ast) → P4.1 (parser) → P4.5-9 → P4.10 (monolith)

- [ ] **P4.4 — buff-lang-ast port** (✅ Most portable: 0 dyn-trait, pure data)
- [ ] **P4.3 — buff-lang-ast deeper port** (full pub fn coverage)
- [ ] **P4.1 — buff-lang-parser port** (🟡 Large: 14,204 LOC — CRITICAL PATH)
  **Abort**: exceeds 5 agent resume cycles on this single task.
  **AI failure point mitigation** (Metis AI-FP-1): Parser is too large for single agent context. Split into sub-tasks: P4.1a (expression parser), P4.1b (statement parser), P4.1c (type parser), P4.1d (module/import parser), P4.1e (integration test).
- [ ] **P4.5 — buff-lang-buffhtml-parser port** (🟡 Medium)
- [ ] **P4.6 — buff-eval port** (🟡 Medium)
- [ ] **P4.7 — buff-template port** (🟡 FFI dependency)
- [ ] **P4.8-P4.9** — Remaining per P0.4 verdict

- [ ] **P4.10 — buff_compiler.buff monolith** (M7 bootstrap proof)
  **AI failure point mitigation** (Metis AI-FP-1): Monolith = 5,000-8,000 LOC. Single agent will hit context limits. **MANDATORY split**:
  - P4.10a: compile_frontend() skeleton + main()
  - P4.10b: Inline lexer + dedup
  - P4.10c: Inline ast + dedup
  - P4.10d: Inline parser + dedup
  - P4.10e: Integration test on ola.buff
  - P4.10f: Integration test on 4 more files

---

### PHASE 5 — Full Parity Verification (CI-ONLY, Linux Docker)

> **Goal**: Exhaustive verification that all ports are byte-identical.

- [ ] **P5.1 — Coverage verification** (cargo-tarpaulin, tiered 90/85/80/75)
- [ ] **P5.2 — Exhaustive equivalence testing** (per P0.4 pub fn inventory)
  **Scale concern** (Metis SC-4): 650+ pub fns × 3 cases = ~1,950 tests. MUST run matrix-parallel (one CI job per crate, 10 jobs). Total CI time budget: ≤30 min.
- [ ] **P5.3 — Stateful snapshot protocol** (T4 fns — snapshot comparison)
  **Schema source**: P0.4 PARITY audit is the source of truth for snapshot fields. Agents implement against spec, not by reading other side's code (Metis AI-FP-5).
- [ ] **P5.4 — Property-based testing** (proptest, 1000+ random programs)
- [ ] **P5.5 — EMI differential testing** (research-grade; if budget insufficient, double P5.4)
- [ ] **P5.6-P5.10** — Performance regression, cross-platform, Oracle review, compliance report

---

### PHASE 6 — Framework Crate Fixes (parallel, Rust-side)

> **Scope clarification** (v15 per Metis SC-2): These are Rust-side API drift fixes, parallel to self-host. NOT self-host dependencies.

- [ ] **P6.1 — buff-fake fixes**
- [ ] **P6.2 — buff-fuzz fixes**
- [ ] **P6.3 — buff-jobs fixes** (companion to P0.22 — Worker backoff)
- [ ] **P6.4 — buff-web3 fixes** (companion to P0.29 — test coverage)

Note: buff-crypto-extras is covered by P0.16 (test vectors). buff-http-client by P0.17 (timeout).

---

## M7 — Front-End Bootstrap Milestone

> **Goal**: `buff_compiler.buff` monolith ingests 5+ .buff files → AST matching Rust parser.

- [ ] **M7.1a — Monolith produces byte-identical AST for ola.buff** (via `buff check --dump-ast` comparison)
- [ ] **M7.2 — Performance check**: parse time for buff_compiler.buff itself within 10% of baseline
- [ ] **M7.3 — Oracle compliance review** (VERIFIED / NOT_VERIFIED; 3× PARTIAL = escalate)

---

## Final Verification Wave (after ALL tasks)

- [ ] **F1 — Plan Compliance Audit** (oracle): Read plan end-to-end. For each Must Have: verify implementation. For each Must NOT Have: grep for violations.
  **Git-history check** (Metis AI-FP-6): `git diff main..self-host/v1 -- crates/buff-lang-*/src/ | grep "^+" | wc -l` must equal 0 (no Rust-original modifications in portable crates).
- [ ] **F2 — Code Quality Review**: tsc/clippy/test + AI slop check
- [ ] **F3 — Real Manual QA**: Execute EVERY QA scenario
- [ ] **F4 — Scope Fidelity**: 1:1 check — everything in spec built, nothing beyond spec

---

## Commit Strategy

- **1 commit per task** (P0.x, S.x, P.x, M7.x)
- **Pre-commit**: relevant test command (equivalence harness / cargo test / buff check)
- **Message format**: `type(scope): description (P0.x)`
- **Branch**: `self-host/v1` for port work; `main` for audit remediation
- **Rebase**: weekly from main into self-host/v1

---

## Success Criteria

### Verification Commands
```bash
cargo deny check bans           # Expected: exit 0
cargo test -p buff-lang-*       # Expected: all pass
bash scripts/equivalence-rust-vs-buff.sh  # Expected: 10/10 pass
buff check --dump-ast examples/ola.buff | jq .  # Expected: valid JSON
git diff main..self-host/v1 -- crates/buff-lang-*/src/ | wc -l  # Expected: 0
```

### Final Checklist
- [ ] All Must Have present
- [ ] All Must NOT Have absent (verified via git diff)
- [ ] All tests pass
- [ ] cargo-deny passes
- [ ] Equivalence harness passes
- [ ] M7 monolith produces byte-identical AST
- [ ] Oracle VERIFIED
- [ ] All 31 audit findings + 3 FNs resolved
- [ ] Migration guide written
- [ ] Deprecation Phase B defined
- [ ] ≤10% cumulative performance regression
