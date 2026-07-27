# Self-Host Completion Roadmap (v16 — OPTIMIZED with all 5-agent review fixes)

**Status:** READY FOR EXECUTION. Tags v1.0.0-v1.39.0 ALL canonical (14 tags EXIST per coh-001). AI-executed (no time estimates).
**Created:** 2026-07-26 (v16: fixes ALL issues from 2-round 5-agent review: Momus ✅OKAY, Oracle factual corrections, Metis B1-B7 blocks, Explore executability gaps)
**Governing authority:** T110 (`.sisyphus/decisions/buff-direction-speed-moat-selfhost.md`) — TRIAD: Speed + MOAT + Self-host-frontend
**Originating decision:** DR-014 (`.sisyphus/decisions/selfhost-feasibility.md`) — 10 potentially portable crates
**Audit baseline:** v3.2 audit (`.sisyphus/reports/buff-audit-2026-07-26-2019-v3.{json,md}`) — 31 verified findings (17 crit + 14 high) + FP-2 corrected

---

## TL;DR

> **Goal**: (1) Port 10 Rust compiler crates to Buff with behavioral parity per Equivalence Contract v2. (2) Remediate ALL 31 audit findings + 3 high-severity FNs. (3) Ship migration guide + deprecation plan.
> **Critical path** (CORRECTED v16 — removed 3 false deps): `S1 (multi_dispatch spike) → [P2.1 IF multi_dispatch insufficient] → P4.1 (parser) → P4.10 (monolith) → Phase 5 → M7`
> **Abort triggers**: S1 shows multi_dispatch insufficient AND adding dyn Trait infeasible; P4.1 >5 resume cycles; P4.10 >5 resume cycles; P0.4 <5 GREEN; perf >10%; 3rd Oracle NOT_VERIFIED; extension cap hit with unported crates remaining

---

## Context

### Original Request
"Convert all achievable Rust crates to Buff for self-hosting with identical behavior."

### Research Findings (CORRECTED v16 — Oracle verified against actual codebase)

| Claim | v15 (wrong) | v16 (corrected) | Source |
|-------|-------------|-----------------|--------|
| Parser LOC | 14,204 | **3,640** (across 6 files) | Oracle grep |
| Lexer LOC | 3,995 | **2,911** | Oracle grep |
| codegen-rust LOC | 58,617 | **15,786** (wall = syn-dep, not LOC) | Oracle grep |
| AST LOC | 6,547 | **5,246** (9 files) | Oracle grep |
| `Type::DynamicDispatch` at ty.rs:1192 | "exists" | **DOES NOT EXIST** — ty.rs is 164 lines | Oracle verified |
| Multiple dispatch | Not mentioned | **`multi_dispatch.rs` (500 LOC) ships in v1.19** — Julia-style multiple dispatch already available | Oracle discovered |
| Unsafe blocks in codegen-rust | 28 | **~11-14** (`unsafe {` blocks) | Oracle + Explore |
| Insta snapshots | 100 | **154** | Explore verified |
| Self-host .buff files | 56 | **56** ✅ | Confirmed |
| Tags v1.26-v1.39 | 14 EXIST | **14 EXIST** ✅ | 3/4 agents confirmed |
| `ui_dev/server.rs` | referenced in P0.27 | **DOES NOT EXIST** — WS code in `http.rs` + `broadcaster.rs` + `mod.rs` | Oracle verified |

**KEY DISCOVERY (v16)**: Buff v1.19 shipped `crates/buff-lang-types/src/analysis/multi_dispatch.rs` (500 LOC) — Julia-style multiple dispatch. This may provide the runtime semantics that `dyn Trait` would offer, potentially **eliminating Phase 2 entirely**. S1 spike now tests THIS, not TypeRef::Dyn addition.

### Crate Scope (per DR-014 — verified v16 with corrected LOC)

**12 IMPOSSIBLE** (DR-014 §🟥): `buff-lang-codegen-rust` (15,786 LOC), `buff-lang-types` (27,146†), `buff-lang-runtime` (11,433), `buff-lang-codegen-buffhtml` (2,559), `buff-lang-codegen-wgsl` (1,995), `buff-registry` (5,396), `buff-jupyter` (4,839), `buff-lsp` (3,722), `buff-dap` (2,214), `buff-ui-dioxus` (1,053), `buff-playground-wasm` (366), `buff-mcp` (1,603). (†DR-014 LOC includes tests/; wall = 18 dyn-trait usages + Hindley-Milner inference, not LOC.)

**Not ported**: `buff-lang-cli` (monolith HOST), `buff-repl` (rustyline FFI)

**~40 Framework Wrappers** (DR-014 §🟧 — category error): `buff-dataframe`, `buff-tensor`, `buff-image`, `buff-pubsub`, `buff-fsm`, etc. — "Buff's PRODUCT, not self-host candidates."

**10 TARGET crates** (DR-014 §🟩, corrected LOC):

| # | Crate | Actual LOC | Files | DR-014 Verdict |
|---|-------|-----------|-------|----------------|
| 1 | `buff-lang-ast` | 5,246 | 9 (common, decl, expr, ir, lib, lossless, op, stmt, ty) | ✅ Most portable (0 dyn-trait) |
| 2 | `buff-lang-ast-rsx` | 418 | 1 | ✅ Easy (tiny, pure data) |
| 3 | `buff-lang-error` | 2,404 | 3 (span, types, code) | 🟡 Subset (Span port, thiserror doesn't) |
| 4 | `buff-lang-debug-info` | 1,117 | 2 | 🟡 Medium (data + source map) |
| 5 | `buff-lang-lexer` | 2,911 | 5 (lexer, string_interp, token, indent_tracker, stream) | 🟡 Medium (byte scanner) |
| 6 | `buff-lang-parser` | 3,640 | 6 (expr, lib, options, parser, stmt, stream) | 🟡 Medium (recursive descent + Pratt) |
| 7 | `buff-lang-buffhtml-parser` | 2,748 | 4 | 🟡 Medium (3-mode lexer) |
| 8 | `buff-lang-ffi-guide` | 0 (MD only) | 1 (GUIDE.md) | ✅ Trivial (docs) |
| 9 | `buff-eval` | 865 | 2 | 🟡 Medium (thin eval) |
| 10 | `buff-template` | 150 | 2 | 🟡 Depends on tera/handlebars FFI |

P0.4 produces the AUTHORITATIVE verdict per crate.

---

## Work Objectives

### Core Objective
Port 10 crates to Buff with **behavioral parity per Equivalence Contract v2** (not raw "byte-identical" — that's impossible for .buff vs Rust source due to span differences).

### Definition of Done
1. P0.4-authoritative crate list at tiered coverage parity (90/85/80/75)
2. M7: `buff_compiler.buff` ingests 5+ .buff files → AST matching Rust parser output (via `buff check --dump-ast` with span-normalized comparison)
3. Performance: ≤3% regression per phase, ≤10% cumulative, ABORT >10%
4. Oracle issues VERIFIED
5. ALL 31 audit findings resolved (FIXED or formally DEFERRED with DR)

### Must NOT Have (Guardrails)
- Any of the 12 IMPOSSIBLE crates ported
- Framework wrapper crates ported (category error per DR-014)
- Multi-file linking (T29 — monolith workaround)
- Rust backend replaced (T110 forbids)
- Raw-string codegen (project rule)
- **Rust originals in TARGET crates modified to make equivalence pass** (F1 verifies: `git diff main..self-host/v1 -- crates/buff-lang-{ast,ast-rsx,error,debug-info,lexer,parser,buffhtml-parser,ffi-guide}/src/ | grep "^+" | wc -l` must equal 0; NOTE: Phase 1 bug fixes modify Rust compiler on `main` branch, NOT on `self-host/v1` — F1 greps `self-host/v1` only)
- **Buff features that Rust original lacks** (faithful port only)
- **Error message divergence** (text + error codes byte-for-byte; spans compared AFTER normalization per Contract §Span Normalization)
- **More than 3 Buff language extensions** (hard cap with enforcement per §Extension Cap)

### Equivalence Contract v2 (Metis B1 fix — span normalization defined)

> **Core principle**: Compare STRUCTURAL EQUALITY of semantic output, not raw byte equality of source-derived metadata.

| Tier | Scope | Comparison Method |
|------|-------|-------------------|
| **T1: Pure-value** | Fns returning Bool, Int, String, struct | Canonical JSON: `serde_json::to_string` (compact, no whitespace) with `BTreeMap`-sorted keys. Byte-identical. |
| **T2: Collection** | Fns returning Vec, Map, Set | Serialize to `BTreeMap<JsonKey, JsonValue>`. **Vec ordering IS significant** — compared element-by-element, NOT sorted. Only HashMap-sourced collections are sorted before comparison. |
| **T3: Timestamped** | Fns with volatile fields (time, UUID, random) | Structural equality with **explicit volatile-field allowlist** per fn (recorded in `parity-audit.json` `volatile_fields[]`). Non-volatile fields must match exactly. |
| **T4: Stateful** | Fns with internal state, tokio | Snapshot protocol (§Snapshot Schema below). Two runs compared via snapshot diff. |

**Span Normalization** (fixes Metis B1):
- Spans in `.buff` port reference DIFFERENT byte offsets than Rust source (different file content).
- Comparison method: **normalize spans to token-sequence positions** — span.start/end are converted to `{token_index, offset_within_token}` before comparison. Both sides produce the same token sequence (that's what equivalence means), so normalized spans match.
- `buff check --dump-ast` (P0.1.2b) emits BOTH raw spans AND normalized spans. Comparison uses normalized.

**Float Policy**: Compare as `format!("{:.15}", value)` strings (15 significant figures). If both sides produce the same 15-sig-fig string, they're equivalent. Bit-pattern comparison is NOT required (different optimization levels produce different bit patterns for same mathematical result).

**Error Comparison**: Error text must match byte-for-byte EXCEPT for span values (which use normalization above). Error CODES (E10xx/E11xx/E12xx/E13xx) must match exactly. Errors sorted by `normalized_span.start` before comparison.

**Async Output Ordering**: For T4 fns where tokio task completion order varies, compare output as a **sorted set** (sort by normalized content hash). Two runs with different ordering but same set = PASS.

### Snapshot Schema (Metis B4 fix — P5.3 unblocked)

```json
{
  "snapshot_version": "1.0",
  "function": "pub fn name",
  "crate": "buff-lang-parser",
  "input": "<JSON-serialized input args>",
  "output_type": "T1|T2|T3|T4",
  "volatile_fields": ["timestamp", "uuid"],
  "state_before": "<JSON of internal state before call>",
  "state_after": "<JSON of internal state after call>",
  "result": "<JSON of return value>",
  "errors_emitted": [{"code": "E1101", "message": "...", "normalized_span": {"token_index": 5, "offset": 0}}]
}
```

Snapshot files: `.sisyphus/evidence/snapshots/{crate}/{fn_name}.json`. P0.4 inventories which fns need snapshots (T4 classification).

### Extension Cap Enforcement (Metis Q4 fix)

**Definition**: A "Buff language extension" is any of: {new keyword, new AST node variant, new prelude TYPE (not function), new syntax form, new attribute}. Prelude FUNCTIONS and new error codes do NOT count.

**Counter file**: `.sisyphus/evidence/extensions-counter.json`:
```json
{"used": 0, "max": 3, "items": [], "reserved": [{"slot": 1, "feature": "multiple_dispatch_test", "status": "testing"}]}
```

**Enforcement**: Each extension commit MUST atomically update this file (increment `used`, append to `items`). F1 final audit verifies `used ≤ max`. Pre-commit hook rejects if `used > max`.

**Escape valve**: If >3 genuine gaps found during spikes, re-scope target crate list (drop affected crate) rather than raise cap. Do NOT raise cap above 3.

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification agent-executed.

### CI Strategy (Metis B7 fix — matrix-parallel)

P5.2 testing: 650+ pub fns × 3 cases = ~1,950 tests. Strategy:
- **Matrix**: one CI job per crate (10 jobs, well under GitHub's 20-concurrent limit)
- **Per-crate**: ~65 tests × 5s = ~5 min per job
- **Total wall-clock**: ~5-10 min (all 10 parallel)
- **Budget cap**: if any single job >15 min, split further

### Phase Exit Gates (concrete commands per phase)

Each phase has a single bash one-liner that returns 0 iff phase complete:
```bash
# Phase 0 exit: CI infrastructure + inventory ready
cargo deny check bans && cargo test -p buff-lang-{ast,error,lexer,parser} --lib && test -f .sisyphus/evidence/parity-audit.json

# Phase 0.8 exit: all audit findings tracked
test -f .sisyphus/evidence/audit-remediation-tracker.md && grep -c "FIXED\|DEFERRED" .sisyphus/evidence/audit-remediation-tracker.md | grep -q "31"

# Phase 1 exit: all bug-class failures fixed
bash scripts/equivalence-rust-vs-buff.sh && buff check self-host/parser/parser.buff

# M7 exit: monolith produces AST
buff check --dump-ast examples/ola.buff | jq '.declarations | length' | grep -v '^0$'
```

---

## Execution Strategy

### Branch Strategy (Metis B2 fix — F1 git-diff corrected)
- **`main`**: Audit remediation (P0.8-P0.29), Phase 1 bug fixes (modify Rust compiler), CI changes
- **`self-host/v1`**: Port work (P3.x, P4.x, P4.10, M7) — branched from `main` AFTER P0.1.2b lands
- **F1 grep**: `git diff main..self-host/v1 -- crates/buff-lang-{ast,error,debug-info,lexer,parser,buffhtml-parser,ffi-guide,ast-rsx}/src/` — checks self-host/v1 ONLY (Phase 1 bug fixes on `main` don't trigger)

### File Edit Queues (Oracle finding — 5 collision risks)

| File | Queue (sequential) | Parallel OK |
|------|-------------------|-------------|
| `.github/workflows/ci.yml` | P0.8 → P0.11 → P0.1 → P0.20 | All others |
| `Cargo.toml` (root) | P0.8 (deny) → P0.13 (license) → P0.26 (new crates) | All others |
| `AGENTS.md` | P0.12 (reconcile) → P0.14 (DR ref) → P0.16 (fix claim) | All others |
| `README.md` | P0.12 → P0.13 → P0.19 | All others |
| `crates/buff-registry/src/lib.rs` | P0.18 (routes) → P0.25 (oauth) → P0.28 (validation) | All others |

### Wave Structure (CORRECTED v16 — removed false deps)

```
Wave 0 (Foundation + Audit — PARALLEL tracks):
  Track A (main):     P0.8-P0.29 (audit remediation, respecting edit queues)
  Track B (self-host): P0.1-P0.4 + P0.1.2a/b + S1-S7 + P0.6-P0.7

Wave 1 (Bug Fixes — main branch):
  Phase 1: P1.1-P1.5 (fix Rust compiler transpile bugs)

Wave 2 (Language Extension — IF S1 shows multi_dispatch insufficient):
  Phase 2: P2.1 (ONLY if needed; multi_dispatch.rs may eliminate this)

Wave 3 (Tier 1 Ports — self-host/v1):
  Phase 3: P3.1-P3.8 (all parallel after P0.4 verdict)

Wave 4 (Tier 2 Ports — self-host/v1, CORRECTED critical path):
  P4.4 (ast, pure data) ∥ P4.1 (parser, 2-way split) ∥ P4.5-P4.9
  THEN P4.10 (monolith) after all ports done

Wave 5 (Framework Fixes — main, parallel):
  Phase 6: P6.1-P6.4

Wave 6 (Verification — CI only):
  Phase 5: P5.1-P5.10

Wave 7 (Bootstrap):
  M7: M7.1a-c
```

**Critical path** (TRUE deps only): `S1 → [P2.1 IF needed] → P4.1 → P4.10 → Phase 5 → M7`

---

## TODOs

### WAVE 0.0 — Pre-Execution Specs (Metis PP-1 through PP-5, inline)

> These are SPECS, not code. Complete before any Wave 0 work. All are writing tasks.

- [ ] **P0.0 — Author Porting Conventions doc** (Metis PP-2)
  **What**: Create `.sisyphus/decisions/porting-conventions.md` specifying: `.buff` file header format, naming conventions (snake_case preserved), comment policy (preserve Rust comments), error-handling pattern (Buff has no `?` — use Result propagation), test file layout (`tests/equivalence_{fn_name}.buff`), import ordering, module structure. ALL 10 port agents MUST read this as first step.
  **Acceptance**: Doc covers 7+ convention categories with before/after snippets per convention.
  **Commit**: `docs: porting conventions (P0.0)`

- [ ] **P0.0.1 — Initialize extensions-counter.json** (Metis PP-4)
  **What**: Create `.sisyphus/evidence/extensions-counter.json` with `{"used": 0, "max": 3, "items": [], "reserved": []}`. Add pre-commit hook (`.githooks/pre-commit`) that rejects commits where `extensions-counter.json` has `used > max`.
  **Acceptance**: File exists; pre-commit hook executable; `echo '{"used":4}' > extensions-counter.json && git commit` gets REJECTED.
  **Commit**: `ci: extension cap enforcement (P0.0.1)`

### PHASE 0 — Foundation & Triage

- [ ] **P0.1 — Equivalence harness as required CI check**
  **Files**: `.github/workflows/ci.yml`
  **What**: Add `equivalence-check` job: ubuntu-only, Docker, builds buff-lang-cli release, runs `scripts/equivalence-rust-vs-buff.sh` (9 tests), HARD gate (no continue-on-error).
  **Detection**: `grep 'equivalence-check' .github/workflows/ci.yml` returns match.
  **Acceptance**: Job on every PR; 9/9 tests pass; deliberate divergence blocks merge.
  **Edit queue**: After P0.8, P0.11 in ci.yml queue.
  **Commit**: `ci: equivalence-check hard gate (P0.1)`

- [ ] **P0.1.2a — `buff check --dump-ast` flag plumbing**
  **Files**: `crates/buff-lang-cli/src/cli.rs` (add `DumpAst(bool)` to Command enum), `crates/buff-lang-cli/src/check.rs` (wire flag — after successful parse, if flag set, print stub message)
  **Detection**: `cargo run -p buff-lang-cli -- check --dump-ast examples/ola.buff` prints output.
  **Acceptance**: Flag in `--help`; prints something (stub OK for now).
  **Commit**: `feat(check): --dump-ast flag (P0.1.2a)`

- [ ] **P0.1.2b — AST JSON serializers (BTreeMap-ordered, deterministic)**
  **Files**: ALL 9 AST files: `crates/buff-lang-ast/src/{common,decl,expr,ir,lib,lossless,op,stmt,ty}.rs` (add `to_json() -> serde_json::Value` per node type — ~100 enum variants total)
  **Code pattern** (apply identically across all 9 files):
  ```rust
  // In each enum/struct definition:
  pub fn to_json(&self) -> serde_json::Value {
      use serde_json::json;
      match self {
          Self::VariantA(field) => json!({"type": "VariantA", "field": field.to_json()}),
          // ... one arm per variant
      }
  }
  // Use serde_json::Value (BTreeMap-backed by default for objects)
  // Spans: emit as {"token_index": N, "offset": N, "raw": {"start": N, "end": N}}
  ```
  **PREREQUISITE**: S3 (HashMap audit) MUST run first. If S3 finds HashMap in AST code, determinism is broken — must switch to BTreeMap in Rust original BEFORE implementing serializers.
  **Parallelization**: 9 parallel agents (one per file) — each implements `to_json()` for nodes in that file.
  **Detection**: `cargo run -p buff-lang-cli -- check --dump-ast examples/ola.buff | jq .` produces valid JSON; run twice → byte-identical.
  **Acceptance**: Valid JSON; deterministic (2 runs = identical); `jq .` accepts.
  **Commit**: `feat(check): AST JSON serializers (P0.1.2b)`

- [ ] **P0.2 — Self-host-check hard gate; lock 7-file baseline**
  **Files**: `.github/workflows/ci.yml` (~line 162-193)
  **What**: Remove `continue-on-error: true` from `self-host-check`. Lock 7 baseline files in `self-host/` directory (NOT `crates/buff-lang-cli/selfhost/`): `parser/expr_pattern.buff`, `parser/expr_postfix.buff`, `parser/parser.buff`, `parser/stmt.buff`, `parser/stream.buff`, `types/lib.buff`, `types/prelude_types.buff`.
  **Detection**: `grep 'continue-on-error' .github/workflows/ci.yml | grep -c 'self-host-check'` returns 0.
  **Acceptance**: continue-on-error removed; 7 files listed; breaking any FAILS CI.
  **Commit**: `ci: lock self-host baseline (P0.2)`

- [ ] **P0.3 — Triage 49 transpile failures**
  **Files**: `self-host/triage.md` (NEW)
  **What**: Independently re-run `buff check` on EACH of the 56 self-host/*.buff files (do NOT trust bootstrap-report categorization blindly). Classify each failure as: `bug` (Rust compiler fix), `lang-gap` (needs Buff extension), `unsupported` (fundamental limit), or `unknown` (needs spike). Cap `unknown` at 10.
  **Template**:
  ```markdown
  | File | Stage | Classification | Root Cause | Fix Phase |
  |------|-------|---------------|------------|-----------|
  | parser/expr_pattern.buff | PARSE-A | bug | Pratt binding table missing `as` | P1.2 |
  ```
  **Detection**: `grep -c '|' self-host/triage.md` ≥ 56 (header + 49 failures + summary).
  **Acceptance**: All 49 failures classified; counts: bugs + lang-gaps + unsupported + unknown = 49; unknown ≤ 10.
  **Commit**: `docs: self-host triage (P0.3)`

- [ ] **P0.4 — PARITY audit (CRITICAL UNBLOCK)**
  **Files**: `.sisyphus/evidence/parity-audit.json` (NEW), `.sisyphus/evidence/parity-audit.md`
  **What**: For each of 10 target crates: count pub fns/structs/enums, classify portability (GREEN/YELLOW/RED), classify purity (T1/T2/T3/T4 per Contract), list volatile fields (T3 only).
  **Method**: `grep -rn "pub fn\|pub struct\|pub enum" crates/<crate>/src/` directly — SKIP `cargo doc` (slow + flaky).
  **JSON schema**:
  ```json
  {
    "schema_version": "1.0",
    "crates": [{
      "name": "buff-lang-ast",
      "loc": 5246,
      "files": ["common.rs", "decl.rs", "expr.rs", "ir.rs", "lib.rs", "lossless.rs", "op.rs", "stmt.rs", "ty.rs"],
      "pub_fns": 45, "pub_structs": 12, "pub_enums": 8,
      "verdict": "GREEN",
      "green_count": 60, "yellow_count": 5, "red_count": 0,
      "purity": {"T1_pure": 40, "T2_collection": 15, "T3_volatile": 5, "T4_stateful": 5},
      "volatile_fields": {"T3_fn_name": ["timestamp"]},
      "yellow_detail": ["expr.rs uses trait object for visitor pattern"],
      "red_detail": []
    }]
  }
  ```
  **Detection**: `jq '.crates | length' parity-audit.json` returns 10.
  **Acceptance**: 10 crate entries; each has verdict; if <5 GREEN, documented rationale.
  **Parallelization**: 10 parallel agents (one per crate).
  **Commit**: `docs(parity): authoritative inventory (P0.4)`

---

### PHASE 0.8 — Audit Remediation

> **Source**: `.sisyphus/reports/buff-audit-2026-07-26-2019-v3.{json,md}`
> **Finding IDs**: These are the ACTUAL IDs from the v3.2 audit JSON (NOT invented hi-001..014).

#### Finding → Task Mapping (v16 CORRECTED — actual audit IDs)

| Finding ID | Sev | Description | Task |
|------------|-----|-------------|------|
| dep-001 | CRIT | No cargo-deny gate | P0.8 |
| dep-002 | CRIT | buff-registry rusqlite bundled | P0.8 |
| dep-003 | CRIT | ring 0.17 via jsonwebtoken 9.3 (buff-auth) | P0.8 |
| cq-001 | CRIT | God fn assoc_fn 3340 LOC | P0.14 (defer) |
| cq-002 | CRIT | God fn instance_fn 3501 LOC | P0.14 (defer) |
| cicd-001 | CRIT | CI continue-on-error + --lib | P0.11 |
| sec-001 | CRIT | buffup no checksum (RCE) | P0.9 |
| tc-001 | CRIT | crypto-extras zero tests | P0.16 |
| obs-001 | CRIT | Tracer::bootstrap() never called | P0.15 |
| ft-001 | CRIT | http-client no timeout | P0.17 |
| arch-001 | CRIT | Scheduler.start() never executes | P0.22 |
| sec-002 | CRIT | setup-buff curl\|sh | P0.10 |
| cicd-002 | CRIT | Homebrew FILL-ME sha256 | P0.23 |
| coh-001 | CRIT | Plan false tags-deleted claim | P0.24 (DONE) |
| lic-001 | CRIT | License 3-way split | P0.13 |
| lms-001 | CRIT | evidence/ gitignored | P0.20 |
| prd-001 | CRIT | AGENTS.md stale | P0.12 |
| FP-2 | FP | ethers/bzip2 DISPROVEN | P0.8 (regression prevention) |
| cicd-003 | HIGH | Actions mutable @v4 refs | P0.10 |
| cicd-004 | HIGH | buff-validation ::notice not ::error | P0.11 |
| cicd-005 | HIGH | No permissions block (4/6 workflows) | P0.10 |
| cq-003 | HIGH | generate() third god-fn 1038 LOC | P0.14 (defer) |
| arch-002 | HIGH | buff-lsp depends on buff-lang-cli for 1 fn | P0.26 |
| arch-003 | HIGH | buff-eval #[path] cross-crate include | P0.26 |
| ft-002 | HIGH | doc.rs Mutex::lock().unwrap() panics | P0.25 |
| ft-003 | HIGH | buff-resilience Timeout leaks thread | P0.25 |
| obs-002 | HIGH | bootstrap_otlp permanently returns Err | P0.15 |
| obs-003 | HIGH | No /health or /ready endpoint | P0.18 |
| prd-002 | HIGH | README Status stops at v1.24 | P0.12 |
| prd-003 | HIGH | CHANGELOG stops at v1.25 | P0.12 |
| sec-003 | HIGH | OAuth state param dead_code (CSRF) | P0.25 |
| sec-004 | HIGH | OAuth cookie missing Secure + token echoed | P0.25 |
| FN-1 | FN | WebSocket hardening (ui_dev/) | P0.27 |
| FN-2 | FN | Registry input validation | P0.28 |
| FN-3 | FN | buff-web3 zero test coverage | P0.29 |

#### Wave 0.8A — Supply Chain

- [ ] **P0.8 — cargo-deny hard gate** (dep-001/002/003 + FP-2 prevention)
  **Files**: `deny.toml` (NEW), `.github/workflows/ci.yml`
  **deny.toml skeleton**:
  ```toml
  [bans]
  deny = [
      { name = "cc", version = "*" },
      { name = "gcc", version = "*" },
      { name = "cmake", version = "*" },
      { name = "bindgen", version = "*" },
      { name = "pkg-config", version = "*" },
  ]
  # Allowlist *-sys on case-by-case basis
  multiple-versions = "warn"
  ```
  **What**: Create deny.toml; add cargo-deny CI job (HARD gate); fix ring paths (jsonwebtoken `default-features=false` OR ring allowlist DR); fix libsqlite3-sys (revert buff-registry to in-memory OR DR); verify `cargo deny check bans` exits 0.
  **FP-2 prevention**: Do NOT re-add ethers-solc bans (v2.0 false claim). Verify: `cargo tree -i ethers-solc` → "nothing to print".
  **Detection**: `cargo deny check bans; echo $?` returns 0.
  **Acceptance**: deny.toml exists; CI HARD gate; exits 0; ethers-solc stays disabled.
  **Edit queue**: First in ci.yml queue.
  **Commit**: `ci: cargo-deny gate (P0.8)`

- [ ] **P0.9 — buffup SHA-256 verification** (sec-001)
  **Files**: `crates/buffup/src/commands/install.rs:60-67`, `crates/buffup/src/github.rs:75-97`
  **What**: Fetch `.sha256` sidecar, compute local SHA-256 (`sha2::Sha256::digest(&tarball_bytes)`), refuse on mismatch. Add `--skip-checksum` flag with security warning.
  **Detection**: `grep -r 'sha256\|Sha256' crates/buffup/src/` returns matches.
  **Acceptance**: Checksum verified; mismatch → error; --skip-checksum documented.
  **Commit**: `fix(buffup): checksum verification (P0.9)`

- [ ] **P0.10 — Pin Actions to SHAs + Docker by digest + permissions blocks** (sec-002 + cicd-003/005)
  **Files**: `.github/workflows/*.yml`, `docker/builder.Dockerfile`, `docker/slim.Dockerfile`
  **What**: Replace `@v4`/`@master` with SHA pins. Pin Docker base by digest. Add `permissions:` block to all 6 workflows (4 currently missing).
  **Detection**: `grep -r '@v[0-9]\|@master' .github/workflows/` returns 0 matches.
  **Acceptance**: All SHAs pinned; all Docker digests pinned; all workflows have permissions block.
  **Commit**: `ci: SHA pinning + permissions (P0.10)`

#### Wave 0.8B — CI Hardening

- [ ] **P0.11 — Split CI test job** (cicd-001 + cicd-004)
  **Files**: `.github/workflows/ci.yml`
  **What**: Replace `cargo test --lib` with: `test-core` (gating, `cargo test -p buff-lang-*`, HARD gate) + `test-framework` (advisory, `cargo test --workspace --exclude buff-lang-*`, continue-on-error + 15min deadline). Flip buff-validation `::notice` → `::error`.
  **Detection**: `grep 'test-core\|test-framework' .github/workflows/ci.yml` returns matches.
  **Acceptance**: test-core HARD gate; test-framework advisory; integration tests run; buff-validation is ::error.
  **Edit queue**: Second in ci.yml queue.
  **Commit**: `ci: split test job (P0.11)`

#### Wave 0.8C — Documentation

- [ ] **P0.12 — AGENTS.md + CHANGELOG + README reconciliation** (prd-001/002/003)
  **Files**: `AGENTS.md:3-6` (commit `0467fbd`→HEAD, branch `v1x-frameworks`→`main`), `CHANGELOG.md` (backfill v1.26-v1.39 from tag annotations), `README.md` (extend Status table to v1.39), `.githooks/pre-commit` (NEW — regenerate metadata from git)
  **Detection**: `grep "$(git rev-parse --short HEAD)" AGENTS.md` returns match.
  **Acceptance**: AGENTS.md matches HEAD; branch=main; README has v1.26-v1.39 rows; pre-commit hook works.
  **Edit queue**: First in AGENTS.md queue. First in README.md queue.
  **Commit**: `docs: reconcile metadata (P0.12)`

- [ ] **P0.13 — LICENSE-APACHE + LICENSE-MIT** (lic-001)
  **Files**: `LICENSE-APACHE` (NEW), `LICENSE-MIT` (rename from LICENSE), `README.md` (## License section)
  **Detection**: `test -f LICENSE-APACHE && test -f LICENSE-MIT` returns 0.
  **Acceptance**: Both files exist; README references dual license; `cargo deny check licenses` passes.
  **Edit queue**: Second in README.md queue.
  **Commit**: `legal: dual license (P0.13)`

#### Wave 0.8D-G — Remaining Audit Tasks

- [ ] **P0.14 — Document codegen-rust god-functions as deferred** (cq-001/002/003)
  **Files**: `.sisyphus/decisions/codegen-rust-god-functions-deferred.md` (NEW DR)
  **Acceptance**: DR documents deferral + references cq-001/002/003.
  **Edit queue**: Second in AGENTS.md queue.
  **Commit**: `docs(codegen-rust): defer god-fn split (P0.14)`

- [ ] **P0.15 — buff-observe: defer (obs-001/002)** — Write DR documenting deferral. Feature-gate the crate. **Commit**: `decision(observe): defer (P0.15)`

- [ ] **P0.16 — crypto-extras test vectors** (tc-001) — Add `crates/buff-crypto-extras/tests/{aes,rsa,ecc,argon2}.rs` with NIST/RFC vectors. Fix AGENTS.md false claim. **Edit queue**: Third in AGENTS.md queue. **Commit**: `test(crypto-extras): NIST vectors (P0.16)`

- [ ] **P0.17 — http-client default timeout** (ft-001) — Add `.timeout(Duration::from_secs(30)).connect_timeout(Duration::from_secs(10))` to `crates/buff-http-client/src/lib.rs:75-82`. **Commit**: `fix(http-client): timeout (P0.17)`

- [ ] **P0.18 — registry /health + /ready** (obs-003) — Add to `crates/buff-registry/src/lib.rs:257-282`. **Edit queue**: First in registry/lib.rs queue. **Commit**: `feat(registry): health endpoints (P0.18)`

- [ ] **P0.19 — Strategy-practice relabel** — Mark codegen-deferred examples in README; flip buff-validation severity. **Edit queue**: Third in README.md queue. **Commit**: `docs: honest labeling (P0.19)`

- [ ] **P0.20 — Evidence persistence** (lms-001) — CI artifact upload + MANIFEST.json. **Edit queue**: Fourth in ci.yml queue. **Commit**: `ci(evidence): artifact backup (P0.20)`

- [ ] **P0.22 — buff-jobs Scheduler.start() executes handlers** (arch-001) — Add handler dispatch in `crates/buff-jobs/src/scheduler.rs:87-102` after `next_fire` update. Add Worker backoff (hi-014 companion). Integration test proves execution. **Commit**: `fix(jobs): scheduler executes (P0.22)`

- [ ] **P0.23 — Homebrew sha256 placeholders** (cicd-002) — Compute real hashes for `installers/homebrew/buff.rb:23,28,35,40`. Add CI check for `<FILL-ME`. **Commit**: `fix(homebrew): sha256 (P0.23)`

- [ ] **P0.24 — Plan reconciliation** (coh-001) — DONE in v16 (tags acknowledged as EXISTING, all contradictions resolved). **Commit**: `docs(plan): v16 reconciled (P0.24)`

- [ ] **P0.25 — buff-auth OAuth + buff-resilience timeout + doc.rs Mutex** (sec-003/004 + ft-002/003)
  **Files**: `crates/buff-registry/src/oauth.rs` (CSRF state, Secure cookie, no token echo, exchange timeout), `crates/buff-resilience/src/lib.rs:485` (timeout thread leak), `crates/buff-lang-cli/src/commands/doc.rs:1233,1250` (Mutex::lock().unwrap())
  **Edit queue**: Second in registry/lib.rs queue.
  **Acceptance**: State param validated; Secure flag set; token not echoed; exchange has timeout; no thread leak; no unwrap on Mutex.
  **Commit**: `fix(auth+resilience): OAuth + timeout + Mutex (P0.25)`

- [ ] **P0.26 — Layer violation: extract buff-lang-{fmt,check,pipeline}** (arch-002/003)
  **What**: Extract sibling crates so buff-lsp doesn't depend on buff-lang-cli, buff-eval doesn't use #[path].
  **Files**: `crates/buff-lang-{fmt,check,pipeline}/` (NEW crates), `Cargo.toml` (workspace members)
  **Detection**: `cargo tree -p buff-lsp | grep buff-lang-cli` returns nothing.
  **Acceptance**: buff-lsp no longer depends on buff-lang-cli; buff-eval no #[path]; workspace clean.
  **Edit queue**: Second in Cargo.toml queue.
  **Commit**: `refactor: extract sibling crates (P0.26)`

- [ ] **P0.27 — WebSocket hardening** (FN-1)
  **Files**: `crates/buff-lang-cli/src/ui_dev/http.rs`, `broadcaster.rs`, `mod.rs` (NOT `server.rs` — doesn't exist per Oracle)
  **What**: Add origin validation, message size cap (1MB default), connection lifecycle timeout (60s idle).
  **Detection**: `grep -r 'origin\|max_size\|idle_timeout' crates/buff-lang-cli/src/ui_dev/` returns matches.
  **Commit**: `fix(ui-dev): WebSocket hardening (P0.27)`

- [ ] **P0.28 — Registry input validation** (FN-2)
  **Files**: `crates/buff-registry/src/handlers.rs`
  **What**: Package name regex validation (`^[a-z][a-z0-9-]{1,63}$`), path traversal check (reject `..`), size limit (50MB default).
  **Acceptance**: Invalid names rejected; path traversal blocked; size limit enforced.
  **Commit**: `fix(registry): input validation (P0.28)`

- [ ] **P0.29 — buff-web3 test coverage** (FN-3)
  **Files**: `crates/buff-web3/tests/` (NEW)
  **What**: ABI binding round-trip tests, mock provider integration tests.
  **Commit**: `test(web3): ABI coverage (P0.29)`

- [ ] **P0.21 — Audit remediation tracker**
  **What**: Create `.sisyphus/evidence/audit-remediation-tracker.md` tracking ALL 31+3 findings with status (FIXED by Px / DEFERRED via DR / N/A).
  **Unreachable criterion FIXED** (Metis B3): "0 UNRESOLVED critical + 0 UNRESOLVED high **EXCLUDING** findings formally deferred in DR-X" (deferred ≠ resolved, but tracked).
  **Detection**: `grep -c 'FIXED\|DEFERRED\|N/A' audit-remediation-tracker.md` ≥ 34.
  **Commit**: `docs: audit remediation tracker (P0.21)`

---

### PHASE 0.5 — Validation Spikes (HARD GATE)

> **Exit**: ALL S1-S7 complete. S1 AND S4 BOTH fail → ABORT.

- [ ] **S1 — Multiple dispatch coverage spike** (REFRAMED v16 — NOT "add TypeRef::Dyn")
  **What**: Buff v1.19 shipped `multi_dispatch.rs` (500 LOC, Julia-style). Test if multiple dispatch covers the parser's trait-object needs WITHOUT adding `dyn Trait` / `TypeRef::Dyn`.
  **Steps**:
  1. Identify all trait-object usages in 10 target crates: `grep -rn 'dyn\|Box<dyn\|&dyn' crates/buff-lang-{ast,error,lexer,parser,debug-info,buffhtml-parser}/src/ crates/buff-{eval,template}/src/`
  2. For each usage: can Buff's multiple dispatch express the same semantics?
  3. Write `examples/spike_multi_dispatch.buff` demonstrating: a trait with 2 impls, stored in a collection, iterated + method called.
  4. Run `buff check` and `buff run` on the spike file.
  5. **Verdict**: `MULTI_DISPATCH_SUFFICIENT` (skip Phase 2) / `NEEDS_DYN_TRAIT` (proceed to P2.1) / `IMPOSSIBLE` (abort)
  **If MULTIPLE_DISPATCH_SUFFICIENT**: Phase 2 is SKIPPED. Extension cap slots freed. Massive simplification.
  **Commit**: `spike: multiple dispatch coverage — <verdict> (S1)`

- [ ] **S2 — insta rustc drift** — Run 154 snapshots on 1.95.0. Document any drift.

- [ ] **S3 — HashMap audit** (CRITICAL — run BEFORE P0.1.2b)
  **What**: `grep -rn 'HashMap' crates/buff-lang-{ast,error,lexer,parser,debug-info,buffhtml-parser,ast-rsx}/src/ crates/buff-{eval,template}/src/`
  **If HashMap found in target crate**: Either (a) switch to BTreeMap in Rust original FIRST (on `main`), or (b) mark crate YELLOW and use T2 sorted comparison.
  **PREREQUISITE FOR**: P0.1.2b (deterministic serialization requires no HashMap).

- [ ] **S4 — LexCallback portability** — Read `crates/buff-lang-lexer/src/string_interp.rs:1-100`. Can Buff express `&mut dyn LexCallback` via multiple dispatch?

- [ ] **S6 — Extend harness to 10 targets** — Add 1 entry to `scripts/equivalence-rust-vs-buff.sh` (currently 9 tests, need 10 for 10 targets per P0.4).

- [ ] **S7 — Unsafe audit (CORRECTED)** — Verify **10 TARGET crates** have ZERO unsafe (NOT codegen-rust — it's IMPOSSIBLE per DR-014). `grep -rn 'unsafe' crates/buff-lang-{ast,ast-rsx,error,debug-info,lexer,parser,buffhtml-parser,ffi-guide}/src/ crates/buff-{eval,template}/src/` → expect 0.

---

### PHASE 0.6-0.7 — Baseline + Meta-Validation

- [ ] **P0.6 — Baseline benchmark** — `cargo build --release -p buff-lang-cli --timings` + `buff run examples/{ola,fibonacci}.buff`. Save to `.sisyphus/evidence/baseline-benchmark.json`.
- [ ] **P0.6.1 — Per-phase re-record** — After each phase, re-benchmark. Gate: ≤3% regression.
- [ ] **P0.7 — Harness divergence detection** — Inject deliberate divergence. Verify harness catches it.

---

### PHASE 1 — Compiler Bug Fixes (main branch)

> **Exit**: All `bug`-class failures from P0.3 fixed. Baseline ratcheted beyond 7 files.
> **Branch**: `main` (these fix the RUST COMPILER — allowed by F1 since F1 greps `self-host/v1` only).

- [ ] **P1.1 — Fix LEX category failures** (4 files per bootstrap-report)
  **Files**: `crates/buff-lang-lexer/src/*.rs` — identify which specific lexer rules fail on the 4 LEX-category self-host files.
  **Detection**: `buff check self-host/lexer/tokenize.buff` (and other LEX files) exits 0.
  **Acceptance**: All 4 LEX-category files pass `buff check`.

- [ ] **P1.2 — Fix PARSE-A category failures** (34 files — struct/enum indent parsing)
  **Files**: `crates/buff-lang-parser/src/{parser,stmt}.rs` — indent-sensitivity rules for struct/enum declarations.
  **Detection**: All 34 PARSE-A files pass `buff check`.

- [ ] **P1.3 — Fix PARSE-B category failures** (5 files — qualified enum patterns)
  **Detection**: All 5 PARSE-B files pass `buff check`.

- [ ] **P1.4 — Fix PARSE-C category failures** (1 file)
  **Detection**: The 1 PARSE-C file passes `buff check`.

- [ ] **P1.5 — Fix CODEGEN category failures** (5 files)
  **Files**: `crates/buff-lang-codegen-rust/src/*.rs` — codegen lowering for specific patterns in self-host files.
  **Detection**: All 5 CODEGEN files pass `buff check` AND `buff build`.

---

### PHASE 2 — Language Extension (ONLY IF S1 = NEEDS_DYN_TRAIT)

> **If S1 = MULTI_DISPATCH_SUFFICIENT**: SKIP THIS PHASE ENTIRELY. Extension cap slots freed.
> **If S1 = IMPOSSIBLE**: ABORT plan (can't port parser without trait dispatch).

- [ ] **P2.1a — Add TypeRef::Dyn variant** (extension #1 of max 3, requires DR first)
  **Files**: `crates/buff-lang-ast/src/ty.rs` (add variant), `crates/buff-lang-types/src/ty.rs` (type lowering)
  **Counter update**: Atomically increment `.sisyphus/evidence/extensions-counter.json` used count.
  **Acceptance**: `buff check` accepts `dyn Trait` syntax; counter updated.

- [ ] **P2.1b-e** — KwDyn token, autoboxing, lint, error codes (each updates counter if needed)

---

### PHASE 3 — Tier 1 Ports (self-host/v1 branch)

> **Recipe per crate** (all port agents read `.sisyphus/decisions/porting-conventions.md` first):
> 1. Read Rust source files for crate (listed in P0.4 inventory)
> 2. Write `.buff` equivalent following Porting Conventions
> 3. Run `buff check` on the .buff port — fix syntax errors
> 4. Run equivalence harness — compare output per Equivalence Contract tier
> 5. Fix divergences by editing the .buff port (NEVER the Rust original)
> 6. Ratchet self-host baseline

- [ ] **P3.1 — buff-lang-ffi-guide port** (0 LOC — docs only, trivial)
- [ ] **P3.2 — buff-lang-ast-rsx port** (418 LOC, 1 file, pure data)
- [ ] **P3.3 — buff-lang-error port** (2,404 LOC, Span structs only — thiserror derives stay in Rust)
- [ ] **P3.4 — buff-lang-debug-info port** (1,117 LOC, 2 files)
- [ ] **P3.5 — buff-eval port** (865 LOC, 2 files)
- [ ] **P3.6 — buff-template port** (150 LOC, 2 files — FFI dependency)
- [ ] **P3.7-P3.8** — Remaining per P0.4 verdict

**All Phase 3 tasks parallel** (after P0.4, P0.0 conventions doc).

---

### PHASE 4 — Tier 2 Ports (self-host/v1 branch)

> **CORRECTED v16**: Parser is 3,640 LOC (not 14,204). 2-way split (not 5-way).

- [ ] **P4.4 — buff-lang-ast port** (5,246 LOC, 9 files — MOST PORTABLE, 0 dyn-trait)
  **Parallelization**: 9 agents (one per file: common, decl, expr, ir, lib, lossless, op, stmt, ty).

- [ ] **P4.1 — buff-lang-parser port** (3,640 LOC, 6 files)
  **Split** (v16 CORRECTED — by file boundary, not syntax category):
  - **P4.1a**: Port `expr.rs` (1,186 LOC — expression parser + Pratt) ∥
  - **P4.1b**: Port `stmt.rs` (1,326 LOC — statement parser) ∥
  - **P4.1c**: Port `parser.rs` (422 LOC — dispatcher) + `stream.rs` (577 LOC — token stream) + `lib.rs` (41 LOC) + `options.rs` (88 LOC) — AFTER a+b (these import from expr.rs + stmt.rs)
  - **P4.1d**: Integration test (run equivalence harness on parser port)
  **Abort**: P4.1 >5 resume cycles (ALL sub-tasks combined).

- [ ] **P4.3 — buff-lang-ast deeper port** (full pub fn coverage beyond P4.4)

- [ ] **P4.5 — buff-lang-buffhtml-parser port** (2,748 LOC, 4 files, 3-mode lexer)
- [ ] **P4.6-P4.9** — Remaining per P0.4 verdict

- [ ] **P4.10 — buff_compiler.buff monolith** (M7 bootstrap proof)
  **Split** (AI context management):
  - P4.10a: `compile_frontend()` skeleton + `main()` entry point
  - P4.10b: Inline lexer + dedup
  - P4.10c: Inline ast + dedup
  - P4.10d: Inline parser + dedup
  - P4.10e: Integration test on `ola.buff`
  - P4.10f: Integration test on 4 more files
  **Abort**: P4.10 >5 resume cycles.
  **Detection**: `buff check --dump-ast examples/ola.buff` via monolith produces valid JSON.

---

### PHASE 5 — Full Parity Verification (CI-only, Linux Docker)

- [ ] **P5.1 — Coverage** — cargo-tarpaulin. Tier mapping: T1 fns ≥90%, T2 ≥85%, T3 ≥80%, T4 ≥75%.
- [ ] **P5.2 — Exhaustive equivalence** — Matrix: one CI job per crate (10 jobs, ≤20 GitHub concurrent). ~65 tests/job × 5s = ~5 min/job.
- [ ] **P5.3 — Stateful snapshots** — Use Snapshot Schema (§Equivalence Contract v2). Schema source: P0.4 `parity-audit.json` T4 classification.
- [ ] **P5.4 — Property-based** — proptest 1000+ random Buff programs (lexer + parser).
- [ ] **P5.5 — EMI differential** — If budget insufficient, double P5.4 instead.
- [ ] **P5.6** — Performance regression (≤3%/phase, ≤10% cumulative).
- [ ] **P5.7** — Cross-platform (3-OS CI matrix; buff-validation only on Windows/macOS).
- [ ] **P5.8** — Oracle compliance review (VERIFIED or NOT_VERIFIED; 3× PARTIAL = escalate to user replan).
- [ ] **P5.9** — Compliance report (all tasks, all acceptance criteria, all evidence).
- [ ] **P5.10** — Deprecation Phase B definition (see below).

**Deprecation Phase B definition** (Metis finding — previously undefined):
> Post-M7, Rust originals in TARGET crates (buff-lang-{ast,error,lexer,parser,...}) are FROZEN: no new features, bug fixes only. The `.buff` ports become canonical. Full deletion deferred to v2.x. Migration guide (below) documents the transition.

**Migration guide outline** (Metis finding — previously just a deliverable name):
1. Why: Buff self-host front-end achieved
2. What changed: Rust crates frozen, .buff ports canonical
3. For compiler contributors: how to edit the .buff ports
4. For framework users: no change (framework wrappers unaffected)
5. For tooling (LSP/REPL/Jupyter): how to consume .buff ports

---

### PHASE 6 — Framework Fixes (main branch, parallel)

> **Scope clarification** (Metis SC-2): Rust-side API drift fixes. Parallel to self-host.

- [ ] **P6.1 — buff-fake fixes** — `cargo test -p buff-fake` failing tests. Fix each.
- [ ] **P6.2 — buff-fuzz fixes** — `cargo test -p buff-fuzz` failing tests. Fix each.
- [ ] **P6.3 — buff-jobs Worker backoff** (companion to P0.22) — Add exponential backoff in `crates/buff-jobs/src/worker.rs:100`.
- [ ] **P6.4 — buff-web3 ABI fixes** (companion to P0.29) — Fix failing ABI bindings in `crates/buff-web3/src/`.

---

## M7 — Front-End Bootstrap Milestone

- [ ] **M7.1a — Monolith produces span-normalized AST for ola.buff** — `buff check --dump-ast examples/ola.buff` via `buff_compiler.buff` monolith produces JSON matching Rust-side output (per Equivalence Contract v2 span normalization).
- [ ] **M7.2 — Performance** — Monolith parse time within 10% of Rust baseline (P0.6).
- [ ] **M7.3 — Oracle VERIFIED** — Oracle reviews all evidence. 3× PARTIAL_VERIFIED = escalate to user replan.

---

## Final Verification Wave

- [ ] **F1 — Plan compliance** (oracle)
  - For each Must Have: verify implementation exists.
  - For each Must NOT Have: `git diff main..self-host/v1 -- crates/buff-lang-{ast,ast-rsx,error,debug-info,lexer,parser,buffhtml-parser,ffi-guide}/src/ | grep "^+" | wc -l` must equal 0.
  - Extension cap: `jq '.used' .sisyphus/evidence/extensions-counter.json` ≤ 3.
  - Output: `Must Have [N/N] | Must NOT Have [N/N] | Extensions [N/3] | VERDICT: APPROVE/REJECT`

- [ ] **F2 — Code quality** — `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --check` + `cargo test --workspace`. Run `/remove-ai-slops` skill on all changed files.
  - Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N/N] | VERDICT`

- [ ] **F3 — Real QA** — Execute EVERY QA scenario from EVERY task. Cross-task integration test (all features working together). Edge cases: empty state, invalid input, rapid actions. Save to `.sisyphus/evidence/final-qa/`.

- [ ] **F4 — Scope fidelity** — 1:1 check: everything in spec built, nothing beyond spec. `git log --oneline main..self-host/v1` — verify each commit maps to a plan task.

---

## Commit Strategy

- **1 commit per task** with format: `type(scope): description (Px.x)`
- **Branch**: `main` for audit remediation + Phase 1 bug fixes. `self-host/v1` for ports + M7.
- **Pre-commit**: relevant test command per task (Detection command)
- **Rebase**: weekly from `main` into `self-host/v1`
- **Extension counter**: any commit adding a language extension MUST update `.sisyphus/evidence/extensions-counter.json`

---

## Success Criteria

### Verification Commands
```bash
cargo deny check bans                                                    # exit 0
cargo test -p buff-lang-{ast,error,lexer,parser} --lib                  # all pass
bash scripts/equivalence-rust-vs-buff.sh                                 # 10/10 pass
buff check --dump-ast examples/ola.buff | jq '.declarations | length'   # > 0
git diff main..self-host/v1 -- crates/buff-lang-*/src/ | grep "^+" | wc -l  # 0
jq '.used' .sisyphus/evidence/extensions-counter.json                   # ≤ 3
grep -c 'FIXED\|DEFERRED' .sisyphus/evidence/audit-remediation-tracker.md  # ≥ 34
```

### Final Checklist
- [ ] All Must Have present
- [ ] All Must NOT Have absent (F1 git-diff = 0 on self-host/v1)
- [ ] All tests pass (cargo test --workspace)
- [ ] cargo-deny passes
- [ ] Equivalence harness 10/10
- [ ] M7 monolith produces span-normalized AST
- [ ] Oracle VERIFIED
- [ ] ALL 34 audit findings tracked (31 + 3 FNs) — FIXED or DEFERRED with DR
- [ ] Migration guide written
- [ ] Deprecation Phase B defined
- [ ] ≤10% cumulative performance regression
- [ ] Extension count ≤ 3
