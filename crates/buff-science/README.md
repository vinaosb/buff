# buff-science

Linear algebra, numerical methods, and statistics for Buff. EXPERIMENTAL.

Builds on [`buff-tensor`](../buff-tensor/) (T8) for matrix operations and
uses [`nalgebra`](https://docs.rs/nalgebra) for heavy linear algebra
(inverse, determinant, solve). Provides ODE integration (RK4),
interpolation, optimization, and statistical functions.

## Quick start

```rust
use buff_science::linalg;
use buff_tensor::Tensor;

let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
let c = linalg::matmul(&a, &b).unwrap();
assert_eq!(c.as_slice(), &[19.0, 22.0, 43.0, 50.0]);
```

## Modules

| Module | Functions | Description |
|---|---|---|
| `linalg` | matmul, transpose, inverse, determinant, solve | Linear algebra |
| `ode` | rk4, rk4_vec | ODE integration |
| `interp` | linear | Linear interpolation |
| `optimize` | gradient_descent | Gradient descent |
| `stats` | mean, variance, stddev, correlation, histogram | Statistics |

## Scope

- **f32 Tensor / f64 numerics** — Tensor ops use f32 (buff-tensor MVP);
  internal computation uses f64 via nalgebra for precision.
- **CPU-only** — no GPU acceleration.
- **No symbolic math** — deferred to v1.18+.
- **No PDE solvers** — deferred to v1.18+.
- **Reuses buff-dsp FFT** — does NOT reimplement FFT.

## License

MIT OR Apache-2.0, same as the rest of the Buff workspace.
