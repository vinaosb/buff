# Chapter 2 — Build a CLI

In this chapter you'll build a real command-line tool in Buff. You'll learn:

- the difference between the **`buff`** compiler binary and the **`buff-cli`**
  framework crate that your *programs* use,
- how to read command-line arguments both the simple way (`args()`) and the
  structured way (`App.new()`),
- how to declare flags, options, positional arguments, and subcommands,
- how to read stdin and write to stdout,
- how `buff build --minimal` produces a sub-5-MB binary for distribution.

The full program you'll write is an `ls`-style file lister with flags, an
option, and a positional argument. By the end you'll know the shape of every
CLI you'll ever write in Buff.

## 2.1 Two meanings of "CLI" in Buff

Buff has two distinct CLI surfaces, and the naming is easy to confuse on first
contact:

| What | Where | Purpose |
|---|---|---|
| The **`buff` compiler binary** | `crates/buff-lang-cli/` | The program *you run* to compile Buff: `buff run`, `buff build`, `buff check`. Not part of your program. |
| The **`buff-cli` framework crate** | `crates/buff-cli/` | A library *your program imports* to parse its own argv. The Click / Cobra / picocli / `System.CommandLine` equivalent. |

This chapter is about the **second** one — the framework crate that gives your
Buff programs a polished argument parser. It wraps the mature Rust
[`clap`](https://crates.io/crates/clap) crate (the same one the `buff`
compiler itself uses internally) behind a safe API that follows Buff's
[FFI safety guide](https://github.com/buff-lang/buff/tree/v1x-frameworks/crates/buff-lang-ffi-guide/GUIDE.md).

## 2.2 The simplest possible CLI — `args()` 🟢

For tiny scripts you don't need the framework crate at all. The prelude
function `args()` returns the command-line arguments as a `Vector<String>`,
exactly like Rust's `std::env::args()` or Python's `sys.argv`:

```buff
func main():
    let argv = args()
    // argv[0] is the program name; argv[1..] are the user's arguments.
    print("program:", argv[0])
    if argv.len() > 1:
        print("first arg:", argv[1])
    else:
        print("no arguments given")
```

Run it:

```bash
buff run cli_min.buff hello world
```

```
program: cli_min.buff
first arg: hello
```

This is great for 20-line utilities. For anything with flags (`--verbose`),
options (`--output foo.txt`), or subcommands (`git commit`), reach for the
`buff-cli` framework instead.

> `args()`, `env("NAME")`, and `exit(code)` are prelude functions shipped in
> v1.0 (T99). They're implicitly in scope — no `import` needed.

## 2.3 Reading stdin and writing stdout 🟢

The prelude gives you three I/O functions:

| Function | Behaviour |
|---|---|
| `print(x)` | Print `x` followed by a newline. Maps to Rust's `println!`. |
| `println(x)` | Same as `print` — both append a newline (the distinction is historical). |
| `read_line()` | Read one line from stdin into a `String` (newline trimmed). |
| `input(prompt)` | Print `prompt`, then read one line. Convenience over `read_line`. |

A tiny interactive program:

```buff
func main():
    let name = input("what is your name? ")
    print("hello, {name}!")
```

Run it and type at the prompt:

```bash
buff run greet.buff
```

```
what is your name? Ada
hello, Ada!
```

The `{name}` syntax is **string interpolation** — Buff string literals
interpolate `{expr}` directly, no `format!` macro needed. *Chapter 6 §6.2*
covers the full interpolation grammar.

## 2.4 The `buff-cli` framework — `App.new()` 🔶

> 🔶 The `buff-cli` crate is shipped (v1.13, T32) and its Rust API is fully
> tested. The Buff-side surface (`App.new(...)` in `.buff` source) is a
> forward-declaration: the prelude-type + codegen lowering arm is a coordinated
> sibling task. The snippets below are valid Buff syntax and match the Rust
> examples in `crates/buff-cli/examples/`; once the wiring lands they'll run
> end-to-end via `buff run`. Today, read them as the *target* API shape.

The framework centres on two types:

- **`App`** — a builder. You construct it with `App.new(name)`, chain
  `.about()`, `.flag()`, `.option()`, `.arg()`, `.command()` calls, then call
  `.parse(args)` to produce a `ParsedArgs`.
- **`ParsedArgs`** — the parsed result. You query it with `.flag(name)`,
  `.option(name)`, `.arg(name)`, `.subcommand()`, and `.subcommand_args()`.

A minimal hello-world CLI, from
[`crates/buff-cli/examples/cli/cli_hello.buff`](../../crates/buff-cli/examples/cli/cli_hello.buff):

```buff
func main():
    let app = App.new("hello")
        .version("0.1.0")
        .about("Says hello to a name")
        .option("name", short: "n", description: "Name to greet")
        .arg("greeting", description: "Optional greeting override")

    let parsed = app.parse(Args.all())
    match parsed.option("name"):
        Some(name):
            print("Hello, {name}!")
        None:
            print("Hello, world!")
```

Notice the **named-argument** style: `.option("name", short: "n", description:
...)`. Buff mandates named arguments for any call with more than one boolean or
stringly-typed parameter (convention §11). You can never write `.option("name",
"n", "Name to greet")` — the compiler's linter flags positional booleans and
the convention forbids positional strings-after-the-first. This is deliberate:
named args stay readable as the parameter list grows.

### `App` builder methods

Every builder method returns `self` so you can chain:

| Method | Adds |
|---|---|
| `App.new(name)` | root app with the given program name |
| `.about(text)` | short description (shown in `--help`) |
| `.version(text)` | version string (shown in `--version`) |
| `.flag(name, short: "..", description: "..")` | a boolean flag (`--verbose` / `-v`) |
| `.option(name, short: "..", description: "..")` | a value option (`--output FILE`) |
| `.arg(name, description: "..")` | a positional argument |
| `.command(name, about: "..")` | a subcommand — returns the child `App` for further building |
| `.parse(args)` | parse argv → `ParsedArgs` (returns `Result`; wraps in `catch_unwind`) |
| `.parse_or_exit(args)` | parse, or print error + exit (use this in `main`) |
| `.help_text()` | the auto-generated help string |

### `ParsedArgs` accessors

| Method | Returns |
|---|---|
| `.flag(name)` | `Bool` — was this flag present? |
| `.option(name)` | `Option<String>` — value of `--name` |
| `.arg(name)` | `Option<String>` — value of positional `name` |
| `.args()` | `Vector<String>` — all positionals in declaration order |
| `.subcommand()` | `Option<String>` — matched subcommand name |
| `.subcommand_args()` | `ParsedArgs` — the subcommand's own parsed args |

## 2.5 Flags, options, and positionals 🔶

Here's the `ls`-style lister, from
[`crates/buff-cli/examples/cli/cli_flags.buff`](../../crates/buff-cli/examples/cli/cli_flags.buff):

```buff
func main():
    let app = App.new("ls-like")
        .about("Demo: flags + options + positionals")
        .flag("all", short: "a", description: "show hidden")
        .flag("long", short: "l", description: "long format")
        .option("sort", short: "s", description: "sort by: name|size|time")
        .arg("path", description: "directory to list")

    let parsed = app.parse(Args.all())
    let show_all = parsed.flag("all")
    let long = parsed.flag("long")
    let sort = parsed.option("sort").or(default: "name")
    let path = parsed.arg("path").or(default: ".")

    print("listing {path}")
    print("  show all: {show_all}")
    print("  long:     {long}")
    print("  sort by:  {sort}")
```

Invoked as `ls-like -al --sort=size /tmp`:

```
listing /tmp
  show all: true
  long:     true
  sort by:  size
```

The `.or(default: ...)` pattern is the Buff idiom for "unwrap this
`Option<T>` or fall back". It's equivalent to Rust's `.unwrap_or(...)`. Flags
return `Bool` directly (never `Option<Bool>`) because a flag's absence *is*
`false`.

## 2.6 Subcommands 🔶

Subcommands are how you build a `git`-style tool. Each `.command()` call
returns a *new* `App` representing the subcommand, which you configure with
its own flags/options/args. From
[`crates/buff-cli/examples/cli/cli_subcommands.buff`](../../crates/buff-cli/examples/cli/cli_subcommands.buff):

```buff
func main():
    let app = App.new("multi").about("Subcommand demo")

    let greet = app.command("greet", about: "Say hello to NAME")
    greet.option("name", short: "n", description: "Who to greet")

    let count = app.command("count", about: "Count to N")
    count.arg("n", description: "How high to count")

    let parsed = app.parse(Args.all())
    match parsed.subcommand():
        Some("greet"):
            let sub = parsed.subcommand_args()
            let name = sub.option("name").or(default: "world")
            print("Hello, {name}!")
        Some("count"):
            let sub = parsed.subcommand_args()
            let n = sub.arg("n").parse_int(or: 3)
            for i in range(1, n + 1):
                print(i)
        _:
            print("unknown subcommand; try: multi --help")
```

Invoked as `multi greet -n Ada` → `Hello, Ada!`. Invoked as
`multi count 3` → `1` / `2` / `3`.

Subcommands nest arbitrarily deep: a subcommand's returned `App` can itself
register sub-subcommands via the same `.command()` call.

## 2.7 Environment variables and exit codes 🟢

Two prelude functions round out the process-control surface:

- `env("NAME")` — returns `Option<String>`. `None` if the variable is unset.
- `exit(code)` — terminate the process with the given exit code (`Int`).

A common pattern — read a config path from the env, fall back to a default,
error out if neither is usable:

```buff
func config_path() -> String:
    match env("MY_APP_CONFIG"):
        Some(p):
            return p
        None:
            return "./config.toml"

func main():
    let path = config_path()
    print("using config: {path}")
    if path.len() == 0:
        print("error: empty config path")
        exit(1)
```

> Never call `exit(0)` to signal success from `main` — just let `main` return
> normally. Buff's runtime exits 0 when `main` completes without panicking.
> Reserve `exit(code)` for non-zero error paths.

## 2.8 `buff build --minimal` — shipping a tiny binary 🟢

Once your CLI works, build it for distribution:

```bash
buff build --minimal my_cli.buff
```

The `--minimal` flag activates five size-minimization knobs simultaneously:

- `opt-level=z` (optimize for size, not speed),
- `panic=abort` (no unwinding machinery),
- `strip=symbols` (strip the symbol table),
- `lto=true` (link-time optimization across crates),
- `codegen-units=1` (maximum inlining opportunity).

A console-template app builds to **under 5 MB** with `--minimal` — often
around 340 KB on Linux x86_64. That's small enough to ship inside an AWS Lambda
layer, an embedded image, or a Docker `slim` base. See
[`docs/binary-size.md`](https://github.com/buff-lang/buff/blob/v1x-frameworks/docs/binary-size.md)
for the size budget per template.

The full trade-off matrix:

| Flag | Binary size | Runtime speed | Use when |
|---|---|---|---|
| `buff build` (debug) | largest | slowest | local iteration |
| `buff build --release` | medium | fastest | production servers, hot loops |
| `buff build --minimal` | smallest | fast | CLI tools, Lambda, embedded |

For a CLI tool, `--minimal` is almost always the right choice: the difference
between "fastest" and "fast" is invisible to a human at a terminal, but a 340 KB
binary downloads and starts noticeably faster than a 12 MB one.

## 2.9 Putting it all together

Here's a complete CLI that combines everything from this chapter — flags,
options, positionals, subcommands, env vars, and stdin:

```buff
// wordcount.buff — count words in a file or stdin.
func main():
    let app = App.new("wordcount")
        .version("1.0.0")
        .about("Count lines, words, and bytes")
        .flag("lines", short: "l", description: "count lines")
        .flag("words", short: "w", description: "count words")
        .flag("bytes", short: "c", description: "count bytes")
        .arg("file", description: "file to read (defaults to stdin)")

    let parsed = app.parse_or_exit(Args.all())

    let want_lines = parsed.flag("lines")
    let want_words = parsed.flag("words")
    let want_bytes = parsed.flag("bytes")
    // If no flags, enable all three.
    let any = want_lines or want_words or want_bytes
    let do_lines = want_lines or not any
    let do_words = want_words or not any
    let do_bytes = want_bytes or not any

    let source = parsed.arg("file").or(default: "-")
    let text = if source == "-":
        read_line()
    else:
        read_file(source)

    if do_lines:
        print(count_lines(text))
    if do_words:
        print(count_words(text))
    if do_bytes:
        print(text.len())

func count_lines(text: String) -> Int:
    let mut n = 0
    for ch in text:
        if ch == "\n":
            n = n + 1
    return n

func count_words(text: String) -> Int:
    return text.split(" ").len()

func read_file(path: String) -> String:
    // In a real program you'd use the File prelude type (Chapter 7).
    // For this demo we return a placeholder.
    return "the quick brown fox"
```

This is the shape of every CLI you'll write in Buff: an `App` builder, a
`parse_or_exit`, a handful of `.flag` / `.option` / `.arg` lookups, then your
business logic.

## 2.10 Recap

- **`buff`** is the compiler. **`buff-cli`** is the framework crate your
  *programs* import to parse their own argv.
- For tiny scripts, `args()` from the prelude is enough.
- For anything with flags/options/subcommands, use `App.new(name)` and chain
  `.flag()`, `.option()`, `.arg()`, `.command()`.
- Named arguments (`short: "v"`, `description: "..."`) are mandatory for
  multi-parameter calls — Buff forbids positional booleans.
- `parsed.flag(name)` → `Bool`; `parsed.option(name)` / `.arg(name)` →
  `Option<String>`; `.or(default: ...)` unwraps.
- `print` / `read_line` / `input` / `env` / `exit` are the prelude I/O and
  process surface.
- `buff build --minimal` ships a sub-5 MB binary.

---

*Next: [Chapter 3 — Build an API Server](./chapter-3.md)*
