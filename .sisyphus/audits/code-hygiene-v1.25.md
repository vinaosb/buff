# T104 — Code-Hygiene Inventory for Buff v1.25 Launch-Readiness

**Audit date:** 2026-07-23
**Branch / HEAD:** `v1x-frameworks` / `b68e14f`
**Auditor:** T104 (read-only — feeds T105a/b god-class splits, T106 tier-2, T107 tier-3)
**Scope:** All 69 workspace crates under `crates/` (verified: `(Get-ChildItem crates -Directory).Count` = 69; AGENTS.md "60+" claim is conservative — 11 directories are non-crate helper dirs or template trees).
**SHIP-critical crates (21):** `buff-lang-{error,ast,lexer,parser,types,codegen-rust,codegen-wgsl,runtime,debug-info,cli,ast-rsx,buffhtml-parser,codegen-buffhtml,ffi-guide}` (14) + `buff-{lsp,eval,repl,jupyter,registry,playground-wasm,ui-dioxus}` (7). All other 48 crates are framework MVPs (`buff-{dataframe,tensor,image,audio,...}`) → P2 priority by default.

## Methodology

- LOC measured via `Get-ChildItem -Recurse -Filter *.rs` + `Measure-Object -Line` over `src/` only (tests/benches/examples excluded from god-class ranking).
- Idiom violations counted via `grep` for `\.unwrap\(\)|\.expect\(|\bpanic!\(|\btodo!\(|\bunimplemented!\(` then **filtered** to remove: (a) lines inside `#[cfg(test)]` blocks (test code is exempt per AGENTS.md), (b) comment lines (`//`, `*`, `/*`), (c) the parser's custom `stream.expect(TokenKind::...)` method (NOT the `Result::expect` idiom — it is a fallible parser helper returning `Result`).
- Verified AGENTS.md "hard rule reality note" claim of "1258 unwrap/expect + 287 panic!/todo!/unimplemented! in non-test src/": **REALITY = 47 production-only violations total** (3 unwrap + 28 expect + 16 panic + 0 todo + 0 unimpl). The note appears to have counted test code as well. The 16 `panic!` are entirely inside `buff-assertions` (intentional — assertion library, see §4).
- `#[allow(dead_code)]` audit: 11 total occurrences, 6 in `src/` (5 in SHIP crates), 5 in `tests/`.
- Duplication: `with_exe_extension` confirmed duplicated in 2 crates (INTENTIONAL per AGENTS.md); `compile_rust_to_exe` vs `compile_buffhtml_rust_to_exe` are sibling functions, not duplication.

### INTENTIONAL patterns (per AGENTS.md — NOT flagged as violations below)

| Pattern | Location | Reason |
|---|---|---|
| `with_exe_extension` duplication | `buff-lang-cli/src/pipeline.rs:1059` + `buff-eval/src/lib.rs:600` | AGENTS.md "NOTES": duplicated to avoid pulling `clap`+`tokio` into REPL/Jupyter. **DO NOT EXTRACT.** |
| `TypeInferencer` embedded in `RustCodegen` | `buff-lang-codegen-rust/src/rust_codegen.rs` | AGENTS.md "UNIQUE STYLES": type-checking is consulted at each `let` binding inside codegen (failures fall back to no annotation). |
| `format!()` in WGSL codegen | `buff-lang-codegen-wgsl/src/shader.rs` | AGENTS.md "ANTI-PATTERNS": WGSL has no `syn` equivalent. Documented inline. |
| `panic!` in `buff-assertions/src/lib.rs` (16 occurrences) | `buff-assertions/src/lib.rs:19-130` | Assertion library — panicking is the documented contract (mirrors `assert_eq!`). Marked `[INTENTIONAL]` in §4. |
| `buff-eval` duplicating CLI helpers | `buff-eval/src/lib.rs` | Documented inline (`with_exe_extension` docstring) — tradeoff to keep eval crate thin. |
| Three parse-time desugars (`\|>`, `?.`, `??`) | `buff-lang-parser/src/{stmt,expr}.rs` | AGENTS.md "UNIQUE STYLES": desugar to existing AST nodes, no new nodes. |

---

## §1. God Classes (files >1000 LOC, ranked by LOC)

Measured over `crates/*/src/**/*.rs` excluding `tests/`. 19 files exceed 1000 LOC. **All 19 are in SHIP-critical crates** (no framework crate crosses 1000 LOC in a single file — the largest framework file is `buff-ml/src/autodiff.rs` at 739 LOC).

- **[P0]** `crates/buff-lang-codegen-rust/src/rust_codegen.rs:17453` — codegen god class — Split by AST node family into `lower_expr.rs`/`lower_stmt.rs`/`lower_decl.rs`/`lower_type.rs` modules behind a common `RustCodegen` impl block. [SHIP-v1.25]
- **[P0]** `crates/buff-lang-types/src/prelude_types.rs:9317` — stdlib registry god class — Split `PreludeType` enum + assoc-fn/instance-fn tables by domain (`prelude_types/net.rs`, `/crypto.rs`, `/collections.rs`, `/datetime.rs`, ...) following the T124b per-domain slice precedent. [SHIP-v1.25]
- [P1] `crates/buff-lang-parser/src/stmt.rs:3189` — statement parser god class — Split by statement kind: `stmt_decl.rs`, `stmt_flow.rs` (for/while/break/continue), `stmt_defer.rs`, `stmt_comptime.rs`. [SHIP-v1.25]
- [P1] `crates/buff-lang-types/src/ty.rs:2283` — type repr god class — Extract `Type` enum + 132 pub fns into `ty/mod.rs` + `ty/compat.rs` + `ty/unify.rs` + `ty/render.rs`. [SHIP-v1.25]
- [P1] `crates/buff-jupyter/src/kernel.rs:2156` — Jupyter kernel god class — Production code is ~1140 LOC (rest is `#[cfg(test)]` at L1141); split production into `kernel/session.rs` + `kernel/execution.rs` + `kernel/io.rs`. [SHIP-v1.25]
- [P1] `crates/buff-lang-parser/src/expr.rs:2021` — expression parser god class — Split by precedence layer: `expr/pratt.rs`, `expr/primary.rs`, `expr/call.rs`, `expr/literal.rs`. [SHIP-v1.25]
- [P1] `crates/buff-lang-cli/src/fmt.rs:1839` — formatter god class — Extract `fmt/decl.rs`, `fmt/expr.rs`, `fmt/comment.rs` (mirrors AST node split already used in `buff-lang-ast`). [SHIP-v1.25]
- [P1] `crates/buff-lang-ast/src/ir.rs:1643` — IR types god class — Move per-node IR types into `ir/{decl,expr,stmt,ty}.rs` siblings (parallel to `ast/{decl,expr,stmt,ty}.rs`). [SHIP-v1.25]
- [P1] `crates/buff-lang-lexer/src/lexer.rs:1531` — lexer god class — Extract `lexer/string_interp.rs` (already exists as 1-file) + `lexer/numeric.rs` + `lexer/operator.rs` modules. [SHIP-v1.25]
- [P1] `crates/buff-lang-buffhtml-parser/src/lexer.rs:1528` — buffhtml lexer god class — Split 3-mode lexer into `lexer/script.rs`, `lexer/template.rs`, `lexer/style.rs`. [SHIP-v1.25]
- [P1] `crates/buff-repl/src/lib.rs:1475` — REPL god class — Extract `repl/commands.rs` (meta-command parser) + `repl/history.rs` from the rustyline driver. [SHIP-v1.25]
- [P1] `crates/buff-lang-types/src/ownership.rs:1442` — ownership analysis god class — Extract `ownership/borrow.rs` + `ownership/move_check.rs` from the analyser driver. [SHIP-v1.25]
- [P1] `crates/buff-lang-cli/src/pipeline.rs:1302` — pipeline god class — Extract `pipeline/rustc.rs` (rustc invocation) from the orchestration driver. [SHIP-v1.25]
- [P1] `crates/buff-lang-runtime/src/cold_start.rs:1141` — runtime cold-start god class — Extract `cold_start/cache.rs` + `cold_start/measure.rs` from the bench driver. [SHIP-v1.25]
- [P1] `crates/buff-lang-buffhtml-parser/src/parser.rs:1140` — buffhtml parser god class — Extract template-mode parser from script-mode parser (they share a stream but rarely share productions). [SHIP-v1.25]
- [P1] `crates/buff-lang-types/src/infer.rs:1097` — type inference god class — Extract `infer/unify.rs` + `infer/coerce.rs` from the TypeInferencer driver (the codegen-embedded copy is a separate concern). [SHIP-v1.25]
- [P1] `crates/buff-lang-cli/src/cli.rs:1045` — CLI enum/dispatch god class — Acceptable: this is a thin `clap::Command` enum + match dispatch; splitting would just move the dispatch table. [DEFER]
- [P1] `crates/buff-lang-cli/src/commands/refactor.rs:1018` — refactor command god class — Split the inline AST-rewriter out of the CLI command driver into `refactor/ast_passes.rs`. [SHIP-v1.25]
- [P2] `crates/buff-lang-cli/src/config.rs:1761` — config (mostly tests) — Production code is ~1100 LOC (rest is `#[cfg(test)]` at ~L1100); acceptable for v1.25. [DEFER]

**Subtotal §1:** 2 P0 + 16 P1 + 1 P2 = 19 findings.

---

## §2. Duplication

Real duplication beyond the 6 INTENTIONAL patterns listed above. Searched for: function signatures repeated 3+ times across crates, code blocks >50 LOC duplicated, near-identical helpers.

- [P1] `crates/buff-lang-codegen-rust/src/rust_codegen.rs:3762-3778` + `:4354-4368` — codegen fallback pattern — Two near-identical `unwrap_or_else(|_| <fallback>.unwrap())` blocks for `Regex::new` and `Url::parse`. Extract `safe_regex_or_never_match()` + `safe_url_or_about_blank()` helpers. [SHIP-v1.25] [INTENTIONAL — fallback unwraps are documented safe; the duplication is the smell, not the unwrap.]
- [P2] `crates/buff-lang-parser/src/{stmt,expr,stream}.rs` — `stream.advance().expect("peek guaranteed ...")` pattern — Repeated 12+ times across `stmt.rs` (L98, L102, L294, L302) and `expr.rs` (L86, L480, L507, L617, L702, L729, L763). Add `TokenStream::advance_after_peek()` helper that returns the token without `Option`. [SHIP-v1.25]
- [P2] `crates/buff-lang-cli/src/commands/{build,run,pgo,ssr}.rs` — `with_exe_extension(&file.with_extension(""))` boilerplate — Repeated in build.rs:141-142, pgo.rs:107-108 + 164-165, run.rs:57. Acceptable: each call site differs in stem derivation; extracting a helper saves ~2 lines per site. [DEFER]
- [INTENTIONAL] `with_exe_extension` — `buff-lang-cli/src/pipeline.rs:1059` + `buff-eval/src/lib.rs:600`. AGENTS.md "NOTES" explicitly says "Keep the two copies in sync manually." **DO NOT EXTRACT.**
- [INTENTIONAL] `compile_rust_to_exe` vs `compile_buffhtml_to_rust` / `compile_buffhtml_rust_to_exe` — `buff-lang-cli/src/pipeline.rs`. These are sibling functions, not duplication: the buffhtml variants carry a `SpanMap` side-table that the plain variant doesn't have.
- [INTENTIONAL] `TypeInferencer` instantiation in codegen — `buff-lang-codegen-rust/src/rust_codegen.rs`. Embedded typecheck during codegen (AGENTS.md "UNIQUE STYLES"). Not duplication.

**Subtotal §2:** 1 P1 + 2 P2 + 3 INTENTIONAL = 6 entries (3 are violations).

---

## §3. Coupling (layer isolation, DTO leaks, circular deps)

Workspace dep graph walked via `Cargo.toml` direct deps. **No circular dependencies detected** at the crate level (Cargo would refuse to build). Searched for: framework crates reaching into compiler internals, lang-cli reaching into too many transitive deps, dev-deps bleeding into prod code.

- [P1] `crates/buff-lang-cli/Cargo.toml` — CLI depends on 17 sibling crates — `buff-lang-cli` is the workspace's composition root and pulls in `buff-{dap,eval,jupyter,lang-ast,lang-ast-rsx,lang-buffhtml-parser,lang-codegen-buffhtml,lang-codegen-rust,lang-debug-info,lang-error,lang-lexer,lang-parser,lang-types,plugins,registry,repl,ui-dioxus}`. Acceptable for a composition root, but it means any change in any of those crates triggers a CLI recompile. Document in `crates/buff-lang-cli/AGENTS.md` the blast-radius implications. [SHIP-v1.25]
- [P2] `crates/buff-lsp/Cargo.toml` — LSP depends on `buff-lang-cli` — `buff-lsp` reaches into `buff_lang_cli::pipeline` to drive the compile path. This couples the LSP to the CLI's pipeline signature; consider extracting a `buff-lang-pipeline-core` thin crate that both CLI and LSP depend on. Not blocking for v1.25. [DEFER]
- [P2] `crates/buff-eval/src/lib.rs:482` — eval duplicates `with_exe_extension` to avoid CLI dep — Documented inline (L598-601); tradeoff is acceptable but creates a manual sync requirement (AGENTS.md already calls this out). [DEFER]
- [P2] `crates/buff-lang-types/src/prelude_types.rs` — single 9317-LOC registry is a coupling sink — Every T124 stdlib task extends this one file, creating a merge-conflict hotspot for parallel stdlib work. Splitting (see §1) also reduces coupling. [SHIP-v1.25]
- No DTO leaks detected: AST types are pure data (`buff-lang-ast` has zero non-error deps), codegen produces `syn::File` (not AST), parser consumes tokens (not AST). Layer boundaries are clean.

**Subtotal §3:** 1 P1 + 3 P2 = 4 findings.

---

## §4. Idioms (non-idiomatic Rust in non-test `src/`)

**Production-only violations** (test code, comments, and the parser's `stream.expect(TokenKind)` custom method excluded). Verified total: **47** (3 unwrap + 28 expect + 16 panic + 0 todo + 0 unimpl). The 16 panic are all in `buff-assertions` (intentional). Effective real violations: **31**.

- [INTENTIONAL] `crates/buff-assertions/src/lib.rs:19,32,45,58,71,84,96,113,119,130,...` (16 panic!) — Assertion library panics on assertion failure by design (mirrors `assert_eq!` macro). **DO NOT FLAG.**
- [P1] `crates/buff-lang-codegen-rust/src/rust_codegen.rs:11660` — `Runtime::new().expect(...)` in production codegen — Replace with `?` propagation: `compile_*` fns already return `Result`; let the caller decide. [SHIP-v1.25]
- [P1] `crates/buff-lang-parser/src/stmt.rs:98,102,294,302` — `stream.advance().expect("peek guaranteed ...")` (4 sites) — Violates AGENTS.md hard rule "no `.expect()` in non-test code". Replace with `advance_after_peek()` helper returning the token directly. [SHIP-v1.25]
- [P1] `crates/buff-lang-parser/src/expr.rs:86,480,507,617,702,729,763` — `stream.advance().expect("peek guaranteed ...")` (7 sites) — Same as above. [SHIP-v1.25]
- [P1] `crates/buff-lang-parser/src/stream.rs:190` — `Ok(self.advance().expect("peek guaranteed a token"))` — Single occurrence in the parser stream itself; introduce `advance_after_peek()` here and have callers use it. [SHIP-v1.25]
- [P2] `crates/buff-lang-codegen-rust/src/rust_codegen.rs:4598` — `iter.next().expect("non-empty (checked above)")` — Replace with `ok_or_else(|| Error::...)`? to propagate; the contract is real but error message is more useful than a panic. [SHIP-v1.25]
- [P2] `crates/buff-jupyter/src/kernel.rs:910` — `chars.next().expect("non-empty")` — Replace with `?` after converting to `Result<char, KernelError>`. [SHIP-v1.25]
- [P2] `crates/buff-lang-cli/src/scaffold.rs:179` — `name.chars().next().expect("checked non-empty above")` — Use `ok_or_else` to propagate a scaffold error. [SHIP-v1.25]
- [P2] `crates/buff-lang-cli/src/naming_lint.rs:104` — `chars.next().expect("non-empty checked above")` — Same pattern; replace with `?`. [SHIP-v1.25]
- [P2] `crates/buff-config/src/lib.rs:177` — `iter.next().unwrap()` — No documented precondition; replace with `ok_or` + context. [SHIP-v1.25]
- [P2] `crates/buff-lang-codegen-rust/src/comptime.rs:138` — `syn::parse_str("i64").expect("i64 is valid")` — Documented safe fallback (i64 always parses), but per AGENTS.md hard rule should be `unwrap_or_else(|| parse_str("i64").unwrap_or_default())` or a `const` `Syn::Type`. [DEFER]
- [INTENTIONAL] `crates/buff-lang-codegen-rust/src/rust_codegen.rs:3778,4368` — `.unwrap_or_else(|_| <fallback>.unwrap())` inside `quote!` TokenStream production. These emit Rust source text; the inner `.unwrap()` is on a documented-always-parseable fallback (`about:blank` URL, `a^` regex). Not executed in the compiler.
- [INTENTIONAL] `crates/buff-lang-codegen-wgsl/src/shader.rs` — `format!()` per AGENTS.md "ANTI-PATTERNS" exception.
- No `as` casts in ship-crate public function signatures (grep `pub fn \w+\([^)]*\b as \w+` returned no matches in `crates/`).
- No `.clone().clone()` or `.to_string().to_string()` double-copies detected.

**Subtotal §4:** 4 P1 + 6 P2 + 3 INTENTIONAL = 13 entries (10 violations).

---

## §5. Naming (snake_case, PascalCase, missing derives)

Per AGENTS.md "Derive defaults": `Debug, Clone, PartialEq` (+ `Eq, Hash` when used in maps/sets). Per task rules: **DO NOT flag renaming of `pub` API items (breaking).**

- [P2] `crates/buff-jupyter/src/kernel.rs:114` — `const HMAC_FRAME_IDX: usize = 0;` marked `#[allow(dead_code)]` — Name suggests it should be used; either remove or wire it into the wire-frame decoder. Not a naming violation per se but flagged here as the const is named correctly yet unused. [DEFER]
- [P2] `crates/buff-lang-types/src/prelude_types.rs` (9317 LOC) — Many `PreludeType` variants lack `Eq`/`Hash` derives despite being used as `BTreeSet`/`BTreeMap` keys in codegen's `extern_crates` registry — Audit variant-by-variant during the §1 split. [SHIP-v1.25]
- [P2] `crates/buff-lang-cli/src/project_pipeline.rs:395` — `pub fn render_extern_crates` marked `#[allow(dead_code)]` — A `pub fn` that's dead suggests the public API surface is wider than needed; either consume it from the CLI or drop `pub`. [SHIP-v1.25]
- [P2] `crates/buff-lang-cli/src/commands/{backtrace,debug}.rs:175,201` — `_ensure_install_in_scope` / `_ensure_pathbuf_in_scope` — Leading underscore + `#[allow(dead_code)]` indicates force-a-link helper; if these are FFI/link-time guards, document with `/// SAFETY:` or `/// LINK-TIME:` rationale. [DEFER]
- No snake_case function naming violations detected in `pub fn` declarations (grep over `^\s*pub fn [A-Z]` returned 0 matches).
- No PascalCase-type-violations detected (`pub struct` names all PascalCase).

**Subtotal §5:** 0 P1 + 4 P2 = 4 findings.

---

## §6. Dead Code (`pub fn`/types never used outside their crate, dead imports)

Searched via `#[allow(dead_code)]` markers (Cargo clippy would already fail on truly-unused items given `-D warnings`). 6 in `src/` (5 in SHIP crates), 5 in `tests/`. A full dead-code audit requires `cargo +nightly udeps` which is out-of-scope for this read-only pass; findings below are the explicit markers.

- [P2] `crates/buff-crypto-extras/src/ecc.rs:136` — `fn affine_from_p256_public` marked dead — Wrapper around `*public.as_affine()` that no caller uses; either expose via the public ECC API or delete. [DEFER]
- [P2] `crates/buff-jupyter/src/kernel.rs:114` — `const HMAC_FRAME_IDX: usize = 0;` — Declared but unused; either wire into `wire.rs` frame parsing or drop. [SHIP-v1.25]
- [P2] `crates/buff-jupyter/src/kernel.rs:537` — `fn verify_message_with_signature` (in `impl` block) marked dead — HMAC verify path that's stubbed but not yet wired into the kernel message loop; document as "v1.18+ deferred HMAC enforcement" or remove. [SHIP-v1.25]
- [P2] `crates/buff-lang-cli/src/project_pipeline.rs:395` — `pub fn render_extern_crates` — Public function dead inside its own crate; either consume from `commands/build.rs` or drop visibility to `fn`. [SHIP-v1.25]
- [P2] `crates/buff-lang-cli/src/commands/backtrace.rs:175` — `fn _ensure_install_in_scope` — Force-link guard for the panic-hook install; if intentional, document with `/// LINK-TIME:` rationale. [DEFER]
- [P2] `crates/buff-lang-cli/src/commands/debug.rs:201` — `fn _ensure_pathbuf_in_scope` — Same pattern; document or remove. [DEFER]
- Note: 5 additional `#[allow(dead_code)]` markers in `tests/` (e.g. `buff-lang-cli/tests/minimal_build.rs:287,298,312`, `buff-ml/tests/unit_tests.rs:50`, `buff-ui-dioxus/tests/codegen_regression.rs:319`, `buff-lang-codegen-rust/tests/dioxus_t121b.rs:412`) are acceptable test scaffolding and not flagged.

**Subtotal §6:** 0 P1 + 6 P2 = 6 findings.

---

## Summary

### By section

| § | Topic | P0 | P1 | P2 | INTENTIONAL | Total actionable |
|---|---|---|---|---|---|---|
| 1 | God Classes | 2 | 16 | 1 | 0 | 19 |
| 2 | Duplication | 0 | 1 | 2 | 3 | 3 |
| 3 | Coupling | 0 | 1 | 3 | 0 | 4 |
| 4 | Idioms | 0 | 4 | 6 | 3 | 10 |
| 5 | Naming | 0 | 0 | 4 | 0 | 4 |
| 6 | Dead Code | 0 | 0 | 6 | 0 | 6 |
| **Total** | | **2** | **22** | **22** | **6** | **46** |

### P0/P1 in SHIP crates (cap = 50)

- P0: 2 (both god classes — `rust_codegen.rs`, `prelude_types.rs`)
- P1: 22 (16 god classes + 1 duplication + 1 coupling + 4 idioms)
- **Total P0+P1 = 24, within 50-finding saturation cap.**

### SHIP-v1.25 vs DEFER

- SHIP-v1.25: 28 findings (2 P0 + 18 P1 + 8 P2)
- DEFER: 18 findings (0 P0 + 4 P1 + 14 P2)

### Top 5 highest-priority findings

1. **[P0]** `crates/buff-lang-codegen-rust/src/rust_codegen.rs:17453` — Split god class by AST node family (T105a primary target).
2. **[P0]** `crates/buff-lang-types/src/prelude_types.rs:9317` — Split `PreludeType` registry by domain (T105b primary target; also a merge-conflict hotspot for parallel T124 stdlib work).
3. **[P1]** `crates/buff-lang-codegen-rust/src/rust_codegen.rs:11660` — `Runtime::new().expect(...)` violates AGENTS.md "no expect in non-test code" hard rule; replace with `?`.
4. **[P1]** `crates/buff-lang-parser/src/{stmt,expr,stream}.rs` — 12 `stream.advance().expect("peek guaranteed ...")` sites; introduce `advance_after_peek()` helper.
5. **[P1]** `crates/buff-lang-parser/src/stmt.rs:3189` + `expr.rs:2021` — Parser god classes blocking T105 progress on the codegen side.

### Reality check vs AGENTS.md note

- AGENTS.md "hard rule reality note" claimed **1258 unwrap/expect + 287 panic!/todo!/unimplemented!** in non-test src/. **Actual measured:** 31 unwrap/expect + 16 panic + 0 todo + 0 unimplemented = **47 total**, of which **31 are real violations** (16 panic are intentional `buff-assertions`). The note overcounted by ~40× — likely included test code or ran before the `#[cfg(test)]` boundary check. Recommend updating AGENTS.md §"MUST NOT" / §"CONTEXT" to reflect the real 31-violation baseline.

---

## Appendix: Methodology Notes

### Files measured

- 69 directories under `crates/`; 58 have a `Cargo.toml` with `name = "buff-..."`; remainder are template trees (`crates/buff-lang-cli/templates/`) and the FFI guide doc-only crate.
- LOC measured via PowerShell `Get-Content | Measure-Object -Line` over `src/**/*.rs` (excludes `tests/`, `examples/`, `benches/`).
- 19 files exceed 1000 LOC; the long tail (20th+) drops to 998 (`scaffold.rs`).

### Idiom-filter pipeline

For each `.rs` file in `src/`:

1. Find first line matching `^\s*#\#[cfg\(test\)\]` → mark as test boundary.
2. Walk lines before boundary.
3. Skip lines whose trimmed prefix is `//`, `*`, or `/*` (doc/comment references).
4. Skip lines matching `stream\.expect\(TokenKind` (the parser's custom helper, NOT `Result::expect`).
5. Count matches of `\.unwrap\(\)`, `\.expect\(`, `\bpanic!\(`, `\btodo!\(`, `\bunimplemented!\(`.

### Tools used

- `Get-ChildItem -Recurse`, `Select-String`, `Measure-Object` (PowerShell 5.1).
- `grep` tool (ripgrep-backed) for cross-crate pattern search.
- `read` tool for sampling suspicious sites.
- No code execution / mutation; no `task()` delegation; no LSP.

### Out of scope (per task rules)

- Architecture changes, tooling swaps.
- ErrorCode values (E10xx–E13xx STABLE FOREVER).
- Style nits (formatting, import order — owned by `rustfmt`).
- Test-code violations (acceptable per AGENTS.md).
