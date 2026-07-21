//! T53 parser tests — `comptime { ... }` block + `comptime T: Type` parameter.

use buff_lang_ast::{stmt::Stmt, Param};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse, parse_statement, stream::TokenStream};

fn parse_stmt(src: &str) -> Stmt {
    let sid = SourceId(7);
    let toks = tokenize(src, sid).expect("lexer should succeed");
    let mut s = TokenStream::new(&toks, sid);
    parse_statement(&mut s).expect("parse_statement should succeed")
}

fn parse_first_decl_param(src: &str) -> Vec<Param> {
    let sid = SourceId(7);
    let toks = tokenize(src, sid).expect("lexer should succeed");
    let decls = parse(&toks, sid).expect("parse should succeed");
    match decls.first().expect("at least one decl") {
        buff_lang_ast::Decl::FuncDecl(f) => f.params.clone(),
        _ => panic!("expected FuncDecl"),
    }
}

#[test]
fn parses_comptime_block_with_braces() {
    let stmt = parse_stmt("comptime { 42 }");
    assert!(matches!(stmt, Stmt::ComptimeBlock { .. }));
}

#[test]
fn parses_comptime_block_with_layout() {
    // Layout form: `comptime:` then newline + indented body.
    let stmt = parse_stmt("comptime:\n    let x = 1");
    assert!(matches!(stmt, Stmt::ComptimeBlock { .. }));
}

#[test]
fn parses_empty_comptime_block() {
    let stmt = parse_stmt("comptime { }");
    assert!(matches!(stmt, Stmt::ComptimeBlock { .. }));
}

#[test]
fn parses_comptime_param_in_function() {
    let params = parse_first_decl_param("func id(comptime T: Type, x: T) -> T:\n    return x");
    assert_eq!(params.len(), 2);
    assert!(params[0].is_comptime, "first param must be comptime");
    assert!(!params[1].is_comptime, "second param must NOT be comptime");
    assert_eq!(params[0].name.name, "T");
    assert_eq!(params[1].name.name, "x");
}

#[test]
fn parses_multiple_comptime_params() {
    let params = parse_first_decl_param(
        "func pair(comptime A: Type, comptime B: Type, a: A, b: B):\n    return a",
    );
    assert_eq!(params.len(), 4);
    assert!(params[0].is_comptime);
    assert!(params[1].is_comptime);
    assert!(!params[2].is_comptime);
    assert!(!params[3].is_comptime);
}

#[test]
fn ordinary_identifier_not_treated_as_comptime_in_expression_position() {
    // A bare `comptime` identifier in expression position (e.g. a
    // variable named `comptime`) is NOT routed to the comptime parser;
    // it falls through to the assignment-or-expr-stmt path. The T53
    // spec says `comptime` is NOT a reserved keyword.
    let stmt = parse_stmt("comptime");
    assert!(
        !matches!(stmt, Stmt::ComptimeBlock { .. }),
        "bare comptime identifier should not be a comptime block"
    );
}

#[test]
fn parses_comptime_block_followed_by_normal_stmt() {
    let sid = SourceId(7);
    let src = "func f():\n    comptime:\n        42\n    print(\"after\")";
    let toks = tokenize(src, sid).expect("lexer");
    let decls = parse(&toks, sid).expect("parse");
    assert_eq!(decls.len(), 1);
    if let buff_lang_ast::Decl::FuncDecl(f) = &decls[0] {
        assert_eq!(f.body.stmts.len(), 2);
        assert!(matches!(f.body.stmts[0], Stmt::ComptimeBlock { .. }));
    } else {
        panic!("expected FuncDecl");
    }
}
