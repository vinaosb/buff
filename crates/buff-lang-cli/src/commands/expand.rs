//! `buff expand <FILE>` — show the generated Rust source for a `.buff` file.
//!
//! Like `cargo expand` shows macro expansion, this subcommand runs the
//! front-end of the compiler (lex → parse → codegen) and prints the
//! resulting Rust source to stdout (or writes it to `--output <FILE>`).
//! No `rustc` invocation — just the intermediate Rust.

use std::path::Path;

use anyhow::Result;

use crate::pipeline;

/// Run the expand subcommand.
///
/// # Errors
///
/// Propagates file-read / lex / parse / codegen errors from
/// [`pipeline::compile_to_rust`].
pub fn run(file: &Path, output: Option<&Path>) -> Result<()> {
    let out = pipeline::compile_to_rust(file)?;

    match output {
        Some(path) => {
            std::fs::write(path, &out.rust_source)
                .map_err(|e| anyhow::anyhow!("failed to write `{}`: {e}", path.display()))?;
        }
        None => {
            print!("{}", out.rust_source);
        }
    }

    Ok(())
}
