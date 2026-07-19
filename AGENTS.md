# PROJECT KNOWLEDGE BASE

**Generated:** 2026-07-16
**Commit:** 7d58448
**Branch:** v0.1-dev

## OVERVIEW

Buff — high-level language that transpiles `.buff` → Rust → native via rustc/LLVM. Implemented as a 9-crate Rust workspace. v0.1 "Olá, Buff" shipped end-to-end. Hides borrow-checker pain from users; compiler emits only "easy" Rust.

## STRUCTURE

```
buff/
├── crates/                     # 9 workspace members (the compiler)
│   ├── buff-lang-error/        # LEAF: Span + Diagnostic + SourceMap (depended on by all)
│   ├── buff-lang-ast/          # Pure AST data nodes (decl/expr/stmt/ty/op/ir)
│   ├── buff-lang-lexer/        # Hand-rolled byte-scanner + offside-rule indent tracker
│   ├── buff-lang-parser/       # Hand-rolled recursive-descent + Pratt (NOT chumsky)
│   ├── buff-lang-types/        # Type inference + prelude + range analysis
│   ├── buff-lang-codegen-rust/ # AST → syn::File → prettyplease → Rust source
│   ├── buff-lang-codegen-wgsl/ # STUB (v1.0): AST → WGSL GPU shaders
│   ├── buff-lang-runtime/      # STUB (v1.0): rayon + wgpu + tokio host
│   └── buff-lang-cli/          # Binary + library: pipeline orchestration
├── crates-io/                  # EMPTY (reserved for future crates.io publishing)
├── examples/                   # ola.buff, fibonacci.buff, calculadora.buff (PT-BR names)
├── tests/                      # Golden .buff fixtures (valid/ + invalid/) + snapshot README
├── .sisyphus/                  # Project orchestration: boulder.json + plans/ (see NOTES)
├── .github/workflows/ci.yml    # 3-OS matrix: fmt --check + clippy -D warnings + test
├── Cargo.toml                  # Pure workspace (no [package]); all deps centralized
└── rust-toolchain.toml         # Pin: 1.95.0 + rustfmt + clippy
```

## WHERE TO LOOK

| Task | Location | Notes |
|---|---|---|
| Add a CLI subcommand | `crates/buff-lang-cli/src/cli.rs` + `commands/<name>.rs` + `main.rs` dispatch arm | main.rs is 20 lines, thin |
| Add a new AST node | `crates/buff-lang-ast/src/{decl,expr,stmt,ty}.rs` | Ripple: parser + codegen-rust |
| Add a TokenKind | `crates/buff-lang-lexer/src/token.rs` → `lexer.rs` → parser `stream.rs` | |
| Add a prelude/builtin fn | `crates/buff-lang-types/src/prelude.rs` (type sig) + codegen-rust (Rust lowering) | Implicit — no `import` needed |
| Add an error variant | `crates/buff-lang-error/src/span.rs` | Leaf crate, ripple everywhere |
| Add a snapshot test | `crates/<crate>/tests/*.rs` + commit the `.snap` | Per-crate `tests/snapshots/` |
| Find phase status (v0.1/v0.5/v1.0) | `.sisyphus/plans/buff-{v01,v05,v10}-*.md` | Master: `buff-master.md` |
| Find the conventions doc | `.sisyphus/plans/buff-conventions.md` | 18 conventions for BUFF LANGUAGE (not Rust) |

## CODE MAP — Pipeline

```
.buff source
    │
    ▼ read_to_string
buff-lang-lexer::tokenize        →  Vec<Token>      (hand-rolled, offside-rule)
    │
    ▼
buff-lang-parser::parse          →  Vec<Decl>       (hand-rolled recursive-descent + Pratt)
    │
    ▼  (types: TypeInferencer runs INSIDE codegen in v0.1, no separate pass yet)
buff-lang-codegen-rust::generate_rust  →  syn::File → String   (prettyplease)
    │
    ▼  pipeline::compile_rust_to_exe   (rustc --edition 2021)
native executable
```

**v0.1 wiring**: lexer → parser → codegen-rust → rustc. Type errors are WARNINGS today (deferred to v0.5). WGSL + runtime are stubs.

## CONVENTIONS

- **Workspace dependency resolution**: every crate uses `dep.workspace = true`. NEVER pin a version in a crate `Cargo.toml` — add to root `[workspace.dependencies]` first.
- **Edition 2021, license `MIT OR Apache-2.0`, version `0.1.0`** on every crate.
- **Rust crate naming**: folder `buff-lang-<thing>` (hyphen) → crate ident `buff_lang_<thing>` (underscore) → import `buff_lang_<thing>::...`.
- **Derive defaults**: `Debug, Clone, PartialEq` (+ `Eq, Hash` when used in maps/sets).
- **Errors**: `thiserror::Error` derive; map to `buff_lang_error::*Error` variants.
- **Tests in per-crate `tests/`** (not src). Inline `#[cfg(test)]` ok for unit smoke tests.
- **No `[features]`, `[lints]`, `[profile.*]` sections** in any Cargo.toml. No crate-level `#![deny(...)]` / `#![forbid(unsafe_code)]`.

## ANTI-PATTERNS (THIS REPO)

- ❌ **Raw-string Rust codegen** — must build via `syn`/`quote`, format via `prettyplease`. The single string producer is `prettyplease`.
- ❌ **`unwrap`/`expect`/`panic!`/`unimplemented!`/`todo!`** in non-test code (hard rule from README).
- ❌ **Tabs** — Buff lexer rejects them (4 spaces only).
- ❌ **`_async` suffix** on async functions (Buff language rule §6).
- ❌ **Positional boolean args** — Buff mandates named args: `fetch(url, cache: true)` (§11).
- ❌ **`new Person()` / `Person.create()` / `Person.build()`** — Buff constructors are `Type.new()` / `Type.from()` only (§7).
- ❌ **Committing `.snap.new` / `.pending-snap`** — those are insta pending files (gitignored).
- ❌ **Populating `crates-io/`** — currently empty/reserved; do not add without coordination.
- ❌ **Trailing whitespace, >2 consecutive blank lines, missing trailing commas in multi-line collections** (Buff fmt rules).

## UNIQUE STYLES

- **PT-BR example names**: `ola`, `calculadora` — keep the convention when adding examples.
- **`.sisyphus/` orchestration**: plans track v0.1/v0.5/v1.0 task breakdown. `boulder.json` is active session state. Read `buff-conventions.md` for Buff-language rules.
- **Dual bin+lib in CLI**: `main.rs` (20 lines, thin) + `lib.rs` (real logic). Lets integration tests drive the pipeline without subprocess.
- **`compile_to_rust` vs `compile_rust_to_exe`** split in `pipeline.rs`: callers can inspect intermediate Rust source before invoking rustc.
- **Type checking is INSIDE codegen** for v0.1 (separate pass deferred to v0.5). Type errors are warnings today.
- **Prelude**: `print`, etc. are implicit (no `import`). Type sigs in `buff-lang-types/src/prelude.rs`.
- **`logos` + `chumsky` listed in `Cargo.toml` but UNUSED** — both lexer and parser are hand-rolled. See crate AGENTS.md for the reason.

## COMMANDS

```bash
# Build (release)
cargo build --release -p buff-lang-cli    # → target/release/buff[.exe]

# Run an example
cargo run -p buff-lang-cli -- run examples/ola.buff
cargo run -p buff-lang-cli -- run examples/fibonacci.buff    # → 55

# Scaffold a new project
cargo run -p buff-lang-cli -- new my_app
cargo run -p buff-lang-cli -- run my_app/src/main.buff

# Check / test / lint (what CI runs — see NOTES for mismatch)
cargo check --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Snapshot workflow (insta)
cargo insta review        # interactively accept pending snapshots
cargo insta accept        # accept ALL pending (use sparingly)
```

## NOTES

- **Toolchain mismatch**: `rust-toolchain.toml` pins `1.95.0`, but `.github/workflows/ci.yml` uses `dtolnay/rust-toolchain@stable` (ignores the pin). CI may diverge from local.
- **CI clippy omits `--all-targets`** — README mentions it, CI doesn't. Local lint with `--all-targets` to catch test code.
- **CI runs on 3 OSes**: ubuntu-latest, windows-latest, macos-latest.
- **`crates-io/` is empty** — reserved for future crates.io publishing workflow.
- **`buff.lock`** is gitignored — Buff's future lockfile (not yet generated).
- **Hand-rolled lexer/parser**: chumsky 1.0.0-alpha.8 transitively requires `stacker` → `cc-rs` → C shim that fails on Windows hosts missing `excpt.h` from the Windows SDK. Same family of issues pushed the lexer to hand-roll. Cleanup of unused `logos`/`chumsky` deps is a TODO.
- **See per-crate `AGENTS.md`** in `crates/buff-lang-{cli,ast,codegen-rust,types,lexer,parser,error}/` and `tests/AGENTS.md` for detailed file-level guidance.
