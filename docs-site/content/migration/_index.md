+++
title = "Migration guide"
weight = 50
sort_by = "weight"
+++

# Migration guide

> **Status:** placeholder — the migration guides ship as **T69** in the
> v1.21.0 "Community & Quality" milestone. They depend on this docs site
> (T67) shipping first.

The migration guides will help developers coming from other languages
(primarily Rust, Go, TypeScript, and Python) ramp up on Buff quickly.
Each guide will cover:

- **Syntax mapping** — table of "you write X in Rust, here's the Buff
  equivalent"
- **Tooling mapping** — `cargo` → `buff`, `rust-analyzer` → `buff-lsp`, etc.
- **Ecosystem mapping** — which `buff-*` crate wraps which popular Rust /
  Go / npm package
- **Mental-model shifts** — the things Buff deliberately omits and why

## Rust → Buff

Buff transpiles to Rust, so Rust developers are the primary audience. The
shifts to internalize:

| Rust concept | Buff equivalent |
|---|---|
| `&T`, `&mut T`, lifetimes | *(hidden — owned data + intelligent clones)* |
| `Result<T, E>`, `?` | identical (`Result<T, E>`, `?`) |
| `Option<T>` | identical (`Option<T>`) |
| `async fn`, `.await` | `async func`, no `await` keyword |
| `tokio::spawn` | `spawn fn()` |
| `#[tokio::main]` | auto-emitted when `main` joins the async set |
| `Vec<T>`, `HashMap<K,V>` | `Vector<T>`, `Map<K,V>` |
| `match` arms with `{ }` | `match v { Pat => body }` |
| `impl Trait for Type` | identical |
| `trait Shape { ... }` | identical |
| `panic!`, `unwrap` | *(forbidden in non-test code)* |
| `unsafe { }` | `unsafe` keyword exists but is discouraged |

## Go → Buff

| Go concept | Buff equivalent |
|---|---|
| `goroutine` | `spawn fn()` |
| `channel` | *(use `buff-pubsub` EventBus)* |
| `interface{}` / `any` | *(use generics or `Trait`)* |
| `nil` | `None` (`Option<T>`) |
| `error` return value | `Result<T, E>` |
| `defer` | *(not supported — use explicit cleanup)* |
| `go mod` | `buff.toml` + `buff add` |
| `go test` | `buff test` |

## TypeScript → Buff

| TS concept | Buff equivalent |
|---|---|
| `Promise<T>` | `async func` returning `T` (no `await`) |
| `null` / `undefined` | `Option<T>` |
| `try / catch` | `Result<T, E>` + `match` |
| `any` | *(forbidden — use generics)* |
| `npm` | `buff add` + `buff-registry` |
| `tsc` | `buff check` |

## Python → Buff

| Python concept | Buff equivalent |
|---|---|
| `None` | `None` singleton (`Option<T>`) |
| `try / except` | `Result<T, E>` + `match` |
| `async def` / `await` | `async func` (no `await`) |
| `list` / `dict` | `Vector<T>` / `Map<K,V>` |
| `pip` | `buff add` |
| `pytest` | `buff test` |

## Until T69 ships

For now, the fastest ramp-up is:

1. Read [Your first program](../getting-started/first-program/).
2. Browse [`examples/`][examples] — pick the one closest to what you want
   to build.
3. Skim the [syntax reference](../language/syntax/).

[examples]: https://github.com/buff-lang/buff/tree/master/examples
