+++
title = "Types"
weight = 20
+++

# Types

Buff is **statically typed** with aggressive inference. You rarely write type
annotations — the compiler figures them out from context. When inference is
ambiguous, an explicit annotation resolves it.

## Primitives

| Type | Rust lowering | Notes |
|---|---|---|
| `Int` | `i64` | default integer type |
| `Float` | `f64` | default float type |
| `Bool` | `bool` | `true` / `false` |
| `String` | `String` | owned UTF-8 |
| `Char` | `char` | 4-byte Unicode scalar |

Narrow integer types (`i8`, `u32`, `usize`, etc.) exist for FFI interop but
are rarely written in user code — `Int` is almost always what you want.

## Collections

| Type | Literal | Rust lowering |
|---|---|---|
| `Vector<T>` | `[1, 2, 3]` | `Vec<T>` |
| `Map<K, V>` | `{1: "a", 2: "b"}` | `HashMap<K, V>` |
| `Set<T>` | *(from stdlib)* | `HashSet<T>` |
| `Tuple<T, U>` | `(1, "two")` | `(T, U)` |

```buff
let v = [10, 20, 30, 40]
print(v[0])              // 10
print(v.len())           // 4

let mut stack = [1, 2, 3]
stack.push(4)
let top = stack.pop()    // Option<Int>
match top { Some(x) => print(x), None => print(0) }

let scores = {1: 10, 2: 20}
print(scores.len())      // 2
```

Small integer literals infer the narrowest width that fits — `[10, 20, 30]`
starts as `Vector<i8>` and widens as needed by indexing context.

## `Option<T>`

Absence is a value, not a null pointer. `Option<T>` is Rust's `Option<T>`
under the hood.

```buff
let maybe = find_user(id)        // Option<User>
match maybe {
    Some(u) => print(u.name),
    None    => print("no such user"),
}

// Null-conditional: `?.` desugars to and_then
let name = find_user(id)?.name
```

There is **no `null`** in Buff. The compiler refuses to emit `Option::unwrap`
on a `None`; you must match or use `?.`.

## `Result<T, E>`

Errors are values. A fallible function returns `Result<T, E>`:

```buff
func parse_int(s: String) -> Result<Int, Error>:
    ...
```

See [Error handling](../error-handling/) for the full model.

## Structs

```buff
struct Point:
    x: Int
    y: Int

func main():
    let p = Point.new(x: 3, y: 4)
    print(p.x + p.y)        // 7
```

Structs are constructed with `Type.new(...)`. Named arguments are required
for clarity — Buff rejects positional struct construction.

## Enums

```buff
enum Color:
    Red
    Green
    Blue

func name(c: Color) -> String:
    match c {
        Color.Red   => "red",
        Color.Green => "green",
        Color.Blue  => "blue",
    }
```

> **Known gap (v1.x):** enum *value* matching is codegen-verified but does
> not yet compile end-to-end — generated Rust refers to a variant as `Red`
> rather than `Color::Red`. The built-in `Option` and `Result` enums work
> correctly. Tracked in the v0.5 codegen-gap notepad.

## Traits

```buff
trait Shape:
    func area(self) -> Float

struct Circle:
    radius: Float

impl Shape for Circle:
    func area(self) -> Float:
        return 3.14 * self.radius * self.radius
```

Traits are Rust traits. Buff's borrow checker is hidden, but the trait system
is exposed — you write `impl Trait for Type` blocks like in Rust.

## Type inference

```buff
let x = 5                // Int
let y = x + 1.0          // Float (x widens)
let v = [1, 2, 3]        // Vector<Int>
let first = v[0]         // Int
let doubled = v.map({ n => n * 2 })   // Vector<Int>
```

The inferencer runs as a call-graph fixpoint: a `let` may pick up its type
from how it's used later. If a binding's type is genuinely ambiguous, add an
explicit annotation:

```buff
let counter: Int = 0
```

## Generics

```buff
func identity<T>(x: T) -> T:
    return x

func main():
    let s = identity("hello")     // String
    let n = identity(42)          // Int
```

Generics use `<T>` syntax identical to Rust. The transpiler preserves them
through to the generated Rust.
