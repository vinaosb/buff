# buff-resilience

Resilience patterns for the Buff language. Pure-Rust MVP providing four composable primitives — Retry with exponential backoff, Circuit Breaker, Rate Limiter, Timeout — plus a `Pipeline` that chains them. Hand-rolled on `std::time` + `std::thread` (NO `tower`, NO `governor`, NO async runtime) per the T36 spec.

**Status: experimental** (T36 v1.16 frameworks wave 5).

## STRUCTURE

```
buff-resilience/
├── Cargo.toml            # thiserror + insta deps
├── src/
│   ├── lib.rs            # RetryPolicy + CircuitBreaker + RateLimiter + Timeout + Pipeline + BreakerState
│   └── error.rs          # ResilienceError enum
├── examples/
│   ├── retry_basic.rs             # exponential backoff
│   ├── circuit_breaker_basic.rs   # Closed -> Open -> HalfOpen -> Closed
│   ├── rate_limiter_basic.rs      # token bucket: blocking + non-blocking
│   ├── pipeline_composition.rs    # all 4 layers composed + Timeout firing
│   └── resilience/
│       └── pipeline_composition.buff   # Buff-side forward-decl
└── tests/
    └── core.rs           # 27 unit tests + 5 insta snapshots
```

Total: ~1100 LOC (well under the 2500 LOC T36 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Tune the retry backoff formula | `src/lib.rs::RetryPolicy::delay_for_attempt` |
| Adjust the circuit-breaker state machine | `src/lib.rs::CircuitBreaker::{record_success, record_failure, maybe_transition_to_half_open}` |
| Change the rate-limiter refill math | `src/lib.rs::RateLimiter::{refill, consume_one}` |
| Tune the soft-timeout poll cadence | `src/lib.rs::POLL_CADENCE` |
| Add a new error variant | `src/error.rs` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (≤20 fns cap)

### `RetryPolicy` (5 functions, `Copy + Sync`)
- Constructors: `new`, `no_delay`, `default`
- Accessors: `max_attempts`, `initial_delay`, `backoff_factor`, `delay_for_attempt`
- Instance: `execute(handler) -> Result<T, ResilienceError>`

### `CircuitBreaker` (6 functions, stateful `&mut self`)
- Constructor: `new(failure_threshold, reset_timeout)`
- Accessors: `state`, `failure_count`, `failure_threshold`, `reset_timeout`
- Instance: `execute(handler) -> Result<T, ResilienceError>`

### `RateLimiter` (5 functions, stateful `&mut self`)
- Constructor: `new(requests_per_second)`
- Accessors: `requests_per_second`, `available_tokens`
- Instance: `execute(handler)` (blocking), `try_execute(handler)` (non-blocking)

### `Timeout` (3 functions, `Copy + Sync`)
- Constructor: `new(duration)`
- Accessor: `duration`
- Instance: `execute(handler) -> Result<T, ResilienceError>`

### `Pipeline` (7 functions, `Clone + Send`)
- Constructor: `new`
- Builders: `retry(policy)`, `circuit_breaker(cb)`, `rate_limiter(rl)`, `timeout(to)`
- Introspection: `retry_policy`, `timeout_config`, `has_layers`
- Instance: `execute(handler) -> Result<T, ResilienceError>`

### `BreakerState` (enum)
- `Closed`, `Open`, `HalfOpen`

### `ResilienceError` (5 variants)
- `Exhausted { attempts, last_error }`, `CircuitOpen { failure_count, threshold }`, `RateLimited { requests_per_second }`, `Timeout(Duration)`, `Panic`

## CONVENTIONS

- **Pure-Rust only**: NO `tower`, NO `governor`, NO async runtime. The T36 spec allows either wrappers or standalone; we picked standalone because tower's `Service` trait is async-first (would force the Buff surface to be async — violating Buff's "no `await`, async propagates invisibly" rule), and governor pulls `quanta` (which pulls cc-rs on non-x86). Hand-rolled ≈300 LOC on `std::time` + `std::thread`.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in non-test code. Every fallible op returns `Result`.
- **catch_unwind boundary**: `Pipeline::execute` wraps its body in `catch_unwind` per FFI guide R6.
- **Buff §6 / §7 compliance**: NO `_async` suffix (synchronous surface); constructors use `Type::new(...)` only (no `Type.create()` / `Type.build()`).
- **Send bounds**: stateful primitives (`CircuitBreaker`, `RateLimiter`, `Pipeline`) are `Send` but NOT `Sync` (state mutation requires `&mut self`). `RetryPolicy` + `Timeout` are `Copy + Sync`. `Pipeline` wraps its stateful layers in `Arc<Mutex<_>>` so it is both `Send` and `Clone`.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-types` | (T36 follow-up) `prelude_types.rs` will register `PreludeType::ResiliencePipeline` + `PreludeType::CircuitBreaker` + assoc/instance fns. `ty.rs` will gain a `Type::ResiliencePipeline` variant. Deferred to keep this MVP self-contained (matches the buff-template / buff-fuzz precedent of shipping the Rust crate ahead of codegen wiring). |
| `buff-lang-codegen-rust` | (T36 follow-up) `rust_codegen.rs::buff_type_to_syn` will gain the `Type::ResiliencePipeline => "buff_resilience::Pipeline"` arm. `lower_prelude_type_assoc_fn` + `lower_prelude_type_instance_fn` will gain the matching arms. `program_uses_namespace` records `buff-resilience` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |
| `tower` | The canonical Rust resilience crate. The T36 spec mentions it as a backend candidate. We deliberately hand-roll instead — see "Pure-Rust only" above + the rationale block in the root `Cargo.toml` workspace comment. |

## NOTES

- **Soft timeout**: `Timeout::execute` runs the handler on a worker thread and polls `JoinHandle::is_finished()` on a 1ms cadence. If the deadline elapses, the join handle is dropped (worker thread continues in the background until it finishes or panics). This is the only safe way to enforce a timeout in pure Rust without async. A future v1.18+ async variant could use `tokio::select!` for true cancellation.
- **Pipeline layer semantics**: Retry re-invokes the downstream ONLY on `Exhausted` (handler failure). It does NOT retry `CircuitOpen`, `RateLimited`, `Timeout`, or `Panic` — those are fail-fast signals. This matches the cross-language consensus (Polly / Resilience4j / failsafe-go).
- **No bulkhead**: T36 explicitly forbids the bulkhead pattern (v1.22+ work). The Pipeline does NOT include any concurrency-limit layer.
- **No distributed rate limiting**: T36 explicitly forbids Redis-backed rate limiting in the MVP. `RateLimiter` is single-process, in-memory only.
- **`pipeline.debug` output**: `Debug for Pipeline` does NOT dump full inner state (avoiding lock acquisition during formatting). It reports `(BreakerState, failure_count)` for the circuit breaker + boolean flags for the other layers.
