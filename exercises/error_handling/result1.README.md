# Composing Fallible Functions with `?`

Buff has no `try`/`catch`. Errors flow through `Result<T, E>` return types, and the `?` operator lets a function **propagate** an error from any nested call without writing an explicit `match` every time.

```buff
func parse_int(s: String) -> Result<Int, Error>:
    if s == "42":
        return Ok(42)
    return Error("not 42")

func parse_and_double(s: String) -> Result<Int, Error>:
    let n = parse_int(s)?     // unwraps Ok OR returns Err from this fn
    return Ok(n * 2)
```

The `?` reads as "unwrap or return early with the error". The enclosing function's return type MUST also be a `Result<_, E>` (the error type propagates unchanged). On the happy path, `n` is the unwrapped `Int` and the next line runs normally.

This is how Buff avoids `await`-style coloring: `?` is one character of ceremony instead of a whole new calling convention.

## Your task

Define `parse_and_double(s: String) -> Result<Int, Error>` between `parse_int` and `main`. Inside:
1. `let n = parse_int(s)?` — unwrap-or-propagate.
2. `return Ok(n * 2)`.

In `main`, match each result: `match parse_and_double("42") { Ok(v) => print(v), Err(_) => print(0) }`. Expected output: `84` then `0`.

**Hint:** the magic is just the single `?` character between `parse_int(s)` and the `let`. Everything else is a normal `Ok(...)` return.
