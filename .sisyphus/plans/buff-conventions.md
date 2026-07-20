# Buff Conventions Standard

> 18 conventions covering naming, formatting, documentation, errors, testing, APIs, and more.
> Enforced by `buff fmt` (formatter) and `buff check` (linter).

---

## 1. Naming Conventions

| Element | Convention | Example |
|---------|-----------|---------|
| Functions | `snake_case` | `func calculate_total()` |
| Variables | `snake_case` | `let item_count = 42` |
| Types (Struct/Enum) | `PascalCase` | `struct HttpRequest`, `enum Color` |
| Constants | `SCREAMING_SNAKE` | `let MAX_RETRIES = 3` |
| Modules/Files | `snake_case` | `src/data_processor.buff` |
| Enum variants | `PascalCase` | `Red`, `Green`, `Ok`, `Err` |
| Traits | `PascalCase` | `trait Iterable` |
| Generic params | `PascalCase` (single letter ok) | `T`, `K`, `V`, `Item` |

## 2. Formatting Rules (enforced by `buff fmt`)

- Indentation: **4 spaces** (NO TABS — lexer rejects)
- Line length: **100 characters** max
- No trailing whitespace
- Max **2 consecutive blank lines**
- **Trailing comma** in multi-line collections: YES
- Import ordering: stdlib → external → local (alphabetical within group)

## 3. Documentation Comments

```buff
/// Calculates the sum of a vector of integers.
///
/// # Arguments
/// * `items` - The vector of integers to sum
///
/// # Returns
/// The total sum as an Int
///
/// # Errors
/// Returns Err(EmptyVector) if the vector is empty
///
/// # Example
/// let total = sum([1, 2, 3])  // Returns 6
func sum(items: Vector<Int>) -> Result<Int, Error>
```

Standard sections: `# Arguments`, `# Returns`, `# Errors`, `# Example`, `# Panics`, `# Safety`

## 4. Error Messages

- **Format**: lowercase, no trailing period, describe what happened
- **Include context**: `"file not found: {path}"`, not just `"not found"`
- **Templates**: `"index {index} out of bounds for length {len}"`, `"type mismatch: expected {expected}, found {found}"`

## 5. Test Naming

- Functions: `test_*` prefix with descriptive name: `test_addition_returns_correct_sum`
- Files: `test_*.buff` in `tests/` directory
- Inline: `@test` attribute with optional description

## 6. Async API Naming

- **NO `_async` suffix** — async is in the type, not the name
- `async func fetch_data()` ✅ | `async func fetch_data_async()` ❌

## 7. Constructors

- **Struct literal** (simple): `Person { name: "Alice", age: 30 }`
- **Type.new()** (complex): `HttpServer.new(port: 8080)`
- **Type.from()** (conversions): `Int.from("42")`
- **NEVER**: `new Person()`, `Person.create()`, `Person.build()`

## 8. Import Ordering

```buff
// 1. Standard library (alphabetical)
import { print } from "std/io"

// 2. External packages (alphabetical)
import { HttpServer } from "http"

// 3. Local modules (alphabetical)
import { helper } from "./utils"
```

## 9. Deprecation

```buff
@deprecated("use new_function() instead", since = "0.5.0")
func old_function()
```

## 10. Logging

```buff
log.debug("Processing {n} items")
log.info("Server started on port {port}")
log.warn("Cache miss for key {key}")
log.error("Failed to connect: {error}")
```
Structured logging with interpolation — no format strings.

## 11. Boolean Parameters

- **ALWAYS use named arguments** for booleans: `fetch(url, cache: true, redirect: false)`
- **NEVER positional**: `fetch(url, true, false)` — what does true mean?

## 12. Iterator Methods (Rust-compatible)

```buff
items.iter()           // Borrow iterator
items.iter_mut()       // Mutable borrow iterator
items.into_iter()      // Consuming iterator
items.par_iter()       // Parallel iterator (Rayon)
items.par_map({ ... }) // Parallel map
```

## 13. Result/Option Methods

```buff
opt.unwrap()              // Panic if None
opt.unwrap_or(default)    // Provide default
opt.expect("message")     // Panic with message
opt.is_some() / .is_none()
opt.map({ x => f(x) })
opt.filter({ x => p(x) })

result.unwrap() / .unwrap_or(default)
result.is_ok() / .is_err()
result.map({ x => f(x) })
result.map_err({ e => f(e) })
```

## 14. File Organization (within a .buff file)

```
// 1. Imports
// 2. Constants
// 3. Type definitions (struct, enum, type aliases)
// 4. Trait definitions
// 5. Function definitions
// 6. Tests (@test functions at bottom)
```

## 15. Visibility

- **Default**: module-private (no keyword needed)
- **Export**: `export func public_api()` — visible to importers
- **Convention**: minimize public API surface. If unsure, make it private.

## 16. Versioning (SemVer)

- `0.x.y`: Pre-1.0, anything can change
- `1.0.0+`: Breaking = MAJOR, Feature = MINOR, Fix = PATCH

## 17. Changelog Format

```markdown
## [VERSION] - DATE
### Added / Changed / Deprecated / Removed / Fixed
```

## 18. .gitignore Standard

```gitignore
target/
buff.lock
.vscode/
.idea/
*.swp
.DS_Store
Thumbs.db
.env
```


## 19. Error Code Stability

Every user-facing diagnostic the Buff compiler emits MAY carry a stable error code of the form `E1xxx` (see the `ErrorCode` enum in `crates/buff-lang-error/src/code.rs`). When present, the code renders alongside the message — e.g. `[Error] error[E1001]: unexpected character: '@'` — and the static catalog at `docs/errors/` documents each code with a longer explanation and a fix recipe.

### Numbering scheme

Codes are grouped by compiler phase so that reading a code alone tells the user which part of the pipeline produced it:

| Range      | Phase          | Source crate(s)                               |
|------------|----------------|-----------------------------------------------|
| `E10xx`    | Lexing         | `buff-lang-lexer`                             |
| `E11xx`    | Parsing        | `buff-lang-parser`                            |
| `E12xx`    | Type-checking  | `buff-lang-types`                             |
| `E13xx`    | Codegen        | `buff-lang-codegen-rust`                      |
| `E14xx`    | Runtime        | `buff-lang-runtime` (reserved — unused today) |

### Stability guarantee (STRICT, versioned contract)

Once an `E1xxx` code ships in a release, it is **stable across all future releases**. The following rules are NON-NEGOTIABLE and apply the moment a code appears on the static site (`docs/errors/`) or in the public `ErrorCode` enum:

1. **Never renumber.** `E1001` is `E1001` forever. The numeric value is part of the public API and may appear in user documentation, CI lint configs, IDE plugins, RFCs, and search queries. Renumbering a code is a breaking change under §16 (SemVer) and would require a major version bump — and even then, it is forbidden because there is no way to alert every user.
2. **Never reuse.** A code's meaning never changes. If `E1007` ships as "unterminated regex literal", it stays "unterminated regex literal" forever — even if a future lexer rewrite surfaces that condition under a different message. Reusing a code for a different failure mode is forbidden because users with old documentation or scripts would silently mis-diagnose.
3. **Never silently remove.** If a code becomes impossible to trigger (e.g. the underlying feature is deleted), the code stays in the `ErrorCode` enum AND on the static site with its existing text unchanged, plus a tombstone note (e.g. "This code is no longer emitted as of v2.0; it is retained for historical lookups."). The variant is never deleted.
4. **New codes are appended at the end of their phase block.** New lexer errors get the next free `E10xx`, new parser errors get the next free `E11xx`, and so on. Codes within a phase are allocated strictly in ascending order; gaps left by tombstoned codes are NOT back-filled (see rule 3).
5. **The `ErrorCode` enum is the source of truth.** `code.rs` defines the canonical mapping; the static site at `docs/errors/` is generated from it via `cargo run -p buff-lang-error --example gen_error_docs`. The two must never drift — if a code is in `code.rs`, it MUST have a page on the site (enforced by the `error_catalog_site_pages_exist_for_every_code` test).
6. **Codes are append-only across releases.** A release MAY add new codes (appended at the end of their phase) and MAY tombstone existing codes (rule 3); a release MAY NOT renumber, reuse, or delete codes.

This policy mirrors `rustc`'s `E0xxx` stability guarantee (see <https://github.com/rust-lang/rust/blob/master/compiler/rustc_error_codes/>). Users can cite a Buff error code in a bug report, a Stack Overflow answer, or a CI lint rule, and trust that the citation stays meaningful across releases.

### When to attach a code

- **Attach a code** at the major user-facing diagnostic construction sites — the named error constructors (`LexerError::unexpected_char`, etc.) and the central helpers (`TokenStream::expect`, the type-checker's operator-mismatch path, the codegen `unsupported` helper). Pragmatic coverage is the bar; not every internal variant needs its own code.
- **Do NOT invent speculative codes** for failure modes the compiler does not currently emit. The catalog documents actual behaviour, not aspirational behaviour. New codes arrive in the same release that first emits them.
- **A diagnostic without a code is still valid.** The `Diagnostic::code` field is `Option<ErrorCode>`; ad-hoc / uncategorised diagnostics render without an `E1xxx` tag. The render format is `[Error] message` (no code) or `[Error] error[E1xxx]: message` (with code) — both are public API and byte-stable.
