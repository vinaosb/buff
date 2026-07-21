# Result<T, E> and the `?` Operator

Buff uses `Result<T, E>` for operations that can fail. A `Result` is either:

- `Ok(value)` — success carrying a `T`
- `Error("message")` — failure carrying the builtin `Error` type (lowers to `Err(Error::new(...))` in Rust)

The `?` operator propagates errors automatically: `expr?` unwraps the `Ok` value on success, or **immediately returns the `Err`** from the enclosing function.

```buff
func half(n: Int) -> Result<Int, Error>:
    if n < 2:
        return Error("too small")
    return Ok(n / 2)

func add_one(n: Int) -> Result<Int, Error>:
    let h = half(n)?
    return Ok(h + 1)
```

In `add_one`, if `half(n)` errors, `?` short-circuits and the error flows out as `add_one`'s return value. No `try`/`catch` keyword — Buff has neither.

## Your task

Implement `parse_small(n: Int) -> Result<Int, Error>` that returns `Error("too small")` when `n < 10`, otherwise `Ok(n)`. Call it on `5` and `20` and match each result with the brace form: `match parse_small(5) { Ok(v) => print(v), Err(_) => print(0) }`.

**Hint:** the function header is `func parse_small(n: Int) -> Result<Int, Error>:`. Inside, use a plain `if/return` for the error path and a final `return Ok(n)` for success.
