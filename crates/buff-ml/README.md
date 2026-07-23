# buff-ml

Neural network layers + reverse-mode autodiff for Buff, built on `buff-tensor`.

## Quick start

```rust
use buff_ml::{Tape, mse_loss, Model, Optimizer, SGD, Linear};
use buff_tensor::Tensor;

// Train y = 2x + 3 on synthetic data.
let x = Tensor::from_vec(vec![-1.0, -0.5, 0.0, 0.5, 1.0], vec![5, 1]).unwrap();
let y = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0], vec![5, 1]).unwrap();

let mut model = Model::sequential(vec![Box::new(Linear::new(1, 1).unwrap())]);
let mut opt = SGD::new(0.1).unwrap();

for _ in 0..200 {
    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).unwrap();
    let pred = model.forward(xv, true).unwrap();
    let loss = mse_loss(&pred, &y).unwrap();
    model.backward(&loss).unwrap();
    opt.step(&mut model).unwrap();
}
// model now approximates y = 2x + 3.
```

## Run examples

```bash
cargo run --example linear_regression -p buff-ml
cargo run --example xor -p buff-ml
cargo run --example classification -p buff-ml
```

## Features

- **Reverse-mode autodiff** (micrograd pattern — define-by-run tape)
- **Layers**: `Linear`, `ReLU`, `Sigmoid`, `Softmax`, `Dropout`
- **Losses**: `mse_loss`, `cross_entropy`
- **Optimizers**: `SGD`, `Adam`
- **Serialization**: JSON save/load for models
- **f32 ONLY** for MVP

## Constraints (MVP / v1.18+)

- f32 only (no f64)
- CPU-only, single-threaded training
- No CNNs/RNNs/Transformers
- No distributed training
- JSON serialization only (no ONNX/safetensors)

## License

MIT OR Apache-2.0
