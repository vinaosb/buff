+++
title = "Project structure"
weight = 30
+++

# Project structure

A Buff project is the smallest unit the `buff` CLI can build, run, typecheck,
and (eventually) package. This page documents what `buff new` scaffolds and
why each piece exists.

## Scaffolding

```bash
buff new my_app
cd my_app
buff run src/main.buff
```

The `buff new <NAME>` subcommand creates this layout:

```
my_app/
├── buff.toml           # project manifest (T121 Cargo-style metadata)
├── src/
│   └── main.buff       # entry point: `func main():`
└── README.md           # one-paragraph description
```

`buff init` does the same thing but in the current directory instead of a
fresh subdirectory — useful for converting an existing folder into a Buff
project.

## `buff.toml`

The manifest holds project metadata. It is intentionally tiny:

```toml
[package]
name = "my_app"
version = "0.1.0"
edition = "2021"
```

There is *no* `[dependencies]` section yet — Buff does not have a Cargo-style
dependency resolver in the v1.x series. External crates are pulled in via the
`extern` keyword at the language level (see
[`extern_reqwest.buff`][extern-req] in the examples).

[extern-req]: https://github.com/buff-lang/buff/blob/master/examples/extern_reqwest.buff

## Source layout

```
src/
├── main.buff            # the entry point (must define `func main():`)
├── utils.buff           # sibling module, imported via `import utils`
└── net/
    └── http.buff        # nested module: `import net.http`
```

Imports use a dotted path with no file extension:

```buff
import utils
import net.http

func main():
    utils.greet("world")
    let status = http.ping("https://buff-lang.org")
    print(status)
```

> **Note (v1.x limitation):** the v1 CLI pipeline invokes `rustc` on a single
> `.rs` file, so multi-file `import` / `export` programs parse and type-check
> but do not yet link end-to-end. This is tracked in the v1.13+ Cargo-polish
> milestone (T120). The module graph resolves correctly — only the linker
> step is missing.

## Where generated Rust goes

Buff does **not** litter your source tree with `.rs` files. Intermediate
Rust lives under `target/` (gitignored) alongside the normal `rustc` build
artifacts. To inspect the generated source, run:

```bash
buff build --emit rust examples/ola.buff   # → target/.../ola.rs
```

This is invaluable for debugging — Buff's contract is that the generated
Rust is always "easy" Rust (no lifetimes, no manual clones, no `unsafe`).
If you ever see something scary in the output, file a bug.

## Binary size: `--minimal`

For deployment targets that care about size (AWS Lambda layers, embedded,
distribution images), pass `--minimal`:

```bash
buff build --minimal examples/minimal_console.buff
```

The flag activates five size-minimization knobs simultaneously:

- `opt-level = "z"` (optimize for size, not speed)
- `panic = "abort"` (no unwind tables)
- `strip = "symbols"`
- `lto = true`
- `codegen-units = 1`

Console-template apps typically land under 5 MB, often around **340 KB** on
Linux x86_64. See [`docs/binary-size.md`][bin] for the full size budget and
per-template reference.

[bin]: https://github.com/buff-lang/buff/blob/master/docs/binary-size.md

## The workspace itself

The Buff *compiler* is developed as a 19-crate Cargo workspace. If you're
contributing to Buff itself rather than writing Buff programs, see the
top-level [`AGENTS.md`][agents] for the workspace layout and per-crate
guidance.

[agents]: https://github.com/buff-lang/buff/blob/master/AGENTS.md
