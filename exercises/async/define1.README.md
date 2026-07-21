# Async Functions and Spawn

Buff's headline feature: **there is no `await` keyword**. You declare a function `async func`, and the compiler inserts `.await` at call sites automatically. Any function that *calls* an async function is itself promoted to `async`, all the way up to `main` (which receives `#[tokio::main]` automatically).

```buff
async func fetch_value() -> Int:
    return 42

func main():
    let value = fetch_value()        // `.await` inserted for you
    print(value)                     // 42

    // Spawn a concurrent task, then await its result via .result()
    let task = spawn fetch_value()
    let answer = task.result()
    print(answer)                    // 42
```

Naming rule: never suffix an async function with `_async` (rule §6) — Buff doesn't need the visual marker because the call site looks identical to a sync call.

**Note (codegen-only):** Buff's async model transpiles correctly (verified by `crates/buff-lang-codegen-rust/tests/async_codegen.rs`) but the single-file `rustc` CLI pipeline cannot link the external `tokio` crate yet. End-to-end async execution arrives when the Cargo-project pipeline (v1.3) lands. The exercise here focuses on SYNTAX.

## Your task

Define `async func fetch_value() -> Int:` above `main` (returning `42`). In `main`, follow the example: call `fetch_value()`, then spawn it and call `.result()`. Print both values.

**Hint:** the function header is `async func fetch_value() -> Int:` with an indented `return 42`. The spawn line is `let task = spawn fetch_value()` (no parentheses around the call after `spawn`).
