# buff-cli

CLI framework for user Buff programs (clap-equivalent). Pure-Rust MVP wrapping the [`clap`](https://crates.io/crates/clap) crate. Exposes clap's builder API to USER programs (the `buff` compiler binary already uses clap internally via `crates/buff-lang-cli`; this crate makes clap available to Buff programs as a Click / Commander / Cobra / Picocli / System.CommandLine equivalent).

**Status: experimental** (T32 v1.13 frameworks wave 6).

## Name note

`buff-cli` is BOTH this new framework crate AND the existing `crates/buff-lang-cli/` (the compiler binary). The framework crate is `crates/buff-cli/` (no `lang-` infix). The compiler binary stays `crates/buff-lang-cli/`. Do not confuse — both consume the same workspace `clap` pin; this crate is the one exposed to user `.buff` programs.

## STRUCTURE

```
buff-cli/
├── Cargo.toml            # clap (workspace) + thiserror + insta deps
├── src/
│   ├── lib.rs            # App + ParsedArgs + Node (main surface, ~430 LOC)
│   └── error.rs          # CliError enum (~40 LOC)
├── examples/
│   ├── cli_hello.rs            # minimal hello-world CLI
│   ├── cli_subcommands.rs      # subcommand nesting + dispatch
│   ├── cli_flags.rs            # flags + options + positionals
│   └── cli/
│       ├── cli_hello.buff           # Buff-side forward-decl (matches .rs)
│       ├── cli_subcommands.buff     # Buff-side forward-decl
│       └── cli_flags.buff           # Buff-side forward-decl
└── tests/
    └── core.rs           # 16 unit tests + 3 insta snapshots (~280 LOC)
```

Total: ~780 LOC (well under the 2000 LOC T32 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new builder method | `src/lib.rs` (add `pub fn` on `App`) + push to `Node` + extend `build_command` + test in `tests/core.rs` |
| Add a new ParsedArgs accessor | `src/lib.rs` (add `pub fn` on `ParsedArgs`) |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps a clap error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |
| Inspect what clap sees | `App::help_text` — renders the auto-generated help |

## PUBLIC API (16 functions, ≤20 cap)

### `App` (10 functions)
- Constructors: `new`
- Builders: `about`, `version`, `flag`, `option`, `arg`, `command` (each returns `self` for chaining; `command` returns the subcommand App for further building)
- Lifecycle: `parse`, `parse_or_exit`, `help_text`

### `ParsedArgs` (6 functions)
- Subcommand: `subcommand`, `subcommand_args`
- Getters: `flag`, `option`, `arg`, `args`

## CONVENTIONS

- **Pure-Rust only**: clap's default features pull in pure-Rust help/version/error rendering. No native deps, no cc-rs — matches the "no C library, no Docker" hard rule.
- **Builder API, NOT derive**: Buff cannot use clap's `#[derive(Parser)]` proc-macro across the FFI boundary (Buff's codegen layer can't generate proc-macro-attribute usage). The crate uses `clap::Command::{new, about, version, arg, subcommand}` builders exclusively. The workspace `clap` pin still enables the `derive` feature because `crates/buff-lang-cli/` (the compiler) uses it; buff-cli ignores that feature.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. All getters on `ParsedArgs` use `try_contains_id(...).unwrap_or(false)` to guard clap's `get_flag` / `get_one` from missing-id panics.
- **catch_unwind boundary**: `parse` / `parse_or_exit` wrap their bodies in `catch_unwind` per FFI guide R6 (a panic in clap's matcher becomes `Err(CliError::Panic)` instead of process abort).
- **No interactive prompts**: per T32 "Must NOT" list — no Inquirer-style prompts. No shell-completion generation (v1.22+).
- **`App` is `Send + Sync`**: wraps `Arc<Mutex<Node>>`. Mutex poisoning is treated as a no-op — the builder falls silent and the next `parse` returns a degenerate `clap::Command` (named after the poison error string). `ParsedArgs` is also `Send + Sync` because `clap::ArgMatches` is `Send + Sync`.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `clap` | Upstream argument parser. `buff-cli` is a safe wrapper; never re-exports `clap::*` types directly. |
| `buff-lang-cli` (compiler) | Sibling crate — already uses clap via the derive API for its 21 subcommands. This crate exposes clap to USER programs (the derive API is not safe across the FFI boundary). |
| `buff-lang-types` | **Forward-declared**: `prelude_types.rs` will register `PreludeType::App` + `PreludeType::ParsedArgs` + `PreludeAssocFn::New` + 6 `PreludeInstanceFn` variants (about / version / flag / option / arg / command / parse / help_text). Deferred to a coordinated sibling codegen commit per the buff-config precedent. |
| `buff-lang-codegen-rust` | **Forward-declared**: `rust_codegen.rs::buff_type_to_syn` will get the `Type::App => "buff_cli::App"` arm. `lower_prelude_type_assoc_fn` + `lower_prelude_type_instance_fn` will get the App arms. `program_uses_namespace("App")` will record `buff-cli` + `clap` in `extern_crates`. Deferred to a coordinated sibling codegen commit per the buff-config precedent. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **`command()` returns the subcommand, not the parent**: `App.command(name, about)` mutates `self` to register a new subcommand, then returns a NEW `App` representing the subcommand (further builder calls configure the subcommand). Mutations to the returned App are visible to the parent because the child shares its internal `Node` via `Arc<Mutex<>>`. This matches the buff-config `.watch()` precedent (shared internal state).
- **`parse_or_exit` for normal CLI tools, `parse` for library callers**: `parse_or_exit` prints errors and exits with clap's status code (exit 0 on `--help` / `--version`, exit 1 on parse errors). `parse` returns the error so the caller can react. Codegen lowering will use `parse_or_exit` for `func main` and `parse` for test harnesses.
- **clap version**: workspace pins `clap = { version = "4.5", features = ["derive"] }`. The `derive` feature is on because `buff-lang-cli` uses it; this crate uses only the builder API. Future clap 5.x bumps need re-test of the builder-API surface.
- **`ParsedArgs` is NOT `Clone`**: `clap::ArgMatches` does not impl `Clone` in clap 4.x... actually it does — ArgMatches: Clone since clap 4.0. The decision to NOT impl Clone on `ParsedArgs` is intentional: ParsedArgs is a one-shot result consumed by the caller's dispatch logic. Cloning would imply two parallel dispatch paths, which is an anti-pattern for CLI parsers.
- **MSVC host blocker (same as buff-image)**: `cargo test -p buff-cli` fails on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue. CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. The crate's library `cargo check -p buff-cli --lib` and `cargo clippy -p buff-cli --all-targets -- -D warnings` pass clean.
- **Pre-existing workspace resolution issues at the time of this commit**: `buff-validate` (sibling T29 crate) declares `validator` with the wrong feature name (`unicode` instead of `unic`), and `buff-observe` (sibling T21 crate) requires `opentelemetry-proto` which is not yet in the local crates.io cache. Both pre-date this commit and are NOT caused by buff-cli. They block `cargo check --workspace` on this host; the buff-cli crate itself has no resolution issues (clap + thiserror + insta are all pre-pinned at the workspace level).
