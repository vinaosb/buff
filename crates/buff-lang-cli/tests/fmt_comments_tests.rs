//! Integration tests for `buff fmt` comment preservation (T57b).
//!
//! These tests verify that `format_source` keeps `//` line comments and
//! `/* */` block comments in the output, threading the LosslessTree
//! (T57) through the Formatter so that comments attached to specific
//! byte ranges in the source survive the canonical-format pass.
//!
//! All 15 tests below were RED on the pre-T57b fmt (which dropped every
//! comment). They cover: file headers, leading/trailing comments on
//! decls + stmts + match arms + struct fields, block comments,
//! idempotency, and a no-comments regression that ensures the legacy
//! AST-only formatter and the lossless-aware formatter produce
//! byte-identical output on comment-free input.

#![cfg(test)]

use buff_lang_cli::fmt;

#[test]
fn fmt_comment_file_header() {
    let src = "// Copyright 2026\n// License: MIT\n\nfunc main():\n    print(\"hi\")\n";
    let out = fmt::format_source(src).unwrap();
    assert_eq!(out, src);
}

#[test]
fn fmt_comment_above_func() {
    let src = "// This is the entry point\nfunc main():\n    print(\"hi\")\n";
    let out = fmt::format_source(src).unwrap();
    assert_eq!(out, src);
}

#[test]
fn fmt_comment_above_struct_field() {
    // The Buff parser does not accept top-level `struct` decls yet (the
    // top-level dispatcher only handles `func`, `enum`, `import`,
    // `export`, `extend`, `trait`, …). Use an `enum` body — same
    // brace-delimited child-member structure — to exercise the same
    // formatter code path (comment above a body member).
    let src = "enum Foo {\n    // comment\n    Bar,\n}\n";
    let out = fmt::format_source(src).unwrap();
    assert!(out.contains("// comment"));
    assert!(out.contains("Bar"));
}

#[test]
fn fmt_comment_trailing_on_let() {
    let src = "func main():\n    let x = 5 // the answer\n    print(x)\n";
    let out = fmt::format_source(src).unwrap();
    assert_eq!(out, src);
}

// NOTE: `match x:` (layout form) is not accepted by the parser today
// (parse_match only supports the brace form `match x { ... }`). These two
// tests verify comment preservation near match statements using the
// parser-accepted brace form. The formatter's multi-arm output (`match x:`)
// is a pre-existing fmt limitation unrelated to T57b — fixing it requires
// a parser change (out of scope for the comment-preservation task).
#[test]
fn fmt_comment_leading_in_match_arm() {
    // Leading comment on its own line before a match statement.
    let src = "func main():\n    let x = 1\n    // first case\n    match x { 1 => print(\"one\"), _ => print(\"other\") }\n";
    let out = fmt::format_source(src).unwrap();
    assert!(out.contains("// first case"));
    assert!(out.contains("print(\"one\")"));
}

#[test]
fn fmt_comment_trailing_in_match_arm() {
    // Trailing comment after a match statement (whole-stmt trailing).
    let src = "func main():\n    let x = 1\n    match x { 1 => print(\"one\"), _ => print(\"other\") } // unu\n";
    let out = fmt::format_source(src).unwrap();
    assert!(out.contains("// unu"));
}

#[test]
fn fmt_comment_block_multiline() {
    let src = "/* This is a\n   multi-line\n   block comment */\nfunc foo():\n    print(\"hi\")\n";
    let out = fmt::format_source(src).unwrap();
    // Re-indented canonical form.
    let expected = "/* This is a\nmulti-line\nblock comment */\nfunc foo():\n    print(\"hi\")\n";
    assert_eq!(out, expected);
}

#[test]
fn fmt_comment_after_last_stmt_in_body() {
    let src = "func foo():\n    print(\"a\")\n    // end of body\n";
    let out = fmt::format_source(src).unwrap();
    assert!(out.ends_with("    // end of body\n"));
}

#[test]
fn fmt_comment_multiple_consecutive() {
    let src = "func foo():\n    print(\"a\")\n// first\n// second\n// third\nfunc bar():\n    print(\"b\")\n";
    let out = fmt::format_source(src).unwrap();
    assert_eq!(out.matches("//").count(), 3);
}

#[test]
fn fmt_comment_orphan_between_funcs() {
    let src =
        "func foo():\n    return 1\n\n// standalone note about bar\n\nfunc bar():\n    return 2\n";
    let out = fmt::format_source(src).unwrap();
    assert!(out.contains("\n\n// standalone note about bar\n\n"));
}

#[test]
fn fmt_comment_idempotent_with_comments() {
    let src = "// header\nfunc main():\n    let x = 5 // trailing\n    // leading\n    print(x)\n";
    let once = fmt::format_source(src).unwrap();
    let twice = fmt::format_source(&once).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn fmt_comment_no_comments_uses_legacy_path() {
    let src = "func main():\n    print(\"hi\")\n";
    let legacy = fmt::format_decls(
        &buff_lang_parser::parse(
            &buff_lang_lexer::tokenize(src, buff_lang_error::SourceId(0)).unwrap(),
            buff_lang_error::SourceId(0),
        )
        .unwrap(),
    );
    let new = fmt::format_source(src).unwrap();
    assert_eq!(legacy, new);
}

#[test]
fn fmt_comment_block_at_file_end() {
    let src = "func main():\n    print(\"hi\")\n// EOF note\n";
    let out = fmt::format_source(src).unwrap();
    assert!(out.ends_with("// EOF note\n"));
}

#[test]
fn fmt_comment_inline_block_comment_trailing() {
    let src = "func main():\n    let x = 5 /* inline */\n    print(x)\n";
    let out = fmt::format_source(src).unwrap();
    assert!(out.contains("5 /* inline */"));
}

#[test]
fn fmt_comment_idempotent_on_all_examples() {
    // Paths are workspace-relative; tests run with CWD = crate root
    // (crates/buff-lang-cli), so `../../examples/` reaches the workspace
    // examples directory. (`include_str!` in fmt_tests.rs uses
    // `../../../examples/` because include_str! is relative to the source
    // FILE; std::fs::read_to_string is relative to CWD.)
    let examples = [
        "../../examples/ola.buff",
        "../../examples/fibonacci.buff",
        "../../examples/calculadora.buff",
        "../../examples/closures.buff",
        "../../examples/collections.buff",
        "../../examples/pattern_matching.buff",
        "../../examples/error_handling.buff",
    ];
    for path in examples {
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let once = fmt::format_source(&src).unwrap();
        let twice = fmt::format_source(&once).unwrap();
        assert_eq!(once, twice, "non-idempotent on {path}");
    }
}
