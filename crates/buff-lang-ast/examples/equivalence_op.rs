//! Behavioral equivalence test: Rust original vs Buff port (op.buff).
//!
//! Mirrors the `binary_op_num` / `unary_op_num` functions from
//! `selfhost/op.buff` using the ACTUAL Rust enum types.
//!
//! Run: `cargo run -p buff-lang-ast --example equivalence_op`
//! Expected output: `1\n25\n1\n3`

use buff_lang_ast::{BinaryOp, UnaryOp};

fn binary_op_num(op: BinaryOp) -> u64 {
    match op {
        BinaryOp::Add => 1,
        BinaryOp::Sub => 2,
        BinaryOp::Mul => 3,
        BinaryOp::Div => 4,
        BinaryOp::Mod => 5,
        BinaryOp::Eq => 6,
        BinaryOp::Neq => 7,
        BinaryOp::Lt => 8,
        BinaryOp::Gt => 9,
        BinaryOp::Lte => 10,
        BinaryOp::Gte => 11,
        BinaryOp::And => 12,
        BinaryOp::Or => 13,
        BinaryOp::BitAnd => 14,
        BinaryOp::BitOr => 15,
        BinaryOp::BitXor => 16,
        BinaryOp::Shl => 17,
        BinaryOp::Shr => 18,
        BinaryOp::Assign => 19,
        BinaryOp::AddAssign => 20,
        BinaryOp::SubAssign => 21,
        BinaryOp::MulAssign => 22,
        BinaryOp::DivAssign => 23,
        BinaryOp::ModAssign => 24,
        BinaryOp::NullCoalesce => 25,
    }
}

fn unary_op_num(op: UnaryOp) -> u64 {
    match op {
        UnaryOp::Neg => 1,
        UnaryOp::Not => 2,
        UnaryOp::BitNot => 3,
    }
}

fn main() {
    println!("{}", binary_op_num(BinaryOp::Add));
    println!("{}", binary_op_num(BinaryOp::NullCoalesce));
    println!("{}", unary_op_num(UnaryOp::Neg));
    println!("{}", unary_op_num(UnaryOp::BitNot));
}
