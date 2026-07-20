# Pattern Matching

## What Rust pain does Buff avoid?

Rust pattern matching is powerful but has ergonomic friction:

1. **`&` on borrowed values in patterns** -- matching over `&Option<T>` requires
   `Some(&v)` or `Some(v)` depending on ownership. Getting this wrong produces
   confusing errors.
2. **Verbosity of `match` as expression** -- `let x = match ...` requires the
   match to produce a value, which means arms must agree on type.
3. **`Result` wrapping** -- every fallible function returns `Result<T, E>`,
   adding nesting when composing fallible operations.

## The Buff equivalent

Buff's `match` has the same semantics but works with owned values (no `&`).
`Some(x)`, `Ok(v)`, `Err(_)`, `None`, and `_` all work the same way. The
`?` operator propagates errors exactly as in Rust.

## Key differences

| Rust | Buff |
|---|---|
| `match &val { Some(&x) => ... }` | `match val { Some(x) => ... }` |
| `match result { Ok(v) => ..., Err(e) => ... }` | Same syntax |
| `items.iter().find(\|&&x\| x == target)` | No references needed |
| `if let Some(x) = val { ... }` | `match val { Some(x) => ..., None => ... }` |
