# examples/

Runnable .buff programs, .buffhtml SFC UI apps, rust-vs-buff side-by-side comparisons, and a use-cases/ suite of real-world programs used for v1.26 demo + smoke-testing.

## STRUCTURE

```
examples/
├── ola.buff, fibonacci.buff, closures.buff, ...  # Top-level (v0.1–v0.5 core demos, ✅ runs)
├── calculadora.buff                              # PT-BR naming (hello-world style)
├── *.buffhtml (6 files)                          # UI SFC examples (T133+)
├── *.rs (6 files)                                # Standalone Rust snippets (rust-vs-buff companions)
├── modules/ (2)                                  # Module system demo
├── tensor/ (3)                                   # buff-tensor framework (T8)
├── pipeline/ (3)                                 # buff-pipeline framework (T12)
├── science/ (3)                                  # buff-science framework (T10)
├── ml/ (3)                                       # buff-ml framework (T13)
├── game/ (3)                                     # buff-game framework (T14)
├── dsp/                                          # buff-dsp framework demos
├── plugins/                                      # buff-plugins framework demos
├── integration/ (4)                              # Cross-framework demos (T22)
├── data-science-workbench/ (5)                   # Flagship multi-framework app (T23)
├── rust-vs-buff/ (13 topics)                     # Side-by-side .buff vs .rs comparisons
├── channels/ (2), debug/ (3), fuzz/ (1)          # Feature-specific demos
├── comptime_*, math_*, multi_dispatch_*,         # Advanced feature demos
│   property_wrappers_*, cold_start_*, pgo_*, minimal_*, extern_*, ai_*, plugin_*,
│   refactor_*, cache_*, check_demo, fast_build_demo, sccache_demo,
│   hot_reload_server, watch_demo, format_specifiers, generics, range, medium_project
└── use-cases/                                    # T11–T18 real-world programs (v1.26)
    ├── _sample.buff + _sample.expected           # Template for new use-cases
    ├── <name>.buff + <name>.expected             # 16 programs, each with golden output
    ├── BUGS-FOUND.md                             # Bug tracker from use-case batches
    └── apps/                                     # T16–T18 full-app examples
        ├── cli_file_manager.buff                 # T17 CLI with subcommands
        ├── data_pipeline.buff                    # T18 ETL data processing
        └── rest_api_server.buff                  # T16 REST API + CRUD + middleware
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| Add a core language example | Top-level `examples/<name>.buff` (snake_case) |
| Add a framework demo | `examples/<framework>/` (tensor, pipeline, science, ml, game, dsp, plugins) |
| Add a rust-vs-buff comparison | `examples/rust-vs-buff/<topic>/` (paired .buff + .rs + README.md) |
| Add a UI SFC example | Top-level `examples/<name>.buffhtml` |
| Add a cross-framework integration demo | `examples/integration/<name>.buff` |
| Add a real-world use-case | `examples/use-cases/<name>.buff` + `<name>.expected` (golden output) |
| Add a full-app example | `examples/use-cases/apps/<name>.buff` (≥2-crate framework cap; see BUGS-FOUND.md) |
| Start a new use-case | Copy `examples/use-cases/_sample.buff` + its `.expected` |

## CONVENTIONS (this dir only)

- **Naming**: snake_case filenames (e.g. `cold_start_minimal.buff`). Exception: PT-BR names for hello-world style examples (`ola`, `calculadora`).
- **Language choice**: Portuguese names for hello-world/conversational examples, English for all feature demos.
- **rust-vs-buff/ structure**: each subdirectory has exactly three files: `<topic>.buff`, `<topic>.rs`, `README.md`. The .rs is vanilla Rust; the .buff is the equivalent Buff code. README explains the pain point Buff solves.
- **`.buffhtml` files**: SFC format with `<script>buff</script>` + `<template>RSX</template>` + `<style>CSS</style>` sections.
- **use-cases/ pairs**: every `<name>.buff` has a `<name>.expected` file capturing stdout. Drift = test failure.
- **use-cases/apps/ cap**: full-app examples use ≤2 framework crates (see `BUGS-FOUND.md` T16 rest_api_server which uses only `buff-web`).
- **BUGS-FOUND.md** in `use-cases/` is the running log of bugs found while building use-case examples. Append-only per batch (T11–T18).
- **No test assertions in examples**. Examples are for human reading and `buff run` smoke-testing. Programmatic tests go in `tests/fixtures/` or `crates/*/tests/`.

## NOTES

- **Status legend** (critical distinction):
  - ✅ **runs**: `buff run <file>` succeeds end-to-end (lex → parse → codegen → rustc → native exe).
  - 🔶 **codegen-only / parse-only**: transpiles to Rust but the generated code references types or APIs not yet wired in codegen. Most framework examples (tensor/, pipeline/, science/, ml/, game/) are in this state.
- **Framework type coordination**: `Type::{Tensor, DataFrame, Pipeline, ...}` variants in `buff-lang-types` and their codegen lowering in `buff-lang-codegen-rust` are sibling work tracked across multiple tasks. See `.sisyphus/decisions/api-compat-v20.md`. Don't assume a framework example that parses also compiles.
- **Top-level examples are the ✅ set**: `ola.buff`, `fibonacci.buff`, `closures.buff`, `pattern_matching.buff`, `error_handling.buff`, `collections.buff`, `prelude_demo.buff`, `async_demo.buff` all run end-to-end.
- **use-cases/ is parse-clean + golden-output**: programs are validated by the golden-output harness (T5) — `buff check` clean, `.expected` matches stdout when runnable. See `.github/workflows/ci.yml`.
- **Buff syntax rules** (canonical in root AGENTS.md § CONVENTIONS): 4-space indent, no tabs, offside-rule blocks, named boolean args, `Type.new()` constructors only.
- **No README.md** at `examples/` root. This file (AGENTS.md) is the sole navigation aid; the use-cases/ subdir has its own `BUGS-FOUND.md`.
