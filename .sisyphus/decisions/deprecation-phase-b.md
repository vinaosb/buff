# Deprecation Phase B: Rust Originals Frozen

**Decision Record:** DR-015
**Date:** 2026-08-06
**Status:** ACTIVE
**Origin:** P5.10 of `.sisyphus/plans/self-host-completion-roadmap.md`
**Predecessor:** DR-014 (`.sisyphus/decisions/selfhost-feasibility.md`)
**Migration guide:** `docs/SELF_HOST_MIGRATION.md`

---

## Context

The Buff self-host front-end shipped across v1.25 through v1.37. Ten of the compiler's crates now have verified `.buff` ports. The parity audit confirmed all 10 are GREEN. The bootstrap determinism gate holds for every file that transpiles cleanly.

DR-014 identified these 10 crates as portable and 12 as IMPOSSIBLE. The self-host completion roadmap (P5.10) called for a formal deprecation phase definition, which was previously missing.

---

## Phase A (Completed)

Both Rust originals and `.buff` ports exist side-by-side. The behavioral equivalence harness verifies that both produce semantically identical observable behavior for the same inputs. Phase A covers v1.25 through v1.39.

During Phase A:
- New features could go into either the Rust or `.buff` implementation.
- The equivalence harness was the gate: no port was considered complete until harness tests passed.
- The bootstrap determinism gate (T19) verified Stage 2 == Stage 3 byte-identical output.

Phase A is now concluded. All 10 TARGET crates have `.buff` ports that pass `buff check` and the harness.

---

## Phase B (Active, Starting v1.40)

The Rust originals in the TARGET crates are **frozen**. This is the key behavioral change from Phase A.

**Rules:**

1. **No new features in Rust originals.** All new language constructs, AST nodes, error codes, lexer tokens, parser productions, and diagnostic improvements go into the `.buff` ports under `self-host/`. The Rust originals receive a comment noting the feature's canonical location.

2. **Bug fixes may be applied to both.** If a bug is found in production usage of the Rust crate, fix it there and add a harness regression test. If the `.buff` port shares the same bug, fix it there too.

3. **API additions for downstream compatibility are allowed.** If `buff-lang-codegen-rust` (an IMPOSSIBLE crate, not frozen) needs a new enum variant from `buff-lang-ast`, that variant may be added to the Rust `ast` crate with a `// self-host: canonical in .buff` comment. The `.buff` port should receive the same variant promptly.

4. **Full deletion of Rust originals is deferred to v2.x.** The frozen crates remain in the tree, compilable, tested, and published. They are not deleted, disabled, or moved to an archive directory.

---

## TARGET Crates

These 10 crates are in scope for Phase B:

| Crate | LOC | Port Status |
|---|---|---|
| `buff-lang-ast` | 5,246 | GREEN, 54 pub fns |
| `buff-lang-ast-rsx` | 418 | GREEN, 10 pub fns |
| `buff-lang-error` | 2,404 | GREEN, 49 pub fns |
| `buff-lang-lexer` | 2,911 | GREEN, 15 pub fns |
| `buff-lang-parser` | 7,236 | GREEN, 64 pub fns |
| `buff-lang-buffhtml-parser` | 2,748 | GREEN, 7 pub fns |
| `buff-lang-debug-info` | 1,117 | GREEN, 16 pub fns |
| `buff-lang-ffi-guide` | 19 | GREEN (docs only) |
| `buff-eval` | 865 | GREEN, 5 pub fns |
| `buff-template` | 150 | GREEN, 3 pub fns |

Source: `.sisyphus/evidence/parity-audit.md`

**Excluded from Phase B (IMPOSSIBLE per DR-014, remain active):**

`buff-lang-codegen-rust`, `buff-lang-types`, `buff-lang-runtime`,
`buff-lang-codegen-buffhtml`, `buff-lang-codegen-wgsl`, `buff-registry`,
`buff-jupyter`, `buff-lsp`, `buff-dap`, `buff-ui-dioxus`,
`buff-playground-wasm`, `buff-mcp`.

**Excluded from Phase B (framework crates, unaffected):**

All `buff-{dataframe,tensor,image,audio,ecs,dsp,science,pipeline,ml,game,web,db,...}` crates. These are Buff's product, not self-host candidates.

---

## Enforcement

### Code review

Pull requests that modify Rust source files in TARGET crates are flagged during review. Reviewers check whether the change is:
- A **bug fix** (allowed, with harness regression test)
- A **downstream API addition** for an active crate (allowed, with comment)
- A **new feature** (rejected; redirect to `.buff` port)

### CI ideas

A CI check could diff the pub API surface of each TARGET crate's Rust source against a snapshot taken at v1.40. Any API growth (new pub fn, new enum variant, new struct field) that lacks a corresponding `.buff` port change and a `// self-host: canonical in .buff` comment would fail the check. This is aspirational for Phase B and would be formalized before Phase C.

### Documentation

Each TARGET crate's `AGENTS.md` or `lib.rs` header should note the frozen status with a reference to this decision record.

---

## Timeline

| Phase | Version | Status |
|---|---|---|
| Phase A (side-by-side) | v1.25 -- v1.39 | Completed |
| Phase B (Rust originals frozen) | v1.40+ | Active |
| Phase C (Rust originals deleted) | v2.x | Deferred |

Phase C requires multi-file linking (T29) to be production-ready, all 56 `.buff` port files to transpile cleanly (currently 7/56), and the equivalence harness to cover 100% of the pub API surface. None of these prerequisites are met today. Phase C will be planned in a future decision record once the blockers are resolved.

---

## Consequences

Positive:
- Contributor effort focuses on a single canonical source (the `.buff` ports) instead of dual maintenance.
- The `.buff` ports serve as executable documentation of the compiler's data model and parsing logic.
- Framework and tooling consumers see no disruption.

Negative:
- Contributors must learn to work in `.buff` for compiler front-end changes, not Rust.
- The 49/56 `.buff` files that fail to transpile represent a gap. Until multi-file linking lands, the monolith (`buff_compiler.buff`) is the primary integration point.
- Two sets of source to keep bug-fixed in parallel, even if only one gets new features.

---

## References

- DR-014: `.sisyphus/decisions/selfhost-feasibility.md`
- P5.10: `.sisyphus/plans/self-host-completion-roadmap.md` (lines 679-687)
- Migration guide: `docs/SELF_HOST_MIGRATION.md`
- Parity audit: `.sisyphus/evidence/parity-audit.md`
- Bootstrap report: `self-host/bootstrap-report.md`
