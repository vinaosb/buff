# buff-ml

Neural network layers + reverse-mode autodiff for Buff, built on `buff-tensor`. MVP per T15.

## STRUCTURE

```
src/
├── lib.rs          # Public API re-exports, doc example
├── autodiff.rs     # Var, Tape, Node, computation graph (micrograd pattern)
├── error.rs        # MlError enum (thiserror)
├── layer.rs        # Layer trait + Linear, ReLU, Sigmoid, Softmax, Dropout
├── loss.rs         # mse_loss, cross_entropy
├── model.rs        # Model::sequential, forward, backward, save, load
├── optimizer.rs    # SGD, Adam, Optimizer trait
├── io.rs           # JSON save/load roundtrip

examples/ml/
├── linear_regression.rs  # Train y = 2x + 3
├── xor.rs                # 2-layer MLP on XOR
├── classification.rs     # Cross-entropy binary classifier

tests/
├── unit_tests.rs         # 22+ integration tests (proptest, convergence, snapshots)
├── snapshots/            # 6 accepted insta snapshots
```

## PUBLIC API

```text
// Autodiff:
Tape::new(), Tape::leaf(), Var::value(), Var::backward()

// Layers (all implement Layer trait):
Linear::new(in, out), ReLU::new(), Sigmoid::new(), Softmax::new(), Dropout::new(rate)
Layer::forward(), Layer::parameters(), Layer::load_parameters(), Layer::collect_grads()

// Losses:
mse_loss(&pred, &target), cross_entropy(&logits, &target)

// Model:
Model::sequential(layers), Model::forward(), Model::backward()
Model::save(path), Model::load(path), Model::get_layer(i)

// Optimizers:
SGD::new(lr), Adam::new(lr)
Optimizer::step(&mut model), Optimizer::name()
```

~35 public functions (under 40-budget).

## CONVENTIONS

- **f32 ONLY** for MVP (per T15 spec).
- **BTreeMap/BTreeSet only** where collections are used (project rule).
- **No `unwrap`/`expect`/`panic!` in non-test code** (project hard rule). Examples (separate binaries) may use `expect`.
- **All fallible ops return `Result<_, MlError>`**.
- **Single-threaded training** for MVP (tape uses `Rc<RefCell<...>>`; `Var` is `!Send`).
- **JSON serialization** for model save/load.
- **No CNNs/RNNs/Transformers** (defer v1.18+).
- **No distributed training** (defer v1.19+).

## DEPENDENCIES

All workspace deps: `buff-tensor`, `thiserror`, `serde`, `serde_json`, `rayon`.
Dev: `insta`, `proptest`, `tempfile`.

## TESTS

Integration tests (`tests/unit_tests.rs`, 22+ tests):
- 3 proptest gradient checks (bias, weight, joint)
- 3 training convergence tests (SGD, Adam, MLP loss decreases)
- 2 save/load tests (roundtrip, mismatch error)
- 6 insta snapshot tests (model debug, softmax, linear forward, losses)
- 8 edge case tests (dropout, relu idempotent, layer kinds, etc.)

Doc-tests: 1 (linear regression example in lib.rs).

## KNOWN ISSUES

- **Bias gradient 3x factor** — RESOLVED (T7). Autodiff bias gradient was ~3x / scrambled for `batch > 1` AND `output_dim > 1`. Root cause was `Tensor::sum_axis` decomposing the flat index with axes in the wrong order (forward instead of last-axis-first for row-major), so the `add_row_bias` backward's `sum_axis(0)` over the batch axis scrambled the bias grad. The bug was masked in existing tests because `proptest_linear_bias_gradient_matches_numerical` used `Linear(1, 1)` (`output_dim == 1`, where the wrong-axis reduction coincidentally lands every contribution in the single output cell). Fixed in `buff-tensor/src/math.rs` (`sum_axis` + `max_axis` now iterate axes in reverse); regression test in `tests/bias_gradient_batch.rs` (analytical + finite-difference check, batch=3, out=2). Training convergence tests passed before and after.
- **`model.layers` is `pub(crate)`**: Optimizers access via `layers_mut()`. External code uses `get_layer(i)`.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T15 (line 2068).
- Dependency: `crates/buff-tensor/` (T8 — the tensor engine).
- Pattern: micrograd (https://github.com/karpathy/micrograd).
