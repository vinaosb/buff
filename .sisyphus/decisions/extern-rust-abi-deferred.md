# Decision Record: extern "Rust" ABI Not Supported — Codegen Self-Host Permanently Deferred

**Decision Record:** DR-019
**Date:** 2026-07-27
**Status:** ACCEPTED
**Supersedes:** None
**Related:** DR-014 (selfhost-feasibility), T119 (FFI spec), GUIDE.md Rule 1

## Context

The P0.3 triage (`self-host/triage.md`) identified that **20 of 22** `self-host/codegen/*.buff` files use `extern "Rust" { ... }` blocks to call back into the Rust-written compiler. Buff's T119 spec deliberately restricts `extern` to the `"C"` ABI only.

This is not a compiler bug — it is a **policy decision**: loosen T119 to admit `"Rust"` ABI for the bootstrap use case, or accept that the codegen crate port is permanently blocked.

## Decision

**Do NOT loosen T119. The `extern "Rust"` ABI remains unsupported.**

The 20 affected `self-host/codegen/*.buff` files are **permanently deferred** as unsupported. This decision is final unless the Buff language undergoes a fundamental redesign (see DR-014 §"The Codegen-Rust Wall").

## Rationale

### 1. DR-014 Already Documented This Wall

DR-014 (`selfhost-feasibility.md`) classifies `buff-lang-codegen-rust` (58,617 LOC) as **IMPOSSIBLE** to port. The crate generates Rust via `syn`/`quote`/`prettyplease`. Buff has no Rust-AST stdlib. Raw-string codegen is BANNED by project anti-pattern rule.

The 20 `extern "Rust"` files are aspirational ports of codegen MODULES. They use `extern "Rust"` to call into `syn`/`quote` APIs — the exact wall DR-014 describes.

### 2. Loosening T119 Weakens FFI Safety for Zero Benefit

Buff's `extern "C"` restriction exists because:
- **C ABI is stable and portable** across all platforms
- **Rust ABI is unstable** — the Rust compiler does not guarantee ABI stability between releases
- **`extern "C"` forces a safe boundary** — all types must have a defined C representation
- **`extern "Rust"` would allow arbitrary Rust types** to cross the boundary, including types with Drop impls, lifetimes, and trait objects that Buff cannot reason about

Opening `extern "Rust"` would weaken Buff's FFI safety model for a use case that DR-014 has already documented as impossible.

### 3. C-Shims Are Prohibitively Expensive

Writing `#[no_mangle] extern "C"` wrapper functions for every `syn`/`quote`/`prettyplease` symbol the .buff ports need would require:
- ~500+ wrapper functions (the codegen crate has thousands of `syn` calls)
- Custom C-compatible type translations for every `syn::*` type
- Ongoing maintenance as `syn` evolves

This is more work than porting the compiler itself, and produces worse code.

### 4. The Self-Host Goal Does Not Require It

DR-014 §"What Self-Hosted Means for Buff" is clear:
- **Front-end in Buff** (achievable): lexer + parser + AST + types
- **Back-end stays in Rust** (permanent): codegen layer

The 20 codegen files are in the "permanent" category. Removing them from scope is not a scope reduction — it's an honest acknowledgment of a documented wall.

## Affected Files (20)

All files under `self-host/codegen/` that use `extern "Rust"` blocks:

| File | Status |
|------|--------|
| `codegen/context.buff` | DEFERRED — extern "Rust" |
| `codegen/conv_helpers.buff` | DEFERRED — extern "Rust" |
| `codegen/decl_lowering.buff` | DEFERRED — extern "Rust" |
| `codegen/dependency_detection.buff` | DEFERRED — extern "Rust" |
| `codegen/derive_attrs.buff` | DEFERRED — extern "Rust" |
| `codegen/expr_lowering.buff` | DEFERRED — extern "Rust" |
| `codegen/extern_crate_detection.buff` | DEFERRED — extern "Rust" |
| `codegen/extern_crate_detection_extra.buff` | DEFERRED — extern "Rust" |
| `codegen/format.buff` | DEFERRED — extern "Rust" |
| `codegen/gpu_alignment.buff` | DEFERRED — extern "Rust" |
| `codegen/lib.buff` | DEFERRED — extern "Rust" |
| `codegen/lowering_helpers.buff` | DEFERRED — extern "Rust" |
| `codegen/method_call_lowering.buff` | DEFERRED — extern "Rust" |
| `codegen/move_analysis.buff` | DEFERRED — extern "Rust" |
| `codegen/multi_crate.buff` | DEFERRED — extern "Rust" |
| `codegen/passes.buff` | DEFERRED — extern "Rust" |
| `codegen/race_analysis.buff` | DEFERRED — extern "Rust" |
| `codegen/rust_codegen.buff` | DEFERRED — extern "Rust" |
| `codegen/syn_helpers.buff` | DEFERRED — extern "Rust" |
| `codegen/type_lowering.buff` | DEFERRED — extern "Rust" |

## Also Affected (Not extern "Rust" but Same Category)

| File | Classification | Status |
|------|---------------|--------|
| `codegen/atomic_analysis.buff` | lang-gap | DEFERRED — uses `func` keyword as parameter name; requires raw identifiers (`r#func`) which Buff does not have |
| `codegen/comptime.buff` | bug | FIXABLE — `match EXPR:` colon-form parser bug (P1.2 scope) |

## Consequences

### Positive
- **Honest scope**: 20 files removed from active work, categorized as permanently unsupported
- **No wasted effort**: No one attempts to loosen T119 for a blocked use case
- **Clear self-host boundary**: Front-end (achievable) vs back-end (permanent Rust)

### Negative
- **Self-host corpus max is 36 files** (56 total - 20 deferred), not 56
- **`codegen/atomic_analysis.buff` also deferred** — raw identifiers are post-v2

### Updated Self-Host Scorecard

```
Total files:           56
PASS (already):        12
Now PASS (enum fix):   +5  = 17 PASS
TYPE fixes (remaining): 12 fixable (Phase 1)
LEX fixes (P1.1):        4 fixable
PARSE fix (comptime):    1 fixable
Lang-gap (func kw):      1 DEFERRED (DR-019)
Unsupported (extern Rust): 20 DEFERRED (DR-019)
────────────────────────────────
Achievable max:       17 + 12 + 4 + 1 = 34 files
Permanently deferred: 21 files (20 extern "Rust" + 1 func kw)
```

## Action Items

1. **Update P0.3 triage** to mark the 20 `extern "Rust"` files as DEFERRED (not bug/unsupported — specifically DEFERRED per DR-019)
2. **Update Phase 1 scope** — P1.2/P1.3/P1.4/P1.5 are mostly empty (only `comptime.buff` parse bug remains)
3. **Update audit remediation tracker** — sec/ft findings related to codegen port coverage should reference DR-019
4. **Update roadmap** — Phase 3/4 codegen port tasks should be marked DEFERRED

## References

- DR-014: `.sisyphus/decisions/selfhost-feasibility.md`
- P0.3 triage: `self-host/triage.md`
- T119 FFI spec: `crates/buff-lang-ffi-guide/GUIDE.md`
- AGENTS.md anti-pattern: "Raw-string Rust codegen"
