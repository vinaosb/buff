# buff-tensor

N-dimensional arrays (rank ≤ 4) for Buff. EXPERIMENTAL.

## OVERVIEW

Pure-Rust N-dimensional array type `Tensor<T>` (alias for `TensorCore<f32>`).
MVP per T8 — CPU-only via rayon (per T6 decision
`.sisyphus/decisions/wgsl-extensibility-v1x.md` §3). GPU dispatch for
elementwise ops is a v1.18+ enhancement; matmul + reduce GPU paths are
~1500 LOC / ~15 days, explicitly deferred.

## STRUCTURE

```
src/
├── lib.rs        # Public API re-exports + crate-level docs.
├── error.rs      # TensorError enum (9 variants) + TensorResult alias.
├── shape.rs      # Shape type + strides + flat_offset + matmul/reduce helpers.
├── tensor.rs     # TensorCore<T> + Tensor (f32 alias). Constructors + accessors
│                 # + reshape + transpose (2-D and N-D permutation).
└── math.rs       # Numeric ops: elementwise (add/sub/mul/div/neg/scale),
                  # matmul (2-D × 2-D, row-parallel via rayon),
                  # reduce (sum/mean/max along axis), scalar reduce (sum/mean/max_all).
tests/
├── unit_tests.rs     # 15+ unit tests for shape/tensor/math (incl. property tests).
└── snapshots/        # 5+ insta snapshot tests (shape, matmul result, reduce, ...).
```

## PUBLIC API (24 fns, ≤ 25 cap per T8 spec)

| Category | Function |
|---|---|
| Constructors | `Tensor::zeros`, `Tensor::ones`, `Tensor::filled`, `Tensor::from_vec` |
| Accessors | `shape`, `rank`, `len`, `is_empty`, `as_slice`, `as_mut_slice`, `into_vec` |
| Index | `get`, `set` |
| Shape ops | `reshape`, `transpose` (2-D), `transpose_perm` (N-D) |
| Elementwise | `add`, `sub`, `mul`, `div`, `neg`, `scale` |
| Matmul | `matmul` |
| Reduce (axis) | `sum_axis`, `mean_axis`, `max_axis` |
| Scalar reduce | `sum_all`, `mean_all`, `max_all` |

## WHERE TO LOOK

| Task | File |
|---|---|
| Change error variants | `src/error.rs` |
| Change shape/stride/indexing logic | `src/shape.rs` |
| Change Tensor constructors / accessors / reshape / transpose | `src/tensor.rs` |
| Change elementwise / matmul / reduce algorithms | `src/math.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule). Enforced via `#![forbid(clippy::unwrap_used)]`
  + `#![forbid(clippy::expect_used)]` + `#![forbid(clippy::panic)]` at the
  crate root.
- **Row-major layout**: the last axis varies fastest. Matches
  `ndarray::Array::from_shape_vec`'s default.
- **Negative axis**: counts from the end (Python-style: `-1` is the
  last axis). Resolved in `Shape::reduce_axis`.
- **MVP rank cap**: 4 (`shape::MVP_RANK_CAP`). Enforced at `Shape::new`.
- **Determinism**: rayon-parallel ops preserve input order via rayon's
  ordered `collect` (matches the contract in
  `buff_lang_runtime::cpu::CpuDispatcher::par_map`).
- **All map/set types are `BTreeMap`/`BTreeSet`** where used (project
  hard rule — this crate currently uses none).
- **Errors via `thiserror::Error` derive** (mirrors `RuntimeError` /
  `BuffError` patterns). 9 variants in `TensorError`.
- **Pure-Rust**: only `rayon` + `thiserror` deps (both workspace-pinned).
  No `ndarray` extern in MVP (recorded in `Cargo.toml
  [workspace.dependencies]` for T13/T15 consumers).

## WHY pure-Rust (not `ndarray` extern)

Per T6 decision §5: "`buff-tensor` CPU strategy: matmul via
`ndarray::Array2::dot` or `nalgebra::Matrix::gemm` extern — BLAS-optimized,
near-peak CPU perf." The MVP deliberately keeps the dependency surface
minimal (pure-Rust + rayon only) for these reasons:

1. **AGENTS.md hard rule**: "Pure-Rust preference" — reqwest uses
   `rustls-tls` (NOT native-tls); zeromq (NOT zmq which links C libzmq);
   no diesel/libpq/S3 SDK. The same conservative stance applies here:
   `ndarray` with `blas` feature pulls `blas-src` + a C/C++ BLAS
   implementation (OpenBLAS / Intel MKL / netlib). That breaks the
   Windows MSVC host build (same family of cc-rs failures that pushed
   hand-rolled lexer/parser per AGENTS.md).
2. **LOC budget**: T8 caps at 2500 LOC. `ndarray`'s type gymnastics
   (`ArrayBase<S, D>` + `Ix2` + `ShapeBuilder`) add ~200 LOC of
   adapter code that the pure-Rust naive triple-loop avoids.
3. **v1.18+ BLAS**: When BLAS perf becomes a blocker, the matmul arm
   in `src/math.rs::Tensor::matmul` swaps from triple-loop to
   `ndarray::Array2::dot` in a one-call-site change (~30 LOC diff).
   The public API surface is unaffected.

## MVP scope caps (T8 spec line 1480-1484)

| Cap | MVP value | v1.18+ target |
|---|---|---|
| dtype | `f32` ONLY | add `f64`, `i64` (T15 buff-ml needs f64) |
| rank | ≤ 4 | ≤ 8 (CUDA tensor cores cap at 8 for ND) |
| GPU dispatch | NONE (CPU via rayon) | elementwise YES (~50 LOC), matmul/reduce DEFER (~1500 LOC, ~15 days) |
| Broadcasting | NONE (shapes must match) | NumPy-style broadcasting |
| Sparse tensors | NONE | CSR / COO / CSC |
| Distributed tensors | NONE | multi-node sharding |
| Autodiff | NONE (T15 buff-ml owns this) | graph-based CPU autodiff |

## INTEGRATION WITH BUFF LANGUAGE (codegen lowering — DEFERRED)

`Tensor` is registered as a **namespace-only** prelude type in
`crates/buff-lang-types/src/prelude_types.rs` (append section). The 4
assoc fns (`Tensor.zeros` / `Tensor.ones` / `Tensor.from_vec` /
`Tensor.filled`) are registered but currently return `Type::Unknown`
at the Buff Type level.

The coordinated `Type::Tensor` variant in
`crates/buff-lang-types/src/ty.rs` + codegen lowering arm in
`crates/buff-lang-codegen-rust/src/rust_codegen.rs` is a follow-up task
outside the T8 shared zone (sibling-task coordination concern — Wave 2
parallel tasks T7/T9/T10/T11/T12/T25 each need their own Type variants
added in a coordinated merge to avoid conflicts in the Type enum).

This forward-declaration lets `buff check` validate `Tensor.zeros([3, 4])`
syntax today (parses + resolves + return-type-checks as Unknown); `buff run`
codegen integration lands when the coordinated sibling task does.

## DEPS

- `rayon` (workspace 1.10) — CPU-parallel elementwise / matmul / reduce.
- `thiserror` (workspace 1.0) — `TensorError` derive.
- `insta` (dev-only, workspace 1.40) — snapshot tests.
- `proptest` (dev-only, workspace 1.5) — numeric stability tests.

Recorded at workspace level for T13 (buff-science) + T15 (buff-ml)
future consumers:
- `buff-tensor = { path = "crates/buff-tensor" }`
- `ndarray = { version = "0.16", default-features = false }` (NOT used
  by MVP; recorded for T13/T15 + future v1.18+ BLAS optimization).
