//! Binary classification: train a model with cross-entropy loss.
//!
//! Demonstrates Softmax + cross-entropy for a 2-class classification problem.
//!
//! Usage: cargo run --example classification -p buff-ml

use buff_ml::{Layer, Linear, Model, Optimizer, ReLU, SGD, Tape, cross_entropy};
use buff_tensor::Tensor;

fn main() {
    // 2-class classification: 8 samples, 4 features → 2 classes
    // Class 0: features centered around [1, 1, 0, 0]
    // Class 1: features centered around [0, 0, 1, 1]
    let x = Tensor::from_vec(
        vec![
            // Class 0
            1.0, 0.8, 0.1, 0.2, //
            0.9, 1.1, 0.2, 0.1, //
            1.1, 0.9, 0.0, 0.3, //
            0.8, 1.0, 0.1, 0.0, //
            // Class 1
            0.1, 0.2, 0.9, 1.0, //
            0.2, 0.1, 1.1, 0.9, //
            0.0, 0.3, 0.8, 1.1, //
            0.1, 0.0, 1.0, 0.9, //
        ],
        vec![8, 4],
    )
    .expect("x tensor");

    // One-hot labels: [class_0, class_1]
    let y = Tensor::from_vec(
        vec![
            1.0, 0.0, //
            1.0, 0.0, //
            1.0, 0.0, //
            1.0, 0.0, //
            0.0, 1.0, //
            0.0, 1.0, //
            0.0, 1.0, //
            0.0, 1.0, //
        ],
        vec![8, 2],
    )
    .expect("y tensor");

    // Model: Linear(4→8) → ReLU → Linear(8→2) (2 output logits for softmax)
    let mut model = Model::sequential(vec![
        Box::new(Linear::new(4, 8).expect("linear1")),
        Box::new(ReLU::new()),
        Box::new(Linear::new(8, 2).expect("linear2")),
    ]);
    let mut opt = SGD::new(0.1).expect("sgd");

    // Train for 200 epochs
    println!("Training binary classifier (cross-entropy, 200 epochs)...");
    for epoch in 0..200 {
        let tape = Tape::new();
        let xv = tape.leaf(x.clone(), false).expect("leaf");
        let logits = model.forward(xv, true).expect("forward");
        let loss = cross_entropy(&logits, &y).expect("cross-entropy");
        model.backward(&loss).expect("backward");
        opt.step(&mut model).expect("step");

        if epoch % 40 == 0 || epoch == 199 {
            let val = loss.value().as_slice()[0];
            println!("  epoch {epoch:>3}: loss = {val:.6}");
        }
    }

    // Evaluate predictions (eval mode)
    println!("\nPredictions (eval mode):");
    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).expect("leaf");
    let logits = model.forward(xv, false).expect("forward");
    let probs = buff_ml::Softmax::new().forward(logits, false).expect("softmax");
    let binding = probs.value();
    let prob_vals = binding.as_slice();
    let mut correct = 0;
    for i in 0..8 {
        let p0 = prob_vals[i * 2];
        let p1 = prob_vals[i * 2 + 1];
        let predicted = if p0 > p1 { 0 } else { 1 };
        let actual = if i < 4 { 0 } else { 1 };
        let mark = if predicted == actual { "✓" } else { "✗" };
        println!("  sample {i}: P(0)={p0:.4} P(1)={p1:.4} → class {predicted} (actual={actual}) {mark}");
        if predicted == actual {
            correct += 1;
        }
    }
    println!("\nAccuracy: {correct}/8 ({:.0}%)", correct as f32 / 8.0 * 100.0);
}
