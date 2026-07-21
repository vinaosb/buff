# Map<K, V> Basics

Buff's key-value collection is `Map<K, V>` (lowers to Rust's `HashMap<K, V>`). The literal uses **braces** (because braces are *data* in Buff), with `key: value` pairs separated by commas:

```buff
let scores = {1: 10, 2: 20, 3: 30}
print(scores.len())          // 3
```

Why braces? Buff's rule: braces `{}` are for **data** (struct literals, maps, lambdas `{ x => ... }`, string interpolation `{expr}`). Control flow uses indentation. A map literal carries data, so it gets braces.

## Current v0.5 limitation

Keyed lookup `m[k]` is a documented codegen gap (the generated Rust has no `Index` impl on `HashMap`). Construction and `.len()` work end-to-end; reading a value by key requires pattern matching on the `.get(k)` result, which returns `Option<V>`.

## Your task

Declare `let scores = {1: 10, 2: 20, 3: 30}` and print `scores.len()`. Expected output: `3`.

**Hint:** braces for the literal, colons between each key and value, commas between pairs. No `Map` keyword needed — the compiler infers the type from the literal shape.
