# For-In Loops in Buff

Buff's only loop over iterables is `for VAR in ITERABLE:` — there is no C-style `for(i=0; ...; ...)`. The iterable can be:

- A **Vector literal**: `[10, 20, 30]`
- An **exclusive range**: `0..5` (gives 0,1,2,3,4)
- An **inclusive range**: `1..=5` (gives 1,2,3,4,5)

```buff
let mut total = 0
for n in 1..=5:
    total = total + n
print(total)   // 15
```

`total` must be declared with `let mut` so the body can reassign it. The body is indented (4 spaces, never tabs). Buff's offside-rule lexer ends the loop body at the first dedented line.

## Your task

Replace the TODO with a `let mut sum = 0` binding, a `for n in 1..=5:` loop that adds `n` to `sum`, and a final `print(sum)`. The expected output is `15`.

**Hint:** two new lines go inside the loop body (indented): `sum = sum + n`. Then `print(sum)` goes back at the outer indentation level.
