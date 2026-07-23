//! T11 QA scenario 2: Hann window has expected shape.
//!
//! Generates `hann(8)` and prints the coefficients. Asserts they
//! match the reference vector `[0, 0.146, 0.5, 0.854, 1.0, 0.854,
//! 0.5, 0.146]` within tolerance.
//!
//! Run: `cargo run --example hann_shape -p buff-dsp`

use buff_dsp::Window;

fn main() {
    let w = Window::hann(8);
    let coeffs = w.as_slice();

    println!("hann(8) = [");
    for (i, c) in coeffs.iter().enumerate() {
        println!("  [{i}] = {c:.6},");
    }
    println!("]");

    let expected = [0.0_f64, 0.1464, 0.5, 0.8536, 1.0, 0.8536, 0.5, 0.1464];
    assert_eq!(
        coeffs.len(),
        expected.len(),
        "hann(8) must yield 8 coefficients"
    );
    let mut max_err = 0.0_f64;
    for (got, want) in coeffs.iter().zip(expected.iter()) {
        let err = (got - want).abs();
        if err > max_err {
            max_err = err;
        }
    }
    println!("max_abs_err = {max_err:.6}");

    let tol = 1e-3;
    assert!(
        max_err < tol,
        "max_abs_err {max_err} exceeds tolerance {tol}"
    );
    println!("PASS: hann(8) coefficients match reference within {tol}.");
}
