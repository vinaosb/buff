# Iterators

## What Rust pain does Buff avoid?

Rust's iterator ecosystem is powerful but verbose:

1. **`.iter()` vs `.into_iter()`** -- you must choose between borrowing
   iteration and consuming iteration. Wrong choice = borrow checker error.
2. **`.collect::<Type>()`** -- iterator results need explicit collection
   with type annotation (turbofish or `let x: Vec<i32> = ...`).
3. **`usize` index conversion** -- Rust vectors are indexed by `usize`,
   not `i32`. You must convert: `v[idx as usize]`.
4. **Dereference in closures** -- `.map(|x| x * 2)` on `.iter()` gives `&T`,
   so you often need `|&x|` or `*x`.

## The Buff equivalent

Buff vectors use `.push()`, `.pop()`, `.len()`, `.map()` directly. No
`.iter()`, no `.collect()`, no type annotations. Indexing works with plain
integers. `.map()` consumes and returns a fresh vector.

## Key differences

| Rust | Buff |
|---|---|
| `v.iter().map(\|x\| x * 2).collect()` | `v.map({ x => x * 2 })` |
| `v[idx as usize]` | `v[idx]` |
| `v.len()` | `v.len()` (same) |
| `.into_iter().collect::<Vec<_>>()` | Just `.map()` |
| `.iter().any(\|&x\| x == 20)` | Not yet available (codegen gap) |
