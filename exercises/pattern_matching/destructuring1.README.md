# Destructuring `let` Bindings

Buff's `let` can bind multiple names at once by *destructuring* a compound value. The pattern mirrors the shape of the data:

```buff
let pair = (10, 20)
let (a, b) = pair          // a = 10, b = 20
let (first, _, third) = (1, 2, 3)   // skip the middle with _
```

Tuple destructuring uses parentheses. The `_` wildcard discards a slot — useful when you only care about some positions. Patterns are checked at parse time, so a 3-element tuple pattern on a 2-element value is a compile error.

Struct-pattern destructuring also works on tagged values (`let Point { x, y } = p`), reusing the same `parse_pattern` machinery that drives `match` arms.

## Your task

In `destructuring1.buff`, replace the TODO with:
1. `let pair = (1, 2)` — build a 2-tuple.
2. `let (a, b) = pair` — destructure into `a` and `b`.
3. Print both `a` and `b` on separate lines (expected output: `1` then `2`).

**Hint:** tuple patterns look like `(x, y)`. Wildcards look like `_`. The RHS of `let PATTERN = VALUE` is the value being destructured.
