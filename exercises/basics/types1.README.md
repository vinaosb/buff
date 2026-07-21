# Type Annotations in Buff

Buff is statically typed with aggressive inference — most of the time you write `let x = 5` and the compiler infers `Int`. When you want to be explicit (for clarity, documentation, or to pin a literal to a wider type), use a colon followed by the type name:

```buff
let name: String = "Buff"
let count: Int = 42
let ready: Bool = true
let pi: Float = 3.14
```

The four core primitive types are `Int` (64-bit signed integer), `String` (UTF-8 text), `Bool` (`true`/`false`), and `Float` (32-bit float). Buff hides width annotations like `Int<64>` from the user — the compiler picks the right width internally.

## Your task

In `types1.buff`, add the correct type annotation (`: String`, `: Int`, or `: Bool`) to each of the three `let` bindings, between the variable name and the `=` sign.

**Hint:** the syntax is `let NAME: TYPE = VALUE`. Look at the literal on the right side: `"Buff"` is text, `42` is a whole number, `true` is a Boolean.
