# Porting Conventions — Rust Crates to Buff

**Decision Record:** P0.0 (self-host-completion-roadmap, Metis PP-2)
**Status:** ACTIVE — REQUIRED READING for every port agent in Phase 3 / 4
**Scope:** All 10 target crates in DR-014 §🟩 (`buff-lang-{ast, ast-rsx, error, debug-info, lexer, parser, buffhtml-parser, ffi-guide}`, `buff-eval`, `buff-template`)
**Governing authority:**
- `self-host-completion-roadmap.md` §Equivalence Contract v2 (lines 92-144) — defines how ports are compared
- `buff-conventions.md` — 19 authoritative Buff-language coding conventions (THIS IS THE SOURCE OF TRUTH for Buff syntax)
- `AGENTS.md` (root) — project anti-patterns + unique styles
- DR-014 `selfhost-feasibility.md` — per-crate portability verdicts

> Every port agent MUST read this document end-to-end BEFORE writing any `.buff` file.
> Faithful 1:1 translation is the rule; deviations require a documented decision in the file's header comment block.

---

## 0. Guiding Principle

A port is **faithful**, not creative. The Rust original is the spec; the Buff port is a re-expression of the same semantics in Buff syntax. Concretely:

1. **Structure mirrors the original.** One Rust source file → one `.buff` file. A Rust `impl Block` → a family of free functions whose names share the lowercased type prefix. A Rust `pub fn foo()` → a Buff `export func foo()`. Reordering, merging, or splitting logic across files is forbidden unless the file's header comment block explains why.
2. **Comments are preserved verbatim.** Every Rust comment (doc `///`, line `//`, inline `/* */` converted to `//`) appears in the port with the same intent. Adjusted only when the surrounding syntax change makes the original nonsensical (e.g. "the `&` here borrows" → deleted, since Buff has no `&`).
3. **The Buff original stays untouched.** Per roadmap §Must NOT Have: `git diff main..self-host/v1 -- crates/buff-lang-{ast,...}/src/ | grep "^+" | wc -l` MUST equal 0. If the Rust original has a bug, fix it on `main` separately; do NOT silently fix it inside the port.
4. **Behavioral parity, not byte-identity.** Per Equivalence Contract v2: spans normalize to `{token_index, offset_within_token}`; output is compared via the 4-tier table (T1/T2/T3/T4). Source-text byte-equality is impossible (`.buff` ≠ `.rs`) and not the goal.

---

## 1. File Header Format

**Rule.** Every ported `.buff` file begins with a structured comment block. The block (a) names the original Rust file being ported, (b) records the T-number of the port task, (c) summarizes what the file does, and (d) documents every translation decision where Buff grammar forced a deviation from the Rust original.

The existing corpus uses TWO header shapes (see `self-host/lexer/token.buff` and `self-host/parser/stream.buff`). Use whichever fits; the **content requirements** are mandatory, the exact line wording is not.

**Rust before** (no header convention — Rust files start with `//!` crate-doc or jump straight into `use`):

```rust
// crates/buff-lang-lexer/src/token.rs
use buff_lang_error::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Token { /* ... */ }
```

**Buff after** (mandatory header block):

```buff
// Buff self-host port of `crates/buff-lang-lexer/src/token.rs` (T15).
//
// Defines [`TokenKind`] (every token variant the scanner can emit), the spanned
// [`Token`] struct, keyword lookup, and rendering.
//
// Faithful 1:1 translation of the Rust reference. Where Rust uses `&str` /
// `&[u8]` / `fmt::Display`, Buff uses owned `String` / `Vector<Int>` byte
// arrays / `to_string` functions (Buff hides references and has no
// `fmt::Display` trait — the closest idiom is a free function returning
// `String`).
//
// Numeric widths map to the natural Buff primitives:
//   i64 / usize  -> Int
//   f32          -> Float
//   f64          -> Double
//   u8           -> Byte
//   char         -> Char
```

**Required elements:**

| Element | Purpose |
|---|---|
| Line 1: `Buff self-host port of \`crates/<crate>/src/<file>.rs\` (T<N>)` | Identifies the original file unambiguously + the tracking T-number |
| One-sentence summary | What this file defines / does |
| "Faithful 1:1 translation" sentence | Anchor the parity intent |
| Translation-decision bullets | EVERY deviation from Rust grammar (references removed, `while`→`for`, `()` unit replaced, or-patterns expanded, etc.) listed with the reason |
| Type-mapping table (for files with numeric primitives) | Which Rust int/float widths map to which Buff primitive |

**Notes:**
- Do NOT invent a header for files that have no Rust original (impossible — every port file has one).
- Header comments use `//` line comments, NOT `///` doc comments. Buff doc-comments (`///`) are for public API documentation of the symbol that follows; header context is file-level meta-information.
- The header is the ONLY place deviations are documented. Inline `// NOTE:` comments at the affected line are encouraged for local quirks, but the comprehensive list lives in the header.

---

## 2. Naming Conventions

**Rule.** Source: `buff-conventions.md` §1 (authoritative). Names are preserved verbatim from Rust wherever the Rust and Buff rules agree (which is almost everywhere — Buff deliberately mirrors Rust naming idioms).

| Element | Rust convention | Buff convention | Action on port |
|---|---|---|---|
| Functions / methods | `snake_case` | `snake_case` | Preserve |
| Local variables / fields | `snake_case` | `snake_case` | Preserve |
| Types (struct / enum) | `PascalCase` | `PascalCase` | Preserve |
| Enum variants | `PascalCase` | `PascalCase` | Preserve |
| Constants / statics | `UPPER_SNAKE` | `UPPER_SNAKE` | Preserve |
| Modules / file names | `snake_case` | `snake_case` | Preserve |
| Traits | `PascalCase` | `PascalCase` | Preserve |
| Generic params | `PascalCase` (single letter ok) | `PascalCase` (single letter ok) | Preserve |

**The two Buff-specific naming rules that DO change a port:**

### 2a. No `_async` suffix on async functions (Buff rule §6)

**Rust before:**
```rust
async fn fetch_data_async() -> Vec<u8> { /* ... */ }
```

**Buff after:**
```buff
async func fetch_data() -> Vector<Byte>:
    // ...
```

**Notes:** Buff's async model is in the type system (the function is declared `async`), not in the name. Rename `foo_async` → `foo` on port. This rule applies even when there's a sync sibling (`foo` + `foo_async` in Rust → `foo_sync` + `foo` in Buff, OR restructure so they're not siblings).

### 2b. Constructors are `Type.new()` / `Type.from()` only (Buff rule §7)

**Rust before:**
```rust
impl Span {
    pub fn new(start: usize, end: usize) -> Self { /* ... */ }
    pub fn from_bytes(b: &[u8]) -> Self { /* ... */ }
    pub fn create(...) -> Self { /* ... */ }   // unusual but exists
    pub fn parse(s: &str) -> Self { /* ... */ }
}
```

**Buff after:**
```buff
func span_new(start: Int, end: Int) -> Span:
    return Span.new(start: start, end: end)

func span_from_bytes(b: Vector<Byte>) -> Span:
    return Span.from(bytes: b)
```

**Notes:**
- Buff bans `new Person()`, `Person.create()`, `Person.build()`, `Person.parse()` (per AGENTS.md anti-patterns + buff-conventions.md §7).
- The Rust `impl Span { fn new(...) }` becomes a free function `span_new(...)` whose body calls `Span.new(...)`. The `Span.new` constructor is generated automatically by Buff from the struct definition.
- For simple struct literals, Buff allows `Span { start: 0, end: 1 }` (struct-literal syntax). For ports, prefer `Type.new(...)` uniformly — it works for both simple and complex cases, and the existing logic-port corpus (`token.buff`, `stream.buff`) standardizes on it.
- `Type.from(...)` is reserved for conversion constructors (bytes → value, string → number, etc).

---

## 3. Comment Policy

**Rule.** Comments are part of the source's contract with its future readers. They are preserved across the port with the following conversions:

| Rust form | Buff form | Notes |
|---|---|---|
| `///` doc comment | `///` doc comment | Same syntax, same intent. Buff renders these as API docs. |
| `//!` crate / module doc | `//` file-level header block | Buff has no crate doc; module doc becomes the file-header comment block (see §1). |
| `//` line comment | `//` line comment | Verbatim. |
| `/* ... */` block comment | `// ...` (multiple line comments) | Buff has no block comments. Split across N `//` lines. |
| `/* ... */ ... */` nested | `// ...` lines | Same — split into line comments. |
| `// TODO:` / `// FIXME:` / `// NOTE:` | `// TODO:` / `// FIXME:` / `// NOTE:` | Preserve the tag verbatim. Include the original Rust file:line reference if it had one. |

**Rust before:**
```rust
/// A source span — byte offsets into the original `.buff` file.
///
/// Carried verbatim by every token and AST node so diagnostics can point
/// back at the source. `start` is inclusive, `end` is exclusive.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    // TODO(T15): add source_id once buff-lang-error port lands
}
```

**Buff after:**
```buff
// A source span — byte offsets into the original `.buff` file.
//
// Carried verbatim by every token and AST node so diagnostics can point
// back at the source. `start` is inclusive, `end` is exclusive.
struct Span:
    start: Int
    end: Int
    // TODO(T15): add source_id once buff-lang-error port lands
```

**Notes:**
- Doc-comments on `pub` items in Rust become `///` doc-comments on `export` items in Buff.
- Doc-comments on private items: convention is to keep them as `//` (not `///`) since Buff `///` is for the public API surface. Use judgment; if the doc would appear in a hypothetical reference, use `///`.
- Comments that explain borrow-checker reasoning (`// borrows self so the lifetime...`) are DELETED — Buff has no visible borrows, so the comment would mislead.
- Comments that explain ownership / mutability intent (`// this consumes the vector`) are KEPT — Buff's move-by-default semantics still has the same observable effect.

---

## 4. Error Handling

**Rule.** Buff's error model mirrors Rust's `Result<T, E>` / `Option<T>` exactly. The `?` operator IS supported in Buff (see `examples/error_handling.buff` line 31 — confirmed end-to-end runnable).

### 4a. The `?` propagation operator

**Rust before:**
```rust
pub fn add_one(n: i64) -> Result<i64, Error> {
    let h = half(n)?;
    Ok(h + 1)
}
```

**Buff after:**
```buff
func add_one(n: Int) -> Result<Int, Error>:
    let h = half(n)?
    return Ok(h + 1)
```

**Notes:**
- Identical surface syntax. `?` unwraps `Ok` on success, propagates `Err` on failure.
- Use `?` everywhere the Rust original uses it. Do NOT expand into explicit `match` "for clarity" — the port is faithful, and `?` is the Rust idiom.

### 4b. Constructing errors — `Error("msg")` builtin

**Rust before:**
```rust
return Err(Error::new("input too small"));
// or:
return Err(MyError::NotFound);
```

**Buff after:**
```buff
return Error("input too small")
```

**Notes:**
- Buff's `Error("msg")` literal lowers to `Err(Error::new("msg"))` at codegen time.
- The builtin `Error` struct is implicit (no import).
- Custom error ENUMS (e.g. `enum MyErr { NotFound }`) are codegen-verified but have a known gap: variants emit unqualified (`NotFound` rather than `MyErr::NotFound`) — see `examples/error_handling.buff` header. **For ports, use the builtin `Error` type with descriptive messages until the enum-variant qualification codegen gap is fixed.** Document the substitution in the file header.

### 4c. thiserror derives stay in Rust

**Rule.** Rust crates that use `#[derive(thiserror::Error)]` cannot port the derive itself — Buff has no `thiserror`. The error ENUM (the data structure) ports; the thiserror IMPL (the `Display` / `std::error::Error` boilerplate) does NOT port.

**Rust before:**
```rust
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LexerError {
    #[error("unexpected character: {0}")]
    UnexpectedChar(char),
    #[error("unterminated string literal")]
    UnterminatedString,
}
```

**Buff after:**
```buff
// Ported as a plain enum — thiserror derive stays in Rust.
// Display / std::error::Error impls are emitted by the Rust host code, not
// by this port. Error CODES (E10xx) are attached at the call site (see
// `buff-lang-error/src/code.rs`), not on the variants here.
enum LexerError {
    UnexpectedChar(Char),
    UnterminatedString,
}

func lexer_error_display(e: LexerError) -> String:
    // Manual translation of the thiserror #[error("...")] bodies.
    match e {
        LexerError.UnexpectedChar(c) => return "unexpected character: " + c.to_string(),
        LexerError.UnterminatedString => return "unterminated string literal",
    }
```

**Notes:**
- The `Display` impl becomes a free function `<EnumName>_display(...) -> String`.
- This is a translation decision that MUST be noted in the file header.

### 4d. Error codes (E10xx / E11xx / E12xx / E13xx) are STABLE FOREVER

**Rule.** Per `buff-conventions.md` §19 and root `AGENTS.md` anti-patterns: ErrorCodes are never renumbered, never reused, never silently removed, never back-filled. The numeric value is public API.

- Ports that emit diagnostics MUST use the same ErrorCode the Rust original emits.
- New errors discovered during a port get the NEXT FREE code in their phase block (do NOT back-fill gaps).
- The `ErrorCode` enum in `crates/buff-lang-error/src/code.rs` is the single source of truth.
- A diagnostic without a code is still valid (`Diagnostic::code: Option<ErrorCode>`); not every internal variant needs one.

**Rust before:**
```rust
return Err(LexerError::UnexpectedChar(c)).with_code(E1001);
```

**Buff after:**
```buff
return Error("unexpected character: " + c.to_string())  // E1001 attached by host
```

**Notes:** The port may not need to attach codes itself — the Rust host's `LexerError` → `Diagnostic` translation layer handles that. The port's job is to surface the same semantic error at the same call site.

---

## 5. Test File Layout

**Rule.** Equivalence is verified per the Equivalence Contract v2 tier table (T1/T2/T3/T4). Tests live alongside the port (NOT in a separate `tests/` tree for the corpus as a whole — each crate's port has its own).

### 5a. File location

```
self-host/<crate>/<file>.buff                    # The port itself
self-host/<crate>/equivalence_<fn_name>.buff     # Per-fn equivalence test
```

Plus the existing data-model corpus (harness-tested):
```
crates/<crate>/selfhost/<file>.buff              # Data-model port (type definitions + constructors)
crates/<crate>/selfhost/main.buff                # Smoke-test entry point
```

Two corpora, two purposes (per `self-host/README.md`):
1. `crates/*/selfhost/*.buff` — data-model ports (type definitions + constructor smoke tests). Harness: `scripts/equivalence-rust-vs-buff.sh`.
2. `self-host/*/*.buff` — aspirational LOGIC ports. Harness: AST-dump comparison (see 5c).

### 5b. Equivalence test structure

**File:** `self-host/<crate>/equivalence_<fn_name>.buff`

```buff
// Equivalence test for `tokenize` (T15).
// Tier: T2 (returns Vector<Token> — collection, ordering significant).
//
// Verifies that `tokenize("let x = 1")` produces the same token sequence
// as the Rust reference `crates/buff-lang-lexer/src/lexer.rs::tokenize`.

func equivalence_tokenize_basic() -> Bool:
    let input = "let x = 1"
    let expected_count = 5   // KwLet, Ident("x"), Eq, IntLit(1), EOF
    let tokens = tokenize(input)
    return tokens.len() == expected_count

func main():
    let ok = equivalence_tokenize_basic()
    if ok:
        print("PASS: tokenize basic")
    else:
        print("FAIL: tokenize basic")
```

### 5c. AST comparison via `buff check --dump-ast`

For parser / AST ports, the Equivalence Contract v2 mandates **span-normalized** AST comparison:

```bash
# Rust side: dump AST from the Rust compiler
cargo run -p buff-lang-cli -- check --dump-ast examples/fixture.buff > rust-ast.json

# Buff side: dump AST from the Buff port (once Phase 5 monolith is built)
buff check --dump-ast examples/fixture.buff > buff-ast.json

# Compare (spans normalize to {token_index, offset_within_token})
diff rust-ast.json buff-ast.json || exit 1
```

**Notes:**
- The `--dump-ast` flag emits BOTH raw spans AND normalized spans (per roadmap §Span Normalization). Comparison uses normalized.
- Tier classification (T1 pure-value / T2 collection / T3 timestamped / T4 stateful) decides the comparison method — see roadmap lines 96-101 for the full table.
- Float comparison: `format!("{:.15}", value)` string equality. NOT bit-pattern equality.
- Error comparison: message text byte-identical (except span values), ErrorCode exact match, sorted by `normalized_span.start` before comparison.

### 5d. Test naming

Per `buff-conventions.md` §5:
- Functions: `test_*` or `equivalence_*` prefix with descriptive name.
- Files: `test_*.buff` or `equivalence_*.buff` in the port directory.
- Inline: `@test` attribute with optional description (when supported — Phase 5+).

---

## 6. Import / Module Ordering

**Rule.** Buff's prelude is implicit — `print`, `Vector`, `Map`, `Option`, `Result`, `DateTime`, `Regex`, `Int`, `String`, etc. are all available without `import`. Explicit `import` / `export` / `from` are for cross-file modules only.

### 6a. The prelude is free; do not import it

**Rust before:**
```rust
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::string::String;
use core::option::Option;
```

**Buff after:**
```buff
// (no imports — Vector, Map, Option, String are all prelude)
```

**Notes:** Every Rust `use std::...` line is DELETED on port. The Buff prelude covers it.

### 6b. Cross-file imports — Buff `import` / `export` / `from`

**Rust before** (`crates/buff-lang-lexer/src/lib.rs`):
```rust
mod token;
mod lexer;
mod indent_tracker;

pub use token::*;
pub use lexer::tokenize;
```

**Buff after** (`self-host/lexer/lib.buff`):
```buff
// Buff module wiring for the lexer port (T15).
//
// Buff has no `mod` keyword — files in the same directory are auto-linked
// once the monolith loader (Phase 5, M7) is active. `export` controls
// visibility; `import` pulls names from sibling files.

export func tokenize(source: String) -> Vector<Token>:
    return lexer_tokenize(source)

import { Token, TokenKind } from "token"
import { span_new } from "../error/span"
```

### 6c. Ordering (per `buff-conventions.md` §8)

```buff
// 1. Standard library (alphabetical) — usually empty, prelude is implicit
import { log } from "std/log"

// 2. External packages (alphabetical) — usually empty in self-host ports
//    (the port target crates have NO external deps outside the workspace)

// 3. Local modules (alphabetical within group: siblings first, then parents)
import { Token } from "token"
import { Span, SourceId } from "../error/span"
```

### 6d. File-internal ordering (per `buff-conventions.md` §14)

```buff
// 1. File header comment block (see §1)
// 2. Imports (see §6c)
// 3. Constants
let MAX_TOKEN_LEN = 1024

// 4. Type definitions (struct, enum, type aliases)
struct Token:

enum TokenKind:

// 5. Trait definitions (rare in port targets — most Rust traits don't port; see §7d)
// 6. Function definitions
func tokenize(source: String) -> Vector<Token>:

// 7. Equivalence tests at the bottom (see §5)
func equivalence_tokenize_basic():
```

**Notes:**
- Buff has no `mod` keyword. Files in the same directory are auto-linked by the monolith loader (Phase 5, M7). Until M7 lands, multi-file ports are read as a flat namespace.
- `export` marks module-public symbols (visible to importers). Default is module-private — matches `buff-conventions.md` §15.
- For Phase 3/4 ports: write each file as if multi-file linking already works. The M7 monolith will concatenate them.

---

## 7. Type System Translation

**Rule.** Buff's type system is a strict subset of Rust's — no visible lifetimes, no `&T` / `&mut T`, no trait objects (`dyn Trait`), no `Box<>`. Ports translate to owned semantics.

### 7a. Numeric primitives

| Rust | Buff | Notes |
|---|---|---|
| `i8`, `i16`, `i32`, `i64`, `i128`, `isize` | `Int` | Buff `Int` is the catchall signed integer (lowers to `i64`). Narrower widths are inferred from literal context. |
| `u8` | `Byte` | Buff `Byte` is the unsigned 8-bit type. |
| `u16`, `u32`, `u64`, `u128`, `usize` | `Int` | All unsigned widths collapse to `Int`. Document in header if precision matters (it usually doesn't for indexing). |
| `f32` | `Float` | Buff `Float` is 32-bit IEEE 754. |
| `f64` | `Double` | Buff `Double` is 64-bit IEEE 754. |
| `char` | `Char` | Unicode scalar value. |
| `bool` | `Bool` | |
| `()` (unit) | (none) | **Buff has no unit type.** See §7e. |

### 7b. References — Buff has none

**Rule.** Buff hides references entirely. Every `&T` / `&mut T` / `&[T]` becomes an owned value (clone) or an owned collection.

| Rust | Buff | Translation |
|---|---|---|
| `&T` | `T` | Clone at the call site. |
| `&mut T` | (mutate in place) | Buff mutates struct fields in place via `s.pos = ...`; the `&mut self` of a Rust method is implicit (the free function takes the value as first param). |
| `&[T]` | `Vector<T>` | Owned slice (clone). |
| `&str` | `String` | Owned string. |
| `&'a Token` (borrowed return) | `Token` | Return an owned clone. |
| `Option<&T>` | `Option<T>` | Inner type becomes owned. |
| `Cow<T>` | `T` | Always owned (Buff doesn't have a borrow-or-own abstraction). |

**Rust before:**
```rust
pub fn peek(&self) -> Option<&Token> {
    self.tokens.get(self.pos)
}
```

**Buff after:**
```buff
// Returns owned clone — Buff has no borrowed return values.
func stream_peek(s: TokenStream) -> Option<Token>:
    if s.pos >= s.tokens.len():
        return Option.None
    return Option.Some(s.tokens[s.pos])
```

**Notes:**
- The performance cost of cloning is documented in DR-014 — Buff's bet is that "intelligent clones" + Arc/COW at codegen time make this free in practice. The port does not optimize; it translates.
- For `&mut self` methods: Buff's move-by-default + field mutation covers most cases. Where Rust genuinely needs interior mutability (`RefCell`), the port uses a `MutCell<T>` wrapper (deferred to a per-crate decision).

### 7c. Collections

| Rust | Buff | Notes |
|---|---|---|
| `Vec<T>` | `Vector<T>` | **Buff uses `Vector`, not `Vec`.** This is a hard rule. |
| `&[T]` slice | `Vector<T>` | Owned slice. |
| `[T; N]` array | `Vector<T>` | Buff arrays lower to Vec; fixed-size arrays are not a distinct type. |
| `HashMap<K, V>` | `Map<K, V>` | BUT: per S3 spike / audit finding dep-001, if a Rust crate uses `HashMap`, the port MUST first verify the Rust original can switch to `BTreeMap`. If yes, port as `Map` (BTreeMap-backed, deterministic iteration). If no (genuine O(1) lookup needed), flag the crate as requiring a HashMap stdlib extension. |
| `BTreeMap<K, V>` | `Map<K, V>` | Direct port — Buff `Map` is BTreeMap-backed. |
| `HashSet<T>` | `Set<T>` | Same caveat as HashMap. |
| `BTreeSet<T>` | `Set<T>` | Direct port. |
| `LinkedList<T>` | `Vector<T>` | Rare; if used, document in header. |
| `VecDeque<T>` | `Vector<T>` | `.push_front` / `.pop_front` available on Vector. |

### 7d. Generics, traits, and trait objects

**Rust before (generic fn):**
```rust
pub fn map<T, U>(vec: Vec<T>, f: impl Fn(T) -> U) -> Vec<U> { /* ... */ }
```

**Buff after:**
```buff
func map(vec: Vector<T>, f: { T => U }) -> Vector<U>:
    // ...
```

**Notes:**
- Rust generics → Buff generics. Same `<T, U>` syntax.
- Rust `impl Trait` (trait bound as arg type) → Buff lambda `{ T => U }` for the `Fn` / `FnMut` / `FnOnce` traits. Buff closures are spelled `{ params => body }`.
- Rust trait bounds (`<T: Clone + Debug>`) → Buff trait bounds (syntax-verified in v1.x; check `examples/generics.buff` for current state). If a bound is unsupported, document the workaround in the header.

**Rust before (trait object — DOES NOT PORT directly):**
```rust
pub trait Visitor {
    fn visit(&self, node: &Node);
}

pub struct Walker {
    visitor: Box<dyn Visitor>,
}
```

**Buff after (multiple dispatch — v1.19 `multi_dispatch.rs`):**
```buff
// Buff has no `dyn Trait`. Per DR-014, the S1 spike tests whether
// multiple dispatch (v1.19) suffices for the trait-object use cases in the
// port-target crates. Until S1 lands, trait objects are a HARD WALL.
//
// If S1 succeeds, trait-object call sites translate to multi-dispatch:
//   multi_func visit(node: Node): ...
// If S1 fails, the affected crate is dropped from the port scope.
```

**Notes:**
- This is the SINGLE BIGGEST PORTABILITY QUESTION in the whole effort. The S1 spike (Phase 2) decides it.
- The 10 target crates in DR-014 have ZERO `dyn`-trait usages by design (that's why they're the targets). But Rust `impl Trait` (anonymous closures) IS common and ports cleanly to Buff lambdas.
- Rust `Box<T>` → Buff owned value (`T`). Rust `Arc<T>` → Buff `Arc<T>` (preserved via prelude_types). Rust `Rc<T>` → Buff `Arc<T>` (consolidate on Arc).

### 7e. Option / Result — preserved

**Rust before:**
```rust
pub fn parse_int(s: &str) -> Option<i64> { /* ... */ }
pub fn tokenize(s: &str) -> Result<Vec<Token>, LexerError> { /* ... */ }
```

**Buff after:**
```buff
func parse_int(s: String) -> Option<Int>:
    // ...

func tokenize(s: String) -> Result<Vector<Token>, LexerError>:
    // ...
```

**Construction:**
| Rust | Buff |
|---|---|
| `Some(x)` | `Option.Some(x)` or `Some(x)` |
| `None` | `Option.None` or `None` |
| `Ok(x)` | `Ok(x)` |
| `Err(e)` | `Error("msg")` (builtin Error) or `Err(e)` (typed) |

**Notes:** Both forms work. The qualified form (`Option.Some`) is safer in large files with many imports; the unqualified form (`Some`) is the idiom in the existing corpus. Match the file's existing style.

### 7f. The unit type `()`

**Rule.** Buff has no unit type. A Rust function returning `()` becomes a Buff function with no return type annotation (implicit unit-via-statement).

**Rust before:**
```rust
pub fn print_span(span: Span) {  // implicit ()
    println!("{}", span);
}

pub fn require_edition(edition: Edition, kind: TokenKind) -> Result<(), &str> {
    /* ... */ Ok(())
}
```

**Buff after:**
```buff
// No return type annotation — the function returns implicitly.
func print_span(span: Span):
    print(span.to_string())

// Buff has no `()` unit type. Use Option<String>: None == accepted, Some(msg) == rejected.
func require_edition(edition: Edition, kind: TokenKind) -> Option<String>:
    if is_scientific_only(kind):
        if not edition_is_scientific(edition):
            return Option.Some("this syntax requires edition = \"scientific\"")
    return Option.None
```

**Notes:** This pattern (substitute `Option<String>` for `Result<(), &str>`) is from `self-host/parser/stream.buff` lines 66-74. Document in the file header when applied.

---

## 8. Patterns That Need Special Handling

### 8a. Rust macros → Buff prelude functions

| Rust macro | Buff equivalent | Notes |
|---|---|---|
| `println!("{...}")` | `print("...{var}...")` | Buff interpolation uses `{var}` syntax directly in the string. No format-string mini-language. |
| `println!("{:?}", x)` | `print(x.to_string())` | No `{:?}` Debug formatting — call `.to_string()` explicitly (Buff structs auto-derive a `to_string`). |
| `eprintln!(...)` | (no equivalent in prelude) | Use `log.error(...)` or `log.warn(...)`. |
| `format!("{...}")` | `"...{var}..."` | String interpolation is built into Buff string literals. |
| `vec![1, 2, 3]` | `[1, 2, 3]` | Vector literal syntax. |
| `vec![0; n]` | `Vector.zeros(n)` | Repeat-construction. |
| `panic!("...")` | **FORBIDDEN** | Per project rule: no `panic!` in non-test code. Return an `Error` instead. |
| `assert!(cond)` | `@test assert(cond)` or `if not cond: return Error("...")` | In test code, use `@test`. In production, return an error. |
| `assert_eq!(a, b)` | `@test assert_eq(a, b)` | Test code only. |
| `todo!()` / `unimplemented!()` | **FORBIDDEN** | Same as `panic!`. If the Rust original has these, the port documents the gap in the header and either (a) implements the body, or (b) omits the function and marks it deferred. |
| `unwrap()` / `expect()` | **FORBIDDEN in non-test code** | Per project rule. Use `match` / `unwrap_or` / `?` instead. In test code, allowed. |

**Rust before:**
```rust
println!("token {:?} at byte {}", kind, span.start);
let v: Vec<i64> = vec![1, 2, 3];
let s = format!("sum is {}", v.iter().sum::<i64>());
```

**Buff after:**
```buff
print("token " + kind.to_string() + " at byte " + span.start.to_string())
let v = [1, 2, 3]
let total = v.reduce({ a, b => a + b }).unwrap_or(0)
let s = "sum is " + total.to_string()
```

### 8b. Derive macros

**Rule.** Buff auto-derives `Debug, Clone, PartialEq` on every struct/enum (per project rule "derive defaults"). Additional derives are explicit attributes.

| Rust derive | Buff handling |
|---|---|
| `#[derive(Debug, Clone, PartialEq)]` | (implicit — do not write anything) |
| `#[derive(Eq, Hash)]` | Add when the type is used as a Map/Set key. Syntax: TBD per `examples/generics.buff`; for ports, add a `// derive: Eq, Hash` comment. |
| `#[derive(thiserror::Error)]` | Does NOT port — see §4c. |
| `#[derive(Default)]` | Buff has `Type.default()` if all fields have defaults. |
| `#[derive(Serialize, Deserialize)]` | Does NOT port — serde is a Rust-host concern. |

**Notes:** If a Rust derive does heavy lifting (e.g. serde), the port documents the gap in the header. The Rust host's serde impl stays in Rust.

### 8c. Closures / lambdas

**Rust before:**
```rust
let doubled: Vec<i64> = vec.into_iter().map(|x| x * 2).collect();
let is_even = |x: i64| x % 2 == 0;
let greet = |name: &str| format!("hello {}", name);
```

**Buff after:**
```buff
let doubled = [1, 2, 3].map({ x => x * 2 })
let is_even = { x => x % 2 == 0 }
let greet = { name => "hello " + name }
```

**Notes:**
- Lambdas are `{ params => body }`. Single-expression body.
- Multi-statement lambdas: `{ x => /* let ... */ ... }` (newlines inside the brace).
- Buff has NO `move` keyword — capture is always by move (move-by-default).
- Buff has NO `|x|` single-bar syntax — always `{ x => ... }`.

### 8d. Loops

| Rust | Buff | Notes |
|---|---|---|
| `for x in iter { ... }` | `for x in iter:` | Same. |
| `for x in 0..n { ... }` | `for x in range(n):` or `for x in 0..n:` | Range syntax works; `range()` helper also available. |
| `while cond { ... }` | `for cond:` | **`while` is NOT a Buff keyword.** Conditional loops use `for <cond>:`. |
| `while let Some(x) = iter.next() { ... }` | `for x in iter:` | Restructure as iterator loop where possible. If genuinely needed: `for iter.has_next(): let x = iter.next().expect("checked")` (only in test code; production code returns an Error). |
| `loop { ... break x; }` | `for true:` then `return` from inside | Document in header. |
| `break` | `break` | Same. |
| `continue` | `continue` | Same. |

**Rust before:**
```rust
while self.pos < self.tokens.len() {
    let tok = self.tokens[self.pos];
    if tok.kind == TokenKind::Newline {
        self.pos += 1;
        continue;
    }
    break;
}
```

**Buff after:**
```buff
for s.pos < s.tokens.len():
    let tok = s.tokens[s.pos]
    if tok.kind == TokenKind.Newline:
        s.pos = s.pos + 1
        continue
    break
```

**Notes:** The `while` → `for` translation is THE most surprising Buff-grammar quirk for someone reading the Rust original. Every port file that translates a `while` documents this in the header.

### 8e. Pattern matching — `match`, `if let`, `while let`

**Rust before:**
```rust
match opt {
    Some(x) => print(x),
    None => print(0),
}

match kind {
    TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent => skip(),
    _ => break,
}

if let Some(x) = opt { /* ... */ }
```

**Buff after:**
```buff
match opt {
    Some(x) => print(x),
    None => print(0),
}

// Buff match REJECTS or-patterns. Translate to if/else if with == on enums.
if kind == TokenKind.Newline:
    skip()
else if kind == TokenKind.Indent:
    skip()
else if kind == TokenKind.Dedent:
    skip()
else:
    break

// Buff has no `if let`. Translate to match with one arm + else.
match opt {
    Some(x) => /* ...use x... */,
    None => /* ... or omit this arm if it's a no-op ... */,
}
```

**Notes — what Buff `match` REJECTS (verify against `examples/pattern_matching.buff`):**
- Or-patterns (`A | B | C`) — expand to `if/else if` chains with `==`.
- Qualified patterns (`TokenKind.Newline`) — UNQUALIFIED form (`Newline`) is required INSIDE match arms. The qualified form works in boolean comparisons (`kind == TokenKind.Newline`).
- Pattern guards (`Some(x) if x > 0`) — extract the guard into the arm body.
- Statement bodies — match arms are single EXPRESSIONS. For statements, wrap in an immediately-called lambda or restructure as `if/else`.
- `{ }` block arms — a `{` immediately after `=>` starts a CLOSURE, not a block. Use a single expression, or use `if/else` for multi-statement cases.

**These are documented in `self-host/parser/stream.buff` lines 18-28 (SYNTAX MAPPING NOTES). Every port file that translates a Rust `match` with any of the above patterns documents the translation in its header.**

### 8f. Built-in attribute translation

| Rust attribute | Buff handling |
|---|---|
| `#[derive(...)]` | See §8b. |
| `#[cfg(test)]` | `@test` attribute on test functions. |
| `#[allow(...)]` / `#[warn(...)]` | Drop — Buff has no equivalent lint-allow machinery in v1.x. |
| `#[inline]` / `#[cold]` | Drop — Buff has no inline hints. Codegen decides. |
| `#[non_exhaustive]` | Drop — Buff has no stability annotation. |
| `#[must_use]` | Drop — Buff has no must_use (move-by-default makes it less necessary). |
| `#[deprecated]` | `@deprecated("use X instead", since = "1.x.0")` — see `buff-conventions.md` §9. |

### 8g. impl blocks → free functions

**Rule.** Buff `impl` blocks exist but the existing port corpus standardizes on free functions taking the value as the first parameter. The pattern is:

**Rust before:**
```rust
impl TokenStream<'a> {
    pub fn new(tokens: Vec<Token>) -> Self { /* ... */ }
    pub fn peek(&self) -> Option<&Token> { /* ... */ }
    pub fn advance(&mut self) -> Option<Token> { /* ... */ }
}
```

**Buff after:**
```buff
// The `impl TokenStream { ... }` block becomes a family of free functions
// taking the cursor as the first parameter (`s: TokenStream`). Buff mutates
// struct fields in place via `s.pos = ...`, so `&mut self` is implicit.

func token_stream_new(tokens: Vector<Token>) -> TokenStream:
    return TokenStream.new(tokens: tokens, pos: 0)

func stream_peek(s: TokenStream) -> Option<Token>:
    // ...

func stream_advance(s: TokenStream) -> Option<Token>:
    // ...
```

**Naming convention:**
- Constructors: `<type_snake_case>_new`, `<type_snake_case>_from_<source>`.
- Methods: `<type_snake_case_prefix>_<method_name>` — short prefix (e.g. `stream_peek`, not `token_stream_peek`) once unambiguous.
- The first parameter is always the receiver, named after the prefix (`s` for stream, `t` for token, `tok` if `t` clashes).

**Notes:**
- This is a corpus-wide decision. The existing logic-port files (`token.buff`, `stream.buff`) all use it.
- Alternative: Buff DOES support `impl Type { ... }` blocks (per v1.x language surface). If a port agent prefers impl blocks, document the deviation in the file header. The free-function pattern is preferred for consistency with the corpus.
- A Rust trait impl (`impl Visit for Walker`) does NOT translate cleanly — see §7d.

---

## 9. What DOES NOT Port (Project-Wide Walls)

For completeness — these are documented in DR-014 and `buff-conventions.md` §19. Port agents must recognize them on sight and NOT attempt translation:

| Pattern | Why it's a wall | Action |
|---|---|---|
| `dyn Trait` (trait objects) | Buff has no runtime polymorphism (yet — S1 spike tests v1.19 multiple dispatch as alternative) | S1 spike verdict required first |
| `std::sync::Arc`, interior mutability (`RefCell`, `RwLock`) | FFI to Rust runtime | Keep as `Arc<T>` if prelude supports it; else document and defer |
| `tokio::spawn`, `async` runtime | FFI to tokio | Do NOT port — async-runtime crates (`buff-jupyter`, `buff-registry`) are in DR-014 §🟥 |
| `rayon::par_iter` | FFI to rayon | Do NOT port — runtime crate |
| `wgpu` shader code | FFI to wgpu | Do NOT port — runtime + codegen-wgsl crates |
| `syn::File` / `quote!` / `prettyplease` | Rust-AST manipulation — Buff has no `Syn` stdlib; raw-string codegen is project-rule-banned | THE WALL. `buff-lang-codegen-rust` does not port. |
| `thiserror::Error` derive | Derive stays in Rust host | See §4c |
| `serde::Serialize` / `Deserialize` derive | Derive stays in Rust host | Port the data type; document the serde impl as deferred |
| Raw-string codegen (`format!("pub fn ...")` building Rust source as text) | BANNED by project anti-pattern rule | Do NOT port — see `buff-lang-codegen-rust` / `-buffhtml` |
| `unsafe` blocks | Buff has `unsafe` keyword but the runtime is FFI-heavy in this area | Document each `unsafe` block; port the signature, defer the body |

---

## 10. Verification Checklist (per port file)

Before marking a `.buff` port file complete, verify:

- [ ] Header comment block lists the source Rust file path + T-number
- [ ] Header documents EVERY translation decision (references removed, `while`→`for`, `()`→`Option<String>`, etc.)
- [ ] All Rust comments preserved (adjusted only where borrow-checker references became meaningless)
- [ ] No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in non-test code
- [ ] No `_async` suffix on async functions
- [ ] Constructors are `Type.new()` / `Type.from()` (no `new Person()`, `Person.create()`)
- [ ] Boolean args are named (`foo(x, flag: true)`)
- [ ] No `while` keyword (use `for cond:`)
- [ ] No `if let` / `while let` (use `match` or `if/else`)
- [ ] No or-patterns / qualified patterns / guards / statement bodies in `match` arms
- [ ] No `dyn Trait` (if the Rust original has it, STOP — wall hit, escalate)
- [ ] Numeric primitives mapped per §7a
- [ ] `Vec<T>` → `Vector<T>` (NOT `Vec<T>`)
- [ ] `HashMap`/`HashSet` usage flagged per S3 (try BTreeMap first)
- [ ] References (`&`, `&mut`) translated to owned semantics per §7b
- [ ] ErrorCodes (if any) preserved verbatim
- [ ] File ends with a single trailing newline
- [ ] Indentation is 4 spaces (NO tabs — lexer rejects)
- [ ] No trailing whitespace
- [ ] Multi-line collections have trailing commas
- [ ] `buff check <file>.buff` passes (lex + parse + typecheck clean)
- [ ] Equivalence test (`equivalence_<fn>.buff`) exists for at least one public function
- [ ] Original Rust file is UNCHANGED on the port branch (`git diff` on `crates/<crate>/src/` is empty for the ported file)

---

## 11. References

- **DR-014** — `.sisyphus/decisions/selfhost-feasibility.md` (crate list, portability verdicts, the codegen-rust wall)
- **Roadmap** — `.sisyphus/plans/self-host-completion-roadmap.md` §Equivalence Contract v2 (lines 92-144)
- **Buff language rules** — `.sisyphus/plans/buff-conventions.md` (19 conventions; AUTHORITATIVE for syntax)
- **Project anti-patterns** — `AGENTS.md` (root) §ANTI-PATTERNS, §UNIQUE STYLES
- **Existing corpus style** — `self-host/lexer/token.buff`, `self-host/parser/stream.buff` (logic-port headers + translation-decision format)
- **Data-model corpus style** — `crates/buff-lang-ast/selfhost/common.buff` (struct-literal + smoke-test pattern)
- **Runnable examples** — `examples/ola.buff`, `examples/fibonacci.buff`, `examples/closures.buff`, `examples/collections.buff`, `examples/error_handling.buff`
- **Error code stability** — `buff-conventions.md` §19 (E10xx/E11xx/E12xx/E13xx stable forever)
- **Bootstrap determinism** — `self-host/bootstrap-report.md` (current 7/56 pass rate + failure modes)
- **Multi-dispatch spike** — `crates/buff-lang-types/src/analysis/multi_dispatch.rs` (500 LOC, v1.19) — basis for S1

---

## Appendix A — Decision Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-07-26 | Standardize on `Type.new()` constructor syntax (not struct literal) for logic ports | Matches existing corpus (`token.buff`, `stream.buff`); works for both simple and complex cases; struct-literal alternative is fragile when fields grow |
| 2026-07-26 | Translate `impl Block` as free functions with type-prefixed names | Matches existing corpus; surfaces the receiver as a parameter (clear data flow); avoids Buff's `impl` syntax which is still stabilizing |
| 2026-07-26 | Translate `()` (unit) as `Option<String>` for `Result<(), E>` cases | Matches `stream.buff` precedent; Buff has no unit type; `None`=success / `Some(msg)`=failure is self-documenting |
| 2026-07-26 | Document `?` operator as supported (NOT absent) | `examples/error_handling.buff` confirms end-to-end `?` propagation works; correcting the task-spec assumption |
| 2026-07-26 | Translate `while` as `for cond:` | `while` is not a Buff keyword; `for <cond>:` is the conditional-loop spelling |
| 2026-07-26 | Expand Rust or-patterns in `match` to `if/else if` chains with `==` | Buff `match` rejects or-patterns; enum structural equality supports direct comparison |
| 2026-07-26 | Keep custom error ENUM port (data) but defer Display impl (derive) | thiserror derive stays in Rust host; manual `_display()` free function bridges |
| 2026-07-26 | HashMap portability gated on S3 spike | dep-001 audit finding; `Map` is BTreeMap-backed; genuine HashMap usage requires either a port-scope code change to BTreeMap (verboten per "Rust originals stay untouched") or a HashMap stdlib extension |
