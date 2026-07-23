+++
title = "Testing"
weight = 48
+++

# Testing recipes

Recipes for `buff test`. Test functions are marked with the `@test`
attribute; the runner discovers them at the file level (no separate
test directory required). `buff-assertions` (T38) provides the fluent
`assertThat` API; `buff-mock` (T37) provides mocking.

## Write a unit test

**Problem**: Verify a function returns the expected value.

**Solution**:

```buff
func add(a: Int, b: Int) -> Int:
    return a + b

@test
func test_add():
    assert_eq(add(2, 3), 5)

func main():
    print("run with: buff test")
```

**Explanation**:

`@test` marks the function as a unit test. Run `buff test` from the
project root; the test runner discovers every `@test` function in the
project's `.buff` files, runs each in a fresh process, and reports
pass/fail. `assert_eq(actual, expected)` panics on mismatch (via
Rust's `assert_eq!` macro) — the test runner catches the panic and
reports it as a failure.

Tests live next to the code they test; there's no separate
`tests/` directory at the language level (though projects may
organise them that way). Multiple `@test` functions per file are
fine; each runs independently — a panic in one doesn't affect others.

## Use the fluent assertion API

**Problem**: Write assertions with descriptive failure messages.

**Solution**:

```buff
import { assertThat } from "buff-assertions"

@test
func test_score():
    let score = compute_score("alice")
    assertThat(score)
        .isEqualTo(95)
        .isGreaterThan(50)
        .isLessThan(100)

func compute_score(name: String) -> Int:
    return 95
```

**Explanation**:

`assertThat(value)` returns an `AssertThat<T>` wrapper whose methods
(`isEqualTo`, `isGreaterThan`, `isLessThan`, `isInstanceOf`,
`contains`, etc.) panic with a descriptive message on failure. The
chainable form reads top-to-bottom like a spec; each method returns
`self` so you can stack checks.

`buff-assertions` (T38) is the underlying crate — pure-Rust, no test
framework dependency beyond Rust's built-in `assert_eq!`. The
`assertThat` prelude function (`PreludeFn::AssertThat`) lowers to
`buff_assertions::assertThat(value)`.

## Mock a dependency

**Problem**: Replace a real implementation with a stand-in during tests.

**Solution**:

```buff
import { Mock } from "buff-mock"

@test
func test_with_mock():
    let clock = Mock.new()
    clock.when("now").returns(12345)
    let result = format_time(clock)
    assert_eq(result, "12:34:05")

func format_time(clock: Clock) -> String:
    let n = clock.now()
    return "12:34:05"
```

**Explanation**:

`buff-mock` (T37) ships a `Mock` type that records calls and returns
canned values. `mock.when(method_name).returns(value)` sets up an
expectation; subsequent calls to `mock.method_name()` return the
canned value. `mock.times_called(method_name)` lets you assert the
mock was used the expected number of times.

Mocking is best reserved for things you can't control (the system
clock, an HTTP API you don't own). For your own code, prefer the
recipe below — property tests over mocks, integration tests over
hand-written doubles.

## Write a property test

**Problem**: Verify a property holds for many random inputs, not just
a hand-picked few.

**Solution**:

```buff
import { Fuzz, Strategy } from "buff-fuzz"

@test
func test_reverse_is_involutive():
    let strategy = Strategy.vector(Strategy.int(0, 1000), max_len: 20)
    Fuzz.run(strategy, iterations: 200, closure: { input =>
        let once = reverse(input)
        let twice = reverse(once)
        return twice == input
    })

func reverse(xs: Vector<Int>) -> Vector<Int>:
    var out: Vector<Int> = []
    var i: Int = xs.len() - 1
    while i >= 0:
        out.push(xs[i])
        i = i - 1
    return out
```

**Explanation**:

`Fuzz.run(strategy, iterations, closure)` (T27 v1.13 frameworks wave
5) generates `iterations` random inputs from `strategy` and runs
`closure` on each. If `closure` returns `false` for any input, the
test fails with the failing case attached. `buff-fuzz` is backed by
`proptest` (NOT cargo-fuzz / AFL — those link C shims).

The strategy builder composes: `Strategy.int(min, max)`,
`Strategy.float(min, max)`, `Strategy.vector(elem_strategy,
max_len: N)`, `Strategy.string(max_len: N)`. For deterministic test
runs (CI), seed the strategy via `Strategy.with_seed(seed,
elem_strategy)` — same seed, same inputs.

## Snapshot-test a function

**Problem**: Lock in the exact output of a function so future
regressions are caught.

**Solution**:

```buff
@test
func test_format_snapshot():
    let output = format_report({"name": "Ada", "score": 95})
    assert_eq(output, "Ada: 95")

func format_report(entry: Map<String, String>) -> String:
    let name = entry.get("name") ?? "anonymous"
    let score = entry.get("score") ?? "0"
    return name + ": " + score
```

**Explanation**:

For small outputs, `assert_eq(actual, expected)` is a snapshot test —
update the expected string when the output intentionally changes.
For larger outputs (multi-line reports, AST dumps, codegen), the
workspace uses the `insta` Rust crate via `cargo insta review`; the
Buff-side equivalent is planned for v1.18+.

The pattern: write the function, run the test once to capture the
"known-good" output, paste it into the assertion. Future changes
that alter the output fail the test until you update the assertion
(or roll back the change). This is how the Buff compiler's own
codegen tests work — see `crates/buff-lang-codegen-rust/tests/`.

## Run a micro-benchmark

**Problem**: Measure how long a function takes to run.

**Solution**:

```buff
@bench
func bench_sort():
    let big = [9, 8, 7, 6, 5, 4, 3, 2, 1, 0]
    for _ in 0..1000:
        let sorted = sort_desc(big)

func sort_desc(xs: Vector<Int>) -> Vector<Int>:
    return xs.par_reduce([], { acc, x => [x] + acc })
```

**Explanation**:

`@bench` marks a benchmark function. Run `buff bench`; the runner
executes each benchmark in a loop, measures wall-clock time per
iteration, and reports mean + p99. The shape mirrors Rust's
`#[bench]` from the unstable test crate — simpler than `criterion`,
no plotting, just numbers.

For statistical-rigour benchmarks (regression detection, p-value
tests, plots), use `criterion` via the FFI surface — Buff's
`extern "C" from "criterion" func ...` declares the binding. The
`@bench` attribute is for quick "is this faster?" checks during
development.
