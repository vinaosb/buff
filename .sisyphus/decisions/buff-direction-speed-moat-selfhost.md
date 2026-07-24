# Buff Direction: Speed + MOAT + Self-host-frontend

**Status**: FINAL
**Date**: 2026-07-23
**Task**: T110 (Track F)
**Blocks**: T20 (acceptance criteria sign-off), T21 (stability tiers)
**Plan reference**: `.sisyphus/plans/buff-launch-readiness.md`

---

## User's Governing Priorities

> "I don't really give much importance for FPGAs, but I give importance for adoption and a strong MOAT, I myself don't mind Rust that much to give us the security we currently have, but I am not sure we have the best optimizations for autodetection of CPU/GPU workloads as well as slow compilation times for big projects, those are my 2 most problematic concerns."

> "The self-host idea also I dont mind transpiling our code later to rust, the important thing is to have our custom code in Buff to be easier to maintain."

Two concerns drive every decision below:
1. Adoption and a strong MOAT (not FPGA-centric).
2. Compile speed and CPU/GPU dispatch quality.

Self-host is a means to maintainability, not an ideological goal.

---

## Decision 1: Direction

**Reframe to Speed + MOAT + Self-host-frontend.**

Keep transpiling to Rust. Demote MLIR and custom-memory-model to optional far-future V3 spikes only.

This is v1.25+ under SemVer 2.0. All changes are additive. Everything ships as minor versions after v1.13-v1.24. "2.0.0" is reserved for a genuinely breaking change.

---

## Decision 2: Self-host Scope

**Rewrite compiler crates in Buff but still emit Rust.**

NOT drop-rustc. NOT build an own backend. The self-host goal is making the compiler easier to maintain by writing it in Buff, not replacing the production compilation path.

---

## Decision 3: Memory Model

**Keep Rust's memory model entirely.**

No custom memory work. No ARC, no Perceus, no borrow-inference, no `weak<T>`. Zero new safety risk. Rust's borrow checker remains the safety oracle.

---

## Accept-With-Rationale Items

1. **rustc as safety oracle + production backend.** Free borrow-checker, battle-tested optimizer, no reinvention. Buff's value is syntax and ergonomics, not replacing a world-class compiler.

2. **rustc's slow compile times (mitigated, not replaced).** Speed improvements come via toolchain upgrades: fast-linker (T2), Cranelift backend (T4), salsa incremental compilation (T7). No backend reimplementation.

3. **WGSL/wgpu as GPU path.** Cross-vendor (NVIDIA, AMD, Intel, Apple). Pure-Rust `wgpu` crate with no C dependencies. SPIR-V and CUDA are out of scope.

4. **syn/quote/prettyplease for Rust codegen.** Battle-tested, deterministic output. Raw-string codegen is forbidden (single exception: `codegen-wgsl/shader.rs` which has no syn equivalent for WGSL).

5. **Dioxus 0.7 as UI framework.** T121b spike validated feasibility. RSX model maps naturally to Buff's own RSX syntax (`.buffhtml`). Leptos and Yew were evaluated and rejected.

6. **64+-crate workspace.** Each framework crate is independently shippable. No monorepo-style consolidation. The glob `members = ["crates/*"]` stays.

7. **BTreeMap-only in compiler internals.** Deterministic codegen requires deterministic iteration order. User-facing `HashSet`/`HashMap` from T27 is the exception (codegen-rust type-lowerer + prelude_types registry only).

8. **ErrorCodes E10xx-E13xx STABLE FOREVER.** No renumbering, reusing, or silently removing existing codes. New ranges start at E14xx. This is a stability promise to users.

9. **v1.25+ minor versioning.** Under SemVer 2.0, MAJOR is reserved for backwards-incompatible changes. Everything planned is additive or non-breaking. Ships as v1.25, v1.26, etc.

10. **Codegen-deferred framework types (F7 long-pole).** Framework crate types ship runtime-side independently. Codegen lowering to Buff syntax is a coordinated sibling task. Examples parse cleanly today; full lowering follows.

---

## Demoted Items

| Item | Status | Notes |
|---|---|---|
| MLIR migration | Optional V3 spike only | NOT in v1.25+ |
| Custom memory model (ARC, Perceus, borrow-inference, `weak<T>`) | NOT in scope | Decision 3 above |
| Dropping rustc | NOT in scope | rustc remains safety oracle |
| FPGA-centric direction | NOT in scope | User deprioritized explicitly |

---

## Research Basis

These decisions were informed by 13 research dossiers and analyses (drafted in `.sisyphus/drafts/`, now superseded by this record):

- Metis analysis (wave optimization, task dependency graph, concurrency policy)
- Momus approvals (risk review, scope validation, guardrail enforcement)
- Competitive analysis (Buff vs Rust vs Go vs Python positioning, MOAT assessment)
- Codebase inventory (64-crate audit, god-class identification, hard-rule violation count)
- Dioxus feasibility spike (T121b, proc-macro semver risk assessment)
- RSX syntax feasibility study
- Macro system design analysis
- API compatibility review
- WGSL extensibility analysis
- SDK conventions audit
- Stability promise framework
- Performance baseline capture (T22 benchmark harness design)
- Self-host bootstrap dependency chain analysis

For implementation details, see the full plan at `.sisyphus/plans/buff-launch-readiness.md`.
