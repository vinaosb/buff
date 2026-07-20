# Functions

## What Rust pain does Buff avoid?

Rust functions are relatively clean, but still have friction:

1. **Explicit type annotations on every parameter** -- `name: &str`, `a: i64`,
   `b: i64`. Buff infers most types but still requires parameter types.
2. **`format!()` macro** -- string concatenation in Rust uses `format!()` or
   `format!("{}{}", a, b)`. Buff uses `+` for string concatenation directly.
3. **No default parameters** -- Rust has no built-in default parameter values.
   You need `Option<T>` and unwrap_or, or a builder struct. Buff supports
   default values on parameters.
4. **Semicolons** -- every statement in Rust needs `;`. Buff uses
   indentation-based blocks (offside rule) instead.
5. **Braces on every block** -- `if n <= 1 { ... }` even for single-line
   bodies. Buff uses indentation for blocks.

## The Buff equivalent

Buff functions use `func name(params) -> RetType:` with indented bodies.
Named arguments are required for clarity: `fetch(url, cache: true)` instead
of positional booleans. String concatenation uses `+`.

## Key differences

| Rust | Buff |
|---|---|
| `fn greet(name: &str) -> String { ... }` | `func greet(name: String) -> String:` |
| `format!("Hello, {}!", name)` | `"Hello, " + name + "!"` |
| `{ a, b }` braces on every block | Indentation-based blocks |
| No default parameters | Default parameter values supported |
| Positional args: `connect("h", 80)` | Named args: `connect(host: "h", port: 80)` |
