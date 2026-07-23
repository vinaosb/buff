+++
title = "Parallel"
weight = 45
+++

# Parallel recipes

Recipes for CPU-level data parallelism and task-level concurrency.
Buff's parallel surface is the `Vector<T>` combinators (`.par_map`,
`.par_filter`, `.par_reduce`) and the `Channel` MPSC primitive. CPU
dispatch lowers to `rayon`; async dispatch lowers to `tokio::spawn`.

## Map in parallel

**Problem**: Apply a function to every element of a large vector using
all CPU cores.

**Solution**:

```buff
func square_all(values: Vector<Int>) -> Vector<Int>:
    return values.par_map({ x => x * x })

func main():
    let big = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    let squared = square_all(big)
    print(squared)
```

**Explanation**:

`Vector.par_map(f)` is the parallel map — it splits the input into
chunks, runs `f` on each chunk on a separate rayon worker thread, and
joins the results in order. The output order matches the input order,
unlike a raw thread-per-element approach. The closure body must be
`Send + Sync` (Buff hides this; the borrow checker verifies it on the
generated Rust).

For arithmetic-intensity loops (e.g. elementwise math on big
vectors), the runtime may also dispatch the same call to GPU via
WGSL — see the `@prefer(gpu)` attribute in
[Language → Attributes](../language/attributes/). Without the hint,
the compiler uses an arithmetic-intensity threshold.

## Filter in parallel

**Problem**: Keep only elements that satisfy a predicate, in parallel.

**Solution**:

```buff
func evens(values: Vector<Int>) -> Vector<Int>:
    return values.par_filter({ x => x % 2 == 0 })

func main():
    let nums = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    print(evens(nums))
```

**Explanation**:

`Vector.par_filter(pred)` parallelises the filter — each chunk's
matching elements are collected, then concatenated in order. The
predicate is a closure `{ x => Bool }`. For chained transforms,
prefer `par_map` + `par_filter` separately over a fused
`par_filter_map` — the explicit two-step reads more clearly and lets
rayon pipeline the two passes.

Order is preserved: `par_filter([3, 1, 4, 1, 5, 9, 2, 6], even?)`
returns `[4, 2, 6]`, not `[4, 2, 6]` in some shuffled order. If order
doesn't matter (e.g. set membership checks), use `par_filter_unordered`
(planned) for a small speedup.

## Reduce in parallel

**Problem**: Collapse a vector into a single value in parallel
(e.g. sum, product, max).

**Solution**:

```buff
func sum_all(values: Vector<Int>) -> Int:
    return values.par_reduce(0, { a, b => a + b })

func main():
    let nums = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    print(sum_all(nums))
```

**Explanation**:

`Vector.par_reduce(initial, combine)` is the parallel reduce — it
splits the input into chunks, reduces each chunk to a single value,
then combines the per-chunk results with the `initial` value. The
`combine` closure must be **associative** (`(a + b) + c == a + (b +
c)`); otherwise the result is non-deterministic across chunk
boundaries. `+`, `*`, `min`, `max`, and string concatenation are all
associative.

For sum/mean/min/max/count specifically, the `GroupBy.agg(col, op)`
path on a `DataFrame` is preferred — it's both parallel and
type-checked at the column level.

## Producer / consumer channel

**Problem**: Spawn one task that produces values, another that
consumes them, and decouple their rates via a bounded buffer.

**Solution**:

```buff
func producer(sender: Sender<Int>):
    for i in 0..10:
        sender.send(i)

func consumer(receiver: Receiver<Int>) -> Int:
    var total: Int = 0
    for let next = receiver.recv() {
        total = total + next
    }
    return total

func main():
    let (sender, receiver) = Channel.new(8)
    spawn { producer(sender) }
    let total = consumer(receiver)
    print(total)
```

**Explanation**:

`Channel.new(buf_size)` returns a `(Sender<T>, Receiver<T>)` pair
backed by `tokio::sync::mpsc::channel` (T2 v1.13 frameworks wave 1).
The producer pushes values; the consumer pulls them. When the buffer
fills, `sender.send(value)` blocks (auto-await); when it empties,
`receiver.recv()` returns `None` — that's the loop terminator.

The `for let next = receiver.recv() { ... }` form is the idiomatic
"drain until closed" — the loop exits when `recv()` returns `None`,
which happens once every `Sender` clone is dropped. Buff's
move-by-default semantics hide the ownership transfer: the closure
`{ producer(sender) }` captures `sender` by move, so when the spawn
body exits, the sender drops and the receiver loop terminates.

## Race two futures

**Problem**: Start two async operations and proceed with whichever
finishes first; cancel the other.

**Solution**:

```buff
async func fetch_primary() -> String:
    sleep(Duration.millis(50))
    return "primary"

async func fetch_fallback() -> String:
    sleep(Duration.millis(200))
    return "fallback"

func fastest(client: HttpClient, url: String) -> String:
    let t1 = spawn fetch_primary()
    let t2 = spawn fetch_fallback()
    let first = Channel.new(1)
    spawn {
        let r = t1.result()
        first.sender().send(r)
    }
    spawn {
        let r = t2.result()
        first.sender().send(r)
    }
    return first.receiver().recv()

func main():
    print(fastest(HttpClient.new(), "https://example.com"))
```

**Explanation**:

There's no `select!` macro in Buff v1.x (see
[Language → Async](../language/async/)). The race pattern is: spawn
both operations, have each write its result to a shared channel, read
the first value off the channel. The slower task keeps running —
 cancellation is the user's job (track the `JoinHandle` and call
 `.abort()` on it, planned for v1.18+).

For the common case of "fetch from primary, fall back to replica on
timeout", `buff-resilience` (T36) ships a `Race` type with built-in
cancellation. The recipe above is the explicit-channel version for
when you want full control.
