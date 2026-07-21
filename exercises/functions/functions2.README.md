# Multiple Parameters & Named Arguments

Buff functions accept multiple positional parameters separated by commas. For boolean-like or option-like arguments where positional calls hurt readability, Buff uses **named arguments** — the parameter name followed by a colon and the value, at the call site:

```buff
func greet(name: String, excited: Bool):
    if excited:
        print("Hello, " + name + "!")
    else:
        print("Hello, " + name)

// Callers pass positionals first, then named args:
greet("Buff", excited: true)
greet("Buff", excited: false)
```

The name is part of the *call*, not optional sugar — Buff rejects positional booleans when the parameter reads like a flag (rule §11 of the conventions). Always pair a `Bool` parameter with a named argument.

## Your task

1. Define `greet(name: String, excited: Bool)` above `main`.
2. Inside, branch on `excited` to print the punctuated or plain greeting.
3. Call `greet("Buff", excited: true)` and `greet("Buff", excited: false)`.

**Hint:** the function header is `func greet(name: String, excited: Bool):` with an indented body. Call sites use `greet("Buff", excited: true)` — positional first, named second.
