//! BUG-10 codegen follow-up — untyped (inferred) params must be ANNOTATED
//! in the emitted Rust, not lowered to the Rust-illegal `_` placeholder
//! (rustc E0121: the placeholder `_` is not allowed within types on item
//! signatures).
//!
//! The parser (BUG-10 check-side fix) accepts `func add(a, b):` by parsing
//! unannotated params as `TypeRef::Named { name: "_" }`. Codegen must
//! resolve that placeholder to a concrete Rust type — Buff's default Int,
//! matching the established unknown-args-fall-back-to-i64 convention in
//! `buff_type_to_syn` — so the transpiled program compiles.

use buff_lang_codegen_rust::generate_rust;

/// Full front-end pipeline (lex → parse → codegen), mirroring how the
/// CLI's `buff run` consumes a source file. Same helper shape as
/// `codegen_tests.rs::codegen_from_source`.
fn codegen_from_source(src: &str) -> String {
    use buff_lang_error::SourceId;
    let tokens = buff_lang_lexer::tokenize(src, SourceId(0)).expect("lexer should succeed");
    let decls = buff_lang_parser::parse(&tokens, SourceId(0)).expect("parser should succeed");
    generate_rust(&decls).expect("codegen should succeed")
}

#[test]
fn untyped_params_are_annotated_in_emitted_rust() {
    // Untyped params (`func add(a, b)`) used at a typed call site — the
    // shape BUG-10's parser fix enabled and `buff check` already accepts.
    let src = "\
func add(a, b):
    a + b

func main():
    let n: Int = add(2, 3)
    print(n)
";
    let rust = codegen_from_source(src);

    // The `_` placeholder must never reach the emitted signature — it is
    // not a legal Rust item-signature type (rustc E0121).
    assert!(
        !rust.contains("a: _"),
        "param `a` must not lower to the Rust-illegal `_` placeholder\n--- src ---\n{rust}"
    );
    assert!(
        !rust.contains("b: _"),
        "param `b` must not lower to the Rust-illegal `_` placeholder\n--- src ---\n{rust}"
    );
    // Buff's default Int annotation (same convention as unknown generic
    // args falling back to i64 in buff_type_to_syn).
    assert!(
        rust.contains("a: i64"),
        "param `a` must be annotated i64 (Buff default Int), got:\n{rust}"
    );
    assert!(
        rust.contains("b: i64"),
        "param `b` must be annotated i64 (Buff default Int), got:\n{rust}"
    );
    // The emitted program must re-parse as valid Rust.
    syn::parse_str::<syn::File>(&rust).expect("untyped-param codegen must re-parse");
}
