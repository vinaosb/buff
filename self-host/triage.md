# P0.3 — Self-Host Triage (Independently Re-Run)

**Task:** P0.3 of the self-host-completion-roadmap.
**Method:** Every `.buff` file under `self-host/` was independently re-checked via
`buff check <file>` inside the `buff-dev:latest` Docker image. Classifications
below are derived from the **ACTUAL error messages** emitted by the current
`buff` binary (release build at HEAD of `main`), NOT from the prior
`bootstrap-report.md` (which is now outdated — see §"Disagreements with
bootstrap-report" below).
**Date:** 2026-07-27.
**Total files:** 56 (codegen 22 · lexer 5 · parser 7 · types 22).

---

## TL;DR

| Metric | Count |
|---|---:|
| Total `.buff` files | **56** |
| **PASS** (`no issues found`) | **12** |
| **FAIL** | **44** |
| └ `bug` (Rust-compiler fixable on `main`) | 23 |
| └ `lang-gap` (needs Buff language extension) | 1 |
| └ `unsupported` (T119 policy / fundamental) | 20 |
| └ `unknown` (needs spike) | 0 (cap was 10) |

**Single biggest blocker:** 20 of 22 `codegen/*.buff` files use
`extern "Rust" { ... }` blocks to call back into the Rust-written compiler.
Buff's T119 spec deliberately restricts `extern` to the `"C"` ABI only. This is
a **policy / spec decision** (loosen T119 OR write C-shims), not a compiler
bug — it cannot be "fixed on `main`" without a governing DR.

**Second-biggest blocker:** 18 files fail at the **type-check stage** with
spurious `undefined variable: <parameter>` errors. Root cause is a single
systematic gap in the type-checker: it cannot resolve **qualified enum variant
access in expression context** (`EnumName.Variant` as a value, e.g.
`if f == PreludeFn.Abs:`). The parser accepts the syntax (and `match`-arm
patterns using the same syntax work), but the inferencer reports the function
parameter as `undefined variable` and cascades `if condition must be Bool,
found Unknown` errors downstream. One fix in
`crates/buff-lang-types/src/infer*.rs` would unblock all 18 files.

---

## Per-File Results

| File | Stage | Classification | Root Cause (first error) | Fix Phase |
|------|-------|----------------|--------------------------|-----------|
| codegen/atomic_analysis.buff | PARSE | lang-gap | `func analyze_func(func: FuncDecl)` — uses keyword `func` as parameter name; Buff has no raw-identifier (`r#ident`) escape | P2.x (raw-id extension) |
| codegen/comptime.buff | PARSE | bug | `match lower_one(offset, value):` — parser rejects `match EXPR:` (colon-newline) form; expects `match EXPR {` brace form | P1.2 |
| codegen/context.buff | PARSE | unsupported | `extern "Rust" { ... }` — T119 spec restricts Buff `extern` to `"C"` ABI only | Decision needed (loosen T119 OR C-shim) |
| codegen/conv_helpers.buff | PARSE | unsupported | `extern "Rust"` block — same as codegen/context.buff | Decision needed |
| codegen/decl_lowering.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/dependency_detection.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/derive_attrs.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/expr_lowering.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/extern_crate_detection.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/extern_crate_detection_extra.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/format.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/gpu_alignment.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/lib.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/lowering_helpers.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/method_call_lowering.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/move_analysis.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/multi_crate.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/passes.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/race_analysis.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/rust_codegen.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/syn_helpers.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| codegen/type_lowering.buff | PARSE | unsupported | `extern "Rust"` block — same | Decision needed |
| lexer/error.buff | — | **PASS** | no issues found | — |
| lexer/indent.buff | LEX | bug | `:56:1 inconsistent indentation level` — offside-rule indent tracker rejects the file's continuation-line indents inside multi-line `func` signatures | P1.1 |
| lexer/lexer.buff | LEX | bug | `:154:1 inconsistent indentation level` — same root cause as lexer/indent.buff | P1.1 |
| lexer/string_interp.buff | LEX | bug | `:62:1 inconsistent indentation level` — same root cause | P1.1 |
| lexer/token.buff | TYPE | bug | `:237:11 undefined variable: kind` — type-checker can't resolve `kind == TokenKind.Sum` etc. (qualified enum variant access in expression context); parameter `kind` spuriously flagged | P1.x (type-inference) |
| parser/expr.buff | TYPE | bug | `:242:5 if condition must be Bool, found Unknown` — cascade from qualified-enum-variant type-inference gap (calls into `TokenKind.X` helpers from stream.buff that fail to resolve) | P1.x (type-inference) |
| parser/expr_pattern.buff | TYPE | bug | `:99:5 if condition must be Bool, found Unknown` — same type-inference gap | P1.x (type-inference) |
| parser/expr_postfix.buff | TYPE | bug | `:189:18 undefined variable: e` — same type-inference gap (match on enum-typed param) | P1.x (type-inference) |
| parser/parser.buff | TYPE | bug | `:56:5 if condition must be Bool, found Unknown` — same type-inference gap | P1.x (type-inference) |
| parser/stmt.buff | TYPE | bug | `:116:5 if condition must be Bool, found Unknown` — same type-inference gap | P1.x (type-inference) |
| parser/stmt_decl.buff | TYPE | bug | `:40:18 undefined variable: ty` — same type-inference gap (match on `TypeRef`-typed param with unqualified variant arms) | P1.x (type-inference) |
| parser/stream.buff | TYPE | bug | `:44:12 undefined variable: e` — same type-inference gap (Edition-typed param compared to `Edition.Scientific`) | P1.x (type-inference) |
| types/async_analysis.buff | TYPE | bug | `:73:5 if condition must be Bool, found Unknown` — same type-inference gap | P1.x (type-inference) |
| types/comptime.buff | — | **PASS** | no issues found | — |
| types/cross_file.buff | — | **PASS** | no issues found | — |
| types/env.buff | — | **PASS** | no issues found | — |
| types/exhaustiveness.buff | — | **PASS** | no issues found | — |
| types/infer.buff | — | **PASS** | no issues found | — |
| types/lib.buff | — | **PASS** | no issues found | — |
| types/modules.buff | — | **PASS** | no issues found | — |
| types/multi_dispatch.buff | TYPE | bug | `:224:18 undefined variable: t` — same type-inference gap | P1.x (type-inference) |
| types/ownership.buff | — | **PASS** | no issues found | — |
| types/prelude.buff | TYPE | bug | `:150:11 undefined variable: f` — same type-inference gap (`if f == PreludeFn.Abs:` etc., 30+ cascading errors) | P1.x (type-inference) |
| types/prelude_assoc_const_impl.buff | TYPE | bug | `:86:11 undefined variable: c` — same type-inference gap | P1.x (type-inference) |
| types/prelude_assoc_fn_impl.buff | TYPE | bug | `:328:8 undefined variable: c` — same type-inference gap (≈100+ cascading errors on `c == PreludeAssocFn.X`) | P1.x (type-inference) |
| types/prelude_instance_fn_impl.buff | TYPE | bug | `:119:8 undefined variable: c` — same type-inference gap (≈100+ cascading errors) | P1.x (type-inference) |
| types/prelude_return_types.buff | TYPE | bug | `:45:8 undefined variable: t` — same type-inference gap | P1.x (type-inference) |
| types/prelude_type_metadata.buff | TYPE | bug | `:155:11 undefined variable: t` — same type-inference gap (80+ cascading errors) | P1.x (type-inference) |
| types/prelude_types.buff | — | **PASS** | no issues found | — |
| types/project.buff | — | **PASS** | no issues found | — |
| types/promote.buff | TYPE | bug | `:50:8 undefined variable: lhs` — same type-inference gap (`lhs == type_unknown()` etc.) | P1.x (type-inference) |
| types/range_analysis.buff | LEX | bug | `:119:16 invalid numeric literal` — lexer can't parse `-9223372036854775808` (i64::MIN); parses as `unary -` + `9223372036854775808` which overflows i64 positive range | P1.1 |
| types/recursion.buff | — | **PASS** | no issues found | — |
| types/ty.buff | TYPE | bug | `:293:11 undefined variable: w` — same type-inference gap (also: line 690 uses `not X` prefix operator which Buff does not have — would need rewrite to `!X` or `not` keyword extension; first error is the type-inference one) | P1.x (type-inference) |

---

## Summary

```
bugs:       23
lang-gaps:   1
unsupported: 20
unknown:     0
pass:        12
─────────────────
total:      56
```

`unknown` is well under the cap of 10 — every failure was assignable to a
concrete root cause.

### Sub-tally by stage (FAIL files only)

| Stage | Count | Notes |
|---|---:|---|
| LEX | 4 | 3× inconsistent indentation (P1.1) + 1× invalid numeric literal i64::MIN (P1.1) |
| PARSE | 22 | 20× `extern "Rust"` ABI policy block + 1× `func` keyword as param name + 1× `match EXPR:` colon-form |
| TYPE | 18 | all share a single root cause: qualified enum variant access (`EnumName.Variant`) not resolved in expression context by the inferencer |
| CODEGEN | 0 | (none — `buff check` runs lex+parse+typecheck, no codegen) |

### Sub-tally by subdir

| Subdir | PASS | FAIL | Failure dominant cause |
|---|---:|---:|---|
| codegen/ (22) | 0 | 22 | 20× unsupported `extern "Rust"`, 1× lang-gap `func`-as-param, 1× parse bug |
| lexer/ (5) | 1 | 4 | 3× indent-LEX bug, 1× type-inference bug |
| parser/ (7) | 0 | 7 | 7× type-inference bug (qualified enum access gap) |
| types/ (22) | 11 | 11 | 10× type-inference bug, 1× numeric-literal LEX bug |

---

## Disagreements with `bootstrap-report.md` (why the re-run was necessary)

The existing `self-host/bootstrap-report.md` (commit `3126df6`, branch
`v1x-frameworks`, 2026-07-24) classified the 49 failures into 7 categories:
4 LEX, 34 PARSE-A (struct/enum indent), 5 PARSE-B (qualified enum patterns),
1 PARSE-C (ABI), 5 CODEGEN. **That classification is now stale** for two
reasons:

1. **Different command.** The bootstrap-report ran `lex + parse + codegen`
   (Stage 1A transpile). This triage runs `buff check` which adds the
   **type-checker pass** (T55). Type-check failures now mask the codegen-stage
   failures the bootstrap-report observed in `parser/expr.buff`,
   `parser/stmt_decl.buff`, etc.

2. **Compiler moved on.** Many files the bootstrap-report called PASS
   (parser/expr_pattern, parser/expr_postfix, parser/parser, parser/stmt,
   parser/stream — five of its seven "PASS") now FAIL because the type
   checker got stricter. Conversely `lexer/error.buff` (bootstrap-report's
   PARSE-A failure) now PASSES — the struct-after-enum parse bug it
   documented has been fixed on `main`.

**Net effect:** bootstrap-report's "7 PASS / 49 FAIL" is now
**"12 PASS / 44 FAIL"**. The single biggest category has flipped from
PARSE-A (34 indent-after-brace struct decls) to TYPE (18 qualified-enum
type-inference failures) — the struct-decl parser bug appears to be fixed,
but a new type-inference gap has taken its place as the dominant blocker.

The roadmap's Phase 1 sub-tasks (P1.1 LEX / P1.2 PARSE-A / P1.3 PARSE-B /
P1.4 PARSE-C / P1.5 CODEGEN) were sized off the bootstrap-report's stale
categories. **The actual work-load is different**:
- P1.1 (LEX) is roughly right: 4 files (3 indent + 1 numeric).
- P1.2 / P1.3 / P1.4 (PARSE-A/B/C, 40 files) have largely evaporated:
  only **2** parse failures remain (`atomic_analysis` lang-gap,
  `comptime` match-expr bug).
- P1.5 (CODEGEN) is also gone — `buff check` doesn't reach codegen.
- **A new dominant category has emerged** that the roadmap doesn't have a
  phase for: 18 type-inference failures from a single root cause (qualified
  enum variant access in expression context). Suggest re-mapping Phase 1
  sub-tasks before execution.

The `unsupported` category (20 `extern "Rust"` files) was previously
counted as just 1 file (`codegen/format.buff`, bootstrap-report §4.4).
It is in fact the **entire `codegen/` port directory minus two**. This is
not fixable on `main` without a governing decision record — either loosen
the T119 spec to admit `"Rust"` ABI for the bootstrap use case, or commit
to writing `#[no_mangle] extern "C"` shims on the Rust side for every
symbol the `.buff` ports need.

---

## Recommendation for roadmap re-planning

Before kicking off Phase 1, the roadmap should be amended to:

1. **Add a Phase 1 sub-task for type-inference fixes** (18 files, single root
   cause). Likely 1-2 day fix in `crates/buff-lang-types/src/infer*.rs` to
   teach the inferencer that `EnumName.Variant` in expression context
   resolves to a value of `EnumName` type. This unblocks every `parser/*`
   file and most `types/prelude*` files in one shot.

2. **Re-scope P1.2/P1.3/P1.4 down** to just the 2 actual parse failures
   (codegen/atomic_analysis + codegen/comptime). The original 40-file
   PARSE-A/B/C scope is gone.

3. **Open a Decision Record for the `extern "Rust"` policy** before any
   `codegen/*` port work resumes. The 20 affected files are blocked on
   policy, not on compiler work. (This is the same shape of decision as
   DR-014 / T119.)

4. **Add a Phase 1 sub-task for i64::MIN literal handling** (1 file,
   `types/range_analysis.buff`). Trivial lexer fix.

5. **Defer the `func`-as-param-name lang-gap** (`codegen/atomic_analysis.buff`)
   until raw identifiers are added to Buff (post-v2 per DR-014). One file
   affected — non-blocking.
