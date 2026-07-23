+++
title = "Getting Started"
weight = 10
sort_by = "weight"
+++

This section walks you through installing Buff, writing your first program,
and understanding the layout of a Buff project.

## Prerequisites

- **Rust 1.95.0** (pinned in [`rust-toolchain.toml`][rtc]). Buff transpiles to
  Rust, so `rustc` must be on your `PATH`. The easiest way is via
  [`rustup`][rustup].
- A C-compatible linker (`cc` / `link.exe`) — Rust already requires one.
- *No* Node.js, *no* Docker, *no* external C libraries. Buff's only runtime
  dependencies are pure-Rust crates from crates.io.

[rtc]: https://github.com/buff-lang/buff/blob/master/rust-toolchain.toml
[rustup]: https://rustup.rs/

## Pages in this section

- [Installation](./installation/) — install the `buff` CLI from source.
- [Your first program](./first-program/) — `ola.buff`, the canonical hello world.
- [Project structure](./project-structure/) — what `buff new` scaffolds and why.

## Verifying the install

After installing, run:

```bash
buff --version
buff run examples/fibonacci.buff    # → 55
```

If `buff run` produces `55`, your toolchain is correctly wired.
