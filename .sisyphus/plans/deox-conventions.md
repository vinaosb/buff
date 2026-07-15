# Deox Conventions Standard

> 18 conventions covering naming, formatting, documentation, errors, testing, APIs, and more.
> Enforced by `deox fmt` (formatter) and `deox check` (linter).

---

## 1. Naming Conventions

| Element | Convention | Example |
|---------|-----------|---------|
| Functions | `snake_case` | `func calculate_total()` |
| Variables | `snake_case` | `let item_count = 42` |
| Types (Struct/Enum) | `PascalCase` | `struct HttpRequest`, `enum Color` |
| Constants | `SCREAMING_SNAKE` | `let MAX_RETRIES = 3` |
| Modules/Files | `snake_case` | `src/data_processor.deox` |
| Enum variants | `PascalCase` | `Red`, `Green`, `Ok`, `Err` |
| Traits | `PascalCase` | `trait Iterable` |
| Generic params | `PascalCase` (single letter ok) | `T`, `K`, `V`, `Item` |

## 2. Formatting Rules (enforced by `deox fmt`)

- Indentation: **4 spaces** (NO TABS — lexer rejects)
- Line length: **100 characters** max
- No trailing whitespace
- Max **2 consecutive blank lines**
- **Trailing comma** in multi-line collections: YES
- Import ordering: stdlib → external → local (alphabetical within group)

## 3. Documentation Comments

```deox
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
- Files: `test_*.deox` in `tests/` directory
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

```deox
// 1. Standard library (alphabetical)
import { print } from "std/io"

// 2. External packages (alphabetical)
import { HttpServer } from "http"

// 3. Local modules (alphabetical)
import { helper } from "./utils"
```

## 9. Deprecation

```deox
@deprecated("use new_function() instead", since = "0.5.0")
func old_function()
```

## 10. Logging

```deox
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

```deox
items.iter()           // Borrow iterator
items.iter_mut()       // Mutable borrow iterator
items.into_iter()      // Consuming iterator
items.par_iter()       // Parallel iterator (Rayon)
items.par_map({ ... }) // Parallel map
```

## 13. Result/Option Methods

```deox
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

## 14. File Organization (within a .deox file)

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
deox.lock
.vscode/
.idea/
*.swp
.DS_Store
Thumbs.db
.env
```
