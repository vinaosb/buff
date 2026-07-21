# The Builtin `Error` Type

Buff ships a single builtin error type — written `Error` — that covers most needs. You construct one with the call-like syntax `Error("message")`, which lowers to `Err(Error::new("message"))` in the generated Rust and implements `std::error::Error` automatically.

```buff
func divide(a: Int, b: Int) -> Result<Int, Error>:
    if b == 0:
        return Error("divide by zero")
    return Ok(a / b)
```

Key facts:
- The return type is `Result<T, Error>` — `Error` is the *value* constructor, not a type parameter.
- Custom user-defined error *enums* (e.g. `enum MyErr { NotFound }`) are a codegen gap in v0.5; use the builtin `Error` for now.
- The error message is a plain `String`. It surfaces verbatim if you print it via `match ... { Err(e) => print(e), ... }`.

## Your task

Define `divide(a: Int, b: Int) -> Result<Int, Error>` that returns `Error("divide by zero")` when `b == 0`, else `Ok(a / b)`. In `main`, call `divide(10, 2)` (Ok(5)) and `divide(10, 0)` (error) and print each result.

**Hint:** the error branch is `return Error("divide by zero")` (no `Ok` wrapper, no `Err` keyword — `Error(...)` IS the error constructor). The success branch is `return Ok(a / b)`.
