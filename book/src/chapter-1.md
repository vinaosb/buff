# Chapter 1 — Getting Started

This chapter gets a working `buff` compiler on your machine and runs your first
program. By the end you will have:

- installed the `buff` CLI,
- written and run "Hello, Buff!",
- scaffolded a new project with `buff new`,
- run the standalone typechecker with `buff check`,
- understood the layout of a Buff project on disk.

Let's go.

## 1.1 Installation

Buff ships prebuilt release binaries for the five major platforms. Pick one
channel — they all install the same `buff` CLI.

### Option A — Prebuilt binary (all platforms)

Every tagged release publishes stripped, compressed binaries to
[GitHub Releases](https://github.com/buff-lang/buff/releases). Download the
archive matching your OS/arch, extract it, and put `buff` on your `PATH`:

| Platform | Archive |
|---|---|
| Linux x64 | `buff-vX.Y.Z-linux-x64.tar.gz` |
| Linux arm64 | `buff-vX.Y.Z-linux-arm64.tar.gz` |
| macOS x64 (Intel) | `buff-vX.Y.Z-macos-x64.tar.gz` |
| macOS arm64 (Apple Silicon) | `buff-vX.Y.Z-macos-arm64.tar.gz` |
| Windows x64 | `buff-vX.Y.Z-windows-x64.zip` |

Each archive also has a `.sha256` checksum sidecar for integrity verification.

### Option B — Homebrew (macOS / Linux)

```bash
brew tap buff-lang/tap https://github.com/buff-lang/homebrew-tap
brew install buff
```

### Option C — Scoop (Windows)

```powershell
scoop bucket add buff https://github.com/buff-lang/scoop-bucket
scoop install buff
```

### Option D — `buffup` version manager

[`buffup`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buffup)
manages multiple Buff releases side-by-side, rustup-style. It downloads the
same prebuilt binaries into `~/.buff/versions/` and points `~/.buff/bin/buff`
at the active one:

```bash
cargo install --git https://github.com/buff-lang/buff --path crates/buffup
buffup install 1.24.0
buffup default 1.24.0
echo 'export PATH="$HOME/.buff/bin:$PATH"' >> ~/.bashrc
buff --version
```

### Option E — build from source

You need Rust 1.95.0 (pinned in the repo's `rust-toolchain.toml`). Then:

```bash
git clone https://github.com/buff-lang/buff.git
cd buff
cargo build --release -p buff-lang-cli
# the binary is now at target/release/buff (or target/release/buff.exe on Windows)
```

### Verify the install

Whichever option you chose, confirm `buff` is on your `PATH`:

```bash
buff --version
# buff-lang-cli 1.24.0
```

> **Note on `buff` vs `buff-lang-cli`.** The compiler binary produced by the
> `buff-lang-cli` crate is named `buff` (the user-facing name). The crate name
> has the `lang-` infix only to disambiguate it from
> [`buff-cli`](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-cli),
> a separate framework crate that exposes the `clap` argument parser to *user*
> Buff programs (covered in [Chapter 2](./chapter-2.md)).

## 1.2 Hello, world 🟢

Create a file named `hello.buff`:

```buff
func main():
    print("Hello, Buff!")
```

Save it, then run it:

```bash
buff run hello.buff
```

Output:

```
Hello, Buff!
```

Congratulations — you just transpiled Buff to Rust, compiled that Rust to a
native binary via `rustc`/LLVM, and executed it. The whole pipeline is one
command.

> See also: [`examples/ola.buff`](../../examples/ola.buff) — the canonical
> "Olá, Buff!" first example that ships with the repo. The Portuguese spelling
> (`ola` / `Olá`) is a project convention; the canonical first program in Buff
> greets the world in Portuguese, matching the maintainer's origin.

### Anatomy of the program

```buff
func main():
    print("Hello, Buff!")
```

- `func` — declares a function. One of Buff's 25 keywords.
- `main` — the entry point, exactly like C / Rust / Go. The runtime calls
  `main()` with no arguments.
- `()` — empty parameter list. (Buff has no `argc`/`argv` on `main`; use the
  `args()` prelude function, covered in [Chapter 2](./chapter-2.md).)
- `:` — opens an **indented block**. Buff is layout-sensitive: the block's
  body is everything indented one level deeper than the `:` line. No braces.
- `print(...)` — a prelude function (implicitly in scope, no `import`). Prints
  its argument followed by a newline.
- **4 spaces** of indentation. Tabs are a lexer error (E1004); Buff mandates
  spaces and rejects mixed indentation.

That's it. No `fn`, no `!` macro, no semicolons, no `&str` vs `String`
distinction, no `#[derive]`, no `use`. The simplest possible program is
*actually simple*.

## 1.3 A program with arithmetic 🟢

Buff infers numeric types aggressively — you almost never write a type
annotation. Here's the classic recursive Fibonacci, straight from
[`examples/fibonacci.buff`](../../examples/fibonacci.buff):

```buff
func fib(n: Int) -> Int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

func main():
    let n = 10
    print(fib(n))
```

Run it:

```bash
buff run fibonacci.buff
```

Output:

```
55
```

A few new things:

- `func fib(n: Int) -> Int` — a typed parameter (`n: Int`) and an explicit
  return type (`-> Int`). Both are optional in many positions (inference fills
  them in), but on public functions it's good style to be explicit.
- `Int` — Buff's default integer type. Lowers to Rust's `i64`. Integer *literals*
  infer the narrowest width that fits (`1u8`, `300i16`, …) but named bindings
  without an explicit width promote to `Int` = `i64`.
- `if n < 2:` — opens another indented block. No parens around the condition.
- `return n` — early return, exactly like Rust / C.
- `let n = 10` — a `let` binding. No `mut` keyword means the binding is
  *immutable* (Rust's default). To mutate, write `let mut`.

## 1.4 A program with mutation 🟢

From [`examples/calculadora.buff`](../../examples/calculadora.buff):

```buff
func add(a: Int, b: Int) -> Int:
    return a + b

func main():
    print(add(2, 3))
```

Output: `5`. To see mutation in action, here's a small vector example:

```buff
func main():
    let mut stack = [1, 2, 3]
    stack.push(4)
    print(stack.len())
    print(stack[3])

    let top = stack.pop()
    match top { Some(x) => print(x), None => print(0) }
```

Output:

```
4
4
4
```

New ideas:

- `[1, 2, 3]` — a `Vector<T>` literal (lowers to Rust's `Vec<T>`).
- `let mut stack` — `mut` makes the binding mutable. Without it, `stack.push`
  would be a compile error.
- `stack.push(4)` — vector method. `.len()`, `.pop()`, `.push()` are the
  bread-and-butter collection methods.
- `stack[3]` — indexing. The index is coerced to `usize` for you.
- `stack.pop()` — returns `Option<T>`: `Some(last)` when non-empty, `None`
  otherwise.
- `match top { Some(x) => print(x), None => print(0) }` — pattern matching.
  Buff `match` is exhaustive: the compiler verifies every possible value is
  covered (or you wrote a `_` catch-all). See [Chapter 6 §6.5](./chapter-6.md).

> See also: [`examples/collections.buff`](../../examples/collections.buff) for
> `Vector<T>` and `Map<K, V>` end-to-end, and
> [`examples/pattern_matching.buff`](../../examples/pattern_matching.buff) for
> matching on `Option<T>` and `Result<T, E>`.

## 1.5 `buff new` — scaffold a project

For anything beyond a one-file script, use `buff new` to scaffold a project:

```bash
buff new my_app
```

This creates:

```
my_app/
├── buff.toml          # project manifest (Buff's Cargo.toml equivalent)
├── README.md
└── src/
    └── main.buff      # an empty main() — ready to fill in
```

The scaffolded `src/main.buff` looks like:

```buff
func main():
    print("Hello from my_app!")
```

Run it from inside the project directory:

```bash
cd my_app
buff run src/main.buff
```

You can also run `buff init` inside an *existing* empty directory to lay down
the same scaffold without creating a subdirectory.

## 1.6 `buff check` — fast standalone typecheck

`buff check` runs the lexer → parser → type-inference pipeline **without**
generating Rust or invoking `rustc`. It's the fastest way to get feedback on
whether your program is well-typed:

```bash
buff check examples/ola.buff
# (no output, exit 0 — clean)
```

Introduce a type error and `check` catches it before you ever pay for codegen:

```buff
func main():
    let x: Int = "not an int"
```

```bash
buff check broken.buff
```

```
[Error] error[E1203]: assignment type mismatch
   --> broken.buff:2:18
    |
  2 |     let x: Int = "not an int"
    |                   ^^^^^^^^^^^^ expected `Int`, found `String`
```

The `E1203` is a **stable error code** — it will never be renumbered or reused
across releases. [Chapter 8](./chapter-8.md) is the full handbook; for now
just note that every diagnostic carries a code you can search for.

> `buff check` was shipped in v1.0 (T55). It runs the `TypeInferencer` directly
> from `buff-lang-types` and surfaces the same diagnostics codegen would — but
> in milliseconds, without compiling.

## 1.7 The compiler pipeline (what `buff run` actually does)

A single `buff run hello.buff` invokes this pipeline:

```
hello.buff
    │  buff-lang-lexer (byte-scanner + offside rule)
    ▼
tokens
    │  buff-lang-parser (recursive-descent + Pratt)
    ▼
AST (buff-lang-ast)            .buffhtml SFC ──▶ buff-lang-buffhtml-parser
    │                                            ──▶ buff-lang-ast-rsx
    │  buff-lang-types (inference + async + ownership + exhaustiveness)
    ▼
typed AST
    │  buff-lang-codegen-rust (syn / quote / prettyplease)
    ▼
syn::File ──prettyplease::unparse──▶ String   (a .rs file)
    │
    │  (parallel: buff-lang-codegen-wgsl emits WGSL for @prefer(gpu) fns)
    ▼
rustc --edition 2021   (and wgpu for any GPU shaders)
    │
    ▼
native executable  (or wasm32-unknown-unknown for .buffhtml UI apps)
```

You usually don't need to think about this. The two things worth knowing:

1. **Type-checking happens twice.** Once inside `buff check` (fast, no codegen)
   and once *inside codegen* (consulted at each `let` binding, with failures
   falling back to no annotation). This is why a program that passes `buff
   check` will still compile under `buff run` — they share the same front-end.
2. **Codegen is deterministic and never raw-strings Rust.** Every Rust byte is
   produced via `syn`/`quote` and formatted with `prettyplease`. The *one*
   exception is WGSL (GPU shaders) in `buff-lang-codegen-wgsl`, because WGSL
   has no `syn` equivalent — documented inline. *Chapter 4* covers the WGSL
   path.

## 1.8 Where to go next

You now have a working `buff` and can run, scaffold, and typecheck programs.
The rest of the Foundations chapters build real artifacts:

- [Chapter 2](./chapter-2.md) — build a CLI tool with flags, options, and
  subcommands (using the `buff-cli` framework crate).
- [Chapter 3](./chapter-3.md) — build an HTTP API server with `buff-web`.
- [Chapter 4](./chapter-4.md) — make a hot loop dispatch to the GPU.
- [Chapter 5](./chapter-5.md) — build a UI app with `.buffhtml`.

Or jump to [Chapter 6 (Language Reference)](./chapter-6.md) if you want the
full grammar before writing more code.

---

*Next: [Chapter 2 — Build a CLI](./chapter-2.md)*
