+++
title = "Buff Language Documentation"
weight = 0
+++

# Buff

> Buff is a high-level language that transpiles to Rust. It removes the
> "rust" (complexity), leaving pure performance.

Every modern language forces a trade-off. You can have maximum performance
and memory safety (Rust) at the cost of a brutal learning curve, or
simplicity and productivity (Go / C# / Java) at the cost of a garbage
collector. Buff is a bet that you can deliver Rust's performance *without*
exposing the developer to the borrow checker — if the compiler, not the
human, is the one arguing with Rust.

## The three ideas

1. **Transpile, don't reimplement.** Buff is a source-to-source compiler
   (`.buff` → Rust → native binary via `rustc` / LLVM). It piggybacks on the
   engineering already sunk into `rustc` instead of reinventing codegen.
2. **Hide memory management.** No references (`&`), no visible lifetimes
   (`'a`), no manual pointers. The transpiler emits only "easy" Rust — owned
   data, intelligent clones, `Arc` / copy-on-write where sharing is needed.
3. **Invisible heterogeneous computing.** The same Buff function can run on
   CPU *or* be dispatched to GPU automatically. The compiler analyzes
   arithmetic intensity and emits both a Rayon path *and* a WGSL shader;
   the runtime picks at execution time.

## Install

```bash
cargo install --path crates/buff-lang-cli --locked
```

Buff pins Rust 1.95.0 (see `rust-toolchain.toml` in the repo). No C library,
no Docker, no external runtime.

## Hello, Buff

```buff
func main():
    print("Olá, Buff!")
```

Save as `ola.buff`, then:

```bash
buff run ola.buff
# → Olá, Buff!
```

That's it — no `Cargo.toml`, no `tokio::main`, no `fn main() -> Result<(),
Box<dyn Error>>`. The Buff compiler emits the right Rust scaffolding for you.

## Where to go next

- **New to Buff?** Start with [Getting Started → Installation](./getting-started/installation/).
- **Already installed?** Jump to [Your first program](./getting-started/first-program/).
- **Coming from Rust?** The [Migration guide](./migration/_index/) maps the
  Rust features Buff intentionally omits.
- **Building an app?** Browse the [frameworks catalog](./frameworks/overview/)
  to find the right `buff-*` crate.

## Status

Buff is at **v1.12 "Distribution scale"** as of this writing. The full
milestone table lives in the [GitHub repository README][readme]; the short
version is that the language, CLI, LSP, REPL, Jupyter kernel, and `.buffhtml`
UI pipeline are all shipped and tested.

[readme]: https://github.com/buff-lang/buff#status
