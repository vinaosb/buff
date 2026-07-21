# Match Expression in Buff

Buff's `match` is a multi-arm branch. The canonical syntax uses braces (because match is *data-like*: each arm maps a pattern to a value), with arms separated by commas:

```buff
match n {
    0 => print("zero"),
    1 => print("one"),
    _ => print("many"),
}
```

- `0` and `1` are **literal patterns** — they match by value.
- `_` is the **wildcard** — it matches anything, so it acts as a catch-all / default.
- Arms are tried top-to-bottom; the first match wins.
- Each arm body is a single expression on the right of `=>`.

`match` is exhaustive: if you forget the `_` and miss a value, the compiler tells you.

## Your task

Implement `describe(n: Int)` that prints `"zero"` for 0, `"one"` for 1, and `"many"` for anything else. Call it from `main` with values 0, 1, and 7.

**Hint:** Use `match n { 0 => print("zero"), 1 => print("one"), _ => print("many") }`.
