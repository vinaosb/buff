//! ITER-56: the `Default` fallback backs the panic-free
//! `unwrap_or_default()` Tensor-constructor lowering.

#[test]
fn tensor_default_is_rank1_scalar_zero() {
    let t = buff_tensor::Tensor::default();
    assert_eq!(t.rank(), 1, "default is the rank-1 fallback");
    assert_eq!(t.len(), 1, "default holds exactly one element");
    assert!(!t.is_empty(), "the scalar-zero default is non-empty");
}
