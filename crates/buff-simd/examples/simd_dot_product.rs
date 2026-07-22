//! T54 example: SIMD dot product vs scalar loop.
//!
//! Demonstrates the canonical SIMD reduction (mul + horizontal sum)
//! and verifies it matches the scalar computation.
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-simd --example simd_dot_product
//! ```

use buff_simd::{dot, Simd};

fn main() {
    let a = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    let b = Simd::from_array([5.0, 6.0, 7.0, 8.0]);

    let simd_result = dot(a, b);
    let scalar_result = 1.0 * 5.0 + 2.0 * 6.0 + 3.0 * 7.0 + 4.0 * 8.0;

    println!("a = {}", a);
    println!("b = {}", b);
    println!("SIMD   dot(a, b) = {}", simd_result);
    println!("scalar dot(a, b) = {}", scalar_result);
    println!("match: {}", (simd_result - scalar_result).abs() < 1e-5);
}
