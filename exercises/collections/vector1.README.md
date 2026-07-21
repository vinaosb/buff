# Vector<T> Basics

Buff's growable array is `Vector<T>` (lowers to Rust's `Vec<T>`). The literal uses square brackets. Operations follow Rust naming:

```buff
let mut stack = [1, 2, 3]
stack.push(4)              // mutate; binding MUST be `let mut`
print(stack.len())         // 4

let top = stack.pop()      // returns Option<Int>; removes the last element
match top { Some(x) => print(x), None => print(0) }    // 4

let v = [10, 20, 30]
print(v[0])                // 10 — index access
print(v[2])                // 30
```

Key points:
- `.push(x)` mutates the vector in place → the binding MUST be `let mut`.
- `.pop()` returns `Option<T>` (`Some(last)` if non-empty, `None` if empty) — never panics.
- `.len()` returns an `Int`.
- Index access uses square brackets: `v[i]`.

`.map({ x => x * 2 })` produces a fresh vector (the original is consumed — Buff is move-by-default).

## Your task

1. Declare `let mut stack = [1, 2]` (mut because we'll push).
2. `stack.push(3)` to add a third element.
3. `print(stack.len())` (expected: `3`).
4. `let top = stack.pop()` then `match top { Some(x) => print(x), None => print(0) }` (expected: `3`).

**Hint:** mutation requires `let mut`. `.pop()` returns an Option — always match both `Some` and `None` arms.
