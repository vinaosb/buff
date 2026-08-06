# DR-016: M7.1a Span-Normalized AST Comparison Deferral

**Date:** 2026-08-06
**Status:** DEFERRED (post-roadmap refinement)
**Roadmap reference:** M7.1a (line 708)

## Context

M7.1a requires the self-host monolith (`buff_compiler.buff`) to produce a span-normalized AST that matches the Rust parser's output via Equivalence Contract v2 comparison.

## Current State

- `buff check --dump-ast` produces valid JSON AST representation
- The JSON output is deterministic and well-structured
- The v2 span-normalized COMPARISON (token-index normalization + structural diff) is NOT implemented

## Decision

Defer the v2 span-normalized comparison to post-roadmap refinement. The roadmap itself acknowledges this at line 709: "Full span-normalized AST matching (Equivalence Contract v2 comparison) is post-M7 refinement."

## Rationale

1. **Behavioral equivalence is stronger evidence:** The 14/14 behavioral equivalence harness tests compare actual program OUTPUT (stdout), which is a stronger guarantee than AST structural matching. If two programs produce identical output for the same input, their ASTs are functionally equivalent regardless of span differences.

2. **Property-based testing provides additional coverage:** 3072 random programs tested via proptest (12 properties across lexer + parser) verify crash-safety and roundtrip properties.

3. **Span normalization is a refinement, not a correctness check:** Span differences are cosmetic (byte offsets vary between implementations) and don't affect program semantics. The comparison would verify implementation fidelity, not correctness.

4. **Implementation effort is significant:** Token-index normalization requires a canonical form for spans, a structural diff algorithm, and integration with the dump-ast pipeline. This is a multi-day implementation effort.

## Acceptance Criteria for Deferred Work

When this is eventually implemented:
1. Run `buff check --dump-ast` on each target crate's .buff port
2. Run the Rust parser's dump-ast on the same source
3. Normalize spans (replace byte offsets with sequential token indices)
4. Structural diff the two ASTs
5. Report any structural (non-span) differences

## Impact

Without this comparison, we rely on behavioral equivalence + proptest as evidence of port correctness. This is sufficient for the current milestone. The span-normalized comparison adds confidence but is not blocking.
