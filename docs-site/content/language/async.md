+++
title = "Async"
weight = 30
+++

# Async

Buff's headline ergonomic feature is that **there is no `await` keyword**.
Functions declared `async func` run asynchronously, and any caller of an
async function is propagated async-ness automatically (call-graph fixpoint).
The compiler inserts `.await` at async call sites and emits
`#[tokio::main]` on `main` when it joins the async set.

## The model

| You write | Compiler emits |
|---|---|
| `async func fetch():` | `async fn fetch()` |
| a sync fn calling `fetch` | becomes `async fn` automatically |
| `spawn task()` | `tokio::spawn(async move { task() })` |
| `task.result()` | `task.await` |

Around 95% of user code never knows async exists. You write linear-looking
code, the compiler handles the coloring.

## Example

```buff
async func fetch_value() -> Int:
    return 42

// `pipeline` calls an async fn, so it is auto-propagated to `async fn`
// even though the source has no `async` keyword here.
func pipeline() -> Int:
    return fetch_value()

func main():
    // `pipeline()` is async, so `main` becomes async too and gets
    // `#[tokio::main]` automatically.
    let value = pipeline()
    print(value)

    // Spawn a task and await its result via `.result()`.
    let task = spawn fetch_value()
    let answer = task.result()
    print(answer)
```

This is the canonical example in
[`examples/async_demo.buff`][async-demo]. The generated Rust is fully
tested in `crates/buff-lang-codegen-rust/tests/async_codegen.rs`.

[async-demo]: https://github.com/buff-lang/buff/blob/master/examples/async_demo.buff

## Why no `await`?

The "function-coloring problem" (Bob Nystan's classic essay) is the pain
point: once any function is async, *everything* that calls it must also be
async, recursively. Most languages make the developer thread `await`
through every call site manually.

Buff argues the compiler has enough information to do this for you. The
type inferencer already tracks which functions return `Future`; extending
it to "which functions need to become async" is a small fixpoint
computation. The user benefit is large: linear-looking code that compiles
to correctly-`await`ed Rust.

## Spawning tasks

```buff
async func worker(id: Int):
    print("worker " + id.string())

func main():
    let t1 = spawn worker(1)
    let t2 = spawn worker(2)
    t1.result()
    t2.result()
```

`spawn` schedules an async function onto the tokio runtime and returns a
`JoinHandle<T>`. Call `.result()` to await its completion. The spawner
itself stays synchronous.

## Concurrency primitives

| Construct | Source |
|---|---|
| `spawn fn()` | tokio::spawn |
| `task.result()` | `task.await` |
| `Mutex<T>` | tokio::sync::Mutex (via `buff-sync` stdlib, planned) |
| Channels | `buff-pubsub` EventBus (crossbeam + tokio) |

There is no `select!` or `join!` macro in v1.x. For fan-out / fan-in
patterns, spawn N tasks and collect their results.

## Known end-to-end limitation

The v1.x CLI pipeline invokes `rustc` on a *single generated `.rs` file*
with no Cargo project model, so external crates (like `tokio`) are not
wired end-to-end. The async codegen is fully tested in
`crates/buff-lang-codegen-rust/tests/async_codegen.rs`, but `buff run
examples/async_demo.buff` requires the Cargo-project pipeline (deferred
to T120 / v1.3 "Cargo polish").

The generated Rust is correct; only the linker step is missing. In the
meantime, async Buff code runs inside the REPL, Jupyter, and any host that
already links `tokio`.

## Conventions

Buff convention §6: **no `_async` suffix on async functions.** The
compiler infers async-ness; the user shouldn't have to annotate the name.

```buff
// GOOD
async func fetch_user(id: Int) -> User

// BAD — redundant annotation
async func fetch_user_async(id: Int) -> User
```
