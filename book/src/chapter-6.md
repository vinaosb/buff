# Chapter 6 — Language Reference

This chapter is the authoritative reference for the Buff language grammar and
type system. It's organized so you can dip into a section when you need a
specific rule, not necessarily read top to bottom. The earlier chapters are
the tutorial; this is the spec.

## 6.1 Lexical structure

### Keywords (25)

These identifiers are reserved and cannot be used as names:

```
func let mut struct enum trait type
if else for return break continue in match
async spawn import export from as
true false extern unsafe
```

Notably **absent**: `class`, `null` / `nil`, `await`, `try`, `catch`, `new`,
`delete`, `this`, `self` (methods are standalone functions), `super`.

### Identifiers

- `snake_case` for functions and variables: `calculate_total`, `item_count`.
- `PascalCase` for types (struct, enum, trait) and enum variants: `HttpRequest`,
  `Color`, `Red`, `Ok`, `Err`.
- `SCREAMING_SNAKE_CASE` for constants: `MAX_RETRIES`.
- Single-letter generic parameters are conventional: `T`, `K`, `V`, `Item`.

### Literals

| Literal | Example | Type |
|---|---|---|
| Integer | `42`, `0xFF`, `0b1010` | narrowest width that fits → `Int` (i64) |
| Float | `3.14`, `2.0e10` | `Float` (f32) |
| String | `"hello"`, `"with {interp}"` | `String` (owned) |
| Char | `'a'`, `'\n'`, `'\u{1F600}'` | `Char` |
| Boolean | `true`, `false` | `Bool` |
| Byte | `0b1010b` | `Byte` |
| Regex | `re"[a-z]+"` | `Regex` (compiled at parse time) |

### Comments

```buff
// Line comment — to end of line.

/// Doc comment — attaches to the next declaration.
/// Renders in `buff doc` output and IDE hover.

// Block comments are not nested:
/* a block comment on one line */
```

### Whitespace and indentation

- **Indentation is 4 spaces.** Tabs are a lexer error ([E1004](./chapter-8.md#e1004)).
- The lexer enforces the **offside rule**: a block opens with `:` and a
  newline, and its body is everything indented one level deeper. Dedenting
  closes the block.
- Blank lines and lines containing only a comment do not affect indentation
  tracking.
- Maximum **100 characters** per line (convention, enforced by `buff fmt`).
- No trailing whitespace; maximum 2 consecutive blank lines.

### String interpolation 🟢

String literals interpolate `{expr}` directly — no `format!` macro:

```buff
func main():
    let name = "Ada"
    let count = 7
    print("hello, {name}! you have {count} messages")
    // "hello, Ada! you have 7 messages"
    print("sum: {1 + 2}")
    // "sum: 3"
```

The expression inside `{...}` is any valid Buff expression. Escape a literal
`{` or `}` with `{{` / `}}`. This is the *only* string-formatting mechanism in
Buff — there is no `format!`, no `printf`, no `%s`.

## 6.2 Type system

### Primitive types

| Buff type | Rust lowering | Notes |
|---|---|---|
| `Int` | `i64` | default integer type |
| `Int<N>` | `i8` / `i16` / `i32` / `i64` | explicit width |
| `Float` | `f32` | default float type |
| `Float<N>` | `f32` / `f64` | explicit width |
| `Bool` | `bool` | |
| `Char` | `char` | Unicode scalar value |
| `Byte` | `u8` | raw byte |
| `String` | `String` | owned UTF-8 |
| `Void` / `()` | `()` | unit type |

### Numeric width inference

Integer *literals* infer the narrowest width that fits their value: `1` is
`i8`, `300` is `i16`, `70000` is `i32`, anything bigger is `i64`. When a
literal flows into a `let` binding without an explicit type, it promotes to
`Int` (i64) for safety. Float literals are `Float` (f32) unless the context
demands `f64`.

### Collection types

| Buff type | Rust lowering | Literal syntax |
|---|---|---|
| `Vector<T>` | `Vec<T>` | `[1, 2, 3]` |
| `Map<K, V>` | `HashMap<K, V>` | `{ "a": 1, "b": 2 }` |
| `Set<T>` | `HashSet<T>` | *(constructor only)* |
| `Option<T>` | `Option<T>` | `Some(x)`, `None` |
| `Result<T, E>` | `Result<T, E>` | `Ok(x)`, `Err(e)` |
| `(A, B, C)` | `(A, B, C)` | `(1, "two", true)` |

### `Option<T>` — the absence of null

Buff has **no `null`**. Absence is `Option<T>`: either `Some(value)` or
`None`. The compiler's exhaustiveness checker (T27) verifies every `match`
covers both arms (or has a `_` catch-all), so you cannot forget to handle the
absent case.

```buff
func main():
    let mut drawer = [11, 22, 33]
    let taken = drawer.pop()         // Option<Int>
    match taken { Some(x) => print(x), None => print(0) }
```

### `Result<T, E>` and the `?` operator 🟢

Fallible operations return `Result<T, E>`. The `?` operator propagates errors
early — exactly like Rust's:

```buff
func half(n: Int) -> Result<Int, Error>:
    if n < 2:
        return Error("input too small")
    return Ok(n / 2)

func add_one(n: Int) -> Result<Int, Error>:
    let h = half(n)?          // unwraps Ok, or returns Err from add_one
    return Ok(h + 1)
```

The builtin `Error` type (lowers to `Err(Error::new("msg"))`) means you never
write `impl Display` or `impl std::error::Error` boilerplate. For richer error
types, define an `enum` — though note that custom error *enums* are a
codegen-verified-but-not-yet-end-to-end feature (variants emit unqualified;
see [`examples/error_handling.buff`](../../examples/error_handling.buff)).

## 6.3 Functions

### Declaration

```buff
func name(param: Type, param2: Type) -> ReturnType:
    body
```

- Parameters are `name: Type`. The type annotation is required on public
  functions, optional elsewhere (inferred).
- Return type after `->`. Optional; inferred if absent.
- `main` is the entry point, takes no parameters, returns `Void`.

### Named arguments

For calls with multiple parameters, Buff **mandates named arguments** for any
boolean or stringly-typed parameter after the first (convention §11):

```buff
fetch(url, cache: true, redirect: false)   // ✅
fetch(url, true, false)                     // ❌ linter error: positional booleans
```

Positional arguments are fine for the first 1–2 "obvious" parameters (a URL,
a file path); everything else should be named. This is enforced by convention
and the `buff check` linter, not the parser.

### Constructors (convention §7)

Buff has no `new` keyword. Construction uses:

- **Struct literal** (simple): `Point { x: 3.0, y: 4.0 }`
- **`Type.new(...)`** (complex): `HttpServer.new(port: 8080)`
- **`Type.from(...)`** (conversions): `Int.from("42")`

**Forbidden**: `new Person()`, `Person.create()`, `Person.build()`,
`Person.make()`. The constructor surface is `Type.new()` / `Type.from()` only.

### `let` and `mut`

```buff
let x = 5              // immutable binding (default)
let mut counter = 0    // mutable binding
counter = counter + 1  // ok — counter is mut
```

Bindings are immutable by default (Rust's default). Add `mut` to allow
reassignment or in-place mutation (`.push`, `+=`, etc.).

## 6.4 Collections

### `Vector<T>`

```buff
let v = [10, 20, 30, 40]
print(v[0])            // 10 — index coerced to usize
print(v.len())         // 4

let mut stack = [1, 2, 3]
stack.push(4)
let top = stack.pop()  // Option<Int> = Some(4)

let doubled = v.map({ x => x * 2 })   // [20, 40, 60, 80]
```

Common methods: `.len()`, `.push(x)`, `.pop()`, `.map(fn)`, `.filter(fn)`
(codegen-verified), `.fold(acc, fn)` (codegen-verified), `.iter()`.

### `Map<K, V>`

```buff
let scores = { "alice": 10, "bob": 20 }
print(scores.len())                       // 2
match scores["alice"]:                    // Option<V> — None if absent
    Some(n): print(n)
    None: print("missing")
scores["carol"] = 30                       // insert
let removed = scores.delete("bob")        // Option<V> — Some(20)
```

> `Map` key lookup by `m[k]` returns `Option<V>` in Buff (the Rust `HashMap`
> has no `Index` impl, so `m[k]` would be invalid Rust — Buff's codegen
> lowers it to `m.get(&k).cloned()`).

### Tuples

```buff
let pair = (1, "two")
print(pair.0)   // 1 — field access by index
print(pair.1)   // "two"
```

Tuple fields are accessed with `.0`, `.1`, etc. — no destructuring-in-binding
inside `{#each}` (the `(` would trip the directive's lexer check; see
[Chapter 5 §5.4](./chapter-5.md)).

## 6.5 Control flow and pattern matching

### `if` / `else` 🟢

```buff
if n < 0:
    print("negative")
else if n == 0:
    print("zero")
else:
    print("positive")
```

Conditions must be `Bool` ([E1205](./chapter-8.md#e1205)). `if` / `else`
branches must produce the same type if used in value position
([E1206](./chapter-8.md#e1206)).

### `for` loops

```buff
for item in collection:
    print(item)

for i in range(0, 10):
    print(i)
```

`for x in collection` works on anything iterable (Vector, range, Map keys via
`.keys()`, etc.). `range(a, b)` is half-open: `[a, b)`.

### `while` loops

```buff
let mut i = 0
while i < 10:
    print(i)
    i = i + 1
```

### `match` — exhaustive pattern matching 🟢

```buff
match value:
    Pattern1: body1
    Pattern2: body2
    _: defaultBody
```

Patterns can be:

- **Literals** — `0`, `"hello"`, `true`
- **Bindings** — `Some(x)` binds `x` to the inner value
- **Wildcards** — `_` matches anything (must be last — see
  [E1510](./chapter-8.md#e1510))
- **Destructuring** — `Ok(v)`, `Err(e)`, `Point { x, y }`

Match is **exhaustive**: the compiler verifies every possible value is covered
(T27). A non-exhaustive match is [E1207](./chapter-8.md#e1207). The compact
single-line form `match x { A => b, C => d }` is common for small arms.

```buff
match drawer.pop():
    Some(x) => print(x)
    None => print(0)
```

> **Codegen note**: `match` in *statement* form compiles end-to-end today.
> `match` in *value* position (`return match n { ... }`) is a documented
> codegen gap (arm bodies get a trailing `;` and read as `()`). Use `if` /
> `else` for value-position dispatch until that lands.

### `break` and `continue`

```buff
for i in range(0, 100):
    if i == 5:
        continue        // skip 5
    if i == 10:
        break           // stop at 10
    print(i)
```

### `return`

```buff
func first_even(v: Vector<Int>) -> Int:
    for x in v:
        if x % 2 == 0:
            return x
    return -1
```

## 6.6 Lambdas (closures)

Buff lambdas use the `{ params => body }` syntax — no `|...|` pipes, no `move`
keyword, no `Fn` / `FnMut` / `FnOnce` traits:

```buff
let double = { x => x * 2 }
let add = { x, y => x + y }
let greet = { => print("hi") }    // zero-param lambda
```

The compiler handles all capture semantics for you (T34 capture-aware codegen
inserts clones only where needed). You never write `move`, never reason about
`Fn` vs `FnMut` vs `FnOnce`.

The primary use of lambdas is with collection combinators:

```buff
[1, 2, 3].map({ x => x * 10 })
[1, 2, 3].filter({ x => x > 1 })
[1, 2, 3].fold(0, { acc, x => acc + x })
```

## 6.7 Modules and imports

Buff has two import forms:

```buff
// Local module (relative path):
import { greet } from "./greet.buff"

// Framework crate (namespace):
from "buff/web" import Web, Request, Response
```

- `import { name1, name2 } from "./path.buff"` — imports *values* (functions,
  constants) from a sibling `.buff` file. The path is relative to the current
  file.
- `from "buff/web" import Web, Request, Response` — imports *types* from a
  framework crate (workspace path). You then use them as `Web.method()`.

A `.buff` file exports names with the `export` keyword:

```buff
// greet.buff
export func greet(name: String) -> String:
    return "hello, {name}"

export func greeting_for(name: String) -> String:
    return name
```

The module graph (parsing `import` / `export`, resolving paths, visibility,
circular-import detection, re-export flattening) is fully implemented and
tested in `crates/buff-lang-types/tests/module_system.rs` (T29). End-to-end
multi-file *linking* via `buff run` is a codegen gap today (the CLI compiles
one file at a time); see [`examples/modules/`](../../examples/modules/).

### Import ordering (convention §8)

```buff
// 1. Standard library (alphabetical)
import { print } from "std/io"

// 2. External packages (alphabetical)
import { HttpServer } from "http"

// 3. Local modules (alphabetical)
import { helper } from "./utils"
```

### Visibility (convention §15)

- **Default**: module-private (no keyword needed).
- **Export**: `export func public_api()` — visible to importers.
- **Convention**: minimize the public API surface. If unsure, make it private.

## 6.8 Async — without `await`

Buff has **no `await` keyword**. Async propagates up the call graph
automatically (T31). The rules:

1. A function declared `async func` runs asynchronously and lowers to Rust's
   `async fn`.
2. Any caller of an async function is *itself* promoted to `async`
   automatically (call-graph fixpoint). You don't write `async` on the caller.
3. `main` joins the async set if any function in its call graph is async — and
   receives `#[tokio::main]` automatically.
4. `spawn task()` lowers to `tokio::spawn(async move { task() })`.
5. `task.result()` lowers to `task.await`.

```buff
async func fetch_value() -> Int:
    return 42

// `pipeline` is auto-promoted to async because it calls fetch_value.
// No `async` keyword needed here.
func pipeline() -> Int:
    return fetch_value()

func main():
    // `main` is auto-promoted to async (calls pipeline → fetch_value).
    // It receives #[tokio::main] automatically.
    let value = pipeline()
    print(value)

    // Spawn a task and await its result via .result().
    let task = spawn fetch_value()
    let answer = task.result()
    print(answer)
```

This is one of Buff's signature ergonomics wins: **~95% of user code never
knows async exists**. The function-coloring problem (sync functions can't call
async ones without ceremony) simply doesn't apply — the compiler colors for
you.

> See also: [`examples/async_demo.buff`](../../examples/async_demo.buff). The
> async model is codegen-verified; end-to-end execution requires the
> Cargo-project pipeline (multi-crate linking for tokio), deferred to a
> post-v1.0 task.

### Naming rule (convention §6)

**Never** use an `_async` suffix on async functions. Async is in the type,
not the name: `async func fetch_data()` ✅, `async func fetch_data_async()` ❌.

## 6.9 Operators and desugars

### Arithmetic and comparison

```
+  -  *  /  %        arithmetic
==  !=  <  <=  >  >= comparison
&&  ||               logical (Bool)
&  |  ^  <<  >>      bitwise (Int)
-  !  ~              unary negation / not / bit-not
```

### Three parse-time desugars

Buff has three operators that don't exist in the AST — they desugar at parse
time to existing nodes:

#### Pipeline `|>` — right-to-left function chaining

```buff
data
    |> parse
    |> validate
    |> transform
    |> serialize
```

Desugars to `serialize(transform(validate(parse(data))))`. Reads top-to-bottom
instead of inside-out.

#### Null-conditional `?.` — safe navigation

```buff
user?.profile?.email
```

Desugars to an `and_then` chain over `Option`:

```buff
user.and_then({ u => u.profile }).and_then({ p => p.email })
```

If any step is `None`, the whole expression is `None`. No `if let` boilerplate.

#### Null-coalesce `??` — `Option` unwrapping with default

```buff
config_value ?? default_value
```

Desugars to a `BinaryOp`. If the left side is `Some(x)`, yields `x`; if
`None`, yields the right side. Like Rust's `.unwrap_or(default)` but as an
operator.

## 6.10 Structs, enums, and traits

> **Status**: struct/enum/trait *declarations* are codegen-verified (see
> `crates/buff-lang-codegen-rust/tests/{struct,enum}_codegen.rs`) and the
> generated Rust re-parses via `syn`. The CLI's single-file parser does not
> yet accept these declarations at the top level — it accepts `func`
> declarations only. Matching on user-defined enum *values* is also a codegen
> gap (variants emit unqualified: `Red` not `Color::Red`). The built-in
> `Option` and `Result` enums work end-to-end. This is the v0.5 codegen gap
> tracked in `.sisyphus/notepads/buff-v05-language/issues.md`.

### Structs

```buff
struct Point:
    x: Float
    y: Float

struct Person:
    name: String
    age: Int

func main():
    let p = Point { x: 3.0, y: 4.0 }
    print(p.x)
    print(p.y)
```

Buff structs need no `#[derive]`, no `pub`, no `impl`, no `&self`. The
compiler:

- adds `Debug, Clone, PartialEq` (+ `Eq, Hash` when used in maps/sets)
  automatically,
- makes fields public by default,
- inserts clones for field access where needed (no `.clone()` litter).

### Enums

```buff
enum Shape:
    Circle(Float)
    Rectangle(Float, Float)
    Point

func describe(s: Shape) -> Int:
    match s:
        Circle(r): return 1
        Rectangle(w, h): return 2
        Point: return 0
```

Enum variants are **unqualified** in match arms — `Circle(r)`, not
`Shape::Circle(r)`. Derives are automatic.

### Traits

```buff
trait Greetable:
    func greeting(self) -> String

// (impl syntax is part of the parser-gap above; see status note.)
```

Traits define an interface; types implement them. Buff avoids class
hierarchies entirely — composition via traits + struct embedding replaces
inheritance. `Box<dyn Trait>` trait objects shipped in v1.19 (T68).

## 6.11 Attributes

Attributes start with `@` and attach to declarations:

| Attribute | Effect |
|---|---|
| `@prefer(gpu)` / `@force(gpu)` / `@prefer(cpu)` | GPU dispatch hints ([Chapter 4](./chapter-4.md)) |
| `@test` | marks a function as a test |
| `@deprecated("msg", since = "1.0")` | emits E1501 on use |
| `@internal` | marks an item as crate-internal (not for export) |
| `@bench` | marks a benchmark function |
| `@property` | defines a computed property |
| `@feature("name")` | feature-gates a declaration |
| `@no-alloc` | lint: function must perform zero heap allocations (T69) |

## 6.12 Generics

Buff supports generics with the familiar `<T>` syntax:

```buff
func first<T>(v: Vector<T>) -> Option<T>:
    if v.len() == 0:
        return None
    return Some(v[0])

struct Pair<A, B>:
    first: A
    second: B

trait Iterable<T>:
    func next(self) -> Option<T>
```

Generic parameters are `PascalCase` (single letters `T`, `K`, `V` are
conventional). Type inference fills them in at call sites in most cases;
explicit turbofish (`first::<Int>(v)`) is rarely needed.

## 6.13 What's intentionally absent

The single most important thing to understand about Buff's grammar is what it
*removes* from the C / Rust / Java tradition:

| Removed | Why | What Buff does instead |
|---|---|---|
| `class` + inheritance |耦合 + fragile base classes | structs + traits + embedding |
| `null` / `nil` | billion-dollar mistake | `Option<T>` with exhaustive match |
| `&` references + `'a` lifetimes | borrow-checker pain | owned-by-default + compiler-inserted clones |
| `await` | function coloring | auto-propagation up the call graph |
| `try` / `catch` | hidden control flow | `Result<T, E>` + `?` operator |
| `new` keyword | inconsistent construction | `Type.new()` / `Type.from()` only |
| `delete` / manual memory mgmt | use-after-free | RAII (Rust's ownership, hidden) |
| `this` / `self` receivers | method-binding ceremony | standalone functions |
| semicolons | visual noise | indentation-based blocks |
| braces `{}` for control flow | visual noise | braces reserved for *data* (struct literals, maps, lambdas) |

The result is a grammar that reads like Python but produces binaries that run
like Rust.

## 6.14 Formatting and linting

Two tools keep Buff code consistent:

- **`buff fmt`** — the formatter. Enforces 4-space indentation, 100-char line
  limit, trailing commas in multi-line collections, no trailing whitespace,
  max 2 consecutive blank lines. Deterministic: same input → same output.
- **`buff check`** — the linter + typechecker. Runs lex → parse →
  TypeInferencer → naming-lint WITHOUT codegen. Catches type errors,
  unused variables ([E1502](./chapter-8.md#e1502)), shadowing
  ([E1507](./chapter-8.md#e1507)), dead code
  ([E1506](./chapter-8.md#e1506)), and the full warning catalog.

Run both in CI. The repo's own CI runs `cargo fmt --check` + `cargo clippy
--workspace --all-targets -D warnings` + `cargo test --workspace` on a 3-OS
matrix (ubuntu / windows / macos).

---

*Next: [Chapter 7 — Stdlib Reference](./chapter-7.md)*
