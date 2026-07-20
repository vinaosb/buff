# Null Safety

## What Rust pain does Buff avoid?

Rust and Buff both reject null pointers. But Rust's `Option<T>` has some
ergonomic friction:

1. **`.unwrap()` temptation** -- in quick scripts, `.unwrap()` is everywhere.
   It panics on `None`, which is the very thing `Option` was supposed to
   prevent. There's no lightweight syntax for "give me a default."
2. **`if let Some(x) = val`** -- partial matching adds indentation and
   a second code path. You must decide between `match`, `if let`, `unwrap`,
   `unwrap_or`, `map`, `and_then`, etc.
3. **`.map()` on Option** -- `opt.map(|x| x.to_uppercase())` is clean but
   the closure syntax is heavier than necessary for simple transformations.

## The Buff equivalent

Buff uses the same `Option<T>` / `Some(x)` / `None` model. No null, no nil,
 no undefined. The `match` expression handles both arms cleanly. Buff's
lambda syntax `{ x => ... }` makes `.map()` on collections lighter than
Rust's `|x| ...`.

## Key differences

| Rust | Buff |
|---|---|
| `Option<T>` (same) | `Option<T>` (same) |
| `.unwrap()` (panics!) | Use `match` instead (no unwrap in Buff) |
| `if let Some(x) = val { ... }` | `match val { Some(x) => ..., None => ... }` |
| `val.unwrap_or("default")` | Not yet available (planned) |
| `opt.map(\|x\| x + 1)` | Same pattern with `{ x => x + 1 }` |
