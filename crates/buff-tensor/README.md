# buff-tensor

N-dimensional arrays (rank ≤ 4) for Buff. EXPERIMENTAL.

MVP per T8 (`.sisyphus/plans/buff-v1x-frameworks.md#L1464`):
- **dtype**: `f32` ONLY. f64 / i64 deferred to v1.18+.
- **rank cap**: 4. Higher ranks deferred to v1.18+.
- **CPU-only via rayon**. Per T6 decision
  (`.sisyphus/decisions/wgsl-extensibility-v1x.md` §3), elementwise
  GPU dispatch is feasible as a v1.18+ enhancement (~50 LOC);
  matmul + reduce GPU paths are ~1500 LOC / ~15 days, explicitly deferred.
- **No autodiff** (T15 buff-ml), no broadcasting, no sparse tensors.

## Quick start

```rust
use buff_tensor::Tensor;

let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
let c = a.matmul(&b).unwrap();
assert_eq!(c.as_slice(), &[19.0, 22.0, 43.0, 50.0]);
```

## Public API (≤ 25 public fns)

| Category | Function | Notes |
|---|---|---|
| Constructors | `Tensor::zeros(shape)` | Zero-filled |
| Constructors | `Tensor::ones(shape)` | One-filled |
| Constructors | `Tensor::filled(shape, value)` | Constant-filled |
| Constructors | `Tensor::from_vec(data, shape)` | Wrap flat Vec + shape |
| Accessors | `shape()`, `rank()`, `len()`, `is_empty()` | All infallible |
| Accessors | `as_slice()`, `as_mut_slice()`, `into_vec()` | View/extract |
| Index | `get(index) -> Option<&f32>` | Bounds-checked read |
| Index | `set(index, value) -> Result<()>` | Bounds-checked write |
| Shape ops | `reshape(shape)` | Same element count |
| Shape ops | `transpose()` | 2-D swap axes |
| Shape ops | `transpose_perm(perm)` | N-D axis permutation |
| Elementwise | `add/sub/mul/div(&other)` | Same-shape (no broadcasting) |
| Elementwise | `neg()`, `scale(scalar)` | Unary ops |
| Matmul | `matmul(&other)` | 2-D × 2-D |
| Reduce | `sum_axis(axis)` | Sum along axis |
| Reduce | `mean_axis(axis)` | Mean along axis |
| Reduce | `max_axis(axis)` | Max along axis |
| Scalar reduce | `sum_all()`, `mean_all()`, `max_all()` | Whole-tensor |

Total: 24 public fns.

## Conventions

- **Layout**: row-major (C-order). Last axis varies fastest.
- **Negative axis**: counts from the end (Python-style).
- **Errors**: `Result<_, TensorError>`. No `unwrap`/`expect`/`panic!` in
  non-test code (project hard rule; enforced via `#![forbid(...)]`).
- **Determinism**: rayon-parallel ops preserve input order via rayon's
  ordered `collect` (matches the contract in
  `buff_lang_runtime::cpu::CpuDispatcher::par_map`).

## Integration with Buff language

Tensor is registered as a **namespace-only** prelude type in
`crates/buff-lang-types/src/prelude_types.rs` with assoc fns:
- `Tensor.zeros(shape)`
- `Tensor.ones(shape)`
- `Tensor.from_vec(data, shape)`
- `Tensor.filled(shape, value)`

These return `Type::Unknown` at the Buff Type level for MVP. The
coordinated `Type::Tensor` variant + codegen lowering arm is a follow-up
task outside the T8 shared zone (sibling-task coordination concern). This
forward-declaration lets `buff check` validate `Tensor.zeros([3, 4])`
syntax today; `buff run` codegen integration lands with the coordinated
sibling task.

## Examples

Three `.buff` examples at `examples/tensor/`:
- `hello.buff` — create + shape query
- `matmul.buff` — 2-D matrix multiplication
- `reduce.buff` — axis-aware reduction

## License

MIT OR Apache-2.0 (same as the rest of the workspace).
