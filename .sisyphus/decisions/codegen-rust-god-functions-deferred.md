# Defer god-function refactor in buff-lang-codegen-rust

**Status**: Accepted
**Date**: 2026-07-27
**Task**: P0.14 (v3.2 audit follow-up)
**Audit findings**: cq-001, cq-002, cq-003
**Governing authority**: DR-014 (`.sisyphus/decisions/selfhost-feasibility.md`)

---

## Context

The v3.2 code-quality audit flagged three findings against `buff-lang-codegen-rust`:

- **cq-001**: `crates/buff-lang-codegen-rust/src/rust_codegen.rs` is the largest source file in the workspace.
- **cq-002**: Several functions in `rust_codegen.rs` are god-functions of extreme length.
- **cq-003**: Similar long-function and large-file issues appear in other codegen-rust source files (the broader crate spans 11 src files plus the `rust_codegen/` subdirectory).

### Measured scope (2026-07-27)

`rust_codegen.rs` is **10,505 lines** (measured via `Get-Content | Measure-Object -Line`). The per-crate `AGENTS.md` cites 12,777 lines, a figure from before the T105a submodule split. Both numbers describe the same file at different points in its history.

Function inventory (non-test, line spans computed from `fn` declaration positions):

| Function | Starts | Ends (next fn) | Approx LOC | Role |
|---|---|---|---|---|
| `lower_prelude_type_instance_fn` | 6241 | 9791 | ~3,550 | Prelude-type instance-method dispatcher |
| `lower_prelude_type_assoc_fn` | 2840 | 6181 | ~3,341 | Prelude-type associated-function dispatcher |
| `generate` | 478 | 1519 | ~1,041 | Main entry: runs pre-passes, then lowering loop |
| `lower_expr` | 1966 | 2457 | ~491 | Expression visitor |
| `lower_stmt` | 1574 | 1956 | ~382 | Statement visitor |
| `lower_prelude_call` | 2672 | 2840 | ~168 | Free-function prelude dispatcher |
| `emit_record_copy_methods` | 10070 | 10171 | ~101 | Struct copy-method emitter |
| `union_enum_item` | 9887 | 9982 | ~95 | Union enum item builder |
| `lower_guard_conditions_into` | 2582 | 2672 | ~90 | Guard condition lowering |
| `emit_embedding_delegation` | 10171 | 10258 | ~87 | Delegation impl emitter |
| `lower_log_call` | 9791 | 9865 | ~74 | Log-call lowering |
| `lower_prelude_type_assoc_const` | 6181 | 6241 | ~60 | Associated-const dispatcher |
| `build_delegation_impl` | 10258 | 10322 | ~64 | Delegation impl builder |
| `lower_block` | 1519 | 1574 | ~55 | Block visitor |
| (remaining ~16 fns) | various | various | <50 each | Accessors, helpers, small lowering arms |

The two prelude dispatchers (`lower_prelude_type_assoc_fn` and `lower_prelude_type_instance_fn`) alone account for roughly **6,891 lines**, about **66% of the file**. These are the god-functions cq-002 calls out.

`format.rs`, which cq-003 mentions alongside other codegen-rust files, is itself only 47 lines and contains a single `prettyplease::unparse` wrapper plus two unit tests. The cq-003 concern applies to the broader crate: `atomic_analysis.rs` (1,025 lines), `race_analysis.rs` (856 lines), `gpu_alignment.rs` (649 lines), and the 11 child modules under `rust_codegen/` (ranging 140 to 1,757 lines each).

### Why the god-functions exist

Both dispatchers are large `match` statements over the prelude type registry (`buff-lang-types/src/prelude_types.rs`, the 1,919-line extensible registry described in the root `AGENTS.md`). Every prelude type (DateTime, Regex, URL, Hash, TCP, Toml, Math, Random, Strings, Log, Base64, Hex, UUID, Csv, Yaml, Env, Args, Process, TCP, UDP, WebSocket, and more) contributes associated functions, instance methods, and associated constants. Each arm lowers one method to its corresponding mature Rust crate call (chrono, regex, toml, rand, sha2, hmac, tokio-tungstenite, and so on).

The registry grows by accretion. Every T124 stdlib task adds arms to these dispatchers. There is no procedural-macro or table-driven path because the lowering logic per method is bespoke: each arm constructs a different `syn` expression tree, picks a different extern crate, and emits different error-handling shapes.

### Prior refactoring already done (T105a)

T105a already extracted the safely-extractable `impl RustCodegen` methods into 11 child modules under `rust_codegen/`:

```
rust_codegen/
  decl_lowering.rs            1603 lines
  extern_crate_detection.rs   1757 lines
  method_call_lowering.rs     1341 lines
  expr_lowering.rs             987 lines
  syn_helpers.rs               980 lines
  extern_crate_detection_extra 925 lines
  type_lowering.rs             769 lines
  derive_attrs.rs              257 lines
  lowering_helpers.rs          291 lines
  dependency_detection.rs      385 lines
  conv_helpers.rs              140 lines
```

The two god-functions remain in the parent file because they share mutable state (`self.extern_crates`, `self.context`, the `TypeInferencer`) across thousands of arms, and each arm is tightly coupled to the `syn` construction helpers and the per-type extern-crate detection logic. A mechanical extraction is not straightforward.

## Decision

**Defer the god-function split. Keep cq-001, cq-002, and cq-003 OPEN but DEFERRED.**

The three audit findings are acknowledged and recorded, but no refactoring work is scheduled against them in the v1.26 or near-term window. The findings will be revisited if and when the codegen-rust crate undergoes a structural change for an independent reason (for example, a new backend target, a prelude registry redesign, or a codegen-IR introduction).

## Rationale

### 1. codegen-rust is IMPOSSIBLE to port (DR-014)

DR-014 (`selfhost-feasibility.md` § "IMPOSSIBLE") classifies `buff-lang-codegen-rust` as 🟥 IMPOSSIBLE, calling it "THE WALL":

> Generates Rust via `syn`/`quote`/`prettyplease`. Buff has no Rust-AST stdlib. Raw-string codegen is BANNED by project anti-pattern rule. Without porting this crate, there is no self-hosting.

The crate cannot be ported to Buff because it IS the thing that generates Rust from Buff. Porting it would require Buff to already have a Rust-codegen backend, which is circular. The crate is 58,617 LOC (per DR-014's count of the whole crate, not just `rust_codegen.rs`) and represents 30% of the entire compiler.

Because the crate will live in Rust indefinitely, the usual self-hosting motivation for clean Buff-side structure does not apply. The god-functions are a Rust-maintainability concern, not a porting blocker.

### 2. Splitting the dispatchers risks the codegen pipeline

The codegen pipeline is the project's most correctness-sensitive path. `rust_codegen.rs` is exercised by 95 snapshot tests (per the per-crate `AGENTS.md`) that enforce byte-identical deterministic output. The execution order inside `generate()` matters: atomic analysis, then race analysis (with the atomic exemption hook), then async propagation, then hash-safety fixpoint, then GPU-bound analysis, then named-arg and default collection, then extern-fn collection, then the main lowering loop.

The two god-functions sit inside that lowering loop. Each `match` arm reads and writes shared `RustCodegen` state. Splitting them into per-type sub-functions would require threading that state explicitly, which changes the borrow graph and opens the door to subtle ordering bugs that snapshot tests may or may not catch (snapshots verify output, not internal state invariants).

The risk-to-reward ratio is poor. The file works. The functions are long but linear: each arm is independent of the others, so reading any single arm is easy even when the function is huge.

### 3. The file works correctly today

`cargo test --workspace` passes. `cargo clippy --lib -- -D warnings` is clean (CI gates on this). The 95 snapshots are green. No open bug links the god-functions to a defect. The audit findings are pure code-smell findings (length, complexity), not correctness findings.

### 4. The registry growth pattern resists extraction

The dispatchers grow by accretion. Every T124 stdlib task adds arms. The natural extraction would be one sub-function per prelude type (one for DateTime, one for Regex, etc.), but that still leaves the parent `match` as a routing table and moves the per-type logic into ~30 small files. The win is debatable: the total LOC stays the same, the cross-file coupling increases, and every new prelude type now requires touching two files instead of one.

A table-driven approach (a registry of `(type_name, method_name) -> lowering_fn` closures) is theoretically cleaner but fights the project's "no raw-string codegen" and "explicit `syn` construction" rules. Each lowering arm builds a distinct `syn` expression tree, and a closure-based registry would still need to construct those trees by hand. The dispatch logic is not the problem; the per-method `syn` construction is the bulk of the LOC.

### 5. T105a already captured the safe wins

T105a already did the mechanical extraction. The 11 child modules under `rust_codegen/` hold the safely-extractable logic: declaration lowering, expression lowering, method-call lowering, type lowering, extern-crate detection, derive attributes, dependency detection, and the syn construction helpers. What remains in the parent file is the dispatch core that resists clean extraction without a state-threading redesign.

### 6. format.rs is not actually a concern

cq-003 mentions `format.rs`, but the file is 47 lines: one `pub fn format(file: &File) -> String` wrapper around `prettyplease::unparse`, plus two unit tests. It is the single string producer in the crate, by design. There is nothing to refactor. The cq-003 concern properly applies to the other codegen-rust files (atomic_analysis, race_analysis, gpu_alignment) and the larger child modules, where long functions exist but are bounded and single-purpose.

## Consequences

- **cq-001, cq-002, cq-003 remain OPEN but DEFERRED.** The v3.2 audit report should record their status as deferred, not resolved.
- **No source code changes** in `crates/buff-lang-codegen-rust/` result from this decision.
- **The god-functions will keep growing** as T124 stdlib tasks add prelude types. Each new prelude type adds arms to `lower_prelude_type_assoc_fn` and `lower_prelude_type_instance_fn`. The file will cross 11,000 lines on the next sizable stdlib wave.
- **Future refactoring, if attempted, should be incremental and test-gated.** The safe path is one prelude type at a time: extract a single type's arms into a sub-function, run the full snapshot suite, commit, repeat. A big-bang split risks the pipeline.
- **The T105a precedent is the template.** Any future extraction should follow the same `pub(super)` child-module pattern, inherit imports via `use super::*`, and keep the parent as the routing core.
- **This decision does not block the v1.26 launch** or any planned work. The codegen pipeline is production-correct; the findings are hygiene, not defects.
- **Review trigger.** Revisit this deferral if (a) the file crosses 15,000 lines, (b) a bug is traced to state-threading confusion in one of the dispatchers, or (c) a prelude registry redesign lands that changes the dispatch shape.

## References

- **DR-014** - `.sisyphus/decisions/selfhost-feasibility.md` (per-crate portability verdicts; codegen-rust classified 🟥 IMPOSSIBLE, "THE WALL")
- **Porting conventions** - `.sisyphus/decisions/porting-conventions.md` §9 ("What DOES NOT Port") lists `syn::File` / `quote!` / `prettyplease` as the wall
- **v3.2 audit report** - cq-001 (large file), cq-002 (god-functions), cq-003 (codegen-rust-wide pattern)
- **Per-crate AGENTS.md** - `crates/buff-lang-codegen-rust/AGENTS.md` (file structure, execution order, conventions)
- **T105a submodule split** - the 11 child modules under `crates/buff-lang-codegen-rust/src/rust_codegen/`
- **T124 stdlib registry** - `crates/buff-lang-types/src/prelude_types.rs` (1,919-line extensible registry the dispatchers lower)
- **Strategic direction** - `.sisyphus/decisions/buff-direction-speed-moat-selfhost.md` (self-host scope: rewrite compiler crates in Buff but keep emitting Rust; codegen-rust stays in Rust)
