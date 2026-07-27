# Spike: LexCallback Portability (S4)

**Date:** 2026-07-27
**Scope:** Can the `LexCallback` trait be ported to Buff (which may not support `dyn Trait` / trait objects)?

---

## 1. Trait Definition

**File:** `crates/buff-lang-lexer/src/string_interp.rs:38-47`

```rust
pub trait LexCallback {
    fn lex_range(
        &mut self,
        source: &str,
        range_start: usize,
        range_end: usize,
        _source_id: SourceId,
        out: &mut Vec<Token>,
    ) -> Result<(), LexerError>;
}
```

- **Single method** (`lex_range`)
- Takes `&mut self` (mutable receiver)
- Parameters: source string, byte range (start/end), source ID, mutable output token vector
- Returns `Result<(), LexerError>`

---

## 2. All Usage Sites

### Production code

| File | Line | Usage | Pattern |
|---|---|---|---|
| `string_interp.rs` | 66 | `interp_cb: &mut dyn LexCallback` | Function parameter — `&mut dyn` trait object |
| `string_interp.rs` | 111 | `interp_cb.lex_range(source, after_brace, expr_end, source_id, out)?;` | Method call on trait object |
| `lexer.rs` | 23 | `use crate::string_interp::{scan_string, LexCallback};` | Import |
| `lexer.rs` | 220-221 | `let mut interp_cb = InterpLexer { source, source_id };`<br>`pos = scan_string(source, quote_start, source_id, out, &mut interp_cb)?;` | Instantiation + call |

### Test code

| File | Line | Usage | Pattern |
|---|---|---|---|
| `string_interp.rs` | 283 | `impl LexCallback for RecordInterp { ... }` | Test impl (records inner text) |
| `string_interp.rs` | 303 | `let _ = scan_string(src, 0, SourceId(0), &mut out, &mut cb);` | Test call |
| `string_interp.rs` | 339 | `let result = scan_string("\"abc", 0, SourceId(0), &mut out, &mut cb);` | Test call |
| `string_interp.rs` | 351 | `impl LexCallback for RecordInterpWithSpec { ... }` | Test impl (records text + spec) |
| `string_interp.rs` | 445 | `let _ = scan_string(src, 0, SourceId(0), &mut out, &mut cb);` | Test call |
| `lexer.rs` | 1110 | `impl<'a> LexCallback for InterpLexer<'a> { ... }` | Production impl (delegates to `lex_range`) |

**Total: 7 occurrences** across 2 files. **Zero** in the parser crate.

---

## 3. Usage Pattern Analysis

### How is it used?

- **`&mut dyn LexCallback`** — passed as a function parameter to `scan_string()` (line 66)
- **NOT stored** in any struct field, collection, or `Box<dyn ...>`
- **NOT generic** — no `<T: LexCallback>` bounds anywhere
- **NOT returned** from any function
- **Single call site** in production: `lexer.rs:221` inside the `b'"'` match arm of the main lexer loop

### How many implementations?

**3 total:**

1. **`InterpLexer<'a>`** (lexer.rs:1105-1130) — **production.** Wraps `source: &'a str` and `source_id: SourceId`. Its `lex_range` calls the internal `lex_range()` function (the main lexer's inner loop) with `track_indent = false`.

2. **`RecordInterp`** (string_interp.rs:279-296) — **test only.** Captures the inner text of each interpolation into `Vec<String>`.

3. **`RecordInterpWithSpec`** (string_interp.rs:347-437) — **test only.** Captures `(expr_text, Option<spec_text>)` pairs.

### Data flow

```
lexer.rs:220-221
  let mut interp_cb = InterpLexer { source, source_id };
  pos = scan_string(source, quote_start, source_id, out, &mut interp_cb)?;
       │
       ▼
string_interp.rs:66
  scan_string(..., interp_cb: &mut dyn LexCallback)
       │
       ▼
string_interp.rs:111
  interp_cb.lex_range(source, after_brace, expr_end, source_id, out)?;
       │
       ▼
lexer.rs:1111-1129  (InterpLexer::lex_range)
  lex_range(self.source, range_start, range_end, self.source_id, out, &mut dummy, false)
```

The callback is invoked **once per interpolation expression** inside a string literal. A string like `"a {b} c {d} e"` would call it twice.

---

## 4. Portability Assessment

### Verdict: **PORTABLE (easy)**

### Why it's easy

| Factor | Assessment |
|---|---|
| **Number of methods** | 1 — trivially maps to a function type |
| **Storage** | Never stored in structs/collections — only passed as parameter |
| **Dispatch kind** | `&mut dyn` (runtime, not generic) — simplest form |
| **Implementations** | 3 (1 production, 2 test-only) |
| **Parser dependency** | None — `LexCallback` is lexer-internal |
| **State captured** | `InterpLexer` captures `source: &str` + `source_id` — both are available at the call site |

### Recommended approach for .buff port

**Option A — Function type (simplest):**
Replace the trait with a function type signature. Since the trait is never stored, only called once per string literal, a closure or function reference suffices:

```
// Buff equivalent — function type alias
type LexCallback = func(
    source: &str,
    range_start: Int,
    range_end: Int,
    source_id: SourceId,
    out: &mut Vec<Token>,
) -> Result<(), LexerError>
```

**Option B — Multiple dispatch (Buff v1.19 feature):**
If Buff supports multiple dispatch on a single-method interface, define:

```
interface LexCallback:
    func lex_range(
        &mut self,
        source: &str,
        range_start: Int,
        range_end: Int,
        source_id: SourceId,
        out: &mut Vec<Token>,
    ) -> Result<(), LexerError>
```

Then implement it for `InterpLexer`, `RecordInterp`, `RecordInterpWithSpec`.

**Option C — Enum dispatch (most explicit):**
Since there are only 3 implementations, an enum + match is viable:

```
enum LexCallback:
    InterpLexer(InterpLexer)
    RecordInterp(RecordInterp)
    RecordInterpWithSpec(RecordInterpWithSpec)

func lex_range(cb: &mut LexCallback, ...) -> Result<(), LexerError>:
    match cb:
        LexCallback.InterpLexer(inner) => inner.lex_range(...)
        LexCallback.RecordInterp(inner) => inner.lex_range(...)
        LexCallback.RecordInterpWithSpec(inner) => inner.lex_range(...)
```

**Recommendation:** Option A (function type) is the cleanest. The trait exists solely to pass a callable into `scan_string` — a function pointer does the same job with zero trait machinery. The `InterpLexer` struct exists only to bundle `source` + `source_id`, which are both available at the call site in `lexer.rs:220-221` and could be passed directly.

### What would make it BLOCKING

- If `LexCallback` were stored in a `Vec<Box<dyn LexCallback>>` or a struct field — it is not.
- If it had 5+ methods with complex signatures — it has 1.
- If it were used across crate boundaries in a generic context — it is not (only `string_interp.rs` and `lexer.rs`).

None of these apply.

---

## 5. Summary

| Property | Value |
|---|---|
| **Verdict** | PORTABLE (easy) |
| **Effort** | ~30 min to refactor |
| **Risk** | None — trait is lexer-internal, single-method, never stored |
| **Recommended** | Replace with function type (Option A) |
| **Buff feature needed** | Function types / closures (available since v1.0) |
