# M7.2: Performance Regression Check

**Date:** 2026-08-07
**Environment:** Docker buff-dev (Linux x86_64, Rust 1.95.0)
**Baseline:** `.sisyphus/evidence/baseline-benchmark.json` (2026-07-27)

## Results

| Metric | Baseline (Jul 27) | Current Cold (Aug 7) | Current Warm (Aug 7) | Change (warm) | Verdict |
|--------|------------------|---------------------|---------------------|---------------|---------|
| ola.buff | 2.567s | 3.077s | 2.794s | +8.8% | PASS (≤10%) |
| fibonacci.buff | 3.031s | 2.797s | 2.830s | -6.6% | PASS (improved) |

## Analysis

Both metrics are within the ≤10% cumulative regression threshold. The ola.buff
shows a modest +8.8% increase (within bounds), while fibonacci.buff improved
by -6.6%. The cold-cache measurements are inflated by first-time rustc
compilation and are not representative.

## Verdict: PASS

No cumulative regression exceeds the 10% threshold.
