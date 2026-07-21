# Using Generic Types

Buff ships four core generic types from the prelude:

| Type | Read | Use |
|---|---|---|
| `Vector<T>` | "vector of T" | growable array of T |
| `Map<K, V>` | "map from K to V" | key→value dictionary |
| `Option<T>` | "optional T" | `Some(v)` / `None` |
| `Result<T, E>` | "result of T or E" | `Ok(v)` / `Error(...)` |

The angle brackets carry the type parameters. Most of the time the compiler infers them from the literal (`[1, 2, 3]` → `Vector<Int>`), but you can write them out explicitly:

```buff
let nums: Vector<Int> = [1, 2, 3]
let maybe: Option<String> = None
let outcome: Result<Int, Error> = Ok(7)
```

When `None` has no value to infer from, the annotation is genuinely necessary — the compiler can't know what `T` you meant. Same for `Ok(7)` without context (could be `Result<Int, Anything>`).

## Your task

Add the correct explicit generic annotation to each `let`:
1. `let nums: Vector<Int> = [1, 2, 3]`
2. `let maybe: Option<String> = None`
3. `let outcome: Result<Int, Error> = Ok(7)`

**Hint:** the syntax is `NAME: GENERIC_TYPE<PARAMS> = VALUE`. Angle brackets hold comma-separated type parameters. `None` and `Ok(...)` need an annotation to disambiguate.
