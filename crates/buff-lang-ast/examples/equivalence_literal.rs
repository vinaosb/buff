//! Behavioral equivalence test: Rust original vs Buff port (literal.buff).
//!
//! Mirrors the `lit_kind_num` function from `selfhost/literal.buff`.
//!
//! Run: `cargo run -p buff-lang-ast --example equivalence_literal`
//! Expected output: `0\n1\n2\n3`

use buff_lang_ast::Literal;

fn lit_kind_num(lit: &Literal) -> u64 {
    match lit {
        Literal::Int(_) => 0,
        Literal::Float(_) => 1,
        Literal::String(_) => 2,
        Literal::Bool(_) => 3,
        _ => 99,
    }
}

fn main() {
    let a = Literal::Int(42);
    let b = Literal::Float(3.14);
    let c = Literal::String("hello".to_string());
    let d = Literal::Bool(true);

    println!("{}", lit_kind_num(&a));
    println!("{}", lit_kind_num(&b));
    println!("{}", lit_kind_num(&c));
    println!("{}", lit_kind_num(&d));
}
