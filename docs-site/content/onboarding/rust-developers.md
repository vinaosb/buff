+++
title = "Buff for Rust developers"
weight = 47
+++

# Buff for Rust developers

> Buff transpiles to Rust. If you already know Rust, you know most of
> Buff's semantics — `match`, `Result<T, E>`, `Option<T>`, traits,
> `?`, `Vec<T>` → `Vector<T>`, `HashMap<K,V>` → `Map<K,V>`. What Buff
> removes is the **borrow checker fights** and the **`async`/`await`
> color split** — you write owned-by-default code with no `&`, no
> `'a`, no `.clone()` litter, no `move ||`, and the compiler emits
> the right Rust for you. This guide is the deltas.

This guide assumes you can read Rust 2021+ and have written
`Box<dyn Error>`, fought the borrow checker, and used `tokio`. You'll
be productive in Buff in 20 minutes.

## Why Buff?

Rust developers are the **primary** audience for Buff. The pitch:

1. **No more borrow checker fights.** Buff has no `&`, no `&mut`, no
   `'a` in the surface language. You write owned-by-default code; the
   compiler inserts `.clone()` and `Arc` where needed. About 95% of
   user code never sees a move error.
2. **No async color split.** There is no `await` keyword. A function
   declared `async func` is async; every caller is propagated
   async-ness automatically. The compiler inserts `.await` at async
   call sites and emits `#[tokio::main]` on `main` when it joins the
   async set.
3. **Same toolchain.** Buff uses `cargo` and `rustc` underneath. You
   can `extern` any Rust crate directly. The Buff standard library
   is mostly thin wrappers over mature Rust crates (`chrono`,
   `regex`, `reqwest`, `tokio`, `serde_json`, …).
4. **Same standard types.** `Result<T, E>`, `Option<T>`, `Vec<T>` →
   `Vector<T>`, `HashMap<K,V>` → `Map<K,V>`. You already know how to
   use them.
5. **Same trait system.** `trait Shape { ... }` and `impl Shape for
   Circle { ... }` work the same way. Generics, associated types,
   bounds — all present.

The trade-off: Buff is younger and smaller than Rust, the borrow
checker is hidden so you can't reason about allocation locality, and
some advanced Rust features (const generics, GATs in their full
generality, certain proc-macro patterns) are not yet exposed. For most
application code, the trade is overwhelmingly positive.

## Syntax mapping table

The deltas. Things that are identical (`match`, `Result`, `Option`,
`?`, `for`, `while` is missing — see below) are skipped; the table
below shows where Buff differs from Rust.

### Functions and modules

| Rust | Buff | Notes |
|---|---|---|
| `fn f() -> i32 {` | `func f() -> Int:` | `fn` → `func`; braces → colon + indent. |
| `pub fn f()` | `func f()` | Everything is public by default (no `pub`). |
| `pub(crate) fn f()` | (no equivalent) | No crate-privacy yet. |
| `mod foo { }` | (file = module) | A `foo.buff` file is the `foo` module. |
| `use std::collections::HashMap;` | (implicit prelude) | Prelude types need no `use`. |
| `use crate::foo::bar;` | `import foo.bar` | Explicit import. |
| `pub use foo::Bar as Baz;` | `export Baz = foo.Bar` | Re-export with rename. |
| `extern crate serde;` | `extern "C" from "serde" ...` | See FFI section. |
| `impl Type { fn m(&self) {} }` | `func Type.m():` | Methods are `Type.method()` syntax. |
| `impl Trait for Type {}` | `impl Trait for Type:` | Same idea, colon-syntax body. |
| `trait Foo: Bar {}` | `trait Foo: Bar:` | Same. |
| `where T: Clone` | `where T: Clone` | Same. |
| `Default::default()` | `Type.default()` | Method-style. |
| `Self` | `Self` | Same. |
| `Turbofish::<T>()` | (often unnecessary) | Inference is aggressive. |

### Types and ownership

| Rust | Buff | Notes |
|---|---|---|
| `&T`, `&mut T` | (gone — owned by default) | The compiler inserts clones/borrows. |
| `'a` lifetime | (gone) | No lifetime annotations. |
| `Box<T>` | (just `T`) | Heap allocation is implicit. |
| `Rc<T>`, `Arc<T>` | (compiler inserts when needed) | Hidden from the user. |
| `Cow<'a, T>` | (compiler chooses) | Hidden. |
| `Vec<T>` | `Vector<T>` | Renamed. |
| `HashMap<K, V>` | `Map<K, V>` | Renamed. |
| `HashSet<T>` | `Set<T>` | Renamed. |
| `String` | `String` | Same. |
| `&str` | `String` (mostly) | No string slices in user code; use `String`. |
| `i32`, `u64`, `i64` | `Int` (default integer) | The compiler picks the width. |
| `f64` | `Float` (default float) | The compiler picks the width. |
| `bool` | `Bool` | Capital B. |
| `()` | `Unit` | Has a name. |
| `Option<T>` | `Option<T>` | Same. |
| `Result<T, E>` | `Result<T, E>` | Same. |
| `Vec<u8>` | `Vector<Byte>` or `Bytes` | Byte buffers. |
| `[T; N]` (fixed array) | (no equivalent — use `Vector<T>`) | No const generics in user code yet. |
| `&[T]` slice | (no equivalent) | Use `Vector<T>`. |
| `*const T`, `*mut T` raw ptr | (no equivalent) | Use `extern "unsafe"` blocks. |

### Variables and mutation

| Rust | Buff | Notes |
|---|---|---|
| `let x = 5;` | `let x = 5` | Semicolons are optional / mostly absent. |
| `let mut x = 5;` | `let mut x = 5` | Same. |
| `let x: i32 = 5;` | `let x: Int = 5` | Annotation optional. |
| `const MAX: u32 = 100;` | `const MAX = 100` | Module-level constant. |
| `static X: AtomicUsize = ...` | (no equivalent) | Use `@State` or runtime cell. |
| `x.clone()` | (implicit) | The compiler inserts clones. |
| `x as f64` | `x.float()` | Method-style cast. |
| `x as i32` | `x.int()` | Method-style cast. |

### Control flow

| Rust | Buff | Notes |
|---|---|---|
| `if c { } else { }` | `if c: ... else: ...` | Colon + indent. |
| `match v { Pat => body, }` | `match v { Pat => body, }` | **Same syntax.** |
| `for x in iter { }` | `for x in iter:` | Colon + indent. |
| `while c { }` | (no equivalent) | Use recursion or `for` over an iterator. |
| `loop { }` | (no equivalent) | Use recursion. |
| `break`, `continue` | `break`, `continue` | Same. |
| `break 'outer` | (no equivalent) | No labeled loops. |
| `?` (early return on Err) | `?` | **Same.** |
| `.unwrap()`, `.expect()` | (forbidden in non-test code) | Match or propagate. |
| `return x;` | `return x` | No semicolon needed. |
| `?Sized` bound | (n/a) | Hidden. |

### Closures and combinators

| Rust | Buff | Notes |
|---|---|---|
| `\|x\| x * 2` | `{ x => x * 2 }` | Brace-and-arrow. |
| `move \|x\| ...` | `{ x => ... }` (always moves) | Captures are by-move by default. |
| `Fn(Int) -> Int` trait | `trait Fn(Int) -> Int` | Same trait, same bounds. |
| `FnMut`, `FnOnce` | (hidden — Buff picks) | You don't write these. |
| `.iter().map(\|x\| x * 2).collect::<Vec<_>>()` | `.map({ x => x * 2 })` | No `.iter()`, no `.collect()`. |
| `.iter().filter(...)` | `.filter({ x => ... })` | Same. |
| `.into_iter()` | (default — Buff moves) | Move-by-default. |
| `.iter_mut()` | (use `let mut` + method) | Mutation via `mut` binding. |
| `Iterator::collect::<Vec<_>>()` | (auto-collected) | Inferred. |

### Async

| Rust | Buff | Notes |
|---|---|---|
| `async fn f() -> T` | `async func f() -> T:` | Same shape. |
| `let x = f().await;` | `let x = f()` | **No `.await`.** |
| `tokio::spawn(async { ... })` | `spawn { ... }` or `spawn f()` | Built-in. |
| `tokio::spawn(f()).await` | `let t = spawn f(); let v = t.result()` | Two steps. |
| `#[tokio::main]` | (auto-emitted) | The compiler adds it when needed. |
| `tokio::sync::Mutex<T>` | (hidden) | You don't write locks. |
| `tokio::sync::mpsc::channel()` | `Channel.new(buffer)` | Returns `(Sender<T>, Receiver<T>)`. |
| `tokio::time::sleep(d).await` | `sleep(d)` | Built-in; async-aware. |
| `tokio::select! { ... }` | (use `select` expression — planned) | Or `Channel.recv()` with timeout. |
| `futures::join!(a, b)` | `spawn` + `task.result()` × 2 | Manual gather. |
| `Pin<Box<dyn Future>>` | (hidden) | No `Pin` in user code. |

### Macros and metaprogramming

| Rust | Buff | Notes |
|---|---|---|
| `println!("hi")` | `print("hi")` | Built-in function, not macro. |
| `format!("{}", x)` | `x.string()` or concat | No format macros. |
| `vec![1, 2, 3]` | `[1, 2, 3]` | Literal syntax. |
| `#[derive(Debug, Clone)]` | (implicit) | All types derive Debug, Clone, PartialEq by default. |
| `#[derive(Serialize)]` | (auto via serde integration) | Implicit when type is in a JSON/TOML context. |
| `macro_rules! foo { ... }` | (no equivalent — use `@comptime`) | See comptime section. |
| `proc_macro_derive(Foo)` | (no equivalent) | User-defined proc-macros not yet supported. |
| `concat!()`, `env!()`, `file!()` | `@comptime` expressions | Comptime subsystem (T53). |
| `include_str!("file")` | `Filesystem.read("file")` | Runtime read; or `@comptime` for build-time. |

### Attributes

| Rust | Buff | Notes |
|---|---|---|
| `#[test]` | `@test` | `@` instead of `#[]`. |
| `#[ignore]` | `@allow("ignore")` | Convention. |
| `#[cfg(feature = "x")]` | (use `@feature("x")` — planned) | Feature gates. |
| `#[inline]` | (compiler decides) | No user-level inline attribute. |
| `#[repr(C)]` | `@repr(C)` | FFI layout pinning. |
| `#[tokio::main]` | (auto-emitted) | Async runtime bootstrap. |
| `#[derive(...)]` | (implicit) | See above. |
| `#[allow(dead_code)]` | `@allow("dead_code")` | Lint suppression. |
| `#[deprecated]` | `@deprecated("use X instead")` | Same idea. |

## Tooling migration

Buff is built **on top of** `cargo` and `rustc`. You already have the
toolchain — the only new tool is `buff` itself.

| Rust | Buff | Notes |
|---|---|---|
| `cargo new` | `buff new` | Scaffolds `buff.toml` + `src/main.buff`. |
| `cargo build` | `buff build` | Compiles to a native binary. |
| `cargo run` | `buff run <file>` | Compile + execute. |
| `cargo check` | `buff check` | Type-check only (no codegen). Fast. |
| `cargo test` | `buff test` | Discovers `@test` functions. |
| `cargo fmt` | `buff fmt` | Indent-based formatter (4 spaces). |
| `cargo clippy` | `buff check` (lints included) | Type-checker doubles as linter. |
| `cargo bench` | `buff bench` (planned) | Benchmark runner. |
| `cargo doc` | `buff doc` (planned) | Doc generator. |
| `cargo add serde` | `buff add buff_serde` | Or `[rust-deps]` for raw Rust crates. |
| `cargo update` | `buff update` | Bump a single dep. |
| `cargo outdated` | `buff outdated` | List outdated deps. |
| `cargo publish` | `buff publish` | Publish to `buff-registry`. |
| `cargo install` | `buff install` | Install a binary from the registry. |
| `Cargo.toml` | `buff.toml` | One declarative manifest. |
| `Cargo.lock` | `buff.lock` | Auto-generated (gitignored). |
| `~/.cargo/` | `~/.buff/` | Per-user cache. |
| `rustup` | `buffup` | Version manager (v1.12). |
| `rust-analyzer` | `buff-lsp` | LSP server (v1.2). |
| `rustc --emit=llvm-ir` | (Buff emits Rust, then rustc does the rest) | `buff build --emit rust` shows the intermediate. |

### Inspecting generated Rust

A unique Buff feature: you can see the Rust the compiler emits before
it's handed to `rustc`. This is invaluable when you're migrating from
Rust and want to verify the lowering.

```bash
buff build examples/fibonacci.buff --emit rust
# writes examples/fibonacci.rs (or wherever configured)
```

The generated Rust is **idiomatic** — it uses `Arc` where Buff hides
sharing, calls `.clone()` where Buff hides copies, and emits
`#[tokio::main]` only on `main` when the call graph needs it. You can
copy-paste it into a Rust codebase as a starting point for hand-tuning.

### The project layout

A `buff new my_app` looks like:

```
my_app/
├── buff.toml          # project manifest
├── src/
│   └── main.buff      # entry point
└── tests/
    └── test_main.buff
```

Compare to `cargo new`:

```
my_app/
├── Cargo.toml
├── src/
│   ├── main.rs        # or lib.rs
│   └── ...
└── tests/
    └── integration.rs
```

Buff's layout mirrors Cargo's. The differences:

- No `src/lib.rs` vs `src/main.rs` split — Buff has `src/main.buff` as
  the single entry point. Library crates are a separate scaffold.
- No `build.rs` build script (yet). Comptime evaluation (`@comptime`)
  replaces most build-script use cases.
- No `tests/` for integration tests is fine — Buff's `buff test` runs
  `@test`-marked functions wherever they live.

### Dependency declaration

In `Cargo.toml`:

```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1.40", features = ["full"] }
```

In `buff.toml`:

```toml
[deps]
buff_http_client = "1.0"
buff_dataframe = "1.0"

[rust-deps]
serde = "1.0"
tokio = "1.40"
```

`[deps]` are Buff packages; `[rust-deps]` are raw Rust crates. The
latter require `extern` declarations to be callable from Buff code
(see FFI section).

## Ecosystem mapping

Since Buff transpiles to Rust, **every Rust crate is reachable**.
Most stdlib prelude types are thin wrappers over mature Rust crates:

| Rust crate | Buff prelude type | Notes |
|---|---|---|
| `std::vec::Vec` | `Vector<T>` | Renamed. |
| `std::collections::HashMap` | `Map<K, V>` | Renamed. |
| `std::collections::HashSet` | `Set<T>` | Renamed. |
| `std::option::Option` | `Option<T>` | Same. |
| `std::result::Result` | `Result<T, E>` | Same. |
| `std::string::String` | `String` | Same. |
| `std::time::Instant` | `DateTime` | Wraps `chrono`. |
| `chrono` | `DateTime` | Date arithmetic. |
| `regex` | `Regex` | Compiled regex. |
| `reqwest` | `HttpClient` | HTTP client. |
| `tokio` | `async func`, `spawn`, `Channel<T>` | Async runtime. |
| `serde_json` | `Toml`/`Json` parsers | Note: JSON surfaces as `Map<String, String>`. |
| `toml` | `Toml` | TOML reader/writer. |
| `serde_yaml` | `Yaml` | YAML reader. |
| `csv` | `Csv` | CSV reader. |
| `rand` | `Random` | RNG. |
| `sha2` | `Hash` | SHA-256 etc. |
| `hmac` | `HMAC` | HMAC. |
| `base64` | `Base64` | Encode/decode. |
| `uuid` | `UUID` | UUIDv4. |
| `tracing` | `Log` | Structured logging. |
| `std::process::Command` | `Process` | Spawn external processes. |
| `std::env` | `Env`, `Args` | Environment vars + CLI args. |
| `std::net::TcpListener` | `TCP` | TCP primitives. |
| `std::net::UdpSocket` | `UDP` | UDP primitives. |
| `tokio-tungstenite` | `WebSocket` | Async WebSocket client. |
| `std::path::Path` | `Path` | Filesystem paths. |
| `std::fs` | `Dir`, `Filesystem` | File I/O. |
| `tempfile` | `Tempfile` | Auto-cleanup temp files. |

For Rust crates without a Buff wrapper, declare them in `[rust-deps]`
and bind via `extern`. The next section explains.

## The `extern` FFI guide

Buff's `extern` keyword is a **safe-by-construction** wrapper around
Rust crates. It does not call C — it calls Rust. The syntax:

```buff
extern "C" from "reqwest" func fetch_text(url: String) -> String
```

This tells the Buff compiler:

1. Add `reqwest` to `[rust-deps]` in `buff.toml`.
2. Emit a Rust `extern "C" { fn fetch_text(...); }` foreign-mod item.
3. Wrap every call site in `unsafe { ... }` automatically.
4. Provide a safe Buff-side function named `fetch_text`.

The `"C"` is the ABI (Rust's extern ABI); `"reqwest"` is the source
crate. The full guide is at
[`crates/buff-lang-ffi-guide/GUIDE.md`](https://github.com/buff-lang/buff/blob/master/crates/buff-lang-ffi-guide/GUIDE.md)
— 6 hard rules for safe wrappers.

A realistic example (from
[`examples/extern_reqwest.buff`](https://github.com/buff-lang/buff/blob/master/examples/extern_reqwest.buff)):

```buff
extern "C" from "reqwest" func fetch_text(url: String) -> String

func main():
    let body = fetch_text("https://example.com")
    print(body)
```

The compiler emits something like:

```rust
extern "C" {
    fn fetch_text(url: String) -> String;
}

fn main() {
    let body = unsafe { fetch_text("https://example.com".to_string()) };
    println!("{}", body);
}
```

You provide the safe-wrapper body in a sibling `externs.rs` file (the
`buff.toml` `[rust-externs]` section points at it). This is the same
pattern Rust uses for `bindgen`, except Buff generates the foreign-mod
item for you.

For async extern functions:

```buff
extern "C" from "tokio" func sleep_ms(ms: Int)

func main():
    sleep_ms(100)
    print("slept 100ms")
```

The `"tokio"` source tells Buff to record `tokio` as a `[rust-deps]`
entry and to emit the call as async-aware (the wrapper body lives in
`externs.rs`).

## Trait system: Buff vs Rust

Buff's trait system is **a subset of Rust's** with one extension
(multiple dispatch, T58). The common case is identical:

```rust
// Rust
trait Shape {
    fn area(&self) -> f64;
}

struct Circle { radius: f64 }

impl Shape for Circle {
    fn area(&self) -> f64 {
        3.14159 * self.radius * self.radius
    }
}
```

```buff
// Buff
trait Shape:
    func area() -> Float

struct Circle:
    radius: Float

impl Shape for Circle:
    func area() -> Float:
        return 3.14159 * radius * radius
```

Differences:

- **No `&self`** — methods access fields by name directly.
- **No `pub`** — everything is public.
- **Colon-and-indent body** instead of braces.
- **No `&mut self`** — mutation is via `mut` bindings.

Associated types, default methods, and trait bounds work the same:

```buff
trait Container<T>:
    type Item
    func get(index: Int) -> Option<Self.Item>
    func first() -> Option<T>:
        return Self.get(0)

func sum<C: Container<Int>>(c: C) -> Int:
    ...
```

### Multiple dispatch (T58)

Buff supports multiple dispatch (Julia-inspired) — a method can
dispatch on the runtime type of multiple arguments:

```buff
trait Number:
    func + other: Number) -> Number

impl Number for Int:
    func + other: Number) -> Number:
        match other {
            Int(n) => return self + n,
            Float(f) => return self.float() + f,
            _ => return Error("type mismatch"),
        }
```

This is a v1.19 feature; see
[`examples/multi_dispatch_basic.buff`](https://github.com/buff-lang/buff/blob/master/examples/multi_dispatch_basic.buff)
for the syntax.

## `match` → `match` (familiar)

Buff's `match` is structurally identical to Rust's:

```rust
// Rust
match value {
    Some(x) => println!("{}", x),
    None => println!("empty"),
}
```

```buff
// Buff
match value {
    Some(x) => print(x),
    None => print("empty"),
}
```

Differences:

- **Braces required** for the body (Buff's `match` doesn't use
  offside-rule for arms — it uses `{ ... }`).
- **Comma separators** between arms, like Rust.
- **Patterns** support the same set: literals, `Some(x)`, `Ok(v)`,
  ranges, `_`, struct patterns.
- **Guards** with `if`: `Some(x) if x > 0 => ...`.
- **Exhaustiveness** is enforced — forgetting an arm is a compile
  error.

See
[`examples/pattern_matching.buff`](https://github.com/buff-lang/buff/blob/master/examples/pattern_matching.buff)
for the full pattern set.

## `Result`/`Option` → identical

Buff's `Result<T, E>` and `Option<T>` are direct lifts from Rust. The
`?` operator works the same way:

```rust
// Rust
fn read_config(path: &str) -> Result<Config, Box<dyn Error>> {
    let text = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&text)?;
    Ok(config)
}
```

```buff
// Buff
func read_config(path: String) -> Result<Config, Error>:
    let text = Filesystem.read(path)?
    let config = Toml.parse(text)?
    return Ok(config)
```

The only differences:

- **No `Box<dyn Error>`** — Buff has a builtin `Error` type.
- **No `?` on `Option`** (yet) — Buff's `?` is for `Result` only.
  Use `??` (null-coalesce) for `Option`.
- **`Error("msg")`** is the constructor (lowers to `Err(Error::new(...))`).

## async/.await → async-transparent

This is the single biggest Rust→Buff delta. In Rust, `async fn` colors
every function in the call graph — you must propagate `async`,
`.await`, and `Pin<Box<dyn Future>>` everywhere. In Buff, there's no
`.await` keyword:

```rust
// Rust
async fn fetch_user(uid: u64) -> User {
    let resp = reqwest::get(&format!("/users/{}", uid)).await?;
    let user: User = resp.json().await?;
    user
}

async fn main() {
    let a = fetch_user(1).await;
    let b = fetch_user(2).await;
    println!("{:?} {:?}", a, b);
}
```

```buff
// Buff
async func fetch_user(uid: Int) -> User:
    let resp = HttpClient.get("/users/" + uid.string()).send()?
    let user = resp.json()?
    return user

func main():
    let a = fetch_user(1)        // no .await
    let b = fetch_user(2)
    print(a, b)
```

The compiler:

1. Sees `fetch_user` is `async func`.
2. Inserts `.await` at every call site of `fetch_user`.
3. Sees `main` now contains `.await`, so `main` becomes async.
4. Emits `#[tokio::main]` on the generated Rust `main`.

You only opt into async by writing `async func` on the leaf
function. Every caller is propagated automatically.

For concurrent execution:

```rust
// Rust
let (a, b) = tokio::join!(fetch_user(1), fetch_user(2));
```

```buff
// Buff
let task_a = spawn fetch_user(1)
let task_b = spawn fetch_user(2)
let a = task_a.result()
let b = task_b.result()
```

See [Async cookbook](../cookbook/async/) for `spawn`, `select`,
timeout, gather.

## `macros!` → `@attribute` + comptime

Rust's `macro_rules!` and proc-macros are powerful but famously hard
to write. Buff takes a different approach: **compiler-baked
attributes** (`@test`, `@prefer(gpu)`, `@comptime`, `@deprecated`,
`@State`, `@Cached`, ...) plus a comptime evaluation subsystem (T53).

For example, the `vec![]` macro doesn't exist in Buff — the literal
`[1, 2, 3]` is the syntax. `println!` is the function `print()`.
`format!` is `.string()` or string concatenation.

For build-time computation (the use case for `concat!`, `env!`,
`include_str!`), Buff has comptime:

```buff
@comptime
func factorial(n: Int) -> Int:
    if n <= 1:
        return 1
    return n * factorial(n - 1)

const TABLE = @comptime [factorial(i) for i in 0..10]
```

The compiler evaluates `factorial` at compile time when arguments are
constant, and embeds the result in the binary. See
[`examples/comptime_fib.buff`](https://github.com/buff-lang/buff/blob/master/examples/comptime_fib.buff)
and [`examples/comptime_lookup_table.buff`](https://github.com/buff-lang/buff/blob/master/examples/comptime_lookup_table.buff).

User-defined proc-macros are not yet supported. If you need one, file
an issue — the macro-system decision is tracked as T5 in the v1.x
roadmap.

## cargo → cargo (same toolchain)

Buff reuses Cargo. The `buff` CLI wraps cargo for common operations
(`buff build` invokes cargo under the hood), but you can also run
cargo directly on the generated Rust if you want.

```bash
# Buff wrappers (preferred)
buff build
buff test
buff fmt

# Direct cargo (advanced — on generated Rust)
buff build --emit rust        # write the .rs files
cd target/buff-build/
cargo build                   # build the generated Rust directly
```

The `Cargo.toml` is auto-generated from `buff.toml` during `buff
build`. You shouldn't edit it directly — your changes will be
overwritten on the next build. Configure via `buff.toml` instead.

### Workspace support

Buff supports Cargo workspaces (T123). A multi-crate Buff project
looks like:

```
my_workspace/
├── buff.toml          # workspace manifest
├── crates/
│   ├── my_lib/
│   │   └── buff.toml
│   └── my_app/
│       └── buff.toml
```

See the [T123 workspace test](https://github.com/buff-lang/buff/blob/master/crates/buff-lang-cli/tests/t123_workspace.rs)
for the expected shape.

## Common pitfalls

The five things that trip up Rust developers most:

### 1. Forgetting that there are no references

The biggest habit to unlearn. In Rust, you write `&str`, `&Vec<T>`,
`&mut HashMap`. In Buff, everything is owned. The compiler inserts
clones and borrows behind the scenes.

```rust
// Rust
fn first(v: &Vec<i32>) -> i32 {
    v[0]
}
```

```buff
// Buff — no references
func first(v: Vector<Int>) -> Int:
    return v[0]
```

This looks expensive (clone on every call), but the compiler is smart
enough to use references internally when safe. From the user's
perspective, the call is free.

### 2. Looking for `.iter()` and `.collect()`

Rust's iterator traits (`Fn`/`FnMut`/`FnOnce`, `Iterator`,
`IntoIterator`) are hidden in Buff. `.map()` and `.filter()` work on
`Vector<T>` directly:

```rust
// Rust
let doubled: Vec<i32> = vec![1, 2, 3]
    .iter()
    .map(|x| x * 2)
    .collect();
```

```buff
// Buff
let doubled = [1, 2, 3].map({ x => x * 2 })
```

No `.iter()`, no `.collect()`, no turbofish. The output type is
inferred from usage.

### 3. Missing `.await`? No — there is none

Rust developers reflexively type `.await` after async calls. In Buff,
this is a parse error. The compiler inserts `.await` for you.

```buff
// WRONG
let user = fetch_user(1).await     // ERROR: unexpected `.await`
```

```buff
// RIGHT
let user = fetch_user(1)           // OK — .await inserted by compiler
```

This takes about a day to internalize. Your editor's LSP will flag
the error immediately.

### 4. `String` vs `&str` is gone

In Rust, you choose between `String` (owned) and `&str` (borrowed)
constantly. Buff has only `String` — the compiler chooses `&str` at
the generated Rust layer when safe.

```buff
// Buff — only String
let s = "hello"              // String
let upper = s.to_uppercase() // String
```

You never write `&str` in user code. If you `extern` a Rust crate,
the FFI layer handles the conversion.

### 5. `unwrap`/`expect`/`panic!` are forbidden

Rust's `.unwrap()` is a footgun — it panics on `None`/`Err`. Buff
**bans** it in non-test code:

```buff
// WRONG
let x = maybe.unwrap()    // ERROR: unwrap is forbidden in non-test code
```

You must handle the `Option`/`Result` explicitly:

```buff
// RIGHT
let x = match maybe {
    Some(v) => v,
    None => return Error("was none"),
}
// Or with ?? :
let x = maybe ?? default_value
```

In test code (functions marked `@test`), `unwrap` is allowed. This
matches Rust's convention but is enforced by the compiler.

## Where to go next

1. **Install Buff**: [Getting Started → Installation](../getting-started/installation/).
2. **First program**: [Getting Started → First program](../getting-started/first-program/).
3. **Skim the syntax**: [Language → Syntax](../language/syntax/).
4. **Browse Rust-vs-Buff examples**: [`examples/rust-vs-buff/`](https://github.com/buff-lang/buff/tree/master/examples/rust-vs-buff)
   has 14 side-by-side comparisons (hello world, functions, recursion,
   borrow checker, pattern matching, error handling, closures,
   iterators, collections, null safety, structs, enums, lifetimes,
   async/await).
5. **Read the FFI guide**: [`crates/buff-lang-ffi-guide/GUIDE.md`](https://github.com/buff-lang/buff/blob/master/crates/buff-lang-ffi-guide/GUIDE.md)
   — 6 hard rules for `extern` wrapper crates.
6. **Browse the cookbook**: [Cookbook](../cookbook/_index/) — 55
   recipes, each with a Problem → Solution → Explanation structure.
7. **Try the LSP**: [VSCode extension](https://github.com/buff-lang/buff/tree/master/editors/vscode)
   bundles `buff-lsp`. Hover, completion, goto-definition work out of
   the box.

You're already 80% of the way there. The remaining 20% is
internalizing "no `&`, no `'a`, no `.await`, no `.unwrap()`" —
practice with the examples and it'll stick in a day.

If you get stuck, file an issue in the [buff] repo — the onboarding
guides are tracked by T69 and updated as the language evolves.

[buff]: https://github.com/buff-lang/buff
