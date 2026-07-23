+++
title = "Installation"
weight = 10
+++

# Installation

Buff ships as a single `buff` CLI binary, built from the
`buff-lang-cli` crate. There is no installer wizard, no system daemon, no
`PATH`-polluting runtime — just one executable.

## From source (recommended)

The only supported install path today is `cargo install` from a checkout of
the repository:

```bash
git clone https://github.com/buff-lang/buff.git
cd buff
cargo install --path crates/buff-lang-cli --locked
```

The `--locked` flag pins every transitive dependency to the versions recorded
in `Cargo.lock`, guaranteeing a reproducible build.

To force-reinstall over an older copy:

```bash
cargo install --path crates/buff-lang-cli --locked --force
```

## Verify

```bash
buff --version
buff run examples/ola.buff        # → Olá, Buff!
buff run examples/fibonacci.buff  # → 55
buff check examples/ola.buff      # → no errors (standalone typecheck, no codegen)
```

If any of these fail, check that `rustc --version` reports `1.95.0` (the
pinned toolchain in `rust-toolchain.toml`).

## Toolchain requirements

Buff is sensitive to the Rust toolchain version because it generates Rust and
hands it to `rustc`. The pinned version lives in
[`rust-toolchain.toml`][rtc] at the repo root:

```toml
[toolchain]
channel = "1.95.0"
components = ["rustfmt", "clippy"]
```

If you use `rustup`, simply `cd` into the repo and `rustup` will install the
right toolchain automatically. CI uses
[`dtolnay/rust-toolchain@master`][dtolnay] with the same channel.

[rtc]: https://github.com/buff-lang/buff/blob/master/rust-toolchain.toml
[dtolnay]: https://github.com/dtolnay/rust-toolchain

## Editor support

A VSCode extension is published as `buff-vscode-1.2.0.vsix` in
[`editors/vscode/`][vscode]. It bundles the LSP server and provides:

- Syntax highlighting (TextMate grammar)
- Diagnostics, hover, completion, goto-definition (via `buff-lsp`)
- Document symbols and formatting
- `buff.run`, `buff.build`, `buff.check` command-palette entries

Install with:

```bash
code --install-extension editors/vscode/buff-vscode-1.2.0.vsix
```

[vscode]: https://github.com/buff-lang/buff/tree/master/editors/vscode

## Try Buff without installing

You can transpile Buff in the browser using the
[playground][playground] — no install required. The playground is a single
static HTML page (`playground/index.html` in the repo) with the Buff
compiler compiled to WebAssembly.

[playground]: https://buff-lang.org/playground/

## Distribution channels (planned)

The following install paths are tracked but not yet shipped:

- `cargo install buff-cli` (crates.io publish) — planned post-v1.13.
- `buffup` (a `rustup`-style version manager) — shipped as `v1.12` T83; see
  `crates/buffup/`.
- Homebrew formula, Arch AUR, Debian `.deb` — tracked in the v1.22+ roadmap.

If your platform isn't covered yet, the from-source install above works
anywhere `cargo` does (Linux, macOS, Windows, WSL, FreeBSD).
