+++
title = "Cookbook"
weight = 40
sort_by = "weight"
+++

# Cookbook

> **Status:** placeholder — the cookbook ships as **T68** in the v1.21.0
> "Community & Quality" milestone. It depends on this docs site (T67)
> shipping first.

The cookbook will be a collection of short, copy-pasteable recipes for
common Buff tasks — file I/O, HTTP, parsing, concurrency patterns,
testing, deployment. Each recipe will be:

- 50–150 lines of Buff source.
- Self-contained (runs with `buff run`).
- Cross-referenced with the language reference.
- Tested in CI (code blocks compile).

## Planned recipes (subject to change)

- Read a file line by line
- Make an HTTP GET request
- Parse a TOML config file
- Concurrent fan-out / fan-in with `spawn`
- Retry a fallible operation with exponential backoff
- Write a unit test
- Stream a large response to disk
- Generate a SHA-256 hash
- Spawn a UDP listener
- Build a CLI with subcommands

## In the meantime

Until T68 ships, the canonical examples live in [`examples/`][examples]
in the repo:

[examples]: https://github.com/buff-lang/buff/tree/master/examples

- `ola.buff` — hello world
- `fibonacci.buff` — recursion + arithmetic
- `collections.buff` — `Vector<T>` / `Map<K,V>`
- `pattern_matching.buff` — `match`, `Option`, `Result`
- `error_handling.buff` — `Result`, `?`, builtin `Error`
- `closures.buff` — lambdas + `.map()`
- `async_demo.buff` — `async func`, `spawn`, `.result()`
- `extern_reqwest.buff` — `extern` block for an HTTP client
- `extern_serde_json.buff` — `extern` block for JSON
- `minimal_console.buff` — smallest-possible binary (`--minimal`)
