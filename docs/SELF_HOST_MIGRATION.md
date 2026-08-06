# Self-Host Migration Guide

**Applies to:** v1.40+ (Deprecation Phase B)
**Decision record:** `.sisyphus/decisions/deprecation-phase-b.md`
**Roadmap reference:** `.sisyphus/plans/self-host-completion-roadmap.md` (P5.10)

---

## 1. Why

Buff's self-host front-end is achieved. The compiler's core data model and parsing logic now exist as `.buff` source ports alongside the original Rust implementations. These ports were built across v1.25 through v1.37, verified by the behavioral equivalence harness, and locked by the bootstrap determinism gate (Stage 2 == Stage 3).

The `.buff` ports cover the 10 crates identified as portable in DR-014 (`.sisyphus/decisions/selfhost-feasibility.md`). The parity audit (`.sisyphus/evidence/parity-audit.md`) confirmed all 10 are GREEN: 223 pub functions, 78 structs, 27 enums across 23,014 lines of Rust, 82% pure-value with no unsafe blocks.

This migration guide explains what changes for contributors and users now that the `.buff` ports are the canonical source.

---

## 2. What Changed

**Rust originals in the TARGET crates are frozen.** Starting at v1.40, no new features are added to the Rust implementations of these crates. Bug fixes may still be applied to both the Rust originals and the `.buff` ports in parallel. All new language features, error codes, AST nodes, parser constructs, and lexer tokens go into the `.buff` ports first.

**The TARGET crates are:**

`buff-lang-ast`, `buff-lang-ast-rsx`, `buff-lang-error`, `buff-lang-lexer`,
`buff-lang-parser`, `buff-lang-buffhtml-parser`, `buff-lang-debug-info`,
`buff-lang-ffi-guide`, `buff-eval`, `buff-template`.

**What "frozen" means in practice:**

- New features: `.buff` ports only. The Rust originals get a comment noting the feature lives in the `.buff` source.
- Bug fixes: applied to both sides. If a bug exists only in the Rust crate (unlikely but possible), fix it there too and add a harness regression test.
- Breaking API changes: coordinated across both. The Rust crate retains the old API surface until the `.buff` port has the new one passing equivalence.
- Full deletion of the Rust originals is **deferred to v2.x**. They remain in the tree, compilable, but dormant.

**What is NOT affected:**

- Framework crates (`buff-dataframe`, `buff-tensor`, `buff-image`, etc.) continue to develop independently. They are Buff's product surface, not self-host candidates.
- The 12 IMPOSSIBLE crates (`buff-lang-codegen-rust`, `buff-lang-types`, `buff-lang-runtime`, `buff-lang-codegen-buffhtml`, `buff-lang-codegen-wgsl`, `buff-registry`, `buff-jupyter`, `buff-lsp`, `buff-dap`, `buff-ui-dioxus`, `buff-playground-wasm`, `buff-mcp`) remain active Rust crates with no planned `.buff` port.
- Tooling crates (`buff-repl`, `bufflings`, `buffup`, `buff-cli`) continue normal development.

---

## 3. For Compiler Contributors

### Where the `.buff` ports live

Two directories hold `.buff` port material:

**`self-host/`** at the repo root contains the aspirational logic ports. The main file is `self-host/buff_compiler.buff`, a 1,400-line monolith that combines error, AST, lexer, and parser types with stub function bodies. Subdirectories (`error/`, `ast/`, `lexer/`, `parser/`, `types/`, `codegen/`, `debug-info/`, `buffhtml-parser/`, `ast-rsx/`, `eval/`, `template/`, `ffi-guide/`) hold per-crate individual port files.

**`crates/*/selfhost/*.buff`** files are data-model ports. These contain struct/enum definitions plus constructors and test `main()` functions. They compile via `buff run` and are verified against Rust example binaries through the equivalence harness (`scripts/equivalence-rust-vs-buff.sh`).

The monolith exists because Buff does not yet support multi-file linking (T29). Once that lands, the per-directory individual files become the canonical form and the monolith is superseded.

### How to test

```bash
# Type-check a .buff port (fast, no codegen)
cargo run -p buff-lang-cli -- check self-host/buff_compiler.buff

# Build the full self-host front-end (if multi-file linking available)
cargo run -p buff-lang-cli -- build --self-host self-host/buff_compiler.buff
```

### How the equivalence harness works

The behavioral equivalence harness (`scripts/equivalence-rust-vs-buff.sh`) runs the same test inputs through both the Rust crate and the `.buff` port, then compares stdout. It does not require byte-identical output. Span differences, formatting variation, and ordering within associative containers are tolerated. The contract is: same input produces semantically equivalent observable behavior.

The bootstrap determinism gate (T19, v1.37) verifies that a Buff-compiled compiler produces byte-identical output across two consecutive runs. As of the latest report, 7 of 56 `.buff` files transpile cleanly and all 7 pass the determinism check.

### Workflow for a new feature

1. Implement the feature in the `.buff` port under `self-host/<crate>/`.
2. Add test cases to the port's `main()` or a companion test file.
3. Run the equivalence harness to verify parity with the Rust crate.
4. If the Rust crate needs a matching API addition (e.g., a new enum variant consumed by codegen-rust), add it with a `// self-host: canonical in .buff` comment.
5. Do NOT implement feature logic in the Rust crate first.

---

## 4. For Framework Users

No change is needed. The framework crates (`buff-dataframe`, `buff-tensor`, `buff-image`, `buff-audio`, `buff-ecs`, `buff-dsp`, `buff-science`, `buff-pipeline`, `buff-ml`, `buff-game`, `buff-web`, `buff-db`, `buff-template`, and the remaining ~30 crates) are Buff's product surface. They are not self-host candidates and are unaffected by the self-host transition.

If you maintain an `extern` wrapper crate that depends on `buff-lang-ast` or `buff-lang-error` for type definitions, those Rust crates continue to compile and publish normally. The frozen status only restricts new feature additions, not existing API consumption.

---

## 5. For Tooling (LSP, REPL, Jupyter)

The tooling crates consume the Rust compiler crates as library dependencies. `buff-lsp` calls `buff_lang_parser::parse()`, `buff-eval` wraps the pipeline, `buff-repl` wraps `buff-eval`, and `buff-jupyter` wraps `buff-repl`.

**Nothing changes in the short term.** The Rust crates remain compilable and their APIs are stable. Tooling continues to `use buff_lang_parser`, `use buff_lang_lexer`, etc. as before.

**Longer term**, once multi-file linking (T29) lands and the `.buff` ports have full behavioral parity, the tooling may optionally switch to consuming compiled `.buff` output instead of Rust source. This is not required during Phase B. The equivalence harness guarantees the two surfaces produce the same results, so any such switch is a drop-in.

If you are building new tooling that needs parser or lexer access, use the Rust crates as you would today. The `.buff` ports are the canonical source of truth for feature development, but the Rust crates remain the stable integration point.

---

## References

- DR-014: `.sisyphus/decisions/selfhost-feasibility.md` (feasibility assessment)
- Parity audit: `.sisyphus/evidence/parity-audit.md` (10-crate inventory, all GREEN)
- Bootstrap report: `self-host/bootstrap-report.md` (T19 determinism gate)
- Roadmap: `.sisyphus/plans/self-host-completion-roadmap.md` (P5.10, Phase B)
- Deprecation Phase B: `.sisyphus/decisions/deprecation-phase-b.md` (formal definition)
- Equivalence harness: `scripts/equivalence-rust-vs-buff.sh`
