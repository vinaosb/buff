//! XOR problem: train a 2-layer MLP to learn the XOR function.
//!
//! This demonstrates that the autodiff + layers can learn a non-linear
//! function (XOR is not linearly separable — a single linear layer cannot solve it).
//!
//! Usage: cargo run --example xor -p buff-ml

use buff_ml::{Linear, Model, Optimizer, ReLU, SGD, Tape, mse_loss};
use buff_tensor::Tensor;

fn main() {
    // XOR truth table: 4 samples, 2 inputs → 1 output
    let x = Tensor::from_vec(
        vec![
            0.0, 0.0, //
            0.0, 1.0, //
            1.0, 0.0, //
            1.0, 1.0, //
        ],
        vec![4, 2],
    )
    .expect("x tensor");

    let y = Tensor::from_vec(
        vec![
            0.0, // 0 XOR 0 = 0
            1.0, // 0 XOR 1 = 1
            1.0, // 1 XOR 0 = 1
            0.0, // 1 XOR 1 = 0
        ],
        vec![4, 1],
    )
    .expect("y tensor");

    // Model: Linear(2→8) → ReLU → Linear(8→1)
    let mut model = Model::sequential(vec![
        Box::new(Linear::new(2, 8).expect("linear1")),
        Box::new(ReLU::new()),
        Box::new(Linear::new(8, 1).expect("linear2")),
    ]);
    let mut opt = SGD::new(0.5).expect("sgd");

    // Train for 500 epochs
    println!("Training XOR (2-layer MLP, 500 epochs)...");
    for epoch in 0..500 {
        let tape = Tape::new();
        let xv = tape.leaf(x.clone(), false).expect("leaf");
        let pred = model.forward(xv, true).expect("forward");
        let loss = mse_loss(&pred, &y).expect("mse");
        model.backward(&loss).expect("backward");
        opt.step(&mut model).expect("step");

        if epoch % 100 == 0 || epoch == 499 {
            let val = loss.value().as_slice()[0];
            println!("  epoch {epoch:>3}: loss = {val:.6}");
        }
    }

    // Evaluate predictions (eval mode — no dropout randomness)
    println!("\nPredictions (eval mode):");
    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).expect("leaf");
    let pred = model.forward(xv, false).expect("forward");
    let binding = pred.value();
    let pred_vals = binding.as_slice();
    let labels = ["0 XOR 0", "0 XOR 1", "1 XOR 0", "1 XOR 1"];
    let mut all_correct = true;
    for (i, label) in labels.iter().enumerate() {
        let p = pred_vals[i];
        let rounded = if p > 0.5 { 1.0 } else { 0.0 };
        let correct = rounded == y.as_slice()[i];
        let mark = if correct { "✓" } else { "✗" };
        println!("  {label} = {p:.4} → {rounded:.0} {mark}");
        if !correct {
            all_correct = false;
        }
    }
    if all_correct {
        println!("\nAll predictions correct!");
    } else {
        println!("\nSome predictions incorrect — try more epochs or adjust lr.");
    }
}
