//! Linear regression: train y = 2x + 3 with a single Linear layer.
//!
//! Usage: cargo run --example linear_regression -p buff-ml

use buff_ml::{Linear, Model, Optimizer, SGD, Tape, mse_loss};
use buff_tensor::Tensor;

fn main() {
    // Synthetic data: y = 2x + 3
    let x_data = vec![-2.0, -1.0, 0.0, 1.0, 2.0];
    let y_data: Vec<f32> = x_data.iter().map(|&v| 2.0 * v + 3.0).collect();
    let x = Tensor::from_vec(x_data, vec![5, 1]).expect("x tensor");
    let y = Tensor::from_vec(y_data, vec![5, 1]).expect("y tensor");

    // Model: single Linear(1→1)
    let mut model = Model::sequential(vec![Box::new(Linear::new(1, 1).expect("linear"))]);
    let mut opt = SGD::new(0.05).expect("sgd");

    // Train for 300 epochs
    for epoch in 0..300 {
        let tape = Tape::new();
        let xv = tape.leaf(x.clone(), false).expect("leaf");
        let pred = model.forward(xv, true).expect("forward");
        let loss = mse_loss(&pred, &y).expect("mse");
        model.backward(&loss).expect("backward");
        opt.step(&mut model).expect("step");

        if epoch % 50 == 0 {
            let val = loss.value().as_slice()[0];
            println!("epoch {epoch:>3}: loss = {val:.6}");
        }
    }

    // Check learned parameters
    let params = model.get_layer(0).expect("layer 0").parameters();
    let w = params[0].as_slice()[0];
    let b = params[1].as_slice()[0];
    println!("\nLearned: y = {w:.4}x + {b:.4}  (expected: y = 2.0000x + 3.0000)");

    // Save model
    let path = "target/linear_regression_model.json";
    model.save(path).expect("save");
    println!("Model saved to {path}");
}
