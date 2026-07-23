# buff-science

Linear algebra + numerical methods + statistics for Buff. EXPERIMENTAL.

## OVERVIEW

Built on top of [`buff-tensor`](../buff-tensor/) (T8) for matrix storage +
[`nalgebra`](https://docs.rs/nalgebra) (extern target) for the heavy lifting
on `inverse` / `determinant` / `solve`. Reuses [`buff-dsp`](../buff-dsp/)
FFT where spectral analysis is exposed (does NOT reimplement per T13 must-not).

MVP per T13 — pure-Rust, CPU-only, f32 only. Symbolic math + PDE solvers
are explicitly deferred to v1.18+ per the T13 must-not list.

## STRUCTURE

```
src/
├── lib.rs        # Public API re-exports + crate-level docs.
├── error.rs      # ScienceError enum + ScienceResult alias.
├── linalg.rs     # matmul / transpose / inverse / determinant / solve (5 fns).
│                 # Wraps Tensor + delegates heavy lifting to nalgebra.
├── ode.rs        # rk4 — classic 4th-order Runge-Kutta integrator (1 fn).
├── interp.rs     # linear — piecewise-linear interpolation (1 fn).
├── optimize.rs   # gradient_descent — numeric-gradient descent (1 fn).
└── stats.rs      # mean / variance / stddev / correlation / histogram (5 fns).

tests/
├── unit_tests.rs     # 15+ tests incl. proptest for numeric stability.
└── snapshots/        # 5+ insta snapshot tests.
```

## PUBLIC API (13 fns, ≤ 40 cap per T13)

| Module | Function | Backend |
|---|---|---|
| `linalg` | `matmul(a, b)` | `buff_tensor::Tensor::matmul` |
| `linalg` | `transpose(t)` | `buff_tensor::Tensor::transpose` |
| `linalg` | `inverse(m)` | `nalgebra::DMatrix::try_inverse` |
| `linalg` | `determinant(m)` | `nalgebra::DMatrix::determinant` |
| `linalg` | `solve(a, b)` | `nalgebra::DMatrix::solve` (LU) |
| `ode` | `rk4(f, y0, t0, t1, step)` | hand-rolled RK4 |
| `interp` | `linear(xs, ys, x)` | hand-rolled piecewise linear |
| `optimize` | `gradient_descent(f, init, lr, steps)` | numeric-gradient descent |
| `stats` | `mean(data)` | `f32` summation |
| `stats` | `variance(data)` | population variance (N divisor) |
| `stats` | `stddev(data)` | `sqrt(variance)` |
| `stats` | `correlation(x, y)` | Pearson coefficient |
| `stats` | `histogram(data, bins)` | equal-width bin counts |

## WHERE TO LOOK

| Task | File |
|---|---|
| Change error variants | `src/error.rs` |
| Add a new linalg op | `src/linalg.rs` (extend the 5-fn count carefully) |
| Change the ODE integrator (e.g. add rk2 / dopri5) | `src/ode.rs` |
| Change the interpolator (e.g. add cubic / nearest) | `src/interp.rs` |
| Change the optimizer (e.g. add adam / momentum) | `src/optimize.rs` |
| Add a new stat | `src/stats.rs` |
| Audit nalgebra extern surface | All `nalgebra::` calls live in `src/linalg.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule). Enforced via `#![forbid(clippy::unwrap_used)]`
  + `#![forbid(clippy::expect_used)]` + `#![forbid(clippy::panic)]` at the
  crate root.
- **dtype `f32` ONLY** (mirrors buff-tensor's MVP cap). `f64` deferred to
  v1.18+ (T15 buff-ml will need it for autodiff precision).
- **Row-major layout**: matrices are `buff_tensor::Tensor` of shape `[rows, cols]`.
  nalgebra's `DMatrix` is also row-major, so `DMatrix::from_row_slice` is a
  zero-copy wrap.
- **Errors via `thiserror::Error` derive** (mirrors `TensorError` /
  `RuntimeError` patterns). `ScienceError` has 9 variants.
- **BTreeMap/BTreeSet only** where used (project hard rule — currently
  used for `histogram` bin assignment is plain `Vec<usize>`, no maps).
- **Pure-Rust extern surface**: only `nalgebra` (no BLAS, no LAPACK). The
  `nalgebra` crate is pure-Rust by default (would need the `blas` cargo
  feature to opt into C BLAS, which is NOT enabled at the workspace level
  per AGENTS.md's "Pure-Rust preference" rule).

## SCOPE CAPS (T13 spec)

| Cap | MVP value | v1.18+ target |
|---|---|---|
| dtype | `f32` ONLY | add `f64` (T15 buff-ml needs it) |
| Symbolic math | NONE | sympy-equivalent expression engine |
| PDE solvers | NONE | finite-difference / finite-element |
| FFT | reuse buff-dsp (T11) — NOT reimplemented | — |
| Autodiff | NONE (T15 buff-ml owns this) | tape-based CPU autodiff |
| Sparse matrices | NONE | CSR / COO via nalgebra's `CsMatrix` |
| Higher-order integrators | rk4 only | rk2 / dopri5 / implicit methods |

## INTEGRATION WITH BUFF LANGUAGE (codegen lowering — DEFERRED)

The 13 public fns are NOT yet registered as prelude types in
`crates/buff-lang-types/src/prelude_types.rs`. The coordinated `Type::Science`
variant + codegen lowering arm in `crates/buff-lang-codegen-rust` is a
follow-up task outside the T13 shared zone (sibling-task coordination
concern — the Type enum grows one variant per Wave sibling). The current
crate is callable directly from Rust tests / examples; `buff run`
codegen integration lands with the coordinated sibling task.

## WHY nalgebra (not hand-rolled)

Per T13 spec: "implement matrix inverse / determinant / solve on top of
buff-tensor". The MVP delegates to nalgebra rather than hand-rolling
Gauss-Jordan / LU decomposition because:

1. **Numerical stability**: nalgebra uses partial pivoting in LU and a
   numerically stable determinant formula; hand-rolled naive impls are
   brittle on near-singular matrices.
2. **LOC budget**: T13 caps at 4000 LOC. A hand-rolled LU + inverse +
   solve + determinant suite is ~600-900 LOC vs ~50 LOC of nalgebra
   delegation. The budget is better spent on numerical methods (ODE,
   optimization) which the MVP hand-rolls.
3. **Pure-Rust guarantee**: nalgebra is pure-Rust by default (no BLAS
   cargo feature on), matching the workspace "no C library" hard rule.
4. **v1.18+ BLAS upgrade path**: when f64 / BLAS perf becomes a
   blocker, the swap is a one-line cargo feature change. The public
   `buff_science::linalg::*` API surface is unaffected.

## DEPS

- `buff-tensor` (workspace path) — primary matrix storage type.
- `thiserror` (workspace 1.0) — `ScienceError` derive.
- `rayon` (workspace 1.10) — parallel numeric ops (currently minimal use).
- `nalgebra` (workspace 0.33) — `DMatrix` for inverse/determinant/solve.
- `insta` (dev-only, workspace 1.40) — snapshot tests.
- `proptest` (dev-only, workspace 1.5) — numeric stability tests.

## WHY reuse buff-dsp FFT (NOT reimplement)

Per T13 must-not: "Do NOT reimplement FFT (reuse T11 buff-dsp)". The MVP
does not directly use FFT in the public API surface (the T13 spec lists
"reuse buff-dsp FFT where applicable" — the only applicable use is in
spectral cross-correlation, which is deferred to v1.18+). The current
13-fn surface has no FFT path; the dep is NOT pulled by buff-science to
keep the dep tree minimal. A future v1.18+ spectral-analysis module will
add `buff-dsp` to the deps + expose `signal.correlate` / `signal.fft` fns.
