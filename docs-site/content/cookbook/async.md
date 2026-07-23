+++
title = "Async"
weight = 46
+++

# Async recipes

Recipes for asynchronous code. The headline rule: **there is no
`await` keyword in Buff**. Functions declared `async func` run
asynchronously; any caller of an async function is auto-propagated to
async by the compiler (call-graph fixpoint). Around 95% of user code
never knows async exists.

## Spawn a background task

**Problem**: Kick off work that runs concurrently with the caller,
without blocking on its result.

**Solution**:

```buff
async func worker(id: Int):
    sleep(Duration.millis(100))
    print("worker " + id.string() + " done")

func main():
    let t1 = spawn worker(1)
    let t2 = spawn worker(2)
    let t3 = spawn worker(3)
    t1.result()
    t2.result()
    t3.result()
```

**Explanation**:

`spawn fn(args)` schedules the call onto the tokio runtime and
returns a `JoinHandle<T>` immediately. The spawner keeps running;
call `.result()` on the handle to await completion. The three workers
above run concurrently — total wall-clock time is ~100ms, not ~300ms.

`main` becomes async automatically because it calls `.result()`
(which awaits). The compiler emits `#[tokio::main]` on the generated
Rust `main` whenever `main` joins the async set — you never write
that attribute yourself.

## Sleep without blocking

**Problem**: Pause execution of an async function for a duration
without blocking the OS thread.

**Solution**:

```buff
async fn_delayed(msg: String):
    sleep(Duration.seconds(1))
    print(msg)

func main():
    let t = spawn delayed("hello after 1s")
    t.result()
```

**Explanation**:

`sleep(duration)` is the async sleep — it lowers to
`tokio::time::sleep(duration).await` and yields control back to the
runtime for the duration. The OS thread is free to run other tasks
during the sleep; this is the difference between `sleep` and a busy
`for` loop.

`Duration` is the chrono-backed prelude type (T124). Construct one
with `Duration.seconds(n)`, `.millis(n)`, `.minutes(n)`, `.hours(n)`,
or `.days(n)`; combine with `+` and `-`. The unit-builder shape
matches Rust's `Duration::from_secs(_)` family but reads more
naturally.

## Apply a timeout

**Problem**: Cap how long an async operation can take; abort if it
exceeds the budget.

**Solution**:

```buff
async func slow_op():
    sleep(Duration.seconds(10))
    return "done"

func with_timeout() -> String:
    let task = spawn slow_op()
    sleep(Duration.seconds(1))
    match task.is_finished() {
        true  => return task.result(),
        false => return "timed out"
    }
```

**Explanation**:

There's no built-in `tokio::time::timeout` wrapper surfaced in the
prelude yet. The recipe polls the task's state after the timeout
budget elapses — `JoinHandle.is_finished()` returns `Bool` without
consuming the handle. If the task is still running, treat it as a
timeout. The task keeps running in the background; explicit
cancellation (`task.abort()`) is on the v1.18+ roadmap.

For production use, `buff-resilience` (T36) ships a `Timeout` type
that wraps any async operation with a deadline and surfaces a
`Result<T, TimeoutError>`. Prefer it over the hand-rolled polling
above.

## Gather results from many tasks

**Problem**: Spawn N tasks and collect all their results, in
submission order.

**Solution**:

```buff
async func fetch_one(url: String) -> String:
    sleep(Duration.millis(50))
    return "body of " + url

func gather_all(urls: Vector<String>) -> Vector<String>:
    var handles: Vector<JoinHandle<String>> = []
    for url in urls:
        let h = spawn fetch_one(url)
        handles.push(h)
    var results: Vector<String> = []
    for h in handles:
        results.push(h.result())
    return results

func main():
    let urls = ["https://a.example", "https://b.example", "https://c.example"]
    for r in gather_all(urls):
        print(r)
```

**Explanation**:

The pattern is "fan out, then fan in." The first `for` builds a
vector of handles; the second `for` awaits each in submission order.
All three fetches run concurrently — total wall-clock is ~50ms, not
~150ms.

The order of `results` matches the order of `urls`. If you want
completion-order instead (first-finished-first-collected), write each
result to a shared `Channel` as it completes and drain the channel —
see [Parallel → Race two futures](./parallel/#race-two-futures).

## Run two operations concurrently

**Problem**: Start two async ops, await both, return both results.

**Solution**:

```buff
async func fetch_a() -> String:
    sleep(Duration.millis(50))
    return "a"

async func fetch_b() -> String:
    sleep(Duration.millis(80))
    return "b"

func both() -> (String, String):
    let ha = spawn fetch_a()
    let hb = spawn fetch_b()
    let a = ha.result()
    let b = hb.result()
    return (a, b)

func main():
    let (a, b) = both()
    print(a)
    print(b)
```

**Explanation**:

Spawning both before awaiting either is what makes them concurrent.
If you wrote `let a = fetch_a(); let b = fetch_b();` (no spawn), each
call would block until done — total time ~130ms instead of ~80ms.

The return type `(String, String)` is a `Tuple<String, String>` —
fixed arity, fixed types. For a variable number of results of the
same type, use `Vector<String>` (see the gather recipe above). Buff
tuples lower to Rust tuples; tuple destructuring `let (a, b) = ...`
works as in Rust.
