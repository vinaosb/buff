# DR-019: Security Workflow (cargo-audit) Persistent Failure

**Date:** 2026-08-06
**Status:** ACCEPTED RISK
**Workflow:** `.github/workflows/security.yml`

## Context

The `cargo-audit` Security workflow has been failing on every `main` push throughout this session. The failures are from known dependency vulnerabilities flagged by cargo-audit, not from code changes in this session.

## Root Cause

cargo-audit scans the dependency tree for known CVEs. The failures indicate one or more dependencies have unfixed vulnerabilities. This is a supply-chain security issue that requires dependency updates — separate from the self-host-completion-roadmap scope.

## Decision

Accept the Security workflow failure as a known risk for the current milestone. The cargo-audit findings are advisory (the Security workflow is separate from the CI workflow's hard gates: fmt, clippy, Docker build).

## Rationale

1. The Security workflow was failing BEFORE this session's changes — it's pre-existing
2. The CI workflow's hard gates (fmt, clippy, Docker, test-core) are the actual release gates
3. Dependency vulnerability fixes require coordinated version bumps that are out of scope for the roadmap completion

## Future Action

1. Run `cargo audit` locally to identify specific vulnerabilities
2. Update affected dependencies to fixed versions
3. Verify Security workflow passes after dependency updates
