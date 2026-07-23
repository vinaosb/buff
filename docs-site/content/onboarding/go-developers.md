+++
title = "Buff for Go developers"
weight = 48
+++

# Buff for Go developers

> Go is the closest spiritual sibling to Buff: garbage-free,
> concurrency-first, indentation-friendly, designed for servers and
> CLI tools. Buff keeps Go's ergonomics (channels, goroutines, simple
> syntax) while removing its pain points (nil panics, `if err != nil`
> litter, manual interface satisfaction). The result is a language
> that feels like Go with safety rails — no nil, no silent error
> drops, no `interface{}` escape hatch.

This guide assumes you can read Go 1.20+ and have written goroutines,
channels, and `if err != nil`. You'll be productive in Buff in 30
minutes.

## Why Buff?

Go developers evaluating Buff typically want one or more of:

1. **No `nil`.** Go's billion-dollar mistake. Buff has no `nil`; absence
   is `Option<T>`, failure is `Result<T, E>`. Both are matched
   exhaustively.
2. **No `if err != nil` boilerplate.** Go forces you to write this 5
   times per function. Buff's `?` operator propagates errors in one
   character, like Rust.
3. **Real generics with bounds.** Go's generics (1.18+) are limited —
   no method bounds on type parameters, no associated types, no
   operator overloading. Buff's trait system has all three.
4. **Stronger typing for interfaces.** Go interfaces are structural
   (duck-typed). Buff traits are nominal (declared) — you know at
   compile time whether a type implements a trait.
5. **Native performance, no GC pauses.** Buff compiles to native code
   via LLVM. There's no garbage collector — memory is managed at
   compile time via Rust's ownership model (which Buff hides from
   you).
6. **GPU dispatch.** A Buff function can run on CPU or be dispatched
   to GPU automatically. Go has no equivalent.
7. **Same goroutine ergonomics.** `spawn fn()` is one token longer
   than `go fn()` and behaves the same. `Channel<T>` is the same idea
   as Go's `chan T`.

The trade-off: Buff's standard library is smaller than Go's, and the
ecosystem is younger. For greenfield server work, Buff is competitive
today; for replacing an existing Go service with extensive stdlib
usage (`net/http`, `database/sql`, `encoding/json`), expect to wire
some `buff-*` crates manually.

## Syntax mapping table

### Fundamentals

| Go | Buff | Notes |
|---|---|---|
| `package main` | (implicit) | The file's location determines the package. |
| `import "fmt"` | (implicit prelude) | `print`, `len`, etc. are in the prelude. |
| `func f() {` | `func f():` | Same keyword, colon + indent instead of braces. |
| `func f(x int) int {` | `func f(x: Int) -> Int:` | Param syntax: `name: Type`, return `-> Type`. |
| `func (s T) M() {` | `func T.M():` | Method syntax: `Type.method`. |
| `func (s *T) M() {` | `func T.M():` (mutate via `mut`) | No pointer receivers. |
| `var x int = 5` | `let x: Int = 5` | Or just `let x = 5` (inferred). |
| `x := 5` | `let x = 5` | `let` is required; no `:=`. |
| `var x = 5` | `let mut x = 5` | `mut` opts into mutation. |
| `const X = 5` | `const X = 5` | Same. |
| `// comment` | `// comment` | Same. |
| `/* block */` | (none — use multiple `//`) | No block comments. |
| 4-space indent | 4-space indent | Tabs are forbidden in Buff. |

### Types

| Go | Buff | Notes |
|---|---|---|
| `int`, `int32`, `int64` | `Int` | Default integer type. |
| `uint`, `uint64` | `UInt` | Default unsigned. |
| `float64` | `Float` | Default float. |
| `string` | `String` | Capital S. |
| `bool` | `Bool` | Capital B. |
| `byte` | `Byte` | Capital B. |
| `[]int` | `Vector<Int>` | Slice → growable vector. |
| `[N]int` (array) | (no equivalent) | Use `Vector<T>`. |
| `map[string]int` | `Map<String, Int>` | Renamed. |
| `struct{...}` | `struct` | Declared type. |
| `interface{}` / `any` | (forbidden) | Use generics or `trait`. |
| `*T` pointer | (no equivalent) | Hidden. |
| `error` | `Result<T, Error>` | Errors are values. |
| `time.Time` | `DateTime` | Prelude type. |
| `time.Duration` | `Duration` | Prelude type. |
| `context.Context` | (planned) | Use explicit cancellation channels for now. |

### Strings

| Go | Buff | Notes |
|---|---|---|
| `"hello"` | `"hello"` | Same. |
| `` `raw` `` | (none yet) | No raw strings. |
| `fmt.Sprintf("%d", n)` | `n.string()` | No format strings. |
| `fmt.Println(a, b)` | `print(a, b)` | Multi-arg print. |
| `strings.Split(s, ",")` | `s.split(",")` | Method on string. |
| `strings.Join(parts, ",")` | `parts.join(",")` | Method on vector. |
| `strings.ToUpper(s)` | `s.to_uppercase()` | Method. |
| `strings.Contains(s, "x")` | `s.contains("x")` | Method. |
| `strings.TrimSpace(s)` | `s.trim()` | Method. |
| `len(s)` | `s.len()` | Method. |
| `s + "!"` | `s + "!"` | Same. |

### Collections

| Go | Buff | Notes |
|---|---|---|
| `[]int{1, 2, 3}` | `[1, 2, 3]` | Literal. |
| `make([]int, 0, 10)` | `Vector<Int>.new()` | Or just `[]`. |
| `append(s, x)` | `s.push(x)` | Method on vector. |
| `len(s)` | `s.len()` | Method. |
| `cap(s)` | (no equivalent) | No capacity exposed. |
| `s[i]` | `s[i]` | Same. |
| `s[1:3]` (slice) | `s.slice(1, 3)` | Method. |
| `for i, v := range s` | `for (i, v) in s.enumerate():` | Enumeration via `.enumerate()`. |
| `for v := range s` | `for v in s:` | Same shape. |
| `map[string]int{"a": 1}` | `{"a": 1}` | Literal. |
| `m["a"]` | `m.get("a")` | Returns `Option<T>`. |
| `delete(m, "k")` | `m.remove("k")` | Method. |
| `if v, ok := m["k"]; ok` | `match m.get("k") { Some(v) => ..., None => ... }` | Match instead of comma-ok. |

### Control flow

| Go | Buff | Notes |
|---|---|---|
| `if c { ... }` | `if c: ...` | Colon + indent. |
| `else if` | `else if` | Same. |
| `for i := 0; i < 10; i++ { }` | `for i in 0..10:` | C-style for is gone. |
| `for cond { }` | (no equivalent) | Use `for x in iter` or recursion. |
| `for { }` (infinite) | (no equivalent) | Use recursion. |
| `for range ch { }` | `for x in ch:` | Channel iteration. |
| `switch x { case A: }` | `match x { Pat => ... }` | Renamed; braces required. |
| `switch { case cond: }` | `if cond: ...` | Switch-without-value → if chain. |
| `break`, `continue` | `break`, `continue` | Same. |
| `goto LABEL` | (no equivalent) | No goto. |
| `defer cleanup()` | (no equivalent) | RAII handles cleanup; see pitfall #5. |
| `return x` | `return x` | Same. |

### Functions and types (advanced)

| Go | Buff | Notes |
|---|---|---|
| `func f(x, y int)` | `func f(x: Int, y: Int)` | Each param needs a type. |
| `func f() (int, error)` | `func f() -> Result<Int, Error>` | Tuple return → Result. |
| `func f() (int, string)` | (no equivalent — use a struct) | No multi-return. |
| `func Variadic(xs ...int)` | `func Variadic(xs: Vector<Int>)` | No `...` syntax; pass a vector. |
| `func f() func()` | `func f() -> (func() -> Unit)` | Higher-order functions supported. |
| `type Foo struct { X int }` | `struct Foo: X: Int` | Colon + indent. |
| `type Foo interface { M() }` | `trait Foo: func M()` | Renamed; colon + indent. |
| `type Alias = int` | `type Alias = Int` | Same. |
| `go f()` | `spawn f()` | Renamed `go` → `spawn`. |
| `go func() { ... }()` | `spawn { ... }` | Anonymous spawn. |

### Concurrency

| Go | Buff | Notes |
|---|---|---|
| `go f()` | `spawn f()` | Renamed. |
| `go func() { ... }()` | `spawn { ... }` | Anonymous. |
| `chan int` | `Channel<Int>` | Renamed. |
| `make(chan int)` | `Channel.new(buffer)` | Buffered by default. |
| `make(chan int, 8)` | `Channel.new(8)` | Same. |
| `ch <- x` | `sender.send(x)` | Method-style send. |
| `x := <-ch` | `x = receiver.recv()` | Method-style receive. |
| `close(ch)` | (sender dropped on scope exit) | Closing is automatic. |
| `select { case ... }` | (planned) | Or `Channel.recv_timeout(d)`. |
| `sync.Mutex` | (runtime-managed) | Hidden; you don't write locks. |
| `sync.WaitGroup` | (use `spawn` + `task.result()`) | Or `Channel<T>`. |
| `context.WithCancel` | (use explicit cancellation) | No Context type yet. |
| `time.Sleep(d)` | `sleep(d)` | Async-aware. |

### Errors

| Go | Buff | Notes |
|---|---|---|
| `if err != nil { return err }` | `?` (one character) | Early-return on `Err`. |
| `errors.New("msg")` | `Error("msg")` | Constructor. |
| `fmt.Errorf("x: %w", err)` | `Error("x: " + err.string())` | No wrapping helpers yet. |
| `errors.Is(err, Target)` | `match err { Target => ... }` | Match instead. |
| `errors.As(err, &target)` | `match err { ... }` | Match instead. |
| `panic("x")` | (forbidden in non-test code) | Match or propagate. |
| `recover()` | (no equivalent) | No panics to recover from. |
| `defer func() { recover() }()` | (no equivalent) | N/A. |

## Tooling migration

Go's toolchain is famously minimal — `go build`, `go test`, `go fmt`.
Buff's is similarly lean.

| Go | Buff | Notes |
|---|---|---|
| `go build` | `buff build` | Compiles to a native binary. |
| `go run` | `buff run <file>` | Compile + execute. |
| `go test` | `buff test` | Discovers `@test` functions. |
| `go fmt` | `buff fmt` | Indent-based formatter (4 spaces). |
| `go vet` | `buff check` | Type-checker + linter combined. |
| `go mod init` | `buff init` | Creates `buff.toml`. |
| `go mod tidy` | `buff deps` | Resolves and writes `buff.lock`. |
| `go get pkg` | `buff add pkg` | Adds a dep to `buff.toml`. |
| `go install pkg` | `buff install pkg` | Installs a binary. |
| `go list -m -u all` | `buff outdated` | Lists outdated deps. |
| `go env` | (env vars only) | No `go env` equivalent. |
| `go doc` | `buff doc` (planned) | Doc generator. |
| `go generate` | (use `@comptime`) | Comptime replaces codegen. |
| `go work` | `buff.toml` workspace section | Multi-module workspace. |
| `gofmt` / `goimports` | `buff fmt` | One formatter. |
| `golangci-lint` | `buff check` | Linter is built-in. |
| `go.mod` | `buff.toml` | Manifest. |
| `go.sum` | `buff.lock` | Lockfile (gitignored). |
| `GOPATH` / `GOBIN` | `~/.buff/` | Per-user cache. |
| `go tool pprof` | (planned) | Profiler. |
| `go tool cover` | `buff coverage` (T137) | Coverage mapping. |
| `delve` (dlv) | `buff-dap` (T139) | Debug Adapter Protocol. |
| `gopls` | `buff-lsp` | LSP server (v1.2). |

### The project layout

A `buff new my_app` looks like:

```
my_app/
├── buff.toml          # project manifest (like go.mod)
├── src/
│   └── main.buff      # entry point (like main.go)
└── tests/
    └── test_main.buff # tests (like main_test.go)
```

Compare to `go mod init my_app`:

```
my_app/
├── go.mod
├── main.go
└── main_test.go
```

Buff's layout has a `src/` directory by convention. Go's is flat. The
differences end there — both have a manifest, a main file, and a
sibling test file.

### Dependency declaration

In `go.mod`:

```
module my_app

go 1.21

require (
    github.com/lib/pq v1.1.0
    github.com/gorilla/mux v1.8.0
)
```

In `buff.toml`:

```toml
[package]
name = "my_app"
version = "0.1.0"
edition = "2021"

[deps]
buff_db = "1.0"
buff_web = "1.0"

[rust-deps]
# Direct Rust crate deps go here, bound via `extern`
```

`[deps]` are Buff packages from `buff-registry`; `[rust-deps]` are
raw Rust crates reachable via `extern` FFI (see the [Rust developer
guide](./rust-developers/) for FFI details).

## Ecosystem mapping

Go's standard library is enormous and well-regarded. Buff's is
smaller but covers the same ground via prelude types and `buff-*`
crates.

| Go package | Buff equivalent | Notes |
|---|---|---|
| `fmt` | `print`, `.string()` | In prelude. |
| `strings` | `Strings` (prelude) | `split`, `join`, `contains`, etc. |
| `strconv` | `.string()`, `.int()`, `.float()` | Method-style conversions. |
| `regexp` | `Regex` (prelude) | Wraps `regex` crate. |
| `time` | `DateTime`, `Duration` (prelude) | Time arithmetic. |
| `os` | `Env`, `Args`, `Process` (prelude) | OS interaction. |
| `io` | `Filesystem`, `Path` (prelude) | File I/O. |
| `path/filepath` | `Path` (prelude) | Path manipulation. |
| `encoding/json` | `Toml.parse` / `Json` (prelude) | JSON parsing. |
| `encoding/csv` | `Csv` (prelude) | CSV reader. |
| `encoding/xml` | `Xml` (v1.18+) | XML reader. |
| `encoding/binary` | (planned) | Binary protocol. |
| `net/http` | `HttpClient` (prelude) + `buff-web` (v1.15+) | Client + server. |
| `net/url` | `URL` (prelude) | URL parsing. |
| `net` | `TCP`, `UDP` (prelude) | Low-level networking. |
| `database/sql` | `buff-db` (v1.15+) | Database access. |
| `crypto/sha256` | `Hash` (prelude) | Hashing. |
| `crypto/hmac` | `HMAC` (prelude) | HMAC. |
| `crypto/rand` | `Random.secure_bytes(N)` | CSPRNG. |
| `encoding/base64` | `Base64` (prelude) | Encode/decode. |
| `log` | `Log` (prelude) | Structured logging. |
| `sync` | (hidden) | Runtime-managed locks. |
| `context` | (planned) | Cancellation context. |
| `os/exec` | `Process.run(cmd)` | Spawn external processes. |
| `flag` | `Args` (prelude) | CLI parsing. |
| `testing` | `buff test` (built-in) + `@test` | Test runner. |
| `runtime` | (no equivalent) | No runtime introspection. |
| `reflect` | (no equivalent) | Static types only. |
| `sort` | `.sort()`, `.sort_by(key)` | Method on vectors. |
| `math` | `Math` (prelude) | `pow`, `sqrt`, `sin`, etc. |
| `math/rand` | `Random` (prelude) | RNG. |
| `image` | `buff-image` (v1.14+) | Image processing. |
| `html/template` | `buff-template` (v1.15+) | HTML templating. |
| `text/template` | `buff-template` (v1.15+) | General templating. |

For Go packages without a Buff equivalent, you can almost always find
a Rust crate that does the same thing and bind it via `extern`. See
the [Rust developer guide](./rust-developers/) for the FFI story.

## Hello World, side by side

A canonical first program. Print a greeting, count to three, do a
tiny calculation.

### Go

```go
package main

import (
    "fmt"
    "os"
)

func greet(name string) string {
    return fmt.Sprintf("Hello, %s!", name)
}

func main() {
    for i := 1; i <= 3; i++ {
        fmt.Printf("count: %d\n", i)
    }
    who := "World"
    if len(os.Args) > 1 {
        who = os.Args[1]
    }
    fmt.Println(greet(who))
    fmt.Printf("2 + 2 = %d\n", 2+2)
}
```

### Buff

```buff
func greet(name: String) -> String:
    return "Hello, " + name + "!"

func main():
    for i in 1..=3:
        print("count: " + i.string())
    let args = Args.all()
    let who = if args.len() > 1 { args[1] } else { "World" }
    print(greet(who))
    print("2 + 2 = " + (2 + 2).string())
```

### Line-by-line

- **`package main`** → gone. The file's location determines the
  package; there's no declaration needed.
- **`import ("fmt", "os")`** → gone. `print` and `Args` are in the
  prelude; no imports needed.
- **`func greet(name string) string`** → `func greet(name: String) ->
  String`. The param syntax is `name: Type`; the return type is `->
  Type`.
- **`fmt.Sprintf("Hello, %s!", name)`** → `"Hello, " + name + "!"`.
  No format strings; use string concatenation. (Interpolation is
  planned.)
- **`for i := 1; i <= 3; i++`** → `for i in 1..=3`. C-style for is
  gone; use range syntax. `1..=3` is inclusive; `1..3` is exclusive.
- **`fmt.Printf("count: %d\n", i)`** → `print("count: " + i.string())`.
  Integers aren't auto-stringified; call `.string()` explicitly.
  `print` adds a newline by default.
- **`os.Args[1]`** → `Args.all()[1]`. The `Args` module wraps OS args.
- **`if len(os.Args) > 1 { who = os.Args[1] }`** → `let who = if
  args.len() > 1 { args[1] } else { "World" }`. Buff's `if` is an
  expression — it returns a value. No need to declare `who` first and
  mutate it.
- **`fmt.Println(...)`** → `print(...)`. Lowercase, in prelude.
- **`fmt.Printf("2 + 2 = %d\n", 2+2)`** → `print("2 + 2 = " + (2 +
  2).string())`. Same idea, explicit stringify.

## `spawn` vs goroutines

Buff's `spawn` is the direct equivalent of Go's `go`. Both schedule a
function to run on a runtime-managed thread pool. The differences are
mostly cosmetic.

### Go

```go
func work(id int) {
    fmt.Println("working", id)
    time.Sleep(100 * time.Millisecond)
    fmt.Println("done", id)
}

func main() {
    go work(1)
    go work(2)
    time.Sleep(200 * time.Millisecond) // wait for goroutines
}
```

### Buff

```buff
func work(id: Int):
    print("working " + id.string())
    sleep(Duration.millis(100))
    print("done " + id.string())

func main():
    spawn work(1)
    spawn work(2)
    sleep(Duration.millis(200)) // wait for spawned tasks
```

Note the parallel structure. Differences:

- `go` → `spawn`.
- `time.Sleep(d)` → `sleep(d)` (async-aware; blocks the current task).
- Buff's `spawn` returns a `Task<T>` you can `.result()` on (next
  section).

### Waiting for results

Go's idiomatic pattern is channels + WaitGroup. Buff's is `spawn` +
`task.result()`:

```go
// Go
func fetch(url string) string {
    // ...
    return body
}

func main() {
    ch := make(chan string, 2)
    go func() { ch <- fetch("/a") }()
    go func() { ch <- fetch("/b") }()
    a := <-ch
    b := <-ch
    fmt.Println(a, b)
}
```

```buff
// Buff
func fetch(url: String) -> String:
    // ...
    return body

func main():
    let task_a = spawn fetch("/a")
    let task_b = spawn fetch("/b")
    let a = task_a.result()
    let b = task_b.result()
    print(a, b)
```

`task.result()` blocks the current task until the spawned task
completes, then returns its value. This is the Buff equivalent of
Go's `<-ch` with a buffered channel of size 1.

## Channel vs channels

Buff's `Channel<T>` (T2, v1.13) is an MPSC (multi-producer,
single-consumer) primitive — same idea as Go's `chan T`, but with a
slightly different API.

### Go

```go
func producer(ch chan<- int) {
    for i := 1; i <= 10; i++ {
        ch <- i
    }
    close(ch)
}

func main() {
    ch := make(chan int, 8)
    go producer(ch)
    sum := 0
    for v := range ch {
        sum += v
    }
    fmt.Println(sum)
}
```

### Buff

```buff
func producer(sender: Sender<Int>):
    for i in 1..=10:
        sender.send(i)
    // sender is dropped on scope exit; receiver sees None

func main():
    let (sender, receiver) = Channel.new(8)
    spawn producer(sender)
    var sum: Int = 0
    for let next = receiver.recv():
        sum = sum + next
    print(sum)
```

Differences:

- `Channel.new(buffer)` returns a `(Sender<T>, Receiver<T>)` tuple
  — Buff doesn't have a unified bidirectional channel type.
- `ch <- x` → `sender.send(x)` (method).
- `<-ch` → `receiver.recv()` (method, returns `Option<T>`).
- `close(ch)` → implicit (sender dropped on scope exit).
- `for v := range ch` → `for let next = receiver.recv():` (loops
  until `recv()` returns `None`).

Buff's `Channel<T>` lowers to `tokio::sync::mpsc::channel` and is
fully async-aware. See
[`examples/channels/producer_consumer.buff`](https://github.com/buff-lang/buff/blob/master/examples/channels/producer_consumer.buff)
for the canonical pattern.

## Interfaces vs traits

Go's interfaces are **structural** — a type satisfies an interface
implicitly if it has the right methods. Buff's traits are
**nominal** — you must explicitly `impl Trait for Type`.

### Go

```go
type Shape interface {
    Area() float64
}

type Circle struct {
    Radius float64
}

func (c Circle) Area() float64 {
    return 3.14159 * c.Radius * c.Radius
}

// Circle implicitly satisfies Shape — no declaration needed.
```

### Buff

```buff
trait Shape:
    func area() -> Float

struct Circle:
    radius: Float

impl Shape for Circle:
    func area() -> Float:
        return 3.14159 * radius * radius
```

Buff requires the explicit `impl Shape for Circle`. This is more
verbose but catches bugs (a method named `are` instead of `area` is
a compile error, not a silent interface non-satisfaction).

The trade-off:

- **Go's structural interfaces** are great for mocking and
  composition — you can define an interface after the fact and any
  type with matching methods satisfies it.
- **Buff's nominal traits** are great for refactoring safety —
  renaming a method breaks every `impl` explicitly, so you can't
  accidentally break an interface.

Buff's trait system also supports **associated types**, **default
methods**, and **bounds** — features Go's interfaces lack.

## Goroutine scheduling → tokio runtime

Go has a runtime-managed goroutine scheduler (the M:N scheduler in
`runtime/proc.go`). Buff has tokio — a mature Rust async runtime.

You don't see tokio in user code. The compiler:

1. Detects which functions are `async func` (or transitively async).
2. Inserts `.await` at async call sites.
3. Emits `#[tokio::main]` on `main` when it joins the async set.
4. Spawns `spawn` tasks onto tokio's worker pool.

The runtime behavior is similar to Go's:

- **Work-stealing scheduler** — tokio's default is multi-threaded
  work-stealing, same as Go's.
- **Cooperative scheduling** — async functions yield at `.await`
  points (which Buff inserts automatically), same as Go's goroutine
  yields at function call boundaries.
- **No GC** — Buff has no garbage collector. Memory is managed via
  Rust's ownership model (hidden from you).
- **Preemption** — tokio does NOT preempt async tasks at arbitrary
  points; long-running CPU-bound tasks should use `spawn_blocking` (or
  just `par_map`) to avoid starving the runtime. Go's goroutines are
  preempted at function calls.

For CPU-bound work, Buff's `par_map` / `par_filter` / `par_reduce`
(run on Rayon, a separate thread pool) are the right tool. For
I/O-bound work, `spawn` + `Channel<T>` + async functions are right.

## go fmt → buff fmt

Buff's formatter is opinionated, like Go's `gofmt`. The rules:

- 4 spaces per indent (no tabs).
- No trailing whitespace.
- No more than 2 consecutive blank lines.
- Named arguments for multi-arg calls with booleans: `f(a, b, opt:
  true)`.
- Trailing commas in multi-line collections.

Run it on save (your editor can do this via `buff-lsp`) or via CLI:

```bash
buff fmt                  # format every .buff file in the project
buff fmt path/to/file.buff # format a single file
buff fmt --check          # check without writing (CI mode)
```

`buff fmt --check` is what CI runs (mirrors `gofmt -l`). Failing CI
means you forgot to run the formatter locally.

## go modules → buff add

Buff's package model mirrors Go modules.

```bash
# Go
go mod init my_app
go get github.com/lib/pq
go mod tidy

# Buff
buff init                 # creates buff.toml
buff add buff_db          # adds buff_db = "1.0" to buff.toml
buff deps                 # resolves and writes buff.lock
```

The `buff.lock` file is auto-generated (and gitignored by default).
It records the exact resolved versions of every transitive dependency.
Think of it as Go's `go.sum` — but binary hashes, not module hashes.

Publishing a package:

```bash
# Go: not built-in. Use GitHub releases + a vanity import path.
# Buff: built-in.
buff publish               # uploads to buff-registry
buff install pkg           # installs a binary
```

Buff has a first-class registry server (`buff-registry`, T126-T127)
that handles publishing, version resolution, and distribution. Go
relies on vanity import paths + `go get` from any git host. Buff's
approach is more like npm/crates.io.

## Error handling: Buff Result vs Go's `if err != nil`

This is the biggest day-to-day improvement over Go. Go forces you to
write `if err != nil { return err }` five times per function. Buff's
`?` operator does it in one character.

### Go

```go
func read_config(path string) (Config, error) {
    text, err := os.ReadFile(path)
    if err != nil {
        return Config{}, err
    }
    config, err := parseConfig(text)
    if err != nil {
        return Config{}, err
    }
    return config, nil
}
```

### Buff

```buff
func read_config(path: String) -> Result<Config, Error>:
    let text = Filesystem.read(path)?
    let config = parse_config(text)?
    return Ok(config)
```

The `?` after `Filesystem.read(path)` says: "if this returned an
error, return it from `read_config` immediately; otherwise, unwrap
the Ok value into `text`."

The Buff version is **half the lines and clearer**. The error
propagation is implicit and uniform — you can't forget to handle an
error because the compiler enforces it.

At the call site, you handle errors with `match`:

```go
// Go
config, err := read_config("config.toml")
if err != nil {
    log.Fatal(err)
}
fmt.Println(config)
```

```buff
// Buff
match read_config("config.toml") {
    Ok(config) => print(config),
    Err(e) => Log.error(e.string()),
}
```

Or with the `??` null-coalesce operator for default values:

```buff
let config = read_config("config.toml") ?? Config.default()
```

## Struct embedding vs trait composition

Go's struct embedding is its substitute for inheritance — embed a
struct, and its methods are promoted to the outer struct.

### Go

```go
type Logger struct{}

func (l Logger) Log(msg string) {
    fmt.Println(msg)
}

type Server struct {
    Logger  // embedded
    Addr string
}

func main() {
    s := Server{Addr: ":8080"}
    s.Log("starting") // promoted from Logger
}
```

### Buff

Buff has no struct embedding. The equivalent is composition +
delegation via traits:

```buff
struct Logger:
    func log(msg: String):
        print(msg)

struct Server:
    addr: String
    logger: Logger

impl Server:
    func log(msg: String):
        logger.log(msg)   // explicit delegation

func main():
    let s = Server { addr: ":8080", logger: Logger.new() }
    s.log("starting")
```

The Buff version is more verbose (you write the delegation method
explicitly), but it's also more explicit — you can see exactly which
methods are forwarded and which are overridden. The Go "promotion"
magic doesn't exist.

For shared behavior across types, use traits:

```buff
trait Loggable:
    func log(msg: String)

impl Loggable for Server:
    func log(msg: String):
        print("[server] " + msg)

impl Loggable for Worker:
    func log(msg: String):
        print("[worker] " + msg)

func announce(l: Loggable, msg: String):
    l.log(msg)
```

## Common pitfalls

The five things that trip up Go developers most:

### 1. No `nil`

Go's `nil` is everywhere — pointers, slices, maps, channels, interfaces,
functions. Buff has none. Absence is `Option<T>`:

```go
// Go
var p *Person           // nil
var s []int             // nil
var m map[string]int    // nil
```

```buff
// Buff
let p: Option<Person> = None
let s: Vector<Int> = []     // empty, not None
let m: Map<String, Int> = {} // empty, not None
```

Collections are never "null" — they're empty. Optional values are
explicitly `Option<T>`. This eliminates a whole class of nil-deref
panics.

### 2. No multi-return

Go's `(value, error)` pattern is everywhere. Buff doesn't have
multi-return; you return a `Result<T, E>`:

```go
// Go
value, err := doThing()
if err != nil {
    return err
}
```

```buff
// Buff
let value = doThing()?    // ? propagates the error
```

For genuinely two-value returns (not error-value), use a struct:

```buff
struct Pair:
    first: Int
    second: String

func two_values() -> Pair:
    return Pair { first: 1, second: "two" }
```

### 3. No `defer`

Go's `defer` runs a function when the enclosing function returns.
Buff has no `defer`; resources are cleaned up automatically via RAII
when their binding goes out of scope:

```go
// Go
func readFile(path string) {
    f, err := os.Open(path)
    if err != nil {
        return
    }
    defer f.Close()
    // ... use f ...
}
```

```buff
// Buff
func read_file(path: String):
    let f = Path.open(path)?
    // ... use f ...
    // f is closed automatically when the function returns
```

This catches the "forgot to Close" bug class. For custom cleanup, use
a wrapper type that implements Drop semantics (advanced; see the
[Rust developer guide](./rust-developers/)).

### 4. Closures have a different syntax

Go's closures use `func(params) { body }` (anonymous function syntax).
Buff uses `{ params => body }`:

```go
// Go
doubled := mapper(func(x int) int { return x * 2 })
```

```buff
// Buff
let doubled = mapper({ x => x * 2 })
```

Buff's syntax is shorter and matches the lambda style of ML-family
languages (OCaml, Haskell, Swift).

### 5. Indentation is the syntax

Go uses braces. Buff uses indentation (like Python). Mixing tabs and
spaces is a hard lexer error — set your editor to "insert spaces for
tabs" and set the width to 4. `buff fmt` enforces this.

```go
// Go
if x > 0 {
    fmt.Println("positive")
}
```

```buff
// Buff
if x > 0:
    print("positive")
```

Bonus pitfall: Buff's `if`/`else if`/`else` chains use indent blocks,
not braces. The `else` keyword goes at the same indent as the
matching `if`:

```buff
if x > 0:
    print("positive")
else if x < 0:
    print("negative")
else:
    print("zero")
```

## Where to go next

1. **Install Buff**: [Getting Started → Installation](../getting-started/installation/).
2. **First program**: [Getting Started → First program](../getting-started/first-program/).
3. **Skim the syntax**: [Language → Syntax](../language/syntax/).
4. **Browse the cookbook**: [Cookbook](../cookbook/_index/) — 55
   recipes. The [Async](../cookbook/async/) and
   [Parallel](../cookbook/parallel/) pages are the closest analogues
   to Go's goroutine/channel patterns.
5. **Read the Channel example**: [`examples/channels/producer_consumer.buff`](https://github.com/buff-lang/buff/blob/master/examples/channels/producer_consumer.buff)
   — the canonical Go-style producer/consumer in Buff.
6. **Browse the frameworks**: [Frameworks → Overview](../frameworks/overview/)
   — every `buff-*` crate, including `buff-web` (v1.15+, the
   net/http equivalent).
7. **Try the LSP**: [VSCode extension](https://github.com/buff-lang/buff/tree/master/editors/vscode)
   bundles `buff-lsp` for hover, completion, goto-definition.

If you get stuck, file an issue in the [buff] repo — the onboarding
guides are tracked by T69 and updated as the language evolves.

[buff]: https://github.com/buff-lang/buff
