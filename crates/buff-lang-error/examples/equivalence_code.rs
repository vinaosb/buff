//! Behavioral equivalence test: Rust original vs Buff port (code.buff).
//!
//! Mirrors the `code_str` function from `selfhost/code.buff` using the
//! ACTUAL Rust ErrorCode enum.
//!
//! Run: `cargo run -p buff-lang-error --example equivalence_code`
//! Expected output: `E1001\nE1003\nE1101\nE1109`

use buff_lang_error::ErrorCode;

fn main() {
    println!("{}", ErrorCode::UnexpectedChar.code_str());
    println!("{}", ErrorCode::InvalidNumber.code_str());
    println!("{}", ErrorCode::ExpectedToken.code_str());
    println!("{}", ErrorCode::ExternGenericsUnsupported.code_str());
}
