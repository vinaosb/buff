//! T54 example: SIMD image-pixel channel ops.
//!
//! A single RGBA pixel is exactly 4 f32 lanes — a perfect 1:1 mapping
//! to `Simd<Float, 4>` (one 128-bit register). This example shows
//! brightness adjustment (mul) and channel mixing (add) on a pixel
//! tile. Real image pipelines would tile across many pixels (the
//! `buff-image` T9 integration point).
//!
//! Run via:
//!
//! ```text
//! cargo run -p buff-simd --example simd_image
//! ```

use buff_simd::Simd;

fn main() {
    let rgba = Simd::from_array([0.10, 0.20, 0.30, 1.0]);
    let brightness = Simd::splat(1.5);
    let offset = Simd::from_array([0.0, 0.0, 0.0, 0.0]);

    let brightened = rgba.mul(brightness).add(offset);

    println!("original  RGBA = {}", rgba);
    println!("x1.5      RGBA = {}", brightened);
    println!("max channel    = {}", brightened.max());
    println!("min channel    = {}", brightened.min());
    println!("luminance sum  = {}", brightened.sum());
}
