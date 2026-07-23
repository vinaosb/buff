//! Snapshot tests for `buff-tensor` (5+ per T8 spec line 1475).
//!
//! Uses `insta::assert_snapshot` to pin byte-exact output for:
//! - shape display + strides computation
//! - tensor Display output
//! - matmul canonical 2x2 result
//! - reduce axis-0 + axis-1 result
//! - transpose 2-D result
//! - 3-D reduce (compound case)

use buff_tensor::{Shape, Tensor};

#[test]
fn snapshot_shape_strides_3d() {
    let s = Shape::new(vec![2, 3, 4]).unwrap();
    let strs = s.strides();
    insta::assert_snapshot!(format!(
        "shape={:?} rank={} num_elements={} strides={:?}",
        s.as_slice(),
        s.rank(),
        s.num_elements(),
        strs,
    ));
}

#[test]
fn snapshot_shape_strides_4d() {
    let s = Shape::new(vec![2, 3, 4, 5]).unwrap();
    let strs = s.strides();
    insta::assert_snapshot!(format!(
        "shape={:?} rank={} num_elements={} strides={:?}",
        s.as_slice(),
        s.rank(),
        s.num_elements(),
        strs,
    ));
}

#[test]
fn snapshot_matmul_canonical_2x2() {
    // T8 spec line 1531-1538 acceptance scenario.
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
    let c = a.matmul(&b).unwrap();
    insta::assert_snapshot!(format!(
        "a shape={:?} b shape={:?} c shape={:?} c data={:?}",
        a.shape().as_slice(),
        b.shape().as_slice(),
        c.shape().as_slice(),
        c.as_slice(),
    ));
}

#[test]
fn snapshot_matmul_non_square() {
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let b = Tensor::from_vec(vec![7.0, 8.0, 9.0, 10.0, 11.0, 12.0], vec![3, 2]).unwrap();
    let c = a.matmul(&b).unwrap();
    insta::assert_snapshot!(format!(
        "c shape={:?} c data={:?}",
        c.shape().as_slice(),
        c.as_slice(),
    ));
}

#[test]
fn snapshot_reduce_axis_0_and_1() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let s0 = t.sum_axis(0).unwrap();
    let s1 = t.sum_axis(1).unwrap();
    let m0 = t.mean_axis(0).unwrap();
    let m1 = t.mean_axis(1).unwrap();
    let x1 = t.max_axis(1).unwrap();
    insta::assert_snapshot!(format!(
        "sum(0)={:?} sum(1)={:?} mean(0)={:?} mean(1)={:?} max(1)={:?}",
        s0.as_slice(),
        s1.as_slice(),
        m0.as_slice(),
        m1.as_slice(),
        x1.as_slice(),
    ));
}

#[test]
fn snapshot_transpose_2d() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let tt = t.transpose().unwrap();
    insta::assert_snapshot!(format!(
        "input shape={:?} input data={:?}\noutput shape={:?} output data={:?}",
        t.shape().as_slice(),
        t.as_slice(),
        tt.shape().as_slice(),
        tt.as_slice(),
    ));
}

#[test]
fn snapshot_3d_reduce_axis_1() {
    // Shape [2,2,2]: 8 elements. Sum along axis 1 -> shape [2,2].
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], vec![2, 2, 2]).unwrap();
    let r = t.sum_axis(1).unwrap();
    insta::assert_snapshot!(format!(
        "input shape={:?} reduce(1) shape={:?} reduce(1) data={:?}",
        t.shape().as_slice(),
        r.shape().as_slice(),
        r.as_slice(),
    ));
}

#[test]
fn snapshot_elementwise_chain() {
    // (a + b) * c with three small tensors.
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![10.0, 20.0, 30.0, 40.0], vec![2, 2]).unwrap();
    let c = Tensor::from_vec(vec![0.1, 0.1, 0.1, 0.1], vec![2, 2]).unwrap();
    let result = a.add(&b).unwrap().mul(&c).unwrap();
    insta::assert_snapshot!(format!(
        "result shape={:?} result data={:?}",
        result.shape().as_slice(),
        result.as_slice(),
    ));
}
