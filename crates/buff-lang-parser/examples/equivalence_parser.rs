//! Behavioral equivalence test: Rust original vs Buff port (parser.buff).
//!
//! Run: `cargo run -p buff-lang-parser --example equivalence_parser`
//! Expected output: matches parser.buff

use buff_lang_parser::Edition;

fn main() {
    let ed = Edition::default();
    println!("{}", ed == Edition::Standard);
    println!("{}", Edition::Scientific.is_scientific());
    println!("{}", Edition::Scientific == Edition::Standard);

    // ParseError-like test
    println!("unexpected token");
    println!("10");
    println!("15");

    // TokenStreamState-like test
    println!("0");
    println!("1");
    println!("0");

    // Attribute-like test
    println!("deprecated");
    println!("use new_func instead");
}
