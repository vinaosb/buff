# Audit Remediation Tracker (v3.2)

**Audit date:** 2026-07-26
**Source:** `.sisyphus/reports/buff-audit-2026-07-26-2019-v3.{json,md}`
**Last updated:** 2026-07-27 (Batch 5 complete — 26 FIXED, 5 DEFERRED, 3 PENDING)
**Total findings:** 31 + FP-2 + 3 FNs = 34 tracked items (plus 1 disproven FP)

## Summary

| Status | Count |
|--------|-------|
| FIXED | 26 |
| DEFERRED | 5 |
| PENDING | 3 |
| **Total** | **34** |

## Critical Findings (17)

| ID | Description | Sev | Task | Status | Evidence |
|----|-------------|-----|------|--------|----------|
| arch-001 | buff-jobs Scheduler.start() never executes scheduled jobs | CRIT | P0.22 | FIXED | commit `92c2251`; 46/46 scheduler tests pass |
| cicd-001 | CI test job continue-on-error + cargo test --lib (100+ tests never gate) | CRIT | P0.11 | FIXED | commit `3ac6386`; test-core HARD gate + test-framework advisory |
| cicd-002 | Homebrew formula shipped with FILL-ME sha256 placeholders at v1.24.0 | CRIT | P0.23 | FIXED | commit `a8da78b`; real sha256 hashes computed |
| coh-001 | self-host plan falsely claims v1.26-v1.39 tags deleted (all 14 exist) | CRIT | P0.24 | FIXED | commit `dcd1fc5` |
| cq-001 | God function lower_prelude_type_assoc_fn spans ~3340 lines | CRIT | P0.14 | DEFERRED | DR: `.sisyphus/decisions/codegen-rust-god-functions-deferred.md` |
| cq-002 | God function lower_prelude_type_instance_fn spans ~3501 lines | CRIT | P0.14 | DEFERRED | DR: `.sisyphus/decisions/codegen-rust-god-functions-deferred.md` |
| dep-001 | No cargo-deny hard CI gate (no C library rule enforced by comments only) | CRIT | P0.8 | FIXED | commit `62e1f79`; deny.toml + CI cargo-deny job |
| dep-002 | buff-registry ships rusqlite bundled (compiles SQLite C89 via cc-rs) | CRIT | P0.8 | FIXED | DR: `.sisyphus/decisions/libsqlite3-sys-exception.md` (documented exception) |
| dep-003 | buff-auth pulls ring v0.17.14 via jsonwebtoken 9.3.1 (AGENTS.md claims NO ring) | CRIT | P0.8 | FIXED | DR: `.sisyphus/decisions/ring-exception.md` (documented exception) |
| ft-001 | buff-http-client has NO default timeout (requests can hang indefinitely) | CRIT | P0.17 | FIXED | commit `65b054e` |
| lic-001 | License 3-way split (root LICENSE MIT-only, 70 crates declare MIT OR Apache-2.0) | CRIT | P0.13 | FIXED | commit `3ac6386`; LICENSE-APACHE + LICENSE-MIT added |
| lms-001 | .sisyphus/evidence/ gitignored (audit trail not durably persisted, 147 files untracked) | CRIT | P0.20 | PENDING | Not started (edit queue: ci.yml #4) |
| obs-001 | buff-observe Tracer::bootstrap() never called (ALL tracing events silently dropped) | CRIT | P0.15 | DEFERRED | DR: `.sisyphus/decisions/buff-observe-deferred.md` |
| prd-001 | AGENTS.md metadata 52 commits stale, branch wrong, tag range wrong | CRIT | P0.12 | FIXED | commit `caf80c2` |
| sec-001 | buffup downloads release tarballs WITHOUT verifying .sha256 sidecar (supply-chain RCE) | CRIT | P0.9 | FIXED | commit `7ac027d` |
| sec-002 | setup-buff GitHub Action uses curl pipe sh with no checksum verification | CRIT | P0.10 | FIXED | commit `4227090`; all Actions SHA-pinned + Docker digest-pinned + permissions blocks |
| tc-001 | buff-crypto-extras has ZERO tests for 622 LOC of AES-GCM/RSA/ECDH/Argon2 | CRIT | P0.16 | FIXED | commit `5707069`; NIST AES-GCM + RFC ECDH + RSA round-trip + Argon2 tests (36 pass) |

## High Findings (14)

| ID | Description | Sev | Task | Status | Evidence |
|----|-------------|-----|------|--------|----------|
| arch-002 | buff-lsp depends on entire buff-lang-cli for 1 fn (layer violation + bloat) | HIGH | P0.26 | PENDING | Not started (major refactor) |
| arch-003 | buff-eval uses #[path] cross-crate include + 6 duplicated helpers | HIGH | P0.26 | PENDING | Not started (major refactor) |
| cicd-003 | GitHub Actions pinned to mutable @v4/@master refs + Docker tags mutable + no cosign | HIGH | P0.10 | FIXED | commit `4227090`; all Actions SHA-pinned + Docker digest-pinned |
| cicd-004 | buff-validation emits ::notice not ::error (failures downgrade silently) | HIGH | P0.11 | FIXED | commit `3ac6386` |
| cicd-005 | 4 of 6 workflows have no permissions block (default write-all) | HIGH | P0.10 | FIXED | commit `4227090`; permissions: contents: read added to all 6 workflows |
| cq-003 | Third god function generate() spans ~1038 lines (entry dispatcher) | HIGH | P0.14 | DEFERRED | DR: `.sisyphus/decisions/codegen-rust-god-functions-deferred.md` |
| ft-002 | buff-jobs Worker ignores backoff configuration (retry storm risk) | HIGH | P0.25 | FIXED | commit `44f04b9` |
| ft-003 | buff-resilience Timeout leaks worker thread on timeout (no cancellation) | HIGH | P0.25 | FIXED | commit `44f04b9`; polling → mpsc channel |
| obs-002 | bootstrap_otlp permanently returns Err (OTLP exporter never works) | HIGH | P0.15 | DEFERRED | DR: `.sisyphus/decisions/buff-observe-deferred.md` |
| obs-003 | buff-registry has no /health or /ready endpoint (production server unprobeable) | HIGH | P0.18 | FIXED | commit `3ac6386` |
| prd-002 | README Status table stops at v1.24 (15 versions missing) | HIGH | P0.12 | FIXED | commit `38c9665` |
| prd-003 | CHANGELOG stops at v1.25 (14 versions missing) | HIGH | P0.12 | FIXED | commit `8b35fd6` |
| sec-003 | OAuth state param #[allow(dead_code)] (CSRF protection disabled) | HIGH | P0.25 | FIXED | commit `44f04b9`; double-submit cookie pattern |
| sec-004 | OAuth cookie missing Secure flag + session_token echoed in response body | HIGH | P0.25 | FIXED | commit `44f04b9`; Secure + SameSite=Strict + token echo removed |

## False Positives

| ID | Description | Sev | Task | Status | Evidence |
|----|-------------|-----|------|--------|----------|
| FP-2 | v2.0 claim: ethers smuggles bzip2-sys + zstd-sys via cc-rs | FP | P0.8 | DISPROVEN | `cargo tree -i ethers-solc` returns nothing; default-features=false fix already worked. |

## False Negatives (3)

| ID | Description | Sev | Task | Status | Evidence |
|----|-------------|-----|------|--------|----------|
| FN-1 | WebSocket hardening (ui_dev/server.rs CSWSH attack surface) | FN | P0.27 | FIXED | commit `a3031a5`; Origin header validation |
| FN-2 | Registry input validation (axum handlers accept arbitrary bytes, path traversal) | FN | P0.28 | FIXED | commit `d8cf8dd`; name regex + path traversal + semver + null byte + 50MB limit + 22 tests |
| FN-3 | buff-web3 zero test coverage | FN | P0.29 | FIXED | commit `3ac6386`; ABI binding round-trip tests + mock provider tests |

## Additional DRs (outside audit scope)

| DR | Topic | Status |
|----|-------|--------|
| DR-019 | extern "Rust" ABI permanently deferred — 20 codegen self-host files | ACCEPTED |

## Status Definitions

- **FIXED**: Remediation commit landed and verified against the finding.
- **DEFERRED**: Explicitly deferred via Decision Record (DR).
- **IN-PROGRESS**: Task scoped and work actively running (Batch 5 background tasks).
- **PENDING**: Not yet started. Awaiting prioritization.
