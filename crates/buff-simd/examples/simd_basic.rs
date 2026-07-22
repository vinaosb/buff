//! T54 example: SIMD basics — splat + add + sum.
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-simd --example simd_basic
//! ```

use buff_simd::Simd;

fn main() {
    let a = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    let b = Simd::splat(10.0);
    let sum_lane = a.add(b);
    println!("a        = {}", a);
    println!("b (splat)= {}", b);
    println!("a + b    = {}", sum_lane);
    println!("sum(a+b) = {}", sum_lane.sum());
}
