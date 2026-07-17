//! T75 integration tests — parser support for `extend TYPE { fn ...; ... }`
//! extension-method blocks.
//!
//! Coverage:
//!
//! - `extend String { fn shout(self) -> String { ... } }` parses to
//!   `Decl::ExtendBlock { target: String, methods: [shout] }`.
//! - Multiple methods per block.
//! - Empty body `extend T { }` is a parse error.
//! - Missing target / missing `{` / missing `}` error paths.
//! - Existing top-level decls (func / enum / import / export) STILL parse
//!   unchanged when an extend block precedes or follows them.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser --test extensions
//! ```

use buff_lang_ast::{Decl, ExtendBlock, TypeRef};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse `src` as a top-level program.
fn parse_program(src: &str) -> Vec<Decl> {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect("parser must succeed")
}

/// Tokenize + parse `src` as a top-level program, expecting FAILURE.
fn parse_program_err(src: &str) -> buff_lang_error::ParseError {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect_err("parser must fail")
}

// ---------------------------------------------------------------------------
// 1. Basic single-method extend block parses to the right AST.
// ---------------------------------------------------------------------------

#[test]
fn extensions_parse_single_method_string() {
    let decls = parse_program(
        "extend String {\n    func shout(self) -> String {\n        return \"x\"\n    }\n}",
    );
    assert_eq!(decls.len(), 1, "expected one decl");
    match &decls[0] {
        Decl::ExtendBlock(ExtendBlock {
            target, methods, ..
        }) => {
            // Target is the named type String.
            assert!(
                matches!(
                    target,
                    TypeRef::Named { name, .. } if name.name == "String"
                ),
                "expected target = String, got {target:?}"
            );
            // One method named `shout`.
            assert_eq!(
                methods.len(),
                1,
                "expected one method, got {}",
                methods.len()
            );
            assert_eq!(methods[0].name.name, "shout");
            assert_eq!(methods[0].params.len(), 1);
            assert_eq!(methods[0].params[0].name.name, "self");
            assert!(
                matches!(
                    &methods[0].return_type,
                    Some(TypeRef::Named { name, .. }) if name.name == "String"
                ),
                "expected return type String"
            );
        }
        other => panic!("expected ExtendBlock, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2. Multiple methods per extend block.
// ---------------------------------------------------------------------------

#[test]
fn extensions_parse_multiple_methods() {
    let decls = parse_program(
        "extend String {\n    func shout(self) -> String {\n        return \"x\"\n    }\n    func whisper(self) -> String {\n        return \"y\"\n    }\n}",
    );
    assert_eq!(decls.len(), 1);
    if let Decl::ExtendBlock(ExtendBlock { methods, .. }) = &decls[0] {
        assert_eq!(methods.len(), 2);
        let names: Vec<&str> = methods.iter().map(|m| m.name.name.as_str()).collect();
        assert_eq!(names, vec!["shout", "whisper"]);
    } else {
        panic!("expected ExtendBlock");
    }
}

// ---------------------------------------------------------------------------
// 3. Extend on a primitive (Int) parses.
// ---------------------------------------------------------------------------

#[test]
fn extensions_parse_primitive_target_int() {
    let decls =
        parse_program("extend Int {\n    func squared(self) -> Int {\n        return 0\n    }\n}");
    assert_eq!(decls.len(), 1);
    if let Decl::ExtendBlock(ExtendBlock {
        target, methods, ..
    }) = &decls[0]
    {
        assert!(matches!(
            target,
            TypeRef::Named { name, .. } if name.name == "Int"
        ));
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.name, "squared");
    } else {
        panic!("expected ExtendBlock");
    }
}

// ---------------------------------------------------------------------------
// 4. Error paths.
// ---------------------------------------------------------------------------

#[test]
fn extensions_parse_empty_body_errors() {
    let err = parse_program_err("extend String {\n}");
    assert!(
        err.diagnostic.message.contains("at least one method"),
        "error should explain the empty-block rule: {}",
        err.diagnostic.message
    );
}

#[test]
fn extensions_parse_missing_target_errors() {
    let err = parse_program_err("extend {\n    func shout(self) { }\n}");
    // The error should be about expecting a type name (parse_type_ref fails).
    assert!(
        err.diagnostic.message.contains("type") || err.diagnostic.message.contains("identifier"),
        "error should mention type name: {}",
        err.diagnostic.message
    );
}

#[test]
fn extensions_parse_missing_close_brace_errors() {
    let err = parse_program_err("extend String {\n    func shout(self) { }");
    assert!(
        err.diagnostic.message.contains("}")
            || err.diagnostic.message.contains("brace")
            || err.diagnostic.message.contains("input"),
        "error should mention missing `}}`: {}",
        err.diagnostic.message
    );
}

// ---------------------------------------------------------------------------
// 5. Existing decls still parse unchanged around an extend block.
// ---------------------------------------------------------------------------

#[test]
fn extensions_parse_mixed_with_other_decls() {
    let src = "extend String {\n    func shout(self) -> String {\n        return \"x\"\n    }\n}\nfunc helper():\n    return 1\n";
    let decls = parse_program(src);
    // Two decls: an ExtendBlock then a FuncDecl.
    assert_eq!(decls.len(), 2);
    assert!(matches!(decls[0], Decl::ExtendBlock(_)));
    assert!(matches!(decls[1], Decl::FuncDecl(_)));
}

#[test]
fn extensions_parse_extend_after_func() {
    let src = "func helper():\n    return 1\nextend String {\n    func shout(self) -> String {\n        return \"x\"\n    }\n}\n";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 2);
    assert!(matches!(decls[0], Decl::FuncDecl(_)));
    assert!(matches!(decls[1], Decl::ExtendBlock(_)));
}

// ---------------------------------------------------------------------------
// 6. Display impl round-trip (smoke).
// ---------------------------------------------------------------------------

#[test]
fn extensions_extend_block_display() {
    let decls = parse_program(
        "extend String {\n    func shout(self) -> String {\n        return \"x\"\n    }\n}",
    );
    let s = decls[0].to_string();
    assert!(
        s.contains("ExtendBlock"),
        "Display should mention ExtendBlock: {s}"
    );
}
