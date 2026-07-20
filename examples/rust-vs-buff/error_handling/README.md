# Error Handling

## What Rust pain does Buff avoid?

Rust error handling requires significant boilerplate:

1. **`Box<dyn Error>`** -- to return different error types from a function,
   you need `Box<dyn Error>` or a custom enum. Both add complexity.
2. **`impl Display` + `impl Error`** -- custom error types must implement
   both `Display` and `std::error::Error`. That's at least 15 lines of
   boilerplate per error type.
3. **`.map_err()` chains** -- converting between error types in a call
   chain creates deeply nested closures: `.map_err(|e| Box::new(...))`.

## The Buff equivalent

Buff provides a **builtin `Error` type**. `Error("message")` constructs an
error directly. The `?` operator works exactly like Rust's. No `impl` blocks,
no `Box<dyn Error>`, no trait implementations. Just `return Error("msg")`.

## Key differences

| Rust | Buff |
|---|---|
| `Box<dyn Error>` return type | `Error` (builtin) |
| `impl Display for MyErr { ... }` | Not needed |
| `impl std::error::Error for MyErr { }` | Not needed |
| `.map_err(\|e\| Box::new(...))` | `return Error("msg")` |
| `Err(MyErr("...".into()))` | `Error("...")` |
| `Ok(value)` | `Ok(value)` (same) |
