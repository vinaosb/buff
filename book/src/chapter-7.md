# Chapter 7 — Stdlib Reference

The Buff standard library is **implicit** — every program gets it without an
`import`. It splits into two registries in
[`crates/buff-lang-types/src/`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-lang-types/src):

- **`prelude.rs`** — free functions (`print`, `abs`, `args`, ...). Called by
  bare identifier.
- **`prelude_types.rs`** — types with associated functions and instance
  methods (`DateTime.now()`, `dt.format(...)`, `Regex.compile(...)`). Called
  via `Type.method()` or `recv.method()`.

This chapter documents both. It's reference material — skim the tables, read
the detail when you need a specific function.

## 7.1 Free functions (the `prelude.rs` registry)

These are implicitly in scope in every Buff program. Grouped by category.

### Math

| Function | Signature | Notes |
|---|---|---|
| `abs(x)` | `(T) -> T` | absolute value; polymorphic over numeric arg |
| `min(a, b)` | `(T, T) -> T` | minimum; returns promoted type |
| `max(a, b)` | `(T, T) -> T` | maximum; returns promoted type |
| `sqrt(x)` | `(T) -> Float` | square root; always `Float` |
| `floor(x)` | `(T) -> Float` | round toward −∞ |
| `ceil(x)` | `(T) -> Float` | round toward +∞ |
| `round(x)` | `(T) -> Float` | nearest, ties away from zero |
| `pow(base, exp)` | `(T, T) -> T` | exponentiation; returns type of `base` |

### Type conversions

| Function | Signature | Notes |
|---|---|---|
| `Int(x)` | `(T) -> Option<Int>` | convert to `Int<64>`; `None` on failure |
| `Float(x)` | `(T) -> Option<Float>` | convert to `Float<32>` |
| `String(x)` | `(T) -> String` | convert to `String` |
| `Bool(x)` | `(T) -> Option<Bool>` | convert to `Bool` |

The conversion functions return `Option<T>` (or `String` for `String()`, which
always succeeds) so you can `.or(default: ...)` on parse failure.

### I/O

| Function | Signature | Notes |
|---|---|---|
| `print(x)` | `(T) -> Void` | print `x` + newline (maps to Rust `println!`) |
| `println(x)` | `(T) -> Void` | same as `print` (both append newline) |
| `read_line()` | `() -> String` | read one line from stdin (newline trimmed) |
| `input(prompt)` | `(String) -> String` | print prompt, then read one line |

### System / environment

| Function | Signature | Notes |
|---|---|---|
| `args()` | `() -> Vector<String>` | command-line arguments (`argv[0]` = program) |
| `env("NAME")` | `(String) -> Option<String>` | environment variable lookup |
| `exit(code)` | `(Int) -> Void` | terminate process with exit code |
| `sleep(duration)` | `(Duration) -> Void` | async-transparent sleep (no `await`) |

### Testing

| Function | Signature | Notes |
|---|---|---|
| `assert_eq(a, b)` | `(T, T) -> Void` | assert equality; panics with diff on failure |
| `assertThat(value)` | `(T) -> AssertThat<T>` | fluent assertion wrapper (T38) |

`assertThat(x).isEqualTo(y)`, `.isGreaterThan(n)`, etc. — see the
`buff-assertions` crate.

## 7.2 Prelude types (the `prelude_types.rs` registry)

These resolve as built-in types via name lookup (like `Option` and `Result`).
They are **not** reserved keywords — shadowing `DateTime` with a user type is
your responsibility.

### DateTime family (chrono-backed)

| Type | Associated functions | Instance methods |
|---|---|---|
| `DateTime` | `.now()`, `.now_utc()`, `.parse(s)`, `.from_timestamp(ts)` | `.format(fmt)`, `.year()`, `.month()`, `.day()`, `.hour()`, `.minute()`, `.second()`, `.timestamp()` |
| `Date` | `.today()`, `.parse(s)` | `.format(fmt)`, `.year()`, `.month()`, `.day()` |
| `Time` | `.now()`, `.parse(s)` | `.format(fmt)`, `.hour()`, `.minute()`, `.second()` |
| `Duration` | `.milliseconds(n)`, `.seconds(n)`, `.minutes(n)`, `.hours(n)`, `.days(n)` | *(arithmetic via operators)* |
| `Instant` | `.now()` | `.elapsed()`, `.elapsed_secs()` |

```buff
func main():
    let now = DateTime.now()
    print(now.format("%Y-%m-%d %H:%M:%S"))
    print(now.year())

    let seven_days = Duration.days(7)
    sleep(seven_days)        // (don't actually do this)

    let t0 = Instant.now()
    // ... do work ...
    print("elapsed: {t0.elapsed_secs()}s")
```

Format strings follow [`chrono`'s `strftime` spec][chrono-fmt].

[chrono-fmt]: https://docs.rs/chrono/latest/chrono/format/strftime/index.html

### Serialization namespaces

Each is a *namespace-only* prelude type (no runtime value) exposing `.parse()`
and `.stringify()`:

| Type | `.parse(s)` returns | `.stringify(v)` returns | Backing crate |
|---|---|---|---|
| `Json` | `Result<Map<String, Unknown>, Error>` | `String` | `serde_json` |
| `Toml` | `Result<Map<String, Unknown>, Error>` | `String` | `toml` |
| `Yaml` | `Result<Map<String, Unknown>, Error>` | `String` | `serde_yaml` |
| `Csv` | `Result<Vector<Map<String, String>>, Error>` | `String` | `csv` |

```buff
func main():
    let text = "{ \"name\": \"Ada\", \"age\": 36 }"
    match Json.parse(text):
        Ok(map):
            print(map["name"])           // Some("Ada")
        Err(e):
            print("parse failed: {e}")

    let data = { "greeting": "hello", "count": 7 }
    print(Json.stringify(data))
```

### `Regex` — compiled regular expressions

| Method | Returns | Notes |
|---|---|---|
| `Regex.compile(pattern)` | `Regex` | associated function; compile once, use many |
| `regex.match(text)` | `Option<...>` | does the text match? |
| `regex.find(text)` | `Option<String>` | first match |
| `regex.replace(text, repl)` | `String` | replace all matches |
| `regex.captures(text)` | `Map<String, String>` | named + numbered capture groups |

```buff
func main():
    let email_re = Regex.compile(r"^[a-z]+@[a-z]+\.[a-z]+$")
    match email_re.match("ada@lovelace.org"):
        Some(_): print("valid email")
        None: print("invalid email")
```

The `re"..."` literal syntax compiles a regex at parse time (see
[§6.1](./chapter-6.md)).

### `Log` — structured logging (tracing-backed)

Namespace-only; never a runtime value. Four levels, each a macro-call
lowering to `tracing::<level>!`:

| Method | Lowers to |
|---|---|
| `Log.debug(msg, ...)` | `tracing::debug!(...)` |
| `Log.info(msg, ...)` | `tracing::info!(...)` |
| `Log.warn(msg, ...)` | `tracing::warn!(...)` |
| `Log.error(msg, ...)` | `tracing::error!(...)` |

```buff
func main():
    Log.info("server started on port {port}", port: 8080)
    Log.warn("cache miss for key {key}", key: "user:42")
    Log.error("failed to connect: {error}", error: "timeout")
```

Structured logging with interpolation — no format strings.

### `Math` namespace + constants

| Constant | Value |
|---|---|
| `Math.PI` | π (f64) |
| `Math.E` | e (f64) |

| Function | Signature |
|---|---|
| `Math.sqrt(x)` | `(Float) -> Float` |
| `Math.sin(x)`, `.cos(x)`, `.tan(x)` | `(Float) -> Float` |
| `Math.asin(x)`, `.acos(x)`, `.atan(x)` | `(Float) -> Float` |
| `Math.log(x)`, `.log10(x)` | `(Float) -> Float` |
| `Math.exp(x)` | `(Float) -> Float` |
| `Math.pow(base, exp)` | `(Float, Float) -> Float` |

### `Random` namespace (rand-backed)

| Function | Returns | Notes |
|---|---|---|
| `Random.int(min, max)` | `Int` | inclusive range |
| `Random.float()` | `Float` | `[0.0, 1.0)` |
| `Random.choice(items)` | `Option<T>` | random element |
| `Random.shuffle(items)` | `Vector<T>` | shuffled copy |

### Crypto and hashing

| Type | Functions |
|---|---|
| `Hash` | `.sha256(data)`, `.sha512(data)`, `.md5(data)` — return `String` (hex) |
| `HMAC` | `.sha256(key, data)` — keyed-hash MAC |
| `Base64` | `.encode(bytes)`, `.decode(s)` |
| `Hex` | `.encode(bytes)`, `.decode(s)` |
| `URLEncode` | `.encode(s)`, `.decode(s)` — percent-encoding |
| `UUID` | `.v4()`, `.v7()`, `.parse(s)` |
| `RsaKeypair` | `.generate(bits)` — returns keypair; `.public_pem()`, `.private_pem()` |

```buff
func main():
    let digest = Hash.sha256("hello, world")
    print(digest)   // 64-char hex string
    let id = UUID.v4()
    print(id)
```

### Filesystem

| Type | Functions |
|---|---|
| `Path` | `.join(other)` (assoc), `.parent()`, `.extension()`, `.basename()`, `.exists()` (instance) |
| `Dir` | `.list(path)`, `.create(path)`, `.remove(path)`, `.walk(path)` |
| `Tempfile` | `.create()`, `.dir()` |

### Networking

| Type | Functions |
|---|---|
| `TCP` | `.connect(addr)` → Connection with `.send()`, `.recv()`, `.close()` |
| `UDP` | `.bind(addr)` → Socket with `.send_to()`, `.recv_from()` |
| `WebSocket` | `.connect(url)` → WsConnection with `.send()`, `.recv()`, `.close()` |
| `URL` | `.parse(s)` (assoc), `.scheme()`, `.host()`, `.path()`, `.query()` (instance) |

### Process control

| Type | Functions |
|---|---|
| `Process` | `.spawn(cmd, args)` (assoc), `.wait()`, `.id()` (instance) |
| `OS` | `.name()`, `.arch()`, `.hostname()`, `.cpus()` |
| `Args` | `.list()`, `.get(i)` — alternative to the `args()` free function |
| `Env` | `.get(name)`, `.set(name, value)`, `.has(name)` — alternative to `env()` |

### Concurrency

| Type | Functions |
|---|---|
| `Channel<T>` | `.new()` → `(Sender<T>, Receiver<T>)` MPSC pair |

`Channel.send(v)` returns `Result<Void, Error>` ([E1407](./chapter-8.md#e1407)
if all receivers dropped); `Channel.receive()` returns `Result<T, Error>`
([E1408](./chapter-8.md#e1408) if all senders dropped). See
[`examples/channels/`](../../examples/channels/).

## 7.3 Instance methods on built-in types

These are the methods you call on `String`, `Vector<T>`, `Map<K, V>`,
`Option<T>`, and `Result<T, E>` values. They're registered in
[`prelude_instance_fn_impl.rs`](https://github.com/buff-lang/buff/blob/v1x-frameworks/crates/buff-lang-types/src/prelude_instance_fn_impl.rs).

### `String`

| Method | Returns | Notes |
|---|---|---|
| `s.split(sep)` | `Vector<String>` | split on separator |
| `s.trim()` | `String` | strip leading/trailing whitespace |
| `s.starts_with(prefix)` | `Bool` | |
| `s.ends_with(suffix)` | `Bool` | |
| `s.to_upper()` | `String` | uppercase |
| `s.to_lower()` | `String` | lowercase |
| `s.contains(sub)` | `Bool` | substring test |
| `s.replace(from, to)` | `String` | replace all occurrences |
| `s.len()` | `Int` | byte length |

### `Vector<T>`

| Method | Returns | Notes |
|---|---|---|
| `v.len()` | `Int` | element count |
| `v.push(x)` | `Void` | append (requires `mut`) |
| `v.pop()` | `Option<T>` | remove + return last |
| `v.map(fn)` | `Vector<U>` | transform (move-by-default) |
| `v.filter(fn)` | `Vector<T>` | keep matching (codegen-verified) |
| `v.fold(acc, fn)` | `U` | reduce (codegen-verified) |
| `v.iter()` | `Iterator<T>` | borrow iterator |
| `v[i]` | `T` | index (coerces `i` to `usize`) |

### `Map<K, V>`

| Method | Returns | Notes |
|---|---|---|
| `m.len()` | `Int` | entry count |
| `m.get(key)` / `m[key]` | `Option<V>` | lookup |
| `m[key] = value` | `Void` | insert |
| `m.delete(key)` | `Option<V>` | remove + return |
| `m.keys()` | `Vector<K>` | all keys |
| `m.values()` | `Vector<V>` | all values |
| `m.entries()` | `Vector<(K, V)>` | all pairs |

### `Option<T>`

| Method | Returns | Notes |
|---|---|---|
| `opt.unwrap()` | `T` | panic if `None` |
| `opt.unwrap_or(default)` / `.or(default: ...)` | `T` | fallback |
| `opt.unwrap_or_else(fn)` | `T` | lazy fallback |
| `opt.is_some()` | `Bool` | |
| `opt.is_none()` | `Bool` | |
| `opt.map(fn)` | `Option<U>` | transform inner |
| `opt.filter(fn)` | `Option<T>` | keep if predicate holds |

### `Result<T, E>`

| Method | Returns | Notes |
|---|---|---|
| `result.unwrap()` | `T` | panic if `Err` |
| `result.unwrap_or(default)` | `T` | fallback |
| `result.unwrap_or_else(fn)` | `T` | lazy fallback |
| `result.is_ok()` | `Bool` | |
| `result.is_err()` | `Bool` | |
| `result.map(fn)` | `Result<U, E>` | transform Ok |
| `result.map_err(fn)` | `Result<T, F>` | transform Err |
| `result.and_then(fn)` | `Result<U, E>` | flatmap Ok |
| `result.or_else(fn)` | `Result<T, F>` | flatmap Err |

## 7.4 `Assert` — the testing prelude

Two styles of assertion, both available without import:

### `assert_eq(a, b)` — direct equality

```buff
@test
func test_addition():
    assert_eq(2 + 2, 4)
```

Panics with a diff if `a != b`. Maps to Rust's `assert_eq!` macro.

### `assertThat(value)` — fluent (T38)

```buff
@test
func test_list():
    assertThat([1, 2, 3])
        .isEqualTo([1, 2, 3])
        .isNotEmpty()

    assertThat(42)
        .isGreaterThan(40)
        .isLessThan(50)
```

The `assertThat` wrapper (from the `buff-assertions` crate) exposes a fluent
chain. Failure messages include the failing value and the predicate.

## 7.5 Collections deep-dive

### `Vector<T>` pitfalls

- **`.map()` consumes the vector** (move-by-default, like all of Buff). Each
  `.map()` call starts from a fresh literal or a `clone()` if you need to
  reuse the source.
- **Indexing coerces** the index to `usize` for you — `v[-1]` is a runtime
  panic (Buff has no negative indexing like Python).
- **`.pop()` returns `Option<T>`** — never panics. Match on it or `.unwrap()`
  if you're sure.

### `Map<K, V>` pitfalls

- **`m[k]` returns `Option<V>`**, not `V`. This is because Rust's `HashMap`
  has no `Index` impl; Buff lowers `m[k]` to `m.get(&k).cloned()`. Use
  `m[k].or(default: ...)` to unwrap.
- **`m[k] = v`** is insert (not update-or-insert — same thing for maps).
- **Iteration order is unspecified** (HashMap, not BTreeMap). If you need
  sorted keys, `.keys()` then `.sort()`.

## 7.6 `buff-db` — database access 🔶

The `buff-db` framework crate (T18, v1.15) provides database access. It wraps
a pure-Rust driver (no libpq / diesel C dependencies — matches the "no C
library" hard rule). The surface:

```buff
from "buff/db" import Connection

func main():
    let conn = Connection.new("postgres://localhost/mydb")
    let rows = conn.query("SELECT id, name FROM users WHERE active = $1", [true])
    for row in rows:
        let id = row["id"]
        let name = row["name"]
        print("{id}: {name}")
```

> 🔶 The `buff-db` Rust crate is shipped with full test coverage; the Buff-side
> surface is a forward-declaration pending the coordinated codegen lowering
> arm. The Rust API is stable; see
> [`crates/buff-db/AGENTS.md`](../../crates/buff-db/AGENTS.md).

Supported backends: PostgreSQL (via `tokio-postgres`), SQLite (via
`rusqlite`). MySQL is deferred.

## 7.7 Framework crate index

Beyond the core stdlib, Buff ships 44+ framework crates (v1.13–v1.23). Each
is a thin safe wrapper over a canonical Rust crate, following the
[FFI safety guide](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-lang-ffi-guide/GUIDE.md).
The most commonly used:

| Crate | Wraps | Purpose |
|---|---|---|
| [`buff-web`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-web) | axum 0.8 | HTTP server ([Chapter 3](./chapter-3.md)) |
| [`buff-cli`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-cli) | clap | CLI argument parser ([Chapter 2](./chapter-2.md)) |
| [`buff-db`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-db) | tokio-postgres / rusqlite | database access (§7.6) |
| [`buff-http-client`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-http-client) | reqwest (rustls) | HTTP client |
| [`buff-cache`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-cache) | moka | in-memory cache (Redis backend deferred) |
| [`buff-template`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-template) | minijinja / handlebars | HTML templating |
| [`buff-reactive`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-reactive) | (custom) | reactive signals |
| [`buff-pubsub`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-pubsub) | crossbeam-channel | in-process pub/sub |
| [`buff-jobs`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-jobs) | (custom) | background job queue |
| [`buff-validate`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-validate) | validator | input validation |
| [`buff-auth`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-auth) | (custom) | auth primitives |
| [`buff-resilience`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-resilience) | (custom) | retry / circuit breaker |
| [`buff-dataframe`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-dataframe) | (custom) | DataFrame operations |
| [`buff-tensor`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-tensor) | (custom) | n-dimensional tensors |
| [`buff-image`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-image) | image | image processing |
| [`buff-ml`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-ml) | (custom) | ML primitives (autodiff, layers) |
| [`buff-fsm`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-fsm) | (custom) | finite state machines |
| [`buff-i18n`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-i18n) | (custom) | internationalization |
| [`buff-scrape`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-scrape) | scraper | web scraping |
| [`buff-crypto-extras`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-crypto-extras) | (custom) | extra crypto (RSA, etc.) |

The full list with per-crate `AGENTS.md` guidance lives under
[`crates/`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates).
Every framework crate follows the same pattern: pure-Rust (no C deps), FFI-
safe, panic-free, with an in-memory MVP where distributed backends are
deferred (see the root `AGENTS.md` "DEFERRED" notes for `buff-cache` and
`buff-pubsub` for the canonical example).

## 7.8 The implicit-prelude philosophy

The most important thing to internalize about the Buff stdlib is that it's
**implicit**. There is no `import std.io` to write, no `use` statement to
forget. Every program starts with `print`, `len`, `abs`, `DateTime`, `Regex`,
`Json`, `Map`, `Vector`, `Option`, `Result`, and all the rest already in scope.

This is a deliberate departure from Rust (where you write `use std::collections::HashMap`
on every file) and Python (where you `import json`, `import re`, `import os`).
The cost is that you can't tell at a glance which names are yours vs the
stdlib's; the benefit is that the 90% case — write a small program that prints,
parses, and computes — has zero import boilerplate.

If you shadow a prelude name with your own (e.g. you define a `print` function),
the local definition wins. This is a documented footgun, identical to shadowing
`print` in Python — use distinct names.

---

*Next: [Chapter 8 — Error Code Handbook](./chapter-8.md)*
