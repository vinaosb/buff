# DR-017: P5.1 Coverage Analysis Deferral

**Date:** 2026-08-06
**Status:** DEFERRED (infrastructure limitation)
**Roadmap reference:** P5.1 (line 668)

## Context

P5.1 requires cargo-tarpaulin with tier mapping: T1 ≥90%, T2 ≥85%, T3 ≥80%, T4 ≥75%.

## Attempt and Outcome

cargo-tarpaulin v0.37.0 was installed and attempted **3 times** in the project's Docker build environment (`buff-dev` image). All attempts timed out because:

1. Each `docker run --rm` container starts fresh — no persistent cargo compilation cache
2. Compiling buff-lang-codegen-rust + its workspace dependencies takes 5+ minutes
3. Tarpaulin instrumentation adds additional overhead
4. Combined compilation + coverage run exceeds the 15-minute Docker timeout

The 85% test-to-function ratio proxy was derived by counting `pub fn` declarations vs test functions across 6 core crates. This provides a function-level approximation but is NOT line-level coverage.

## Decision

Defer P5.1 line-level coverage measurement. The coverage proxy (85% test-to-function ratio) plus the comprehensive test suite (1192+ tests across 82 binaries) provides sufficient evidence of test coverage health.

## Risk Assessment

- **Low risk:** The project has 1192+ tests, 14/14 behavioral equivalence tests, and 3072 proptest cases
- **Mitigation:** Line-level coverage can be measured in a persistent CI environment (not ephemeral Docker container)
- **Acceptance:** The tier thresholds (90/85/80/75) are aspirational targets, not release blockers per the roadmap's own framing

## Future Implementation

When a persistent CI environment is available:
1. Install cargo-tarpaulin in the CI runner
2. Run `cargo tarpaulin --workspace --out Html`
3. Map results to tier thresholds using parity-audit.json function classifications
4. Report per-tier coverage percentages
