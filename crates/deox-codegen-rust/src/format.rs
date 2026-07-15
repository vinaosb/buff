//! Pretty-printing for `syn::File` via `prettyplease`.
//!
//! Every Rust codegen path in this crate terminates by calling [`format`]
//! to turn the constructed `syn::File` into valid Rust source text. We never
//! hand-format Rust — `prettyplease` produces output equivalent to a
//! `rustfmt` pass.

use syn::File;

/// Format a [`syn::File`] into Rust source code using `prettyplease`.
///
/// The returned string is parseable Rust (passes `rustfmt --check`
/// modulo prettyplease's own style decisions, which closely mirror the
/// default `rustfmt` style).
pub fn format(file: &File) -> String {
    prettyplease::unparse(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_formats_to_empty_string() {
        let file = File {
            shebang: None,
            attrs: Vec::new(),
            items: Vec::new(),
        };
        let out = format(&file);
        assert!(out.is_empty(), "empty file should format to empty string");
    }

    #[test]
    fn output_is_valid_rust() {
        // Build a tiny file via parse_quote and confirm the output re-parses.
        let file: File = syn::parse_quote! {
            fn foo() -> i64 {
                42
            }
        };
        let out = format(&file);
        assert!(out.contains("fn foo()"));
        // Re-parse: output must be syntactically valid Rust.
        syn::parse_str::<File>(&out).expect("prettyplease output should re-parse");
    }
}
