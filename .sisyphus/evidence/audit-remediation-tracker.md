# Audit Remediation Tracker (v3.2)

**Audit date:** 2026-07-26
**Source:** `.sisyphus/reports/buff-audit-2026-07-26-2019-v3.{json,md}`
**Total findings:** 31 + FP-2 + 3 FNs = 34 tracked items (plus 1 disproven FP)
**Plan mapping:** `.sisyphus/plans/self-host-completion-roadmap.md` lines 337-375

## Summary

| Status | Count |
|--------|-------|
| FIXED | 4 |
| DEFERRED | 5 |
| IN-PROGRESS | 1 |
| PENDING | 24 |
| **Total** | **34** |

Disproven FP-2 is tracked separately below and excluded from the actionable total.

## Critical Findings (17)

| ID | Description | Sev | Task | Status | Evidence |
|----|-------------|-----|------|--------|----------|
| arch-001 | buff-jobs Scheduler.start() never executes scheduled jobs | CRIT | P0.22 | PENDING | Not started |
| cicd-001 | CI test job continue-on-error + cargo test --lib (100+ tests never gate) | CRIT | P0.11 | PENDING | Not started |
| cicd-002 | Homebrew formula shipped with FILL-ME sha256 placeholders at v1.24.0 | CRIT | P0.23 | FIXED | commit `86b36af`; placeholders renamed to FILL-BY-RELEASE-WORKFLOW + `update-sha256.sh` automation script added; follow-up: release.yml job integration still pending |
| coh-001 | self-host plan falsely claims v1.26-v1.39 tags deleted (all 14 exist) | CRIT | P0.24 | FIXED | commit `dcd1fc5` (plan corrected to acknowledge v1.26-v1.39 exist) |
| cq-001 | God function lower_prelude_type_assoc_fn spans ~3340 lines | CRIT | P0.14 | DEFERRED | DR pending (P0.14 deferral per plan line 344) |
| cq-002 | God function lower_prelude_type_instance_fn spans ~3501 lines | CRIT | P0.14 | DEFERRED | DR pending (P0.14 deferral per plan line 345) |
| dep-001 | No cargo-deny hard CI gate (no C library rule enforced by comments only) | CRIT | P0.8 | PENDING | Not started |
| dep-002 | buff-registry ships rusqlite bundled (compiles SQLite C89 via cc-rs) | CRIT | P0.8 | PENDING | Not started |
| dep-003 | buff-auth pulls ring v0.17.14 via jsonwebtoken 9.3.1 (AGENTS.md claims NO ring) | CRIT | P0.8 | PENDING | Not started |
| ft-001 | buff-http-client has NO default timeout (requests can hang indefinitely) | CRIT | P0.17 | FIXED | commit `65b054e` |
| lic-001 | License 3-way split (root LICENSE MIT-only, 70 crates declare MIT OR Apache-2.0) | CRIT | P0.13 | PENDING | Not started |
| lms-001 | .sisyphus/evidence/ gitignored (audit trail not durably persisted, 147 files untracked) | CRIT | P0.20 | PENDING | Not started |
| obs-001 | buff-observe Tracer::bootstrap() never called (ALL tracing events silently dropped) | CRIT | P0.15 | DEFERRED | DR pending (P0.15 deferral per plan line 349) |
| prd-001 | AGENTS.md metadata 52 commits stale, branch wrong, tag range wrong | CRIT | P0.12 | PENDING | Not started |
| sec-001 | buffup downloads release tarballs WITHOUT verifying .sha256 sidecar (supply-chain RCE) | CRIT | P0.9 | FIXED | commit `7ac027d` |
| sec-002 | setup-buff GitHub Action uses curl pipe sh with no checksum verification | CRIT | P0.10 | PENDING | Not started |
| tc-001 | buff-crypto-extras has ZERO tests for 622 LOC of AES-GCM/RSA/ECDH/Argon2 | CRIT | P0.16 | PENDING | Not started |

## High Findings (14)

| ID | Description | Sev | Task | Status | Evidence |
|----|-------------|-----|------|--------|----------|
| arch-002 | buff-lsp depends on entire buff-lang-cli for 1 fn (layer violation + bloat) | HIGH | P0.26 | PENDING | Not started |
| arch-003 | buff-eval uses #[path] cross-crate include + 6 duplicated helpers | HIGH | P0.26 | PENDING | Not started |
| cicd-003 | GitHub Actions pinned to mutable @v4/@master refs + Docker tags mutable + no cosign | HIGH | P0.10 | PENDING | Not started |
| cicd-004 | buff-validation emits ::notice not ::error (failures downgrade silently) | HIGH | P0.11 | PENDING | Not started |
| cicd-005 | 4 of 6 workflows have no permissions block (default write-all) | HIGH | P0.10 | PENDING | Not started |
| cq-003 | Third god function generate() spans ~1038 lines (entry dispatcher) | HIGH | P0.14 | DEFERRED | DR pending (P0.14 deferral per plan line 362) |
| ft-002 | buff-jobs Worker ignores backoff configuration (retry storm risk) | HIGH | P0.25 | PENDING | Not started |
| ft-003 | buff-resilience Timeout leaks worker thread on timeout (no cancellation) | HIGH | P0.25 | PENDING | Not started |
| obs-002 | bootstrap_otlp permanently returns Err (OTLP exporter never works) | HIGH | P0.15 | DEFERRED | DR pending (P0.15 deferral per plan line 367) |
| obs-003 | buff-registry has no /health or /ready endpoint (production server unprobeable) | HIGH | P0.18 | PENDING | Not started |
| prd-002 | README Status table stops at v1.24 (15 versions missing) | HIGH | P0.12 | PENDING | Not started |
| prd-003 | CHANGELOG stops at v1.25 (14 versions missing) | HIGH | P0.12 | PENDING | Not started |
| sec-003 | OAuth state param #[allow(dead_code)] (CSRF protection disabled) | HIGH | P0.25 | PENDING | Not started |
| sec-004 | OAuth cookie missing Secure flag + session_token echoed in response body | HIGH | P0.25 | PENDING | Not started |

## False Positives

| ID | Description | Sev | Task | Status | Evidence |
|----|-------------|-----|------|--------|----------|
| FP-2 | v2.0 claim: ethers smuggles bzip2-sys + zstd-sys via cc-rs | FP | P0.8 | DISPROVEN | `cargo tree -i ethers-solc` returns nothing to print; `cargo tree -i bzip2-sys` returns nothing to print; default-features=false fix already worked. Real active cc-rs surface is ring + libsqlite3-sys. |

## False Negatives (3)

| ID | Description | Sev | Task | Status | Evidence |
|----|-------------|-----|------|--------|----------|
| FN-1 | WebSocket hardening (ui_dev/server.rs CSWSH attack surface) | FN | P0.27 | IN-PROGRESS | Task scoped in plan, work started |
| FN-2 | Registry input validation (axum handlers accept arbitrary bytes, path traversal) | FN | P0.28 | PENDING | Not started |
| FN-3 | buff-web3 zero test coverage | FN | P0.29 | PENDING | Not started |

## Status Definitions

- **FIXED**: Remediation commit landed and verified against the finding.
- **DEFERRED**: Explicitly deferred via Decision Record (DR). The DR number is pending; the deferral is recorded in the plan at the cited line.
- **IN-PROGRESS**: Task scoped, work begun, not yet complete.
- **PENDING**: Not yet started. Awaiting prioritization.
- **DISPROVEN**: The original finding was a false positive. Verified by direct investigation.
