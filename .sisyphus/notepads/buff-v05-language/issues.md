# Buff v0.5 Issues

## T29 — Flaky `types_modules_export_star_chain` (FIXED)

**Symptom:** `cargo test -p buff-lang-types --test modules` (solo) → all pass.
`cargo test --workspace` (parallel, full pipeline) →
`types_modules_export_star_chain` FAILED intermittently. Same binary,
different result across runs = non-determinism. Did NOT reproduce in
5 sequential workspace runs during initial triage (probabilistic).

**Root cause:** `resolve_reexports` in
`crates/buff-lang-types/src/modules.rs` iterated
`ctx.modules.values()` directly. `ctx.modules` is a `HashMap<PathBuf,
Module>`, and Rust's `HashMap` uses SipHash with a randomly-seeded
hasher per process → iteration order is non-deterministic across runs.

For a re-export chain `a → export * from b → export * from c → export deep`,
the resolution MUST process `c` before `b` and `b` before `a` (each
module's exports must be finalized before being flattened into a parent).
HashMap order doesn't guarantee this — for the 3-module chain, only 1
of 6 permutations is dep-first; the other 5 miss the chain and leave
`a.exports` empty.

**Fix:** iterate `ctx.topo_order` (which `process_module` already
computes as a post-order DFS: dependencies pushed before importers).
Topological order guarantees a module's target is finalized before
the module's own re-exports are flattened — single pass, no fixed-
point loop needed.

```rust
// Before (broken):
for (mod_path, reexports) in ctx.modules.values().map(...) { ... }

// After (deterministic):
let topo = ctx.topo_order.clone();
let reexports_per_mod: HashMap<_, _> = ctx.modules.iter()...collect();
for mod_path in &topo {
    let Some(reexports) = reexports_per_mod.get(mod_path) else { continue };
    ...
}
```

**Validation (test-side hardening added):**
- `types_modules_export_star_chain_is_deterministic_accross_runs`:
  builds the a→b→c chain 50 times in one test, asserts each iteration
  propagates `deep` to `a`. Per-iteration failure probability of the
  broken code was ~5/6, so 50 iterations give near-certain regression
  detection.
- `types_modules_export_star_chain_length_5`: a 5-deep chain
  (a→b→c→d→e→export deep). 5! = 120 permutations, only 1 dep-first;
  broken code would fail ~99.2% of the time.

**Confirmation test (sanity check):** I temporarily reverted the fix
(kept the new stress tests), ran `cargo test -p buff-lang-types --test
modules`, and confirmed BOTH new stress tests FAILED immediately
(`5-deep chained re-export of deep must reach a: {}`). Re-applied
the fix → all 29 tests pass. This proves (a) the bug was real and
logically present even when not manifesting in single-shot runs, and
(b) the new tests detect the regression reliably.

**Verification (post-fix):**
- `cargo test -p buff-lang-types --test modules` 5x → 29 passed each.
- `cargo test --workspace` 4x parallel → 0 failures each (57 test
  result blocks, all `0 failed`).
- `cargo check --workspace` → clean.
- `cargo clippy --workspace --all-targets -- -D warnings` → clean.
- `cargo fmt -p buff-lang-types -- --check` → clean.

**Lesson:** When a graph algorithm's correctness depends on processing
order (deps before importers), NEVER iterate a `HashMap` directly —
either pre-compute a topological order (post-order DFS) and walk that,
or use a `BTreeMap` if there's a natural key ordering. The flakiness
discipline in Buff's guardrails (deterministic codegen, snapshot
tests, etc.) extends to internal data-structure choice.
