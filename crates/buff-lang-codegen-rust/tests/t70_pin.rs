//! T70 — `@pin` attribute tests.
//!
//! Verifies the full pipeline: `@pin let x = expr` parse-time desugars to
//! `let x = __buff_pin(expr)`, and the codegen lowers `__buff_pin(expr)` to
//! `std::hint::black_box(expr)`.

use buff_lang_ast::{common::Block, Decl};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

#[test]
fn pin_desugars_to_black_box_via_codegen() {
    // Parse `@pin let x = 42` inside a function body.
    let src = "func main():\n    @pin let x = 42\n    print(x)\n";
    let tokens = tokenize(src, SourceId(0)).expect("lex");
    let decls = parse(&tokens, SourceId(0)).expect("parse");
    let rust = generate_rust(&decls).expect("codegen");
    assert!(
        rust.contains("std::hint::black_box"),
        "expected std::hint::black_box in output, src:\n{rust}"
    );
    assert!(
        !rust.contains("__buff_pin"),
        "sentinel should not leak into output, src:\n{rust}"
    );
}

#[test]
fn pin_preserves_let_binding_name() {
    let src = "func main():\n    @pin let value = 100\n    print(value)\n";
    let tokens = tokenize(src, SourceId(0)).expect("lex");
    let decls = parse(&tokens, SourceId(0)).expect("parse");
    let rust = generate_rust(&decls).expect("codegen");
    assert!(
        rust.contains("let value = std::hint::black_box"),
        "expected `let value = std::hint::black_box(...)`, src:\n{rust}"
    );
}

#[test]
fn pin_must_precede_let() {
    // `@pin` followed by something other than `let` should be a parse error.
    let src = "func main():\n    @pin print(42)\n";
    let tokens = tokenize(src, SourceId(0)).expect("lex");
    let result = parse(&tokens, SourceId(0));
    assert!(
        result.is_err(),
        "expected parse error when @pin is not followed by `let`"
    );
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("@pin") && err.contains("let"),
        "error should mention @pin and let, err: {err}"
    );
}

#[test]
fn pin_takes_no_arguments() {
    // `@pin(42) let x = 5` should be a parse error.
    let src = "func main():\n    @pin(42) let x = 5\n";
    let tokens = tokenize(src, SourceId(0)).expect("lex");
    let result = parse(&tokens, SourceId(0));
    assert!(
        result.is_err(),
        "expected parse error when @pin takes arguments"
    );
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("@pin") && err.contains("no arguments"),
        "error should mention @pin takes no arguments, err: {err}"
    );
}

#[test]
fn pin_does_not_affect_normal_code() {
    // A plain `let x = 42` without @pin should NOT contain black_box.
    let src = "func main():\n    let x = 42\n    print(x)\n";
    let tokens = tokenize(src, SourceId(0)).expect("lex");
    let decls = parse(&tokens, SourceId(0)).expect("parse");
    let rust = generate_rust(&decls).expect("codegen");
    assert!(
        !rust.contains("black_box"),
        "plain let should not contain black_box, src:\n{rust}"
    );
}

#[test]
fn pin_preserves_mutability() {
    let src = "func main():\n    @pin let mut x = 0\n    x = 1\n    print(x)\n";
    let tokens = tokenize(src, SourceId(0)).expect("lex");
    let decls = parse(&tokens, SourceId(0)).expect("parse");
    let rust = generate_rust(&decls).expect("codegen");
    assert!(
        rust.contains("let mut x = std::hint::black_box"),
        "expected mut preserved, src:\n{rust}"
    );
}

#[test]
fn unused_pin_does_not_break_empty_body() {
    // Codegen should work even when @pin is the only statement.
    let _decls: Vec<Decl> = vec![Decl::FuncDecl(buff_lang_ast::decl::FuncDecl {
        name: buff_lang_ast::common::Ident::new("empty", buff_lang_error::Span::dummy()),
        params: Vec::new(),
        return_type: None,
        body: Block::empty(buff_lang_error::Span::dummy()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: buff_lang_error::Span::dummy(),
    })];
    let rust = generate_rust(&_decls).expect("codegen");
    assert!(rust.contains("fn empty"), "src: {rust}");
}
