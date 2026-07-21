# Composing Async Functions

Because Buff has no `await` keyword, calling one async function from another looks *identical* to a sync call. The compiler propagates async-ness up the call graph automatically: if `second()` calls an async `first()`, then `second()` is silently promoted to `async fn` too — and any caller of `second()` is promoted, and so on, until `main` gets `#[tokio::main]`.

```buff
async func first() -> Int:
    return 10

func second() -> Int:               // declared `func`, but BECOMES `async fn`
    return first() + 5              // `.await` auto-inserted by codegen

func main():                         // also auto-promoted
    print(second())                  // 15
```

This is what "no function-coloring problem" means in Buff: a sync-shaped function can call an async one without ceremony. Compare with Rust/Python/TS where the callee being async forces you to either rewrite the caller as async or wrap the call in a runtime block.

**Note (codegen-only):** end-to-end async execution requires the Cargo pipeline (v1.3). The transpiled Rust is correct; the v0.5/v1.x CLI just can't link `tokio` in single-file mode yet. This exercise focuses on syntax.

## Your task

1. Define `async func first() -> Int:` returning `10`.
2. Define `func second() -> Int:` whose body returns `first() + 5` (no `await`, no special syntax).
3. In `main`, call `print(second())`. Expected output: `15`.

**Hint:** the trick is that `second` looks completely synchronous in the source — just `return first() + 5`. The async-ness is invisible at the call site.
