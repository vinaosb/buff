# Closures

## What Rust pain does Buff avoid?

Rust closures have three traits (`Fn`, `FnMut`, `FnOnce`) that determine
how the closure captures variables from its environment. While Rust infers
the trait most of the time, certain patterns force explicit handling:

1. **`move` keyword** -- to capture by value instead of by reference, you
   must write `move ||`. The captured variable becomes unusable afterward.
2. **`.collect::<Type>()`** -- iterator chains don't know the output type
   without explicit turbofish or type annotation.
3. **Dereference in closures** -- `.filter(|x| *x % 2 == 0)` requires `*x`
   because the closure receives `&i32`, not `i32`.

## The Buff equivalent

Buff closures use the `{ params => body }` syntax. No capture traits,
no `move` keyword, no `*` dereferences. The compiler handles all capture
semantics. `.map()` takes a lambda directly.

## Key differences

| Rust | Buff |
|---|---|
| `\|x\| x * 2` | `{ x => x * 2 }` |
| `move \|x\| format!("{}{}", s, x)` | `{ x => s + x }` (no move) |
| `.iter().map(\|x\| x * 2).collect::<Vec<_>>()` | `.map({ x => x * 2 })` |
| `.filter(\|x\| *x % 2 == 0)` | No filter end-to-end yet (codegen gap) |
| Fn/FnMut/FnOnce traits | Not exposed to the user |
