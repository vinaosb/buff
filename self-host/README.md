# Self-Host Port Status

## Two Distinct Corpora

This repo has TWO separate .buff port directories with different purposes:

### 1. `crates/*/selfhost/*.buff` — Data-Model Ports (harness-tested)

These files port the **data types** (structs, enums, constructors) of each crate
to Buff. They compile and run via `buff run`, and are tested against Rust example
binaries via `scripts/equivalence-rust-vs-buff.sh`.

**What they ARE:** Type definitions + constructors + test `main()` functions
that instantiate types and print field values. This proves the data model
ports cleanly.

**What they are NOT:** Full behavioral reimplementations. The actual crate
LOGIC (e.g., `tokenize()` in lexer, `parse()` in parser, `eval()` in evaluator)
is NOT ported. The `.buff` files' header comments explicitly document this.

**13 crates with data-model ports:** buff-lang-error, buff-lang-ast,
buff-lang-ffi-guide, buff-lang-lexer, buff-lang-buffhtml-parser,
buff-lang-parser, buff-lang-debug-info, buff-template, buff-eval,
buff-pubsub, buff-fsm, buff-tensor, buff-lang-ast-rsx.

### 2. `self-host/*/*.buff` — Aspirational Logic Ports (determinism-tested)

These 56 files are the T15-T19 era attempts to port ACTUAL LOGIC (not just
data types) of the compiler front-end (lexer 5, parser 7, types 22, codegen 22).

**Status:** Only 7/56 transpile cleanly (per `bootstrap-report.md`). The
remaining 49 fail at lex/parse/codegen stages due to language gaps and
parser/codegen bugs documented in the bootstrap report.

**Determinism:** The 7 files that DO transpile produce byte-identical output
across two consecutive runs (Stage 2 == Stage 3 determinism verified via
`bootstrap_t19` example).

## What "Self-Hosting" Means for Buff

Per DR-014 (`.sisyphus/decisions/selfhost-feasibility.md`):

- **12 crates are IMPOSSIBLE** without multi-quarter language redesign
  (codegen-rust 58k LOC requires Rust-AST stdlib; types needs trait objects;
  runtime needs rayon/wgpu/tokio FFI)
- **~13 crates are potentially portable** — data types port cleanly; actual
  logic requires multi-week effort per crate
- **The codegen-rust wall is fundamental** — Buff generates Rust via syn/quote,
  which Buff itself cannot do without either a Rust-AST stdlib or breaking the
  no-raw-string-codegen rule

## Next Steps

1. Port actual logic for the simplest achievable crate (e.g., `compute_level`
   from `indent.rs`) to demonstrate the pattern works
2. Fix the 49/56 `self-host/` transpile failures (parser/codegen gaps)
3. Evaluate whether Buff needs byte-level operations for the lexer port
4. Consider whether to add trait objects to Buff for the types crate port

## Key Artifacts

- **DR-014**: `.sisyphus/decisions/selfhost-feasibility.md` — feasibility analysis
- **Bootstrap report**: `self-host/bootstrap-report.md` — Stage 1A results
- **Equivalence harness**: `scripts/equivalence-rust-vs-buff.sh` — Rust-vs-Buff stdout comparison
- **CI self-host check**: `.github/workflows/ci.yml` job `self-host-check` (informational)
