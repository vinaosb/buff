# Chapter 9 — Migration Guides

This chapter is for readers who already know a language and want a fast
on-ramp to Buff. Three guides, one per source language:

- [§9.1 From Rust](#91-from-rust) — for readers who know Rust and want to see
  exactly what friction Buff removes (the canonical audience — Buff
  transpiles to Rust).
- [§9.2 From Python](#92-from-python) — for readers coming from a high-level
  dynamic language who want Rust-tier performance without the learning curve.
- [§9.3 From Go](#93-from-go) — for readers who already have Go's
  productivity-but-GC trade-off and want to drop the GC.

Each guide is a side-by-side cheat-sheet plus a narrative on the mental-model
shift. The repo ships a full side-by-side example library at
[`examples/rust-vs-buff/`](../../examples/rust-vs-buff/) covering 14 topics
(hello world, functions, recursion, borrow checker, pattern matching, error
handling, closures, iterators, collections, null safety, structs, enums,
lifetimes, async) — this chapter summarizes and links into it.

---

## 9.1 From Rust

Buff transpiles to Rust. If you know Rust, you already know what Buff
*produces* — you're learning what friction it removes. The pitch is in one
table:

| Rust pain | Buff simplification |
|---|---|
| `println!` macro, semicolons, braces | `print()`, indentation-based blocks |
| `&` / `&mut` / `'a` lifetimes | No references, no lifetimes (owned-by-default + compiler clones) |
| `Box<dyn Error>` + `impl Display` + `impl Error` | Builtin `Error`, `?` operator |
| `Fn` / `FnMut` / `FnOnce` + `move` | `{ x => body }` lambdas |
| `.iter()` / `.into_iter()` / `.collect::<Type>()` | `.map(fn)` direct |
| `use std::collections::HashMap; HashMap::new()` | Builtin `Map<K,V>` + literal `{ "k": v }` |
| `Option::Some(x)` / `.unwrap()` temptation | `match opt { Some(x) => ..., None => ... }` |
| `#[derive(Clone, Debug, PartialEq)]` | Automatic |
| `pub` on every field + `impl` blocks | Public by default, no `impl` |
| `Type::Variant` qualification in match | Unqualified variants |
| `.await` everywhere + `#[tokio::main]` | No `await`; async auto-propagates |

### Hello world

**Rust:**

```rust
fn main() {
    println!("Hello, Rust!");
}
```

**Buff** ([`examples/ola.buff`](../../examples/ola.buff)):

```buff
func main():
    print("Olá, Buff!")
```

The differences: `func` not `fn` (Buff keyword), `print(...)` not
`println!(...)` (function not macro), `:` + newline + indent not `{ ... }`,
no trailing semicolons. See
[`examples/rust-vs-buff/hello_world/`](../../examples/rust-vs-buff/hello_world/).

### Borrow checker — the big one

**Rust:**

```rust
fn main() {
    let v = vec![1, 2, 3, 4, 5];
    let first = &v[0];          // borrow
    println!("{}", first);
    let v2 = v;                  // move — v is now dead
    // println!("{}", v[0]);    // ❌ borrowed after move
}
```

**Buff** ([`examples/rust-vs-buff/borrow_checker/borrow_checker.buff`](../../examples/rust-vs-buff/borrow_checker/borrow_checker.buff)):

```buff
func main():
    let v = [1, 2, 3, 4, 5]
    print(v[0])
    let v2 = v                  // "move" — but v stays usable
    print(v2[0])                // ✅ Buff inserts the clone for you
```

Buff has **no `&`, no `&mut`, no `'a`**. Values are owned by default. When
the compiler detects that you're reusing a value after it would have moved,
it inserts a `.clone()` automatically. You never type `.clone()` — the
compiler decides where it's needed.

This is the *core* of Buff's value proposition for Rust refugees: the borrow
checker becomes a free safety reviewer of generated code, never an obstacle
you see.

### Error handling

**Rust:**

```rust
use std::error::Error;
use std::fmt;

#[derive(Debug)]
struct MyError(String);

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for MyError {}

fn half(n: i64) -> Result<i64, Box<dyn Error>> {
    if n < 2 { return Err(Box::new(MyError("too small".into()))); }
    Ok(n / 2)
}
```

**Buff** ([`examples/error_handling.buff`](../../examples/error_handling.buff)):

```buff
func half(n: Int) -> Result<Int, Error>:
    if n < 2:
        return Error("too small")
    return Ok(n / 2)
```

The builtin `Error` type lowers to `Err(Error::new("msg"))`. The `?` operator
works exactly like Rust's. No `impl Display`, no `impl Error`, no
`Box<dyn Error>`. For richer error enums, define an `enum` (codegen-verified;
see [Chapter 6 §6.10](./chapter-6.md) status note).

### Closures

**Rust:**

```rust
let doubled: Vec<i32> = vec![1, 2, 3, 4, 5]
    .iter()
    .map(|x| x * 2)
    .collect();
```

**Buff** ([`examples/closures.buff`](../../examples/closures.buff)):

```buff
let doubled = [1, 2, 3, 4, 5].map({ x => x * 2 })
```

No `Fn` / `FnMut` / `FnOnce`, no `move`, no `.iter()` / `.collect()` dance.
Buff's `{ params => body }` lambda syntax is the only closure form. The
compiler handles all capture semantics (T34 capture-aware codegen).

### Async — without `await`

**Rust:**

```rust
#[tokio::main]
async fn main() {
    let v = fetch_value().await;          // .await everywhere
    println!("{}", v);
    let task = tokio::spawn(async move {
        fetch_value().await
    });
    let answer = task.await.unwrap();
    println!("{}", answer);
}

async fn fetch_value() -> i64 { 42 }
```

**Buff** ([`examples/async_demo.buff`](../../examples/async_demo.buff)):

```buff
async func fetch_value() -> Int:
    return 42

func main():
    let value = fetch_value()             // no .await!
    print(value)
    let task = spawn fetch_value()
    let answer = task.result()
    print(answer)
```

Buff has **no `await` keyword**. Async propagates up the call graph
automatically:

- `async func fetch_value()` → Rust `async fn fetch_value()`.
- A sync function calling `fetch_value()` → auto-promoted to `async fn`. No
  keyword needed on the caller.
- `main` calling any async function → auto-receives `#[tokio::main]`.
- `spawn task()` → `tokio::spawn(async move { task() })`.
- `task.result()` → `task.await`.

This eliminates the function-coloring problem: sync functions can call async
ones without ceremony, because the compiler colors for you. ~95% of user code
never knows async exists. See [Chapter 6 §6.8](./chapter-6.md) for the full
model.

### What you *keep* from Rust

Buff doesn't throw away Rust's strengths — it inherits them:

- **Memory safety** — no GC, no use-after-free, no data races (the borrow
  checker still runs, on generated code).
- **Zero-cost abstractions** — generics monomorphize, traits static-dispatch
  by default, `Box<dyn Trait>` when you need dynamic dispatch (T68).
- **Native performance** — the binary is real `rustc`/LLVM output, competing
  with C and C++ on throughput.
- **Tooling** — `cargo` under the hood, `rustc` error messages (mapped back
  to `.buff` via SpanMap), LLDB / the `buff-dap` debug adapter.
- **The ecosystem** — `extern` bindgen (T17-T21 wave) lets you call into any
  Rust crate. See [`crates/buff-lang-ffi-guide/GUIDE.md`](../../crates/buff-lang-ffi-guide/GUIDE.md).

### The mental-model shift

The hardest thing for a Rust developer learning Buff is *trusting the
compiler*. You'll reach for `.clone()` and stop yourself — Buff inserts it
for you. You'll reach for `&` and find it's not in the grammar. You'll write
`async fn` and find the `async` keyword disappears on the caller. The rule
of thumb: **write the clear version first; the compiler will insert the
plumbing.** If the clear version doesn't compile, *then* reach for the
explicit form.

---

## 9.2 From Python

Buff reads a lot like Python: indentation-based blocks, no semicolons, aggressive
type inference, an implicit prelude. The differences that bite:

| Python | Buff |
|---|---|
| `def main():` | `func main():` |
| `print(x)` (function) | `print(x)` (same — but adds newline like `println`) |
| dynamically typed | statically typed (inferred) |
| `None` | `Option<T>` (`None` → `None`, `x` → `Some(x)`) |
| `try: ... except E: ...` | `Result<T, E>` + `?` operator |
| `for x in xs:` | `for x in xs:` (same!) |
| `dict` literal `{k: v}` | `Map<K, V>` literal `{k: v}` (same syntax!) |
| `list` literal `[1, 2, 3]` | `Vector<T>` literal `[1, 2, 3]` (same!) |
| `f"{name}"` | `"{name}"` (no `f` prefix — always interpolated) |
| GC, pauses, refcounting | none — ownership at compile time |
| duck typing | traits + static dispatch |
| `import os, json, re` | implicit prelude (no imports for stdlib) |

### Hello world

**Python:**

```python
def main():
    print("Hello, Python!")

main()
```

**Buff:**

```buff
func main():
    print("Hello, Buff!")
```

Almost identical. The differences: `func` not `def`, and no explicit `main()`
call at the bottom — the Buff runtime calls `main` for you.

### Types — the big shift

Python is dynamically typed; Buff is statically typed with inference. You
*can* write types, but you usually don't have to:

**Python:**

```python
def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)
```

**Buff** ([`examples/fibonacci.buff`](../../examples/fibonacci.buff)):

```buff
func fib(n: Int) -> Int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)
```

The `Int` annotations are optional (inference fills them in) but recommended
on public functions. The payoff: typos like `fib(n - 1) + fib(n - "two")`
are caught at compile time, not at runtime in production.

### `None` vs `Option<T>`

**Python:**

```python
def find_user(id):
    if id in users:
        return users[id]
    return None

user = find_user(42)
if user is not None:
    print(user.name)
# easy to forget the None check — crashes at runtime
```

**Buff:**

```buff
func find_user(id: Int) -> Option<User>:
    match users[id]:
        Some(u): return Some(u)
        None: return None

match find_user(42):
    Some(user): print(user.name)
    None: print("not found")
// can't forget the None case — compiler enforces exhaustiveness
```

`Option<T>` is `Some(x)` or `None`. The compiler verifies every `match`
covers both arms (or has a `_`), so you cannot forget the absent case. This
is the "billion-dollar mistake" fix: there is no `null` in Buff, only
`Option<T>`, and you must handle it explicitly.

### Error handling — `try/except` → `Result + ?`

**Python:**

```python
def half(n):
    if n < 2:
        raise ValueError("too small")
    return n / 2

try:
    result = half(1)
except ValueError as e:
    print(e)
```

**Buff:**

```buff
func half(n: Int) -> Result<Int, Error>:
    if n < 2:
        return Error("too small")
    return Ok(n / 2)

match half(1):
    Ok(v): print(v)
    Err(e): print(e)
```

Or, with the `?` operator for propagation:

```buff
func add_one(n: Int) -> Result<Int, Error>:
    let h = half(n)?          # propagates Err automatically
    return Ok(h + 1)
```

There is no `try` / `except` / `catch` in Buff. Errors are values
(`Result<T, E>`), and the `?` operator propagates them. This makes the
error-path visible in the type signature — you always know which functions
can fail.

### What you *gain* over Python

- **A 10-100x speedup** typical of Python → compiled-language ports. No GIL,
  no interpreter, no JIT warmup.
- **Real parallelism** — `.map()` fans across CPU cores via Rayon; no
  `multiprocessing` boilerplate.
- **GPU dispatch** — `@prefer(gpu)` (see [Chapter 4](./chapter-4.md)) with no
  CUDA / Numba / CuPy setup.
- **A single deployable** — one native binary, no `pip install`, no virtualenv,
  no Docker image with 200 MB of Python runtime.
- **Catches typos at compile time** — the type system finds the bugs Python
  only surfaces in production.

### What you *lose*

- **REPL-driven exploration** is weaker (there's a `buff repl`, but Python's
  Jupyter ecosystem is unmatched). Use `buff jupyter` (T129) for notebook-style
  workflows — Buff ships a pure-Rust Jupyter kernel.
- **Duck typing** — you can't pass any-old-object-that-has-`.foo()` to a
  function; you need a `trait` (or use `extern` dynamic dispatch).
- **The stdlib breadth** — Python's stdlib is enormous; Buff's is growing but
  smaller today. The framework crates ([Chapter 7 §7.7](./chapter-7.md)) fill
  the gaps.

---

## 9.3 From Go

Go and Buff share a goal — *productive systems programming* — but take
opposite approaches to memory: Go has a GC, Buff has compile-time ownership
(Rust underneath). For Go developers, Buff offers:

| Go | Buff |
|---|---|
| `func main() { ... }` | `func main(): ...` |
| `var x int = 5` / `x := 5` | `let x = 5` |
| GC + pauses | ownership at compile time (no pauses) |
| `if err != nil { return err }` | `?` operator (one char, not four lines) |
| `interface{}` / `any` | traits + generics (real types) |
| `go func() { ... }()` | `spawn task()` |
| `select { case ... }` | `Channel<T>` MPSC |
| `import ("fmt"; "os")` | implicit prelude (no imports for stdlib) |
| `fmt.Sprintf("%s=%d", k, v)` | `"{k}={v}"` (interpolation) |
| `nil` | `Option<T>` (no nil) |
| `for i := 0; i < n; i++ {}` | `for i in range(0, n):` |
| no generics (until 1.18) | generics from day one |
| GC pauses at scale | zero pauses |

### Hello world

**Go:**

```go
package main

import "fmt"

func main() {
    fmt.Println("Hello, Go!")
}
```

**Buff:**

```buff
func main():
    print("Hello, Buff!")
```

No `package main`, no `import "fmt"` (prelude is implicit), braces →
indentation.

### Error handling — the err != nil tax

**Go:**

```go
func half(n int) (int, error) {
    if n < 2 {
        return 0, fmt.Errorf("too small")
    }
    return n / 2, nil
}

func addOne(n int) (int, error) {
    h, err := half(n)
    if err != nil {
        return 0, err
    }
    return h + 1, nil
}
```

**Buff:**

```buff
func half(n: Int) -> Result<Int, Error>:
    if n < 2:
        return Error("too small")
    return Ok(n / 2)

func add_one(n: Int) -> Result<Int, Error>:
    let h = half(n)?          // one char propagates the error
    return Ok(h + 1)
```

Go's `if err != nil { return err }` four-line ceremony becomes a single `?`.
This is the single biggest day-to-day ergonomics win for Go refugees.

### Goroutines → `spawn`

**Go:**

```go
go func() {
    doWork()
}()
```

**Buff:**

```buff
spawn do_work()
```

`spawn task()` lowers to `tokio::spawn(async move { task() })`. The runtime
manages the worker pool. `task.result()` awaits (no `await` keyword — see
[Chapter 6 §6.8](./chapter-6.md)).

### Channels

**Go:**

```go
ch := make(chan int, 10)
ch <- 42
v := <-ch
```

**Buff:**

```buff
let (sender, receiver) = Channel.new()
sender.send(42)
let v = receiver.receive()
```

`Channel<T>` is MPSC (multi-producer, single-consumer) via
`crossbeam-channel`. `.send()` returns `Result<Void, Error>`
([E1407](./chapter-8.md#e1407) if all receivers dropped); `.receive()`
returns `Result<T, Error>` ([E1408](./chapter-8.md#e1408) on stream end). See
[`examples/channels/`](../../examples/channels/).

### What you *gain* over Go

- **No GC pauses** — Buff binaries have deterministic memory behavior, which
  matters for latency-sensitive services (trading, games, embedded).
- **Smaller binaries** — a `--minimal` Buff CLI is ~340 KB; a Go binary is
  rarely under 5 MB.
- **Real generics** — no `interface{}` / `any` / type-assertion boilerplate.
  Buff generics monomorphize at compile time.
- **GPU compute** — `@prefer(gpu)` with zero setup. Go has no answer here.
- **Stronger type system** — `Option<T>` (no nil), exhaustive `match`,
  trait-based polymorphism. Fewer runtime panics.

### What you *lose*

- **GC simplicity** — Rust's ownership model has a learning curve even when
  hidden by Buff. You'll occasionally hit a generated-code borrow error
  (mapped back to `.buff` via SpanMap) and need to understand it.
- **The Go stdlib breadth** — Go's stdlib is excellent and uniform; Buff's is
  growing but smaller today.
- **`go doc` uniformity** — Buff's docs story is good (`buff check`, this
  book, the error catalog) but Go's doc convention is more ingrained.
- **Compile speed** — Go compiles in seconds; Buff goes through `rustc`, which
  is slower for large programs. The `buff watch` / `sccache` integrations
  ([Chapter 1](./chapter-1.md)) mitigate this.

---

## 9.4 Universal migration advice

Regardless of where you're coming from:

1. **Start with [Chapter 1](./chapter-1.md).** Install Buff, run `ola.buff`,
   run `fibonacci.buff`. Twenty minutes.
2. **Read the [rust-vs-buff](../../examples/rust-vs-buff/) examples** even if
   you're not coming from Rust — they're the clearest side-by-side
   illustrations of Buff's design choices.
3. **Use `buff check` constantly.** It's the fastest feedback loop. Run it on
   every save; wire it into your editor (the VSCode extension does this
   automatically).
4. **Trust the compiler.** Write the clear version first. If it doesn't
   compile, the error message (with its stable `E1xxx` code and suggestion
   engine) will tell you exactly what to change.
5. **Read the error catalog.** [Chapter 8](./chapter-8.md) and
   [`docs/errors/`](../../docs/errors/) are your friends. Every code is
   documented with an example and a fix.
6. **Embrace the implicit prelude.** Don't write `import` for stdlib
   functionality — `print`, `Json`, `Regex`, `DateTime`, `Map`, `Vector` are
   all already in scope. This feels wrong coming from Python/Go/Rust; it's
   right for Buff.
7. **When in doubt, look at the examples.** The
   [`examples/`](../../examples/) directory is the canonical "how do I do X
   in Buff" reference. If something isn't there, it may not be wired yet —
   check the status markers (🟢 / 🔶).

## 9.5 Where to go next

- **Foundations:** [Chapter 1](./chapter-1.md) → [Chapter 2](./chapter-2.md)
  → [Chapter 3](./chapter-3.md).
- **Advanced:** [Chapter 4 (GPU)](./chapter-4.md) and
  [Chapter 5 (UI)](./chapter-5.md).
- **Reference:** [Chapter 6 (Language)](./chapter-6.md),
  [Chapter 7 (Stdlib)](./chapter-7.md),
  [Chapter 8 (Errors)](./chapter-8.md).
- **The repo:** [`examples/`](../../examples/) for runnable code,
  [`crates/`](../../crates/) for framework crates,
  [`.sisyphus/plans/`](https://github.com/buff-lang/buff/tree/v1x-frameworks/.sisyphus/plans)
  for the full roadmap and conventions.
- **The community:** [GitHub Discussions](https://github.com/buff-lang/buff/discussions)
  for questions, [GitHub Issues](https://github.com/buff-lang/buff/issues)
  for bugs.

Welcome to Buff. Write clean code; get fast binaries.

---

*Previous: [Chapter 8 — Error Code Handbook](./chapter-8.md)*
*• [Back to the introduction](./chapter-0.md)*
