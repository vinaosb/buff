# buff-science

Linear algebra, numerical methods, and statistics for Buff. EXPERIMENTAL.

## OVERVIEW

Builds on `buff_tensor::Tensor` (T8) for matrix operations and uses
`nalgebra` for heavy linear algebra (inverse, determinant, solve).
Provides ODE integration (RK4), interpolation, optimization, and
statistical functions.

## STRUCTURE

```
src/
├── lib.rs        # Public API re-exports + crate-level docs.
├── error.rs      # ScienceError enum + ScienceResult alias.
├── linalg.rs     # matmul, transpose, inverse, determinant, solve
├── ode.rs        # rk4, rk4_vec (ODE integrators)
├── interp.rs     # linear interpolation
├── optimize.rs   # gradient descent
└── stats.rs      # mean, variance, stddev, correlation, histogram
tests/
├── unit_tests.rs     # 20+ tests incl proptest
└── snapshots/        # 6+ insta snapshots
```

## PUBLIC API (13 fns, ≤ 40 cap)

| Module | Function | Description |
|---|---|---|
| linalg | `matmul(a, b)` | Matrix multiplication (delegates to buff-tensor) |
| linalg | `transpose(t)` | Matrix transpose (delegates to buff-tensor) |
| linalg | `inverse(m)` | Matrix inverse via nalgebra LU |
| linalg | `determinant(m)` | Matrix determinant via nalgebra |
| linalg | `solve(a, b)` | Solve linear system a*x = b |
| ode | `rk4(f, initial, t_start, t_end, step)` | Scalar RK4 |
| ode | `rk4_vec(f, initial, t_start, t_end, step)` | Vector RK4 |
| interp | `linear(xs, ys, x)` | Linear interpolation |
| optimize | `gradient_descent(f, gradient, initial, lr, steps)` | Gradient descent |
| stats | `mean(data)` | Arithmetic mean |
| stats | `variance(data)` | Population variance |
| stats | `stddev(data)` | Population standard deviation |
| stats | `correlation(x, y)` | Pearson correlation |
| stats | `histogram(data, bins)` | Histogram binning |

## WHERE TO LOOK

| Task | File |
|---|---|
| Change error variants | `src/error.rs` |
| Change linear algebra ops | `src/linalg.rs` |
| Change ODE solvers | `src/ode.rs` |
| Change interpolation | `src/interp.rs` |
| Change optimization | `src/optimize.rs` |
| Change statistics | `src/stats.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule). Enforced via `#![forbid(clippy::unwrap_used)]`
  + `#![forbid(clippy::expect_used)]` + `#![forbid(clippy::panic)]` at
  the crate root.
- **All fallible ops return `Result<_, ScienceError>`** via thiserror.
- **BTreeMap/BTreeSet only** (no HashMap/HashSet — project hard rule).
- **f32 Tensor / f64 numerics** — buff-tensor MVP is f32; nalgebra
  internals use f64 for precision; results are cast back to f32.
- **Pure-Rust deps**: nalgebra (no BLAS linking by default), rayon,
  thiserror. Matches the "no C library, no Docker" hard rule.

## DEPS

- `buff-tensor` (workspace path) — Tensor type.
- `nalgebra` (workspace 0.33) — heavy linear algebra (inverse, det, solve).
- `rayon` (workspace 1.10) — CPU parallelism (available for future use).
- `thiserror` (workspace 1.0) — error derive.
- `insta` (dev-only, workspace 1.40) — snapshot tests.
- `proptest` (dev-only, workspace 1.5) — numerical stability tests.
