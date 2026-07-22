//! T54 benchmark: SIMD dot product vs scalar loop.
//!
//! Run via:
//!
//! ```text
//! cargo bench -p buff-simd --bench dot_product
//! ```
//!
//! Per T54 acceptance: "Benchmark shows >=3x speedup on dot product vs
//! scalar loop." This harness compares three dot-product
//! implementations over a 4096-element `Vec<f32>` (1024 SIMD tiles of
//! 4 lanes each):
//!
//! 1. `scalar_dot` — plain `for` loop with `+=` accumulator.
//! 2. `simd_dot_explicit` — manual `Simd::from_array` + `mul` + `sum`
//!    tile loop (the explicit-SIMD path Buff's `Simd<T,N>` surfaces).
//! 3. `simd_dot_wide_direct` — the underlying `wide::f32x4` reduction
//!    (upper bound: what the explicit-SIMD path lowers to after
//!    inlining).

use buff_simd::Simd;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use wide::f32x4;

const N: usize = 4096;

fn make_data() -> Vec<f32> {
    (0..N).map(|i| (i as f32) * 0.5).collect()
}

fn scalar_dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = 0.0_f32;
    for (x, y) in a.iter().zip(b) {
        acc += x * y;
    }
    acc
}

fn simd_dot_explicit(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = 0.0_f32;
    let mut i = 0;
    while i + 4 <= a.len() {
        let va = Simd::from_array([a[i], a[i + 1], a[i + 2], a[i + 3]]);
        let vb = Simd::from_array([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        acc += va.mul(vb).sum();
        i += 4;
    }
    acc
}

fn simd_dot_wide_direct(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = 0.0_f32;
    let mut i = 0;
    while i + 4 <= a.len() {
        let va = f32x4::new([a[i], a[i + 1], a[i + 2], a[i + 3]]);
        let vb = f32x4::new([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let arr = (va * vb).to_array();
        acc += arr[0] + arr[1] + arr[2] + arr[3];
        i += 4;
    }
    acc
}

fn bench_dot_product(c: &mut Criterion) {
    let a = make_data();
    let b = make_data();

    let mut g = c.benchmark_group("dot_product");
    g.bench_function("scalar_loop", |bencher| {
        bencher.iter(|| black_box(scalar_dot(black_box(&a), black_box(&b))));
    });
    g.bench_function("simd_explicit", |bencher| {
        bencher.iter(|| black_box(simd_dot_explicit(black_box(&a), black_box(&b))));
    });
    g.bench_function("simd_wide_direct", |bencher| {
        bencher.iter(|| black_box(simd_dot_wide_direct(black_box(&a), black_box(&b))));
    });
    g.finish();
}

criterion_group!(benches, bench_dot_product);
criterion_main!(benches);
