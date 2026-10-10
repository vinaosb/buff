//! ITER-56 slice-1: Tensor lowering — generated-source contract.
//!
//! The single-file rustc pipeline does NOT link external crates (the
//! accepted codegen-only linking boundary; T32 Cargo-project wiring is
//! deferred), so the verifiable contract per the v1.4 precedent is the
//! EMITTED SOURCE: the Tensor constructors lower to fully-qualified
//! panic-free `buff_tensor::...` calls and the instance queries lower
//! to the usize→i64-cast method calls the Buff surface registers.

use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

fn gen(src: &str) -> String {
    let toks = tokenize(src, SourceId(0)).expect("lexer");
    let decls = parse(&toks, SourceId(0)).expect("parse");
    generate_rust(&decls).expect("codegen")
}

#[test]
fn tensor_filled_lowers() {
    let rust = gen("func main():\n    let f = Tensor.filled([2, 3], 1.5)\n    print(f.len())\n");
    assert!(
        rust.contains("buff_tensor::Tensor::filled"),
        "filled ctor:\n{rust}"
    );
    assert!(
        rust.contains(" as usize"),
        "Vector<Int> shape must cast to the usize API:\n{rust}"
    );
    assert!(
        rust.contains(" as f32"),
        "Float value must cast to the T8 f32 element type:\n{rust}"
    );
    assert!(
        rust.contains("unwrap_or_default"),
        "fallible constructor needs the Default fallback:\n{rust}"
    );
}

#[test]
fn tensor_zeros_and_queries_lower() {
    let rust = gen(
        "func main():\n    let t = Tensor.zeros([3, 4])\n    let dims = t.shape()\n    print(t.rank())\n    print(t.len())\n",
    );
    assert!(
        rust.contains("buff_tensor::Tensor::zeros"),
        "zeros ctor must lower to the qualified call:\n{rust}"
    );
    assert!(
        rust.contains("unwrap_or_default"),
        "constructors must be panic-free via the Default fallback:\n{rust}"
    );
    assert!(
        rust.contains("as usize"),
        "Buff Vec<i*> shape args must cast to the Tensor usize API:\n{rust}"
    );
    assert!(
        rust.contains(".shape().as_slice()"),
        "shape() must lower to the flat dims view:\n{rust}"
    );
    assert!(
        rust.contains(".rank()"),
        "rank() must lower to the direct method call:\n{rust}"
    );
    assert!(
        rust.contains(".len()"),
        "len() must lower to the direct method call:\n{rust}"
    );
    assert!(
        rust.contains(" as i64"),
        "usize returns must cast to the registered Int surface:\n{rust}"
    );
}

#[test]
fn tensor_ones_and_from_vec_lower() {
    let rust = gen(
        "func main():\n    let a = Tensor.ones([2, 2])\n    let b = Tensor.from_vec([1.0, 2.0, 3.0, 4.0], [2, 2])\n    print(a.len())\n    print(b.len())\n",
    );
    assert!(
        rust.contains("buff_tensor::Tensor::ones"),
        "ones ctor:\n{rust}"
    );
    assert!(
        rust.contains("buff_tensor::Tensor::from_vec"),
        "from_vec ctor:\n{rust}"
    );
    assert!(
        rust.contains("as f32"),
        "data must cast to the T8 f32-only element type:\n{rust}"
    );
}
