# Guard Statements (let-else)

Buff's `guard ... else { ... }` is an early-return check that reads like the iterator form `for v in iter`. If the condition (or pattern) fails, the `else` block runs — and it MUST diverge (return/break/continue). If the condition succeeds, execution falls through.

```buff
func sqrt_safe(n: Int) -> Int:
    guard n >= 0 else { return 0 }
    // ... happy path here, n is non-negative ...
    return n

// Pattern form — diverge if the Option is None:
guard let Some(v) = opt else { return 0 }
print(v)
```

The `let PATTERN = EXPR` form binds the pattern's names in the *enclosing* scope when the guard succeeds. Multiple comma-separated conditions are allowed (`guard let Some(x) = opt, x > 0 else { return }`).

`guard` is part of Buff's 25-keyword lexicon and lowers to Rust's `let ... else`.

## Your task

Implement `sqrt_safe(n: Int) -> Int` that:
1. Uses `guard n >= 0 else { return 0 }` to reject negatives.
2. Returns `n` on the happy path (we don't actually compute a square root — just demonstrate the guard).

Call `sqrt_safe(9)` (returns `9`) and `sqrt_safe(-1)` (returns `0`).

**Hint:** the guard line is `guard n >= 0 else { return 0 }`. After it, a plain `return n` is the happy path. Braces around the else-block are required.
