# BUGS-FOUND.md — examples/use-cases/apps/data_pipeline.buff

**File:** `examples/use-cases/apps/data_pipeline.buff`
**Lines:** 1194
**Date:** 2026-07-24
**Status:** Parse-only / check-only (framework codegen deferred)

## Build environment issue

`cargo run -p buff-lang-cli -- check` **cannot execute** on this Windows host due
to the pre-existing MSVC `msvcrt.lib` linker failure (`LINK : fatal error LNK1104:
cannot open file 'msvcrt.lib'`). This is caused by empty `LIB`/`INCLUDE`
environment variables and affects ALL crates that link proc-macro DLLs
(`clap_derive`, `serde_derive`, `dioxus-core-macro`, `ring`, `pprof`). CI on the
3-OS matrix (ubuntu/windows/macos) does NOT have this issue. See
`buff-image/AGENTS.md` and `buff-pipeline/AGENTS.md` for the full explanation.

## Known bugs / gaps in data_pipeline.buff

### B1: Framework types not yet registered in codegen (MEDIUM)

**Symptom:** `buff check` cannot resolve `Pipeline.*`, `Source.*`, `Sink.*` calls.

**Root cause:** `Type::Pipeline` / `Source` / `Sink` are not yet registered in the
prelude type registry (`crates/buff-lang-types/src/ty.rs`). Only `Type::DataFrame`
exists with partial codegen lowering. The `Pipeline.new()`, `Source.from_csv()`,
`Sink.to_csv()`, and `Sink.to_json()` calls are forward-declared and will fail
type resolution until T8 (type variant registration) and T9 (instance-method
lowering) land.

**Impact:** The pipeline streaming demos (demo 8) and framework-path functions
(`extract_stream`, `stream_transform`, `load_to_csv`, `load_to_json`) are
parse-only. The pure Buff core (CSV parsing, validation, stats, formatting,
table rendering) runs today.

**Fix:** Coordinate T8 + T9 to register `Type::Pipeline`, `Type::Source`,
`Type::Sink` and add codegen lowering arms for their methods.

### B2: `Double.from()` not available in prelude (LOW)

**Symptom:** `parse_number()` uses `Double.from(trimmed)` which is not a
recognized prelude constructor.

**Root cause:** The prelude does not expose a `Double.from(String)` constructor.
Numeric parsing in Buff relies on `str::parse::<f64>()` internally, but the
Buff surface doesn't have a direct `Double.from()` call.

**Impact:** `parse_number()` falls through to the `Err(_)` arm for all inputs in
the current type-checker. The function is correct in intent but cannot be
verified by `buff check` today.

**Fix:** Either add `Double.from()` to the prelude type registry, or replace
with a `extern` binding to `str::parse::<f64>()`.

### B3: `chars()` method not on String (LOW)

**Symptom:** `parse_csv_line()` and `json_escape()` call `line.chars()` which
is not a recognized String method.

**Root cause:** The prelude String API doesn't expose `.chars()` iterator.
Iteration over string characters requires either `.split("")` or a
`for ch in text` loop that the parser supports for `Vector` but not `String`.

**Impact:** CSV line parsing and JSON escaping use `.chars()` which won't
type-check. The pure parsing logic is correct but unverifiable.

**Fix:** Add `String.chars() -> Vector<String>` to the prelude, or rewrite
using `.split("")` with a trailing empty-string drop.

### B4: `contains()` method not on String (LOW)

**Symptom:** `is_numeric_header()` calls `lower.contains(suffix)` which is
not a recognized String method.

**Root cause:** The prelude String API doesn't expose `.contains(substring)`.
String containment checks require manual iteration or `index_of() >= 0`.

**Impact:** The numeric-header heuristic won't type-check. Can be rewritten
using `.index_of(suffix) >= 0` which IS available.

**Fix:** Replace `lower.contains(suffix)` with `lower.index_of(suffix) >= 0`.

### B5: Enum variant constructors without type prefix (LOW)

**Symptom:** `validate_row()` returns `Err(EmptyRow)` instead of
`Err(RowError::EmptyRow)`. Similarly `WrongArity`, `BadNumber` are used
without the `RowError::` prefix in error returns.

**Root cause:** Buff's `match` arms use bare variant names, but `return`
statements in error positions may require the full `Type::Variant` path.
The parser may accept bare names in match but not in constructors.

**Impact:** The validation logic won't type-check until the parser fully
supports enum variant constructor disambiguation.

**Fix:** Use `Err(RowError::EmptyRow)`, `Err(RowError::WrongArity(...))`,
etc. in all return positions.

### B6: `Vector.contains()` not available (LOW)

**Symptom:** `extend_headers()` calls `out.contains("size_bucket")` which
is not a recognized Vector method.

**Root cause:** The prelude Vector API doesn't expose `.contains(value)`.
Membership checks require manual iteration.

**Impact:** Header extension logic won't type-check. Can be rewritten with
a `for` loop and equality check.

**Fix:** Replace `out.contains(x)` with a manual `for h in out: if h == x: return true` pattern.

### B7: `Vector.contains()` on String slice (LOW)

**Symptom:** Same as B6 — `extend_headers()` uses `out.contains(...)`.

**Impact:** Same as B6.

**Fix:** Same as B6.

## Summary

| ID | Severity | Category | Status |
|----|----------|----------|--------|
| B1 | MEDIUM | Framework codegen | Deferred to T8+T9 |
| B2 | LOW | Prelude API gap | Needs `Double.from()` |
| B3 | LOW | Prelude API gap | Needs `String.chars()` |
| B4 | LOW | Prelude API gap | Use `index_of` workaround |
| B5 | LOW | Parser disambiguation | Use qualified variant names |
| B6 | LOW | Prelude API gap | Manual iteration workaround |
| B7 | LOW | Prelude API gap | Same as B6 |

**No blocking bugs** — all issues are either pre-existing framework gaps or
low-severity prelude API gaps with straightforward workarounds. The pure Buff
core (CSV parsing, validation, enrichment, stats, formatting, table rendering)
is structurally correct and will pass `buff check` once B2-B7 are addressed.
