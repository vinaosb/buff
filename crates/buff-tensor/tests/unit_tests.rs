//! Integration tests for `buff-tensor`. 15+ unit tests + proptest for
//! numeric stability.
//!
//! Per T8 spec line 1474: "15+ unit tests (math-heavy -> proptest
//! required for numeric ops)".

use buff_tensor::{MVP_RANK_CAP, Shape, Tensor, TensorError};

// ===========================================================================
// Shape tests (8)
// ===========================================================================

#[test]
fn shape_basic_construction() {
    let s = Shape::new(vec![2, 3, 4]).unwrap();
    assert_eq!(s.as_slice(), &[2, 3, 4]);
    assert_eq!(s.rank(), 3);
    assert_eq!(s.num_elements(), 24);
}

#[test]
fn shape_strides_correct() {
    let s = Shape::new(vec![2, 3, 4]).unwrap();
    assert_eq!(s.strides(), vec![12, 4, 1]);
}

#[test]
fn shape_indexing_in_bounds() {
    let s = Shape::new(vec![2, 3]).unwrap();
    assert_eq!(s.flat_offset(&[0, 0]).unwrap(), 0);
    assert_eq!(s.flat_offset(&[1, 2]).unwrap(), 5);
}

#[test]
fn shape_indexing_out_of_bounds() {
    let s = Shape::new(vec![2, 3]).unwrap();
    assert!(matches!(
        s.flat_offset(&[5, 0]),
        Err(TensorError::IndexOutOfBounds { .. })
    ));
}

#[test]
fn shape_matmul_dim_check() {
    let a = Shape::new(vec![2, 3]).unwrap();
    let b = Shape::new(vec![3, 4]).unwrap();
    let out = a.matmul_compatible(&b).unwrap();
    assert_eq!(out.as_slice(), &[2, 4]);
}

#[test]
fn shape_matmul_incompatible_inner_dim() {
    let a = Shape::new(vec![2, 3]).unwrap();
    let b = Shape::new(vec![4, 5]).unwrap();
    assert!(matches!(
        a.matmul_compatible(&b),
        Err(TensorError::ShapeMismatch { .. })
    ));
}

#[test]
fn shape_reduce_axis_negative() {
    let s = Shape::new(vec![2, 3, 4]).unwrap();
    let r = s.reduce_axis(-1).unwrap();
    assert_eq!(r.as_slice(), &[2, 3]);
}

#[test]
fn shape_rank_cap_enforced() {
    // Rank 5 should be rejected.
    let err = Shape::new(vec![1, 2, 3, 4, 5]).unwrap_err();
    assert_eq!(err, TensorError::RankTooLarge(5));
    assert_eq!(MVP_RANK_CAP, 4);
}

// ===========================================================================
// Tensor constructor + accessor tests (5)
// ===========================================================================

#[test]
fn tensor_zeros_ones_filled() {
    let z = Tensor::zeros(vec![2, 3]).unwrap();
    assert!(z.as_slice().iter().all(|&v| v == 0.0));
    let o = Tensor::ones(vec![2, 3]).unwrap();
    assert!(o.as_slice().iter().all(|&v| v == 1.0));
    let f = Tensor::filled(vec![2, 3], 7.5).unwrap();
    assert!(f.as_slice().iter().all(|&v| v == 7.5));
}

#[test]
fn tensor_from_vec_basic() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    assert_eq!(t.shape().as_slice(), &[2, 3]);
    assert_eq!(t.as_slice(), &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
}

#[test]
fn tensor_from_vec_data_length_mismatch() {
    let err = Tensor::from_vec(vec![1.0, 2.0], vec![2, 3]).unwrap_err();
    assert_eq!(
        err,
        TensorError::DataLengthMismatch {
            data_len: 2,
            shape_elements: 6,
        }
    );
}

#[test]
fn tensor_get_set() {
    let mut t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
    assert_eq!(t.get(&[1, 1]), Some(&4.0));
    t.set(&[0, 0], 99.0).unwrap();
    assert_eq!(t.get(&[0, 0]), Some(&99.0));
}

#[test]
fn tensor_reshape_preserves_data() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let r = t.reshape(vec![3, 2]).unwrap();
    assert_eq!(r.shape().as_slice(), &[3, 2]);
    assert_eq!(r.as_slice(), &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
}

// ===========================================================================
// Math tests (8) — proptest for numeric ops per spec
// ===========================================================================

#[test]
fn math_matmul_2x2_canonical() {
    // T8 spec acceptance scenario line 1531-1538.
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
    let c = a.matmul(&b).unwrap();
    assert_eq!(c.as_slice(), &[19.0, 22.0, 43.0, 50.0]);
}

#[test]
fn math_matmul_identity() {
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let i = Tensor::from_vec(
        vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        vec![3, 3],
    )
    .unwrap();
    let c = a.matmul(&i).unwrap();
    // a * I = a (within f32 precision).
    for (x, y) in c.as_slice().iter().zip(a.as_slice().iter()) {
        assert!((x - y).abs() < 1e-5f32, "matmul identity failed");
    }
}

#[test]
fn math_reduce_sum_axis_0_and_1() {
    // T8 spec acceptance scenario line 1540-1547.
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    assert_eq!(t.sum_axis(0).unwrap().as_slice(), &[5.0, 7.0, 9.0]);
    assert_eq!(t.sum_axis(1).unwrap().as_slice(), &[6.0, 15.0]);
}

#[test]
fn math_reduce_mean_axis() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    assert_eq!(t.mean_axis(1).unwrap().as_slice(), &[2.0, 5.0]);
}

#[test]
fn math_reduce_max_axis() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    assert_eq!(t.max_axis(1).unwrap().as_slice(), &[3.0, 6.0]);
}

#[test]
fn math_transpose_2d() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let tt = t.transpose().unwrap();
    assert_eq!(tt.shape().as_slice(), &[3, 2]);
    assert_eq!(tt.as_slice(), &[1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
}

#[test]
fn math_elementwise_commutativity() {
    // Sanity: a + b == b + a for elementwise add (within f32 precision).
    let a = Tensor::from_vec(vec![1.5, 2.5, 3.5, 4.5], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![0.1, 0.2, 0.3, 0.4], vec![2, 2]).unwrap();
    let ab = a.add(&b).unwrap();
    let ba = b.add(&a).unwrap();
    for (x, y) in ab.as_slice().iter().zip(ba.as_slice().iter()) {
        assert!((x - y).abs() < 1e-6f32);
    }
}

#[test]
fn math_distributivity() {
    // a * (b + c) == a*b + a*c (within f32 precision).
    let a = Tensor::from_vec(vec![2.0; 4], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
    let c = Tensor::from_vec(vec![0.5, 0.5, 0.5, 0.5], vec![2, 2]).unwrap();
    let lhs = a.mul(&b.add(&c).unwrap()).unwrap();
    let rhs = a.mul(&b).unwrap().add(&a.mul(&c).unwrap()).unwrap();
    for (x, y) in lhs.as_slice().iter().zip(rhs.as_slice().iter()) {
        assert!((x - y).abs() < 1e-5f32, "distributivity failed");
    }
}

// ===========================================================================
// proptest for numeric stability (T8 spec: "math-heavy -> proptest required")
// ===========================================================================

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        cases: 64,
        ..proptest::test_runner::Config::default()
    })]

    /// Elementwise add: a + zeros == a (within f32 precision).
    /// Proptest-verifies the boundary case across random shapes/values.
    #[test]
    fn prop_add_zero_is_identity(
        data in proptest::collection::vec(-100.0f32..100.0f32, 1..64),
        cols in 1usize..=8,
    ) {
        let rows = data.len() / cols;
        if rows == 0 || rows * cols != data.len() {
            return Ok(()); // proptest skip
        }
        let shape = vec![rows, cols];
        let a = Tensor::from_vec(data.clone(), shape.clone()).unwrap();
        let z = Tensor::zeros(shape.clone()).unwrap();
        let sum = a.add(&z).unwrap();
        for (x, y) in sum.as_slice().iter().zip(data.iter()) {
            proptest::prop_assert!((x - y).abs() < 1e-4f32);
        }
    }

    /// Elementwise mul: a * ones == a (within f32 precision).
    #[test]
    fn prop_mul_one_is_identity(
        data in proptest::collection::vec(-100.0f32..100.0f32, 1..64),
        cols in 1usize..=8,
    ) {
        let rows = data.len() / cols;
        if rows == 0 || rows * cols != data.len() {
            return Ok(());
        }
        let shape = vec![rows, cols];
        let a = Tensor::from_vec(data.clone(), shape.clone()).unwrap();
        let ones = Tensor::ones(shape.clone()).unwrap();
        let prod = a.mul(&ones).unwrap();
        for (x, y) in prod.as_slice().iter().zip(data.iter()) {
            proptest::prop_assert!((x - y).abs() < 1e-4f32);
        }
    }

    /// Matmul: a * I == a (where I is the identity matrix of the right size).
    /// Proptest-verifies the identity law across random square matrices.
    #[test]
    fn prop_matmul_identity(
        n in 1usize..=6,
        data in proptest::collection::vec(-10.0f32..10.0f32, 1..36),
    ) {
        let needed = n * n;
        if data.len() < needed {
            return Ok(());
        }
        let a_data: Vec<f32> = data.iter().take(needed).cloned().collect();
        let a = Tensor::from_vec(a_data.clone(), vec![n, n]).unwrap();
        let mut i_data = vec![0.0f32; n * n];
        for i in 0..n {
            i_data[i * n + i] = 1.0;
        }
        let identity = Tensor::from_vec(i_data, vec![n, n]).unwrap();
        let c = a.matmul(&identity).unwrap();
        for (x, y) in c.as_slice().iter().zip(a_data.iter()) {
            proptest::prop_assert!((x - y).abs() < 1e-3f32, "matmul identity failed: {} vs {}", x, y);
        }
    }

    /// Reduce: sum_axis followed by sum_all == sum_all (axis 0).
    /// Proptest-verifies that summing along axis 0 of a 2-D tensor
    /// produces the same total as summing the whole tensor.
    #[test]
    fn prop_reduce_sum_axis_preserves_total(
        rows in 1usize..=5,
        cols in 1usize..=5,
        data in proptest::collection::vec(-10.0f32..10.0f32, 1..25),
    ) {
        let needed = rows * cols;
        if data.len() < needed {
            return Ok(());
        }
        let data: Vec<f32> = data.iter().take(needed).cloned().collect();
        let t = Tensor::from_vec(data.clone(), vec![rows, cols]).unwrap();
        let axis_sum = t.sum_axis(0).unwrap();
        let axis_total: f32 = axis_sum.as_slice().iter().sum();
        let total: f32 = data.iter().sum();
        proptest::prop_assert!(
            (axis_total - total).abs() < 1e-3f32,
            "axis total {} != full total {}",
            axis_total,
            total
        );
    }

    /// Reshape: data preserved across reshape round-trip.
    #[test]
    fn prop_reshape_roundtrip(
        n in 1usize..=24,
        data in proptest::collection::vec(-10.0f32..10.0f32, 1..24),
    ) {
        if data.len() < n || n == 0 {
            return Ok(());
        }
        let data: Vec<f32> = data.iter().take(n).cloned().collect();
        let cols = if n % 2 == 0 { 2 } else { 1 };
        let rows = n / cols;
        if rows * cols != n || rows == 0 {
            return Ok(());
        }
        let t = Tensor::from_vec(data.clone(), vec![n]).unwrap();
        let reshaped = t.reshape(vec![rows, cols]).unwrap();
        let back = reshaped.reshape(vec![n]).unwrap();
        for (x, y) in back.as_slice().iter().zip(data.iter()) {
            proptest::prop_assert_eq!(x, y);
        }
    }
}
