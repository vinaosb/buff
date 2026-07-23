# buff-science

Linear algebra + numerical methods + statistics for the Buff language. EXPERIMENTAL.

MVP per T13 (`.sisyphus/plans/buff-v1x-frameworks.md#L1893`):
- **linalg** (5 fns): `matmul`, `transpose`, `inverse`, `determinant`, `solve`.
- **numerical** (3 fns): `ode::rk4`, `interp::linear`, `optimize::gradient_descent`.
- **stats** (5 fns): `mean`, `variance`, `stddev`, `correlation`, `histogram`.

Built on top of [`buff-tensor`](../buff-tensor/) (T8) for matrix storage. Reuses
[`buff-dsp`](../buff-dsp/) FFT where spectral analysis is needed (does NOT reimplement).

## Quick start

```rust
use buff_science::{linalg, stats};
use buff_tensor::Tensor;

// 3x3 matrix inverse roundtrip: m * m^-1 ≈ I.
let m = Tensor::from_vec(
    vec![4.0, 7.0, 3.0,
          2.0, 6.0, 5.0,
          1.0, 1.0, 1.0],
    vec![3, 3],
).unwrap();
let m_inv = linalg::inverse(&m).unwrap();
let product = linalg::matmul(&m, &m_inv).unwrap();
// Diagonal entries are ≈ 1.0, off-diagonal ≈ 0.0.

// Statistics.
let data = vec![1.0_f32, 2.0, 3.0, 4.0, 5.0];
assert_eq!(stats::mean(&data), 3.0);
assert_eq!(stats::variance(&data), 2.0);
```

## Public API (≤ 13 fns, ≤ 40 cap per T13)

| Module | Function | Notes |
|---|---|---|
| `linalg` | `matmul(a, b)` | Wraps `Tensor::matmul`. |
| `linalg` | `transpose(t)` | Wraps `Tensor::transpose`. |
| `linalg` | `inverse(m)` | `nalgebra::DMatrix::try_inverse`. |
| `linalg` | `determinant(m)` | `nalgebra::DMatrix::determinant`. |
| `linalg` | `solve(a, b)` | `nalgebra::DMatrix::solve` (LU). |
| `ode` | `rk4(f, y0, t0, t1, step)` | Classic 4th-order Runge-Kutta. |
| `interp` | `linear(xs, ys, x)` | Piecewise-linear interpolation. |
| `optimize` | `gradient_descent(f, init, lr, steps)` | Numeric-gradient descent. |
| `stats` | `mean(data)` | Arithmetic mean. |
| `stats` | `variance(data)` | Population variance (N divisor). |
| `stats` | `stddev(data)` | `sqrt(variance)`. |
| `stats` | `correlation(x, y)` | Pearson correlation coefficient. |
| `stats` | `histogram(data, bins)` | Count per equal-width bin. |

Total: 13 public fns (well within the 40 cap).

## Conventions

- **dtype**: `f32` ONLY (mirrors buff-tensor; f64 deferred to v1.18+).
- **Errors**: `Result<_, ScienceError>`. No `unwrap`/`expect`/`panic!` in
  non-test code (project hard rule; enforced via `#![forbid(clippy::*)]`
  attributes at the crate root).
- **Layout**: row-major matrices via `buff_tensor::Tensor` (shape `[rows, cols]`).
  nalgebra also defaults to row-major for `DMatrix`, so the wrap is zero-copy.
- **BTreeMap/BTreeSet only** (no HashMap/HashSet — project hard rule).

## Scope caps (T13 spec)

| Cap | MVP value | v1.18+ target |
|---|---|---|
| dtype | `f32` ONLY | add `f64` (T15 buff-ml needs it) |
| Symbolic math | NONE | sympy-equivalent expression engine |
| PDE solvers | NONE | finite-difference / finite-element |
| FFT | reuse buff-dsp (T11) | — |
| Autodiff | NONE (T15 buff-ml owns this) | tape-based CPU autodiff |

## Examples

Three `.buff` examples at `examples/science/`:
- `hello.buff` — linalg inverse roundtrip on a 3×3 matrix.
- `rk4.buff` — integrate exponential ODE y' = y from t=0 to t=1.
- `stats.buff` — mean/variance/stddev on [1,2,3,4,5].

## License

MIT OR Apache-2.0 (same as the rest of the workspace).
