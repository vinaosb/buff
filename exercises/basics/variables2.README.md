# Mutable Variables in Buff

Buff bindings are **immutable by default** — once you write `let x = 5`, you cannot reassign `x`. To opt back in to mutation, declare the binding with `let mut`:

```buff
let immut = 10
// immut = 20   // compile error

let mut counter = 0
counter = counter + 1   // OK
counter = counter + 1   // OK
```

The `mut` keyword is part of the 25 reserved Buff keywords. It only needs to appear at the `let` — later assignments use plain `=`.

## Your task

In `variables2.buff`, replace the two TODO markers with:
1. A `let mut counter = 0` declaration.
2. A `counter = counter + 1` reassignment.

Leave the final `print(counter)` call intact (it should print `1`, not `0`).

**Hint:** the syntax is `let mut NAME = VALUE` for the declaration, then plain `NAME = NEW_VALUE` on a later line. Buff uses 4-space indentation; no braces needed.
