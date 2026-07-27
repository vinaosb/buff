# P0.4 — Parity Audit (Rust → Buff Port Feasibility)

**Audit date:** 2026-07-27
**Scope:** 10 target Rust crates (excludes IMPOSSIBLE crates codegen-rust/types/runtime/cli/lsp/etc.)
**Method:** read-only `grep`/`Select-String` inventory of `pub fn` / `pub struct` / `pub enum` / `pub trait` / `dyn` / `unsafe` / external `use` across every file in each crate's `src/` (including nested subdirectories `expr/` and `stmt/`).
**Outputs:** `.sisyphus/evidence/parity-audit.json` (structured), `.sisyphus/evidence/parity-audit.md` (this file).
**Status:** AUTHORITATIVE — supersedes any earlier draft. All counts verified by directory-level ripgrep + content-level inspection of every dyn/unsafe hit.

---

## Headline verdict

| Verdict | Count | Crates |
|---|---|---|
| 🟢 **GREEN** (clean port) | **7** | buff-lang-ast, buff-lang-ast-rsx, buff-lang-error, buff-lang-parser, buff-lang-buffhtml-parser, buff-lang-ffi-guide, buff-template |
| 🟡 **YELLOW** (work needed) | **3** | buff-lang-debug-info, buff-lang-lexer, buff-eval |
| 🔴 **RED** (blocking) | **0** | — |

**No RED crates across the entire target set.** Every crate has a pub API expressible in Buff (no `unsafe` blocks, no `extern` FFI). The 3 YELLOW crates have either (a) a refactorable dyn-trait in a pub fn signature (lexer) or (b) implementation dependencies on OS-level facilities hidden behind a clean API surface (debug-info, eval).

---

## Aggregate inventory (223 pub fns across 10 crates)

| Metric | Total | Notes |
|---|---|---|
| pub fns | **223** | free fns + impl methods, any indent |
| pub structs | **78** | |
| pub enums | **27** | |
| pub traits | **1** | `LexCallback` in buff-lang-lexer (pub trait; used by pub fn `scan_string`) |
| real `dyn` occurrences | **2** | 1 std::error::Error impl (universal) + 1 in pub fn signature (lexer) |
| real `unsafe` blocks | **0** | all textual `unsafe` matches are keyword tokens / doc comments / Display strings |
| `extern "ABI"` in Rust src | **0** | textual matches describe Buff-source syntax being parsed, not Rust FFI |
| total LOC | **23,014** | |

### Purity tier distribution

| Tier | Count | % | Description |
|---|---|---|---|
| T1 — pure-value | **182** | 82% | Returns Bool/Int/String/struct; no I/O, no state |
| T2 — collection | **32** | 14% | Returns Vec / iterator / slice |
| T3 — volatile | **3** | 1% | Reads env / TTY / backtrace (no time/UUID/random in any crate) |
| T4 — stateful | **6** | 3% | File I/O, subprocess, global state mutation |

**82% of all pub fns are pure** — directly mappable to Buff value-returning fns with no runtime intrinsics required.

---

## Per-crate summary table

| # | Crate | LOC | Files | pub fn | pub struct | pub enum | pub trait | dyn | unsafe | Verdict | T1 | T2 | T3 | T4 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | buff-lang-ast | 5,246 | 9 | 54 | 33 | 15 | 0 | 0 | 0¹ | 🟢 GREEN | 40 | 14 | 0 | 0 |
| 2 | buff-lang-ast-rsx | 418 | 1 | 10 | 14 | 2 | 0 | 0 | 0 | 🟢 GREEN | 10 | 0 | 0 | 0 |
| 3 | buff-lang-error | 2,404 | 7 | 49 | 15 | 5 | 0 | 0 | 0 | 🟢 GREEN | 44 | 4 | 1 | 0 |
| 4 | buff-lang-debug-info | 1,117 | 4 | 16 | 8 | 0 | 0 | 0² | 0 | 🟡 YELLOW | 10 | 1 | 2 | 3 |
| 5 | buff-lang-lexer | 2,911 | 6 | 15 | 3 | 1 | 1 | 2³ | 0⁴ | 🟡 YELLOW | 12 | 3 | 0 | 0 |
| 6 | buff-lang-parser | 7,236 | 9 | 64 | 1 | 1 | 0 | 0 | 0⁵ | 🟢 GREEN | 55 | 9 | 0 | 0 |
| 7 | buff-lang-buffhtml-parser | 2,748 | 4 | 7 | 1 | 2 | 0 | 0 | 0 | 🟢 GREEN | 6 | 1 | 0 | 0 |
| 8 | buff-lang-ffi-guide | 19 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 🟢 GREEN | 0 | 0 | 0 | 0 |
| 9 | buff-eval | 865 | 1 | 5 | 2 | 0 | 0 | 0 | 0 | 🟡 YELLOW | 3 | 0 | 0 | 2 |
| 10 | buff-template | 150 | 2 | 3 | 1 | 1 | 0 | 0 | 0 | 🟢 GREEN | 2 | 0 | 0 | 1 |
| | **TOTAL** | **23,014** | **40** | **223** | **78** | **27** | **1** | **2** | **0** | | **182** | **32** | **3** | **6** |

**Footnotes:**
1. buff-lang-ast `unsafe` textual matches are a doc-comment + Display formatter emitting the literal `"unsafe "` prefix for `extern` decls — NOT Rust unsafe blocks.
2. buff-lang-debug-info `"Box<dyn Any>"` is a string literal describing a panic payload type — NOT a real dyn usage.
3. buff-lang-lexer has 2 real `dyn`: (a) `Option<&(dyn std::error::Error + 'static)>` in std::error::Error trait impl (universal Rust pattern, vanishes under Buff's error model), (b) **`interp_cb: &mut dyn LexCallback` in `pub fn scan_string`** (string_interp.rs:61-67 — this IS in a public API signature and drives the YELLOW verdict; refactorable to a closure).
4. buff-lang-lexer `unsafe` textual matches are keyword tokens (`KwUnsafe`) in the Buff keyword table + Display impl — NOT Rust unsafe blocks.
5. buff-lang-parser has 1 textual `unsafe` in a doc-comment (stmt/stmt_decl.rs:25) describing how the parser handles the Buff `unsafe` keyword token — NOT real unsafe code.

---

## YELLOW crates — detailed evidence & port strategy

### 🟡 buff-lang-debug-info (1,117 LOC)

**Why YELLOW:** Implementation requires OS-level facilities behind a clean pub API.

| Concern | Location | Evidence |
|---|---|---|
| File I/O | `format.rs:99` `write_to_file` | `std::fs::write(path, json)` |
| File I/O | `format.rs:106` `read_from_file` | `std::fs::read_to_string(path)` |
| Global state mutation | `panic_hook.rs:56` `install_panic_hook` | `std::panic::set_hook(...)` (non-idempotent) |
| Env + exe path | `panic_hook.rs:71` `resolve_buff_map_path` | `std::env::var("BUFF_MAP_PATH")` + `std::env::current_exe()` |
| Runtime stack capture | `panic_hook.rs:196` `remap_panic_backtrace` | consumes `std::backtrace::Backtrace` |

**What ports cleanly (11 T1/T2 fns):** `build_source_map` (pure AST walk → T2), `SourceMap::new`/`with_buff_file`/`with_rust_file`/`add_line_mapping`/`add_function`/`lookup_buff`/`lookup_name`/`is_empty`, `serialize_to_string`, `deserialize`.

**Port strategy:** Provide Buff runtime intrinsics for (a) file read/write of sidecar JSON, (b) panic-hook installation (or restructure to offline post-processing via `buff backtrace`), (c) backtrace capture. None of these are `extern`-FFI — they are runtime facilities.

---

### 🟡 buff-lang-lexer (2,911 LOC)

**Why YELLOW:** One pub trait + one pub fn with `&mut dyn LexCallback` parameter.

| Concern | Location | Evidence |
|---|---|---|
| **Pub trait** | `string_interp.rs:38` `pub trait LexCallback` | single-method trait (`lex_range`) |
| **Dyn in pub fn sig** | `string_interp.rs:61-67` `pub fn scan_string(..., interp_cb: &mut dyn LexCallback)` | **DRIVES YELLOW VERDICT** — refactorable to closure |
| std::error::Error dyn | `error.rs:91` `Option<&(dyn std::error::Error + 'static)>` | Rust std lib trait impl (universal — auto-disappears in Buff) |

**What ports cleanly (all 15 pub fns):** `tokenize(source, source_id) -> Result<Vec<Token>, LexerError>` is the main public surface and is a pure byte-scanner over `&str`. The offside-rule indent tracker (indent.rs) is pure compute. All LexerError constructors + Token/TokenKind methods are pure T1.

**Port strategy:** Replace `LexCallback` dyn with a Buff closure `Func` or fn pointer (mechanical refactor — the trait has one method `lex_range`). The std::error::Error impl disappears in Buff (Buff has its own `Result<T, BuffError>` model per the ffi-guide R3). Port effort: low — one dyn callback to refactor.

---

### 🟡 buff-eval (865 LOC)

**Why YELLOW:** Implementation spawns `rustc` subprocess + manages temp files + accumulates session state.

| Concern | Location | Evidence |
|---|---|---|
| Subprocess spawn | `lib.rs:650` `Command::new("rustc")`, `lib.rs:710` `Command::new(&exe_path).output()` | T4 — process spawn + stdout/stderr capture |
| Temp file lifecycle | `lib.rs:627/660/678/685/695/696/713/714/728/729` | `std::fs::write` + `std::fs::remove_file` |
| Mutable session state | `lib.rs:299-304` `top_level_src`, `body_stmts_src` | accumulated across `eval`/`eval_line` calls |
| Env-var config | `lib.rs:193/206/652` | reads `BUFF_EVAL_BACKEND`, `BUFF_EVAL_TARGET`, `BUFF_EVAL_SCCACHE` |
| Static counter | `lib.rs:764` `static STEM_COUNTER: AtomicU64` | process-wide unique temp-stem generation |
| Duplicated pipeline | `lib.rs` (whole) | copy of `buff_lang_cli::pipeline::compile_rust_to_exe` (kept inline to avoid clap/tokio transitive deps) |

**What ports cleanly (3 T1 fns):** `Evaluator::new`, `EvalResult::is_ok`, `Evaluator::type_of` (pure lex+parse+infer — docstring contract guarantees no state mutation).

**Port strategy:** (a) Requires a Buff runtime primitive for process spawn (or restrict to in-process eval via future `buff-lang-runtime` wasm interpreter). (b) Session state ports directly as a Buff struct with two `String` fields. (c) `type_of` ports immediately. Semantic note: in a self-hosted Buff, the eval engine would orchestrate the Buff compiler itself — likely a redesign rather than a literal port.

---

## GREEN crates — quick rationale

| Crate | Why GREEN |
|---|---|
| **buff-lang-ast** | Pure AST + IR graph + lossless tree. 54 pub fns are constructors/accessors/builders/graph-ops. 14 T2 fns return Vec/Iterator of children (bindings, dependencies, topological_order, etc.). No I/O. |
| **buff-lang-ast-rsx** | 418-LOC single file, 14 structs + 10 constructors + 1 predicate (`is_component_tag`). Pure data. |
| **buff-lang-error** | LEAF crate. Stable ErrorCode enum (E10xx–E15xx, stable forever). 44/49 fns are pure accessors/renderers/constructors. 1 T3 (`should_use_color`) is opt-out via existing `render_with_color(use_color: bool)`. 4 T2 (suggest_identifier, ErrorCode::all, render batches). |
| **buff-lang-parser** | Pure recursive-descent + Pratt. 64 pub fns across 9 files (incl. `expr/` + `stmt/` subdirs). 55 T1 + 9 T2 (Vec<Decl>/Vec<Param>/Vec<Attribute> returns). 7,236 LOC — largest target crate (spec said 3,640; understated ~2×). |
| **buff-lang-buffhtml-parser** | Parallel pipeline to main parser. 7 pub fns, clean port. |
| **buff-lang-ffi-guide** | 19 lines of doc-comment; no executable code. Re-author the 6 rules as Buff docs. |
| **buff-template** | 150 LOC, 3 pub fns wrapping pure-Rust `handlebars`. FFI-guide-compliant by design. `from_path` is the standard load-from-disk pattern Buff supports. |

---

## Cross-cutting findings

1. **Zero real `unsafe` blocks** across all 10 crates. Every textual `unsafe` match is a Buff keyword token (`KwUnsafe` in lexer/token.rs), a doc comment (parser stmt_decl.rs), or a Display formatter emitting the literal string `"unsafe "` (ast/decl.rs). Confirms the project's no-unsafe-in-non-test-code rule.
2. **Zero `extern "ABI"` Rust FFI** across all 10 crates. Textual matches describe the Buff-source `extern` syntax being parsed — not Rust FFI in the host crate.
3. **One trait total** (`LexCallback` in buff-lang-lexer) — and its single method is refactorable to a closure. Trait objects are essentially absent from the target set.
4. **No `tokio` / `chrono` / `uuid` / `rand`** in any of the 10 crates' dependencies. (Comments referencing these describe lowerings the *codegen* emits, not deps of these crates.)
5. **`std::error::Error::source` dyn return** is the only "universal Rust" dyn pattern (1 occurrence, in buff-lang-lexer) — disappears under Buff's own error model.
6. **3 YELLOW crates share a common theme:** port blocker is always "Buff needs a runtime primitive for X" (file rw / panic hook / process spawn / backtrace capture / closure-for-dyn-callback) — never a language-level limitation. No crate is fundamentally unportable.
7. **`buff-lang-parser` actual LOC is 7,236**, ~2× the task spec's stated 3,640. The parser grew substantially through v1.x framework waves (matrix literals, generics, property wrappers, extern decls, attributes). All counts in this audit use ACTUAL figures.
8. **File-count discrepancies with the task spec:** buff-lang-error (spec 3 → actual 7), buff-lang-debug-info (spec 2 → actual 4), buff-lang-lexer (spec 5 → actual 6), buff-lang-parser (spec 6 → actual 9), buff-eval (spec 2 → actual 1). All counts in this audit are empirical.
9. **No volatile T3 fields** (timestamp/uuid/random_seed) exist in any of the 10 crates. The 3 T3 fns are env/TTY/backtrace readers, not time/UUID/random consumers — Buff needs no special volatile-field primitive for the port.

---

## Recommended Phase 3 port order

Based on verdict + dependency DAG + complexity (easiest → hardest):

1. **buff-lang-ffi-guide** — port the 6 rules as Buff docs (zero code).
2. **buff-lang-error** — port first; every other crate depends on `Span`/`SourceId`/`Diagnostic`/`ErrorCode`.
3. **buff-lang-ast** — port the data model (depends only on error).
4. **buff-lang-ast-rsx** — port in parallel with ast (sibling, same shape).
5. **buff-lang-lexer** — refactor `LexCallback` to a closure first, then port.
6. **buff-lang-parser** — port once ast+lexer are Buff-native (largest single port: 7,236 LOC, 64 pub fns).
7. **buff-lang-buffhtml-parser** — port in parallel with parser.
8. **buff-template** — port once Buff has `extern` to `handlebars` (or re-implement handlebars in Buff).
9. **buff-lang-debug-info** — port once Buff runtime has panic-hook + backtrace + fs intrinsics.
10. **buff-eval** — port last; requires Buff runtime primitive for `rustc` subprocess spawn (or wait for in-process interpreter).
