+++
title = "Syntax"
weight = 10
+++

# Syntax

Buff is **layout-sensitive** (also called *offside-rule*). Indentation
defines blocks — there are no `{ }` for control flow. Braces are reserved
for *data*: struct literals, maps, lambdas, and `match` arms.

## Blocks and indentation

```buff
func greet(name: String):
    print("hello, " + name)
    if name.len() > 10:
        print("(long name!)")
    print("done")
```

Rules:

- **4 spaces** per level. Tabs are rejected by the lexer.
- The body of a block is everything that's indented deeper than the header
  line (`func ...:`, `if ...:`, `for ...:`, etc.).
- A line at the *same* indentation as the header ends the block.
- **No trailing whitespace.** No more than 2 consecutive blank lines.

## Comments

```buff
// A line comment runs to end of line.
// Buff has no /* block comments */ — write multiple // lines.

func main():
    print("hi")   // trailing comments are fine
```

Buff has only `//` line comments. If you need to comment out a large region,
your editor's "toggle line comment" command is the right tool.

## Literals

```buff
let i = 42              // Int (default integer type)
let f = 3.14            // Float (default float type)
let s = "string"        // String
let b = true            // Bool
let n = None            // Option<T>::None (type inferred from use)
let arr = [1, 2, 3]     // Vector<T>
let map = {1: "a", 2: "b"}  // Map<K, V>
let nothing = []        // Vector<Never> — empty, type filled in later
```

Integer literals support `_` separators: `1_000_000`. Float literals use `.`:
`3.14`, `6.022e23`. String literals are UTF-8 and support escape sequences
(`\n`, `\t`, `\\`, `\"`). Buff does not yet support raw strings or string
interpolation — use `print(a, b, c)` and concatenation.

## Functions

```buff
func add(a: Int, b: Int) -> Int:
    return a + b

func inferred(x):     // param + return type inferred from caller context
    return x * 2
```

- Parameters may omit types when the body unambiguously determines them.
- `-> T` declares the return type. Omitting it requires the body to be a
  single expression or end in `return`.
- `func main():` is the program entry point.

## Control flow

```buff
if x > 0:
    print("positive")
else if x < 0:
    print("negative")
else:
    print("zero")

for item in items:
    print(item)

match value {
    Some(x) => print(x),
    None    => print("empty"),
}
```

`while` is **not** in the language — use recursion or `for` over an
iterator. Buff prefers explicit iteration.

## Variables

```buff
let x = 5            // immutable binding (default)
let mut y = 10       // mutable binding
y = y + 1
```

`let` introduces a binding. `mut` opts into mutation; without it, the binding
is immutable (like Rust). There is no `var`, `val`, or `const`.

## Operators

| Category | Operators |
|---|---|
| Arithmetic | `+ - * / %` |
| Comparison | `== != < <= > >=` |
| Logical | `and or not` (keywords, not `&&`/`||`/`!`) |
| Range | `..` (e.g. `0..10`) |
| Pipeline | `\|>` (left-to-right function application) |
| Error propagate | `?` |
| Null-conditional | `?.` (desugars to `and_then`) |
| Null-coalesce | `??` (desugars to `BinaryOp`) |

The pipeline operator is a parse-time desugar:

```buff
[1, 2, 3] |> sum() |> print()
// equivalent to: print(sum([1, 2, 3]))
```

No new AST nodes are created — `|>` lowers to nested `FuncCall` during
parsing.

## Identifiers and casing

| Kind | Convention | Example |
|---|---|---|
| Types | `PascalCase` | `Vector`, `Result`, `HttpClient` |
| Functions / variables | `snake_case` | `add_one`, `total_count` |
| Constants | `SCREAMING_SNAKE` | `MAX_RETRIES` |
| Modules | `snake_case` | `net.http`, `utils` |
