# Rust vs Buff: Side-by-Side Examples

Each subdirectory contains a Rust example (the "painful" version) and its
clean Buff equivalent, with a README explaining exactly what Rust friction
Buff eliminates.

## Legend

- **runs** -- `buff run <file>` compiles and executes end-to-end (exit 0).
- **codegen-only** -- transpiles to valid Rust (verified by codegen tests),
  but the single-file `rustc` pipeline cannot link it (needs tokio).
- **parser gap** -- the syntax is codegen-verified, but the CLI's single-file
  parser does not yet accept the declaration at top level. The `.buff` file
  contains a workaround that runs.

## Examples

| # | Topic | Rust Pain | Buff Simplification | Status |
|---|---|---|---|---|
| 1 | [hello_world](./hello_world/) | `println!` macro, semicolons, braces | `print()`, indentation-based blocks | runs |
| 2 | [functions](./functions/) | `format!`, explicit type clutter, braces | Indentation blocks, `Int` type | runs |
| 3 | [recursion](./recursion/) | Multiple integer types, `println!` format | `Int`, `print()` | runs |
| 4 | [borrow_checker](./borrow_checker/) | Ownership moves, `.clone()` litter, `&` refs | Move-by-default, auto-clone | runs |
| 5 | [pattern_matching](./pattern_matching/) | `&` on borrowed match patterns | Owned values, no dereference | runs |
| 6 | [error_handling](./error_handling/) | `Box<dyn Error>`, `impl Display`, boilerplate | Builtin `Error`, `?` operator | runs |
| 7 | [closures](./closures/) | `Fn/FnMut/FnOnce`, `move`, `.collect()` | `{ x => body }`, no capture traits | runs |
| 8 | [iterators](./iterators/) | `.iter()/.into_iter()`, `.collect()`, `usize` | `.map()`, direct indexing | runs |
| 9 | [collections](./collections/) | `use std::collections`, `HashMap::new()`, `.get(&k)` | Builtin `Map<K,V>`, literal syntax | runs |
| 10 | [null_safety](./null_safety/) | `.unwrap()` temptation, `if let` verbosity | `match` on `Option<T>`, no null | runs |
| 11 | [structs](./structs/) | `#[derive]`, `pub`, `impl`, `&self`, `.clone()` | No derives, no pub, no impl (parser gap) | parser gap |
| 12 | [enums](./enums/) | `Type::Variant` qualification, `#[derive]` | Unqualified variants (parser gap) | parser gap |
| 13 | [lifetimes](./lifetimes/) | `'a` annotations, `&str` vs `String`, struct lifetimes | No references, no lifetimes (parser gap) | parser gap |
| 14 | [async_await](./async_await/) | Function coloring, `.await` everywhere, `#[tokio::main]` | No `await`, auto-propagate (codegen-only) | codegen-only |

## Running the examples

```bash
# Run a single example (for "runs" status):
cargo run -p buff-lang-cli -- run examples/rust-vs-buff/hello_world/hello_world.buff

# Or with the built binary:
target/release/buff run examples/rust-vs-buff/pattern_matching/pattern_matching.buff

# For codegen-only examples, the Buff source transpiles but cannot link:
# (See the per-topic README for how to verify codegen manually.)
```

## Buff's pitch in one line

Rust performance with Go productivity. Write indentation-based code, get a
native binary compiled via LLVM, without writing a single lifetime annotation,
borrow checker fight, or `.clone()`.
