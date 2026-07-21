# buff-cli

> CLI framework for the **Buff** language (clap-equivalent).

`buff-cli` wraps the mature [`clap`](https://crates.io/crates/clap) crate behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses CLI parsing via the `App` prelude type:

```buff
let app = App.new("myapp")
    .about("does useful things")
    .flag("verbose", short: "v", description: "verbose mode")
    .option("name", short: "n", description: "name to greet")

let parsed = app.parse(Args.all())
if parsed.flag("verbose"):
    print("verbose mode ON")

let name = parsed.option("name").or(default: "world")
print("Hello, {name}!")
```

**Status: experimental** (T32 v1.13 frameworks wave 6).

## Name note

`buff-cli` is BOTH this framework crate AND the existing compiler
binary at `crates/buff-lang-cli/`. The framework crate is
`crates/buff-cli/` (no `lang-` infix). The compiler binary stays at
`crates/buff-lang-cli/`. Both depend on the same upstream `clap`
crate (workspace pin); this crate exposes clap to USER programs.

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users
do not install it directly. It is automatically pulled in as a path
dependency of the workspace when a Buff program uses the `App`
prelude type.

For direct Rust use:

```bash
cargo add buff-cli --path crates/buff-cli
```

## Quick start

```rust
use buff_cli::App;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let app = App::new("greet".to_string())
        .version("1.0.0".to_string())
        .about("Say hello".to_string())
        .flag("loud".to_string(), "l".to_string(), "shout".to_string())
        .option(
            "name".to_string(),
            "n".to_string(),
            "Who to greet".to_string(),
        );

    let parsed = match app.parse(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let name = parsed.option("name").unwrap_or_else(|| "world".to_string());
    if parsed.flag("loud") {
        println!("HELLO, {name}!");
    } else {
        println!("Hello, {name}");
    }
}
```

## Public API (16 functions, ≤20 cap)

### `App` (10 functions)

| Method | Signature | Notes |
|---|---|---|
| `App::new` | `(name) -> App` | Create root app. |
| `app.about` | `(about) -> App` | Set short description. Chained. |
| `app.version` | `(version) -> App` | Set version string. Chained. |
| `app.flag` | `(name, short, description) -> App` | Boolean flag (`SetTrue`). Chained. |
| `app.option` | `(name, short, description) -> App` | Value option (`Set`). Chained. |
| `app.arg` | `(name, description) -> App` | Positional arg. Chained. |
| `app.command` | `(name, about) -> App` | Register subcommand; returns child for further building. |
| `app.parse` | `(args) -> Result<ParsedArgs, CliError>` | Parse argv. `catch_unwind` boundary. |
| `app.parse_or_exit` | `(args) -> ParsedArgs` | Parse + exit-1 on error / exit-0 on help. |
| `app.help_text` | `() -> String` | Auto-generated help text. |

### `ParsedArgs` (6 functions)

| Method | Signature | Notes |
|---|---|---|
| `parsed.subcommand` | `() -> Option<String>` | Matched subcommand name. |
| `parsed.subcommand_args` | `() -> ParsedArgs` | Subcommand's parsed args. |
| `parsed.flag` | `(name) -> bool` | Was this boolean flag present? |
| `parsed.option` | `(name) -> Option<String>` | Value of this option. |
| `parsed.arg` | `(name) -> Option<String>` | Value of this positional arg by name. |
| `parsed.args` | `() -> Vec<String>` | All positional values in declaration order. |

## Subcommand nesting

```rust
let app = App::new("multi".to_string());
let greet = app.command("greet".to_string(), "say hi".to_string());
greet.option("name".to_string(), "n".to_string(), "name".to_string());

let count = app.command("count".to_string(), "count to N".to_string());
count.arg("n".to_string(), "how high".to_string());

let parsed = app.parse(args)?;
match parsed.subcommand().as_deref() {
    Some("greet") => {
        let sub = parsed.subcommand_args();
        let name = sub.option("name").unwrap_or_else(|| "world".to_string());
        println!("Hello, {name}!");
    }
    Some("count") => { /* ... */ }
    _ => { /* ... */ }
}
```

Subcommands can nest arbitrarily deep — a subcommand's returned
`App` can itself register sub-subcommands via the same `.command()`
call.

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `App`, `ParsedArgs`, `CliError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `App` wraps `Arc<Mutex<Node>>`. `parse` consumes self; returns owned `ParsedArgs`. All getters return owned `String` / `bool` / `Vec<String>`. |
| R3 — Error mapping | Every fallible op returns `Result<T, CliError>`. `clap::Error` auto-converts via `From`. |
| R4 — Thread safety | `App` is `Send + Sync` (wraps `Arc<Mutex<Node>>`). `ParsedArgs` is `Send + Sync` (wraps `clap::ArgMatches`). |
| R5 — Lifetime hiding | No public lifetime parameters. All arguments are owned `String`. |
| R6 — Panic boundary | `parse` / `parse_or_exit` wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-cli
cargo clippy -p buff-cli --all-targets -- -D warnings
cargo fmt -p buff-cli --check
```

10+ unit tests + 3 insta snapshots. Tests construct argv inline (no
fixtures needed).

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
