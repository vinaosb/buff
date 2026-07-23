+++
title = "Error handling"
weight = 40
+++

# Error handling

Buff has no `try` / `catch` / `throw`. Errors are **values**, returned as
`Result<T, E>`. This is identical to Rust's `std::result::Result`, and
muscle-memory transfers 1:1.

## The `Result<T, E>` type

```buff
func half(n: Int) -> Result<Int, Error>:
    if n < 2:
        return Error("input too small")
    return Ok(n / 2)
```

- `Ok(value)` constructs a success.
- `return Error("msg")` lowers to `return Err(Error::new("msg"))` — the
  builtin `Error` struct + `std::error::Error` impl are emitted on demand.
- The type parameters are inferred where possible.

## The `?` operator

The `?` operator propagates an error early. `f()?` unwraps the `Ok` value
on success, or returns the `Err` from the enclosing function immediately
on failure:

```buff
func add_one(n: Int) -> Result<Int, Error>:
    let h = half(n)?       // unwrap on success, propagate on Err
    return Ok(h + 1)
```

This is identical to Rust's `?`. You can only use `?` inside a function
that returns `Result<T, E>` (or `Option<T>`).

## Matching on `Result`

```buff
func main():
    let good = add_one(10)
    match good { Ok(v) => print(v), Err(_) => print(0) }

    let bad = add_one(1)
    match bad { Ok(v) => print(v), Err(_) => print(0) }
```

`match` is exhaustive — the compiler rejects a match that doesn't cover
every variant. The `_` wildcard catches anything you don't care about.

## Combinators

Buff's `Result` and `Option` methods mirror Rust so muscle memory
transfers:

```buff
let n = parse_int(s).unwrap_or(0)
let doubled = parse_int(s).map({ x => x * 2 })
let ok = parse_int(s).is_ok()
```

See convention §13 in the repo's conventions doc for the full list. Buff
deliberately uses Rust-compatible method names rather than inventing its
own (`is_some`, `is_ok`, `unwrap_or`, `map`, `and_then`, …).

## The builtin `Error` type

The prelude provides a generic `Error` struct you can construct on the
fly:

```buff
return Error("file not found: " + path)
```

The compiler lowers this to:

```rust
return Err(Error::new(format!("file not found: {}", path)));
```

The `Error` struct implements `std::error::Error` and is emitted into the
generated Rust on demand (only if you actually use it).

## Custom error enums

```buff
enum ApiError:
    NotFound
    RateLimited(retry_after: Int)
    Internal(message: String)

func fetch(id: Int) -> Result<Data, ApiError>:
    if id == 0:
        return ApiError.NotFound
    ...
```

> **Known gap (v1.x):** custom error *enums* are codegen-verified but do
> not compile end-to-end — generated Rust refers to a variant as `NotFound`
> rather than `ApiError::NotFound`. Use the builtin `Error` type for now,
> or wait for the v1.13+ codegen fix. Tracked in the v0.5 codegen-gap
> notepad.

## Error codes (compiler diagnostics)

Compiler diagnostics carry **stable error codes** (`E10xx` lex, `E11xx`
parse, `E12xx` type, `E13xx` codegen). The full catalog lives at
[`docs/errors/`][errors] with one HTML page per code.

[errors]: https://github.com/buff-lang/buff/tree/master/docs/errors

These codes are a *stability contract* (convention §19): once a code is
shipped, it is **never** renumbered, reused, or silently removed. Old
codes may be marked deprecated, but they keep their number.

## Pattern: fallible chains

```buff
func load_config(path: String) -> Result<Config, Error>:
    let text = File.read(path)?           // Result<String, Error>
    let cfg = parse_toml(text)?           // Result<Config, Error>
    return Ok(cfg)
```

Every `?` is a potential early return. The caller of `load_config` sees a
single `Result<Config, Error>` and decides what to do with failure —
propagate further with another `?`, or `match` on both arms.
