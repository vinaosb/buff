# The Buff Book

> The official guide to the **Buff** programming language — *Rust performance
> with Go productivity.*

This directory holds the source for *The Buff Book*, a 9-chapter tutorial that
walks you from "install Buff" through heterogeneous CPU/GPU computing, building
web APIs, authoring `.buffhtml` UI apps, and migrating from Rust, Python, and
Go. It is structured in the spirit of [The Rust Programming Language][trpl]
("The Rust Book") and rendered with [mdBook][mdbook].

[trpl]: https://doc.rust-lang.org/book/
[mdbook]: https://rust-lang.github.io/mdBook/

## Status

T55 (Wave 3). This is the **source** for The Book — it ships as Markdown files
under [`book/src/`](./src/). Every code block uses the `` ```buff `` fenced
language tag and is drawn from — or modelled on — a runnable example that
already lives in [`../examples/`](../examples/).

## Read it three ways

1. **On GitHub.** Every [`book/src/chapter-N.md`](./src/) renders as Markdown
   in any file viewer. Start at [`src/SUMMARY.md`](./src/SUMMARY.md) (the table
   of contents) and follow the links.
2. **In any Markdown viewer.** Clone the repo and open the files locally.
3. **As a rendered HTML site** via mdBook (optional). See "Rendering with
   mdBook" below.

## Chapters

| # | Title | What you learn |
|---|---|---|
| 1 | [Getting Started](./src/chapter-1.md) | Install `buff`, hello world, project structure, `buff run` / `buff check` / `buff new` |
| 2 | [Build a CLI](./src/chapter-2.md) | Arguments, flags, options, stdin/stdout, subcommands |
| 3 | [Build an API Server](./src/chapter-3.md) | `Web.get` / `Web.post`, JSON, middleware, routing |
| 4 | [GPU Compute](./src/chapter-4.md) | `@prefer(gpu)`, `map` / `filter`, WGSL shaders, CPU fallback |
| 5 | [Build a UI App](./src/chapter-5.md) | `.buffhtml` SFCs, RSX, components, props, lifecycle hooks |
| 6 | [Language Reference](./src/chapter-6.md) | Syntax, types, generics, pattern matching, traits, async |
| 7 | [Stdlib Reference](./src/chapter-7.md) | `Json`, `File`, `Http`, `DateTime`, `Regex`, `Assert`, collections |
| 8 | [Error Code Handbook](./src/chapter-8.md) | `E10xx`–`E15xx` with examples and fixes |
| 9 | [Migration Guides](./src/chapter-9.md) | From Rust, from Python, from Go |

## Project layout

```
book/
├── README.md        # this file
├── book.toml        # mdBook configuration (optional — only for `mdbook build`)
└── src/
    ├── SUMMARY.md   # the table of contents (mdBook entry point)
    ├── chapter-1.md  …  chapter-9.md
    └── …
```

## Rendering with mdBook (optional)

mdBook is **not** required to read or edit this book. The Markdown source is
canonical. If you want the rendered HTML site (search bar, theme picker,
previous/next navigation), install mdBook and build:

```bash
# Install mdBook (any of these)
cargo install mdbook
# or: brew install mdbook
# or: download a prebuilt binary from https://github.com/rust-lang/mdBook/releases

# Build the HTML site into book/book/html/
mdbook build book/

# Serve a live-reloading preview at http://localhost:3000
mdbook serve book/ --open
```

The `book/` directory intentionally contains **no JavaScript** of its own —
mdBook injects its own small reader UI at build time. The Buff-side
[`buffhtml`](./src/chapter-5.md) pipeline is a separate concern (it compiles
`.buffhtml` to WebAssembly, not to a docs site).

## Conventions used in this book

- **```buff** fences mark Buff source. Every snippet is either runnable as-is
  (`buff run <file>`) or is a faithful excerpt of a file that is.
- **```bash** fences mark shell commands. They assume a POSIX shell on Linux /
  macOS and PowerShell on Windows (the Buff CLI works identically on all three).
- **Cross-links** of the form `see examples/foo.buff` point at files in the
  repository root's [`examples/`](../examples/) directory, not inside `book/`.
- A 🟢 marker means the example *runs end-to-end* today (`buff run` succeeds).
  A 🔶 marker means the example *type-checks and transpiles* (`buff check`
  passes) but its end-to-end execution depends on a sibling codegen task still
  in flight — the snippet is still valid Buff syntax you can rely on.

## Contributing

Found a typo, a stale example, or a missing explanation? Open a PR against the
[`v1x-frameworks`](https://github.com/buff-lang/buff/tree/v1x-frameworks/book)
branch. The source of truth for what Buff *does* is the compiler
([`crates/buff-lang-*`](../crates/)) and the example library
([`examples/`](../examples/)); this book is downstream of both and should
never describe behaviour the compiler does not exhibit.

## License

Dual-licensed under [MIT](../LICENSE) or [Apache-2.0](../LICENSE), matching the
rest of the Buff workspace.
