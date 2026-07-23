+++
title = "Errors"
weight = 47
+++

# Error-handling recipes

Recipes for the `Result<T, E>` / `Option<T>` model. Buff has no
`try`/`catch`/`throw`; errors are values. The full reference lives
at [Language → Error handling](../language/error-handling/).

## Use `?` to propagate errors

**Problem**: Stop work early when a fallible call fails, without
writing a match at every call site.

**Solution**:

```buff
func half(n: Int) -> Result<Int, Error>:
    if n < 2:
        return Error("input too small")
    return Ok(n / 2)

func add_one(n: Int) -> Result<Int, Error>:
    let h = half(n)?
    return Ok(h + 1)

func main():
    match add_one(10) {
        Ok(v)  => print(v),
        Err(_) => print(0),
    }
```

**Explanation**:

The `?` operator after a `Result<T, E>` value unwraps the `Ok` on
success or returns the `Err` from the enclosing function immediately.
The function's return type must be `Result<T, E>` (or `Option<T>` —
`?` propagates `None` the same way).

`half(n)?` is exactly `match half(n) { Ok(v) => v, Err(e) => return
Err(e) }`. Chains of `?` reads like straight-line code:

```buff
let text = File.read(path)?
let cfg = Toml.parse(text)?
let port = cfg.get("port")?
```

Every `?` is a potential early return; the caller of the outermost
function decides what to do with the propagated error.

## Match on Option

**Problem**: Branch on the presence or absence of a value.

**Solution**:

```buff
func find_name(scores: Map<String, Int>, key: String) -> String:
    match scores.get(key) {
        Some(n) => return "name " + n.string(),
        None    => return "missing"
    }

func main():
    let scores = {"alice": 10, "bob": 20}
    print(find_name(scores, "alice"))
    print(find_name(scores, "carol"))
```

**Explanation**:

`Map.get(key)` returns `Option<V>` — `Some(value)` when present,
`None` when absent. `match` is exhaustive; the compiler rejects a
match that doesn't cover both variants (or has a `_` catch-all).
This is the only way to read an `Option`'s inner value — there's no
`unwrap` exposed to user code (Buff's prelude doesn't surface it).

For the "default on None" case, use `??` (null-coalesce):

```buff
let n = scores.get(key) ?? 0
```

`a ?? b` desugars to `match a { Some(v) => v, None => b }`. It's the
one-liner equivalent of the recipe above when you don't need to log
or branch on the absence.

## Define a custom error type

**Problem**: Surface a typed error from your domain (not the generic
`Error`).

**Solution**:

```buff
enum ApiError:
    NotFound
    RateLimited(retry_after: Int)
    Internal(message: String)

func fetch(id: Int) -> Result<String, ApiError>:
    if id == 0:
        return ApiError.NotFound
    return Ok("user " + id.string())

func main():
    match fetch(0) {
        Ok(body)          => print(body),
        Err(NotFound)     => print("no such user"),
        Err(RateLimited(r)) => print("retry in " + r.string() + "s"),
        Err(Internal(m))  => print("oops: " + m),
    }
```

**Explanation**:

A custom `enum` with payload-carrying variants (`RateLimited(retry_after:
Int)`) is Buff's typed-error mechanism. The `enum` lowers to a Rust
`enum` with the same variants; the match arms destructure the payload
inline.

> **Known gap (v1.x):** custom error *enums* are codegen-verified
> but do not yet compile end-to-end — generated Rust refers to a
> variant as `NotFound` rather than `ApiError::NotFound`. Use the
> builtin `Error` type (which DOES resolve) until the v1.13+ codegen
> fix lands. Tracked in the v0.5 codegen-gap notepad.

## Retry on failure

**Problem**: Re-run a fallible call up to N times before giving up.

**Solution**:

```buff
func fetch_or_retry(url: String, max_tries: Int) -> Result<String, Error>:
    var attempt: Int = 0
    var last: Error = Error("no attempts made")
    while attempt < max_tries:
        let client = HttpClient.new()
        let result = client.get(url).send()
        match result {
            Ok(resp) => return Ok(resp.text()),
            Err(e)   => last = e
        }
        attempt = attempt + 1
    return last

func main():
    match fetch_or_retry("https://flaky.example", max_tries: 3) {
        Ok(body) => print(body),
        Err(e)   => print("gave up: " + e.string()),
    }
```

**Explanation**:

The loop tries the call, returns on success, records the error on
failure. After `max_tries` attempts, the last error is propagated.
For exponential backoff between attempts, see
[HTTP → Retry with exponential backoff](./http/#retry-with-exponential-backoff).

For production use, `buff-resilience` (T36) ships `Retry` with jitter,
circuit-breaker integration, per-attempt timeouts, and policy objects
that can be serialised to disk. The 10-line loop above is the right
shape for one-off scripts; reach for `Retry` when you need observability.

## Convert between error types

**Problem**: Take an error of one type and surface it as another
(e.g. wrap an `IoError` from `File.read` as your app's `AppError`).

**Solution**:

```buff
func load(path: String) -> Result<String, Error>:
    let text = File.read(path)?
    return Ok(text)

func main():
    match load("/maybe/missing") {
        Ok(body) => print(body),
        Err(e)   => print("load failed: " + e.string()),
    }
```

**Explanation**:

The builtin `Error` type is the universal "any error" — every fallible
stdlib call that doesn't have its own typed error returns
`Result<T, Error>`. When your function returns `Result<T, Error>`,
the `?` operator converts automatically (every domain error type has
a `From<_, Error>` impl at the codegen layer).

For a function that returns `Result<T, YourError>`, you'd write an
explicit conversion at the `?` site:

```buff
let text = File.read(path).map_err({ e => YourError.Io(e) })?
```

`Result.map_err(f)` is the standard combinator — it applies `f` to
the `Err` side and leaves `Ok` untouched. Buff's prelude mirrors
Rust's `Result` method names (`map`, `map_err`, `and_then`, `or_else`,
`unwrap_or`, `is_ok`, `is_err`).

## Chain fallible operations

**Problem**: Sequence several fallible calls, propagating any failure.

**Solution**:

```buff
func load_config(path: String) -> Result<String, Error>:
    let text = File.read(path)?
    let cfg = Toml.parse(text)?
    let port = cfg.get("port")?
    return Ok(port)

func main():
    match load_config("app.toml") {
        Ok(port) => print("listening on " + port),
        Err(e)   => print("config error: " + e.string()),
    }
```

**Explanation**:

Each `?` is a potential early return; if any step fails, the function
returns the error without running subsequent steps. The caller sees
one `Result<String, Error>` covering the whole pipeline — no nested
matches, no error-swallowing.

The pipeline operator `|>` reads naturally for this shape:

```buff
let port = path |> File.read() |> Toml.parse() |> _.get("port")
```

`|>` is a parse-time desugar to nested function calls (left-to-right
application). With `?`, you'd write `File.read(path)? |> Toml.parse()?
|> _.get("port")?` — each step propagates its failure. The pipeline
form is sugar; the desugared form is what the compiler actually sees.
