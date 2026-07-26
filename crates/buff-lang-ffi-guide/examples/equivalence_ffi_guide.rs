//! Behavioral equivalence test: Rust original vs Buff port (ffi_guide.buff).
//!
//! The Rust crate is docs-only. This binary outputs the 6 FFI safety rules
//! in the same format as `selfhost/ffi_guide.buff`.
//!
//! Run: `cargo run -p buff-lang-ffi-guide --example equivalence_ffi_guide`
//! Expected output: the 6 FFI rules

fn main() {
    println!("R1: No raw pointers");
    println!("R2: Rust owns memory");
    println!("R3: Map Result errors");
    println!("R4: Send + 'static only");
    println!("R5: No lifetimes");
    println!("R6: Catch panics");
}
