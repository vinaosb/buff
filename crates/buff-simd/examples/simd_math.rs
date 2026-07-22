//! T54 example: SIMD math — lane-wise mul/div + horizontal min/max.
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-simd --example simd_math
//! ```

use buff_simd::Simd;

fn main() {
    let a = Simd::from_array([1.0, 4.0, 9.0, 16.0]);
    let inv = Simd::splat(2.0);

    let halved = a.div(inv);
    let squared_approx = a.mul(a);

    println!("a          = {}", a);
    println!("a / 2      = {}", halved);
    println!("a * a      = {}", squared_approx);
    println!("min(a)     = {}", a.min());
    println!("max(a)     = {}", a.max());
    println!("sum(a*a)   = {}", squared_approx.sum());
}
