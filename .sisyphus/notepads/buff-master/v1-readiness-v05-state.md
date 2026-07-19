# v1.0 Readiness: v0.5 State Report

**Generated:** 2026-07-19
**Source:** Comprehensive analysis of buff repo at C:\Users\vsbb1\source\repos\buff

---

## 1. v0.5 PLAN CHECKLIST COMPLETION

**File:** .sisyphus/plans/buff-v05-language.md (1082 lines)

### Core Tasks (T18-T37): 20 tasks — ALL COMPLETE
- [x] T18: Double (f64) full support
- [x] T19: Byte (Bits<8>) support
- [x] T20: Decimal (128-bit) — rust_decimal integration
- [x] T21: String + Char operations + interpolation
- [x] T22: Numeric coercion rules — flexible vs fixed modes
- [x] T96: Standard library prelude
- [x] T99: Process environment access
- [x] T23: Vector<T> type + codegen
- [x] T24: Matrix<T> type + codegen
- [x] T25: Map<K,V> type + codegen
- [x] T26: Struct type + repr(C) codegen
- [x] T27: Enum type + pattern matching
- [x] T28: Option<T> + null safety
- [x] T29: Module system (import/export, multi-file, path resolution)
- [x] T30: Error types + ? operator
- [x] T31: Async with call graph propagation
- [x] T32: FFI basics — import Rust crates
- [x] T33: Intelligent clone analysis
- [x] T34: Closures/lambdas codegen
- [x] T35: buff test command
- [x] T36: Error message improvements + parser error recovery
- [x] T37: v0.5 milestone — comprehensive example suite

### Enhancement Tasks: 27 tasks — ALL COMPLETE
- [x] T67: Collection literals
- [x] T68: Range syntax
- [x] T69: Pipeline operator |>
- [x] T70: Null-conditional ?.
- [x] T71: Destructuring assignment
- [x] T102: Expression functions =>
- [x] T104: Raw strings
- [x] T72: If-let / For-let
- [x] T73: Early return guards
- [x] T74: Let chains
- [x] T75: Extension methods
- [x] T76: Union types A | B
- [x] T77: Expected-type driven inference
- [x] T92: Struct embedding + delegation
- [x] T93: Traits with default methods
- [x] T101: Null coalescing ??
- [x] T103: Tuples
- [x] T107: Auto-derived record methods
- [x] T78: Error context chaining
- [x] T79: Regex literals
- [x] T100: defer statement
- [x] T105: Named arguments
- [x] T106: Default parameter values
- [x] T111: buff.toml config + project structure enforcement
- [x] T112: buff new templates
- [x] (other enhancements all checked)

### Phase Exit Criteria (lines 1069-1082): ALL UNCHECKED [ ]
This is the most important finding. The entire Phase Exit Criteria block — 12 criteria — are ALL still [ ] (unchecked), even though the README says "Core shipped":

1. [ ] all 13 types working: Int, Bits, Float, Double, Decimal, Byte, Bool, String, Vector, Matrix, Map, Struct, Enum
2. [ ] Pattern matching with exhaustiveness checking
3. [ ] Module system with import/export
4. [ ] async with Tokio
5. [ ] Error handling with ? operator
6. [ ] Closures with type inference
7. [ ] Modern syntax: pipeline, null-conditional, destructuring, guards, ranges, collection literals
8. [ ] buff test runs test suites
9. [ ] Error messages with spans and suggestions
10. [ ] cargo test --workspace passes 100%
11. [ ] cargo clippy --workspace -- -D warnings clean
12. [ ] Git tag v0.5.0 created

**Conclusion:** 47/47 tasks are marked [x] complete at the task level, but the Phase Exit Criteria were NEVER checked off. The README says "Core shipped" but the formal exit criteria were not signed off. Many criteria likely DO pass (cargo test, clippy, type coverage per learnings) but the checklist was never finalized.

---

## 2. EXAMPLES THAT WORK TODAY

**File:** examples/ directory — 8 .buff files + 1 modules/ subdirectory

| Example | Lines | What It Demonstrates | Status per README |
|---------|-------|---------------------|-------------------|
| ola.buff | 3 | Hello world — print("Olá, Buff!") | ✅ v0.1 (runs) |
| ibonacci.buff | 10 | Recursive fibonacci(10) = 55 | ✅ v0.1 (runs) |
| calculadora.buff | 7 | dd(2,3) function call | ✅ v0.1 (runs) |
| closures.buff | 36 | { x => x * 2 } lambda with .map() | ✅ v0.5 (runs) |
| collections.buff | 42 | Vector [10,20,30], Map {1:10}, .push(), .pop(), .map() | ✅ v0.5 (runs) |
| pattern_matching.buff | 60 | match on Option<T>, Result<T,E> with Ok/Err arms | ✅ v0.5 (runs) |
| error_handling.buff | 49 | Result<Int, Error>, ? operator, Error("msg"), Ok(v) | ✅ v0.5 (runs) |
| sync_demo.buff | 44 | sync func, spawn, .result() — codegen ONLY | 🔶 v0.5 (codegen-only) |
| prelude_demo.buff | 2 | Minimal print(1+2) | ✅ runs |
| modules/main.buff | 24 | import { greet } from "./greet.buff" — codegen ONLY | 🔶 v0.5 (codegen-only) |
| modules/greet.buff | 11 | export func for module system | (part of modules/ example) |

**Key gap:** 5 examples run end-to-end (uff run compiles, executes, produces output). 2 examples (async_demo, modules/) are codegen-only because the CLI pipeline lacks Cargo-project wiring for 	okio and multi-file linking.

---

## 3. ASYNC SUPPORT STATE

### AST Nodes (crates/buff-lang-ast/src/expr.rs):
- Expr::Spawn { task: Box<Expr>, span: Span } — T31 additive variant (line 403)
- Expr::SuspendExpr { inner: Box<Expr>, span: Span } — v0.1 placeholder (line 311)
- No Expr::Await variant (by design — Buff has no wait keyword)
- Keywords: KwAsync, KwSpawn (both reserved)

### Codegen (crates/buff-lang-codegen-rust/src/rust_codegen.rs):
- 	okio::spawn(async move { #task_expr }) — lower_spawn method (line 3428)
- #[tokio::main] on main when in async set (line 812-813)
- Runtime::new().expect(...).block_on(#arg) — lock() lowering (line 3507-3508)
- 	ask.await — .result() lowering (line 3465 doc comment)
- sync_fns: BTreeSet<String> — propagated async function names (line 113)
- sync_block_depth: usize — tracking for nested spawn bodies (line 125)
- in_async_context() — determines if .await should be inserted (line 820+)
- 21 codegen tests + 21 async_propagation tests — all pass

### Analysis (crates/buff-lang-types/src/async_analysis.rs):
- CallGraph with BTreeMap<String, BTreeSet<String>> (deterministic)
- Fixpoint algorithm: seed from is_async, propagate through call graph
- 923+ lines of fully tested async analysis
- NO HashMap (T29 determinism lesson applied)

### End-to-end status:
- sync_demo.buff produces valid Rust but CANNOT RUN because the CLI calls ustc directly on a single .rs file with no Cargo.toml — tokio is not linked
- The uff-lang-runtime crate is a stub (will provide tokio execution in v1.0)
- Full E2E async execution is blocked on Cargo-project pipeline

---

## 4. MODULE SYSTEM STATE

### Resolution code (crates/buff-lang-types/src/modules.rs):
- **646 lines** — fully implemented ModuleGraph, uild_graph, esolve_path
- ModuleGraph { modules, topo_order, root } — topological sort with cycle detection
- ModuleLoader trait with FsLoader for production, in-memory for tests
- esolve_path handles ./, ../, auto-appends .buff, std/ reserved
- 61 tests total across parser + types crates
- Critical fix: T29 flaky HashMap → deterministic topo_order (documented in issues.md)

### Multi-file example (examples/modules/):
- main.buff — imports greet from ./greet.buff
- greet.buff — exports greeting_for and greet
- STATUS: CODEGEN-VERIFIED ONLY — the module graph resolves, but CLI compiles one file

### CLI pipeline (crates/buff-lang-cli/src/pipeline.rs):
- compile_to_rust(file: &Path) — reads ONE .buff file, lexes, parses, codegens ONE .rs file
- compile_rust_to_exe — invokes ustc --edition 2021 -O on ONE .rs file
- NO multi-file walk, NO module graph traversal, NO Cargo.toml generation
- commands/run.rs calls compile_to_rust then compile_rust_to_exe — single file end-to-end

### Available CLI commands (crates/buff-lang-cli/src/cli.rs):
- Build — compile .buff to executable
- Run — compile + execute
- New — scaffold project (--lib, --server, --gpu, --workspace)
- Init — scaffold current dir
- Test — run @test functions (T35)

---

## 5. v0.5 ISSUES (from notepad)

**File:** .sisyphus/notepads/buff-v05-language/issues.md

### Single documented issue: T29 — Flaky 	ypes_modules_export_star_chain
- **Status:** FIXED
- **Symptom:** Intermittent test failure in cargo test --workspace (parallel) but always passed in cargo test -p buff-lang-types --test modules (solo)
- **Root cause:** esolve_reexports iterated ctx.modules.values() — a HashMap<PathBuf, Module> with randomly-seeded SipHash → non-deterministic iteration order across runs. For  → export * from b → export * from c → export deep, only 1 of 6 HashMap permutations processes deps-first; the other 5 leave .exports empty.
- **Fix:** Iterate ctx.topo_order (post-order DFS, pre-computed) instead of raw HashMap values. Topological order guarantees deps before importers.
- **Hardening:** 50-iteration stress test + 5-deep chain (120 permutations, 99.2% detection)
- **Lesson:** "When a graph algorithm's correctness depends on processing order, NEVER iterate a HashMap directly"

### No other issues documented
The issues.md file is 74 lines total, all about this one fixed bug. No pending issues are recorded.

---

## 6. v0.5 LEARNINGS — SUMMARY FOR v1.0

**File:** .sisyphus/notepads/buff-v05-language/learnings.md (~2607+ lines across all tasks)

### Most important patterns/conventions relevant to v1.0:

1. **Deterministic data structures everywhere.** Multiple tasks (T29, T31, T32, T33, T34, T35, T69, T70) independently hit the same lesson: NEVER use HashMap/HashSet for codegen output or graph analysis results that feed codegen. Use BTreeMap/BTreeSet consistently so iteration order is deterministic across runs. The T29 flaky-test (non-deterministic re-export resolution) was the canonical case, but T31's async analysis, T32's extern crate collection, T34's closure captures, T35's test discovery, and every codegen pass all internalized this rule.

2. **Parser-desugar pattern avoids AST/codegen ripple.** T69 (|> pipeline), T70 (?. null-conditional), T74 (let chains) all chose parser-level desugaring over new AST variants — rewrite the syntax into existing nodes in the parser, then the existing type inference and codegen handle it automatically. This is more robust than adding new Expr variants that must be matched at every site. T71 (destructuring) was the notable exception where new variants were genuinely needed.

3. **Codegen via syn/quote/prettyplease — never raw strings.** This rule was universally respected. The one subtle case was T20's Decimal: syn::parse_str on the raw decimal text (which builds proc_macro2::TokenStream, NOT a Rust string) was explicitly NOT "raw-string codegen" and was the only correct way to preserve trailing zeros through the ust_decimal_macros::dec!() macro without rounding through f64.

4. **Type system limits in v0.5:** Multiple deferrals documented for v1.0:
   - No Type::Function variant (closures are opaque Unknown)
   - No Type::UserEnum variant (exhaustiveness checker uses pattern-name matching, not resolved types)
   - No real generics beyond builtin collections
   - Type errors are WARNINGS (deferred to v0.5 — still deferred per v0.5 design)
   - Expected-type inference is shallow (only .map/.filter single-param lambdas)

5. **Parser recovery architecture:** T36 implemented parse_recovering() which accumulates errors and syncs to top-level starters (unc/async/enum/import/export/extern/At). Commas are NOT sync points. The infinite-loop guard forces advance when recovery makes no progress. parse() (fail-fast) and parse_recovering() share parse_one_decl — guaranteed agreement between the two modes.

6. **Additive AST change pattern (establishes v1.0 precedent):**
   - New variants ALWAYS added at END of enum (purely additive — no existing variant renamed/reordered)
   - Every match site gets a new arm (exhaustive, caught by cargo check --workspace)
   - Migration notes in doc comments (first seen in T20, formalized by T21)
   - All derives remain Debug, Clone, PartialEq — NO Eq/Hash unless essential

7. **Quote-aware codegen gotchas:**
   - proc_macro2::Literal::f64_unsuffixed(99.90) ROUNDS to 99.9 — T20 used raw text passthrough instead
   - quote!{ vec![ } FAILS (proc-macro2 can't take raw [/] as literal tokens) — use MacroDelimiter::Bracket instead
   - cast_to() wraps EVERY operand in parens → [(0) as usize] — T23 wrote a dedicated cast_to_usize() for clean [0 as usize]
   - syn::parse_str::<Pat> is a TRAP in syn 2.0 — Pat does NOT impl Parse
   - quote! inside println!: adjacent string literals DON'T concat inside macro args — use {} format placeholder

8. **Windows-specific gotchas:**
   - Path::new("/main.buff").is_absolute() returns alse on Windows (needs drive prefix)
   - 	est_all_25_keywords — function name is historical; actual count grew from 25 to 27 (T73 added guard, T75 added extend)
   - .exe image-locking after process exit: T35/Run both use retry loops for cleanup

9. **Codegen flow control for v1.0 awareness:**
   - lower_block special-cases Stmt::Guard (multi-stmt emission at same scope)
   - Spawn/ExtendBlock/union wrapper enums/embedding delegation — all emit ADDITIONAL items AFTER the main lowering loop in generate()
   - T34's closure_capture_stack — a Vec<BTreeSet<String>> pushed/popped per closure, tracks which idents bypass MoveAnalyzer
   - T100's deferred_exprs — defer statements accumulate expressions; drained in LIFO at every eturn and at body tail

10. **The "already existed" pattern:** T18, T19, T67 all found their target features ALREADY IMPLEMENTED across all layers (lexer/parser/types/codegen) — the task only needed new tests. This suggests v0.5 development was sometimes testing after the fact rather than following strict TDD. V1.0 should verify this doesn't indicate gaps in test coverage.

---

## SUMMARY: v0.5 GAPS FOR v1.0

1. **Cargo-project pipeline (BLOCKER):** Single-file ustc invocation cannot link external crates (tokio, rust_decimal, regex for T79) or multi-file Buff programs. T32's extern_crates() set is collected but never wired to a Cargo.toml. V1.0 must switch from ustc <file>.rs to generating a Cargo project with proper [dependencies].

2. **Multi-file codegen (BLOCKER):** T29's module graph is fully resolved but the codegen only processes one file. Need to walk ModuleGraph::topo_order, codegen each module, and emit as Rust mod blocks or inlined decls.

3. **Phase exit criteria not finalized:** 12 formal criteria are all unchecked. Many likely pass (cargo test, clippy, type coverage) but this was never formally verified and committed.

4. **Async E2E:** Requires Cargo-project pipeline (tokio dep). sync_demo.buff is valid and tested at codegen level but cannot execute.

5. **Regex literal codegen stubbed:** T79 lowers /\d+/ as a plain string literal, not Regex::new(...). The egex crate dependency and proper codegen are deferred to v1.0.

6. **No dedicated uff build entry in README's running-examples table** (not a code gap, but v1.0 should update the README).

7. **uff-lang-runtime crate is a stub** — needs tokio, rayon, wgpu wiring for v1.0.

8. **uff-lang-codegen-wgsl crate is a stub** — GPU dispatch is the headline v1.0 feature.

