//! Integration tests for `buff_lang_ast::lossless` (T57).
//!
//! All test names contain the word `lossless` so the acceptance command
//! `cargo test -p buff-lang-ast lossless` matches every test in this file
//! (plus the inline `#[cfg(test)] mod tests` in `src/lossless.rs`).
//!
//! Coverage map:
//!
//! - **Byte-exact roundtrip** (the core deliverable): `lossless_roundtrip_*`
//!   — 11 fixtures including comments, blank lines, mixed indent, UTF-8,
//!   string interpolation, regex literals, char literals, all the decl
//!   kinds the scanner handles.
//! - **Comment preservation** specifically: `lossless_comment_*`.
//! - **Edge cases**: empty file, whitespace-only, comments-only.
//! - **Idempotency of `to_source`**: parse → to_source → parse → identical.
//! - **QA semantic-equivalence**: addressed via byte-exact roundtrip (no
//!   parser dev-dep cycle — see `lossless_qa_semantic_equivalence_*`).
//! - **Incremental reparse**: `lossless_reparse_*`.
//! - **Piece metadata**: `lossless_piece_*` (byte ranges, kinds, lookup).

#![deny(unsafe_code)]

use buff_lang_ast::lossless::{parse_lossless, Piece, TriviaKind};

// ---------------------------------------------------------------------------
// Helper: assert byte-exact roundtrip via BOTH to_source() and
// pieces_to_source() (the latter proves pieces alone reconstruct).
// ---------------------------------------------------------------------------
fn assert_byte_exact(src: &str) {
    let tree = parse_lossless(src);
    let via_src = tree.to_source();
    let via_pieces = tree.pieces_to_source();
    assert_eq!(
        via_src,
        src,
        "to_source() must roundtrip byte-exact (src len = {})",
        src.len()
    );
    assert_eq!(
        via_pieces, src,
        "pieces_to_source() must roundtrip byte-exact — pieces alone must reconstruct src"
    );
    // Integrity invariant: pieces cover the full source range with no
    // gaps or overlaps.
    let mut expected = 0usize;
    for p in tree.pieces() {
        assert_eq!(
            p.start(),
            expected,
            "pieces must be contiguous (gap/overlap at byte {})",
            expected
        );
        expected = p.end();
    }
    assert_eq!(expected, src.len(), "pieces must cover full source");
}

// ---------------------------------------------------------------------------
// 1. Byte-exact roundtrip fixtures (11 fixtures covering the cases the
//    task requires: comments, blank lines, mixed indent, string literals,
//    all decl kinds the scanner handles, UTF-8, regex, char).
// ---------------------------------------------------------------------------

#[test]
fn lossless_roundtrip_empty_file() {
    assert_byte_exact("");
}

#[test]
fn lossless_roundtrip_simple_func_no_comment() {
    let src = "func ola()\n    print(\"Olá, Buff!\")\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_func_with_line_comment() {
    let src = "// top comment\nfunc ola()\n    // body comment\n    print(\"hi\")\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_func_with_block_comment() {
    let src = "/* a /* nested */ block */\nfunc f()\n    print(/* inline */ 1)\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_blank_lines_preserved() {
    let src = "func a()\n    1\n\n\nfunc b()\n    2\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_mixed_indent_preserved() {
    // 2-space + tab + 4-space — Buff fmt would normalize, but lossless MUST
    // preserve the original bytes.
    let src = "func f()\n  a\n\tb\n    c\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_crlf_lf_cr_all_preserved() {
    let src = "func a()\r\n    1\r\nfunc b()\n    2\nfunc c()\r    3\r";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_utf8_in_comments_and_strings() {
    let src = "// Olá, Buff — áéíóú çãñ\nfunc saudacao()\n    print(\"Olá, Joana 🚀\")\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_string_with_escape_and_dquote() {
    // The `//` inside the string must NOT be a comment, and the inner `"`
    // must be escaped so it doesn't close the string.
    let src = "let s = \"a // not a comment \\\" still inside\"\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_string_interpolation_with_braces() {
    // `{x}` interpolation — the `}` inside the string must NOT close the
    // string. Also nested braces: `"a {b + {c}}"`.
    let src = "let s = \"x {y + 1} z {w + {v}}\"\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_char_literal() {
    let src = "let c = 'a'\nlet nl = '\\n'\nlet rocket = '🚀'\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_triple_quoted_raw_string() {
    let src = "let s = \"\"\"multi\nline // not a comment\nstill string \"\"\"\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_raw_string_r_prefix() {
    let src = "let s = r\"a \\n {b} literal\"\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_regex_literal_preserved() {
    // Regex literals are scanned as opaque tokens (the lossless scanner
    // does NOT do regex-disambiguation — it doesn't need to for
    // roundtrip). The bytes reconstruct exactly.
    let src = "let pat = /\\d+/g\nlet div = a / b\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_struct_enum_decl() {
    let src = "struct Point\n    x: Int\n    y: Int\n\nenum Color\n    Red\n    Green\n    Blue\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_real_buff_program() {
    // Synthesised real Buff source: comments, multiple decl kinds,
    // interpolation, mixed trivia.
    let src = "// Fibonacci — recursive\nfunc fib(n: Int) -> Int\n    if n < 2\n        return n\n    return fib(n - 1) + fib(n - 2)\n\n// Entry point\nfunc main()\n    print(\"fib(10) = {fib(10)}\")\n";
    assert_byte_exact(src);
}

#[test]
fn lossless_roundtrip_whitespace_only_file() {
    assert_byte_exact("   \t  \n  \t\t\n");
}

#[test]
fn lossless_roundtrip_comments_only_file() {
    let src = "// first\n// second\n/* block\n   multi\n   line */\n// last\n";
    assert_byte_exact(src);
}

// ---------------------------------------------------------------------------
// 2. Comment preservation specifically.
// ---------------------------------------------------------------------------

#[test]
fn lossless_comment_line_recognized_as_trivia() {
    let tree = parse_lossless("// hi\n");
    let comments: Vec<_> = tree.comments().collect();
    assert_eq!(comments.len(), 1);
    let c = comments[0];
    assert_eq!(c.text(), "// hi");
    assert_eq!(c.comment_kind(), Some(TriviaKind::LineComment));
    assert_eq!(c.start(), 0);
    assert_eq!(c.end(), 5); // "// hi" is 5 bytes
}

#[test]
fn lossless_comment_block_recognized_as_trivia() {
    let tree = parse_lossless("/* hi */\n");
    let comments: Vec<_> = tree.comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "/* hi */");
    assert_eq!(comments[0].comment_kind(), Some(TriviaKind::BlockComment));
}

#[test]
fn lossless_comment_nested_block_preserved() {
    let src = "/* a /* b */ c */";
    let tree = parse_lossless(src);
    assert_eq!(tree.comment_count(), 1);
    assert_eq!(tree.comments().next().unwrap().text(), src);
}

#[test]
fn lossless_comment_inside_string_is_not_a_comment() {
    // `"a // b"` is a STRING, not a string + comment. The scanner must
    // recognize the string boundary.
    let tree = parse_lossless("\"a // not a comment\"");
    assert_eq!(tree.comment_count(), 0);
    assert_eq!(tree.token_count(), 1);
    assert_eq!(
        tree.tokens().next().unwrap().text(),
        "\"a // not a comment\""
    );
}

#[test]
fn lossless_comment_count_correct_on_complex_source() {
    let src = "// 1\nfunc a()\n    // 2\n    1\n/* 3 */ // 4\n";
    let tree = parse_lossless(src);
    assert_eq!(tree.comment_count(), 4);
}

// ---------------------------------------------------------------------------
// 3. Idempotency of to_source — parse → to_source → parse → identical.
// ---------------------------------------------------------------------------

#[test]
fn lossless_to_source_idempotent_on_comments() {
    let src = "// top\nfunc f()\n    // inner\n    print(\"x\")\n";
    let t1 = parse_lossless(src);
    let s1 = t1.to_source();
    let t2 = parse_lossless(&s1);
    let s2 = t2.to_source();
    assert_eq!(s1, s2);
    assert_eq!(
        t1, t2,
        "LosslessTree must be idempotent under parse/to_source"
    );
}

#[test]
fn lossless_pieces_idempotent_under_reparse() {
    let src = "func fib(n) -> Int\n    if n < 2\n        return n\n";
    let t1 = parse_lossless(src);
    let t2 = parse_lossless(&t1.to_source());
    assert_eq!(t1.pieces(), t2.pieces());
    assert_eq!(t1.piece_count(), t2.piece_count());
}

// ---------------------------------------------------------------------------
// 4. QA semantic-equivalence check.
//
// The task asks: "parse → to_source → parse → identical AST". The
// semantic AST is produced by `buff_lang_parser::parse`, but adding
// `buff-lang-parser` as a dev-dep of `buff-lang-ast` would create a
// CYCLE (parser depends on ast, so ast can't dev-depend on parser —
// Cargo rejects it). We therefore prove semantic-equivalence via the
// STRONGER byte-exact roundtrip property:
//
//   parse_lossless(src).to_source() == src
//
// If the reconstructed source is byte-identical, then re-parsing through
// the normal pipeline (lexer + parser) MUST yield the same `Vec<Decl>`
// (the pipeline is deterministic). This is the argument documented in
// the task spec ("byte-exact roundtrip is the stronger check").
//
// The tests below assert the byte-exact invariant on a comprehensive
// fixture set, providing the QA guarantee the task requires.
// ---------------------------------------------------------------------------

#[test]
fn lossless_qa_semantic_equivalence_via_byte_exact_roundtrip() {
    // Representative fixtures spanning all decl kinds the scanner
    // handles, with comments (the case that motivated T57).
    let fixtures = [
        // Func with comments
        "// doc\nfunc ola()\n    print(\"hi\")\n",
        // Struct + enum
        "struct P\n    x: Int\nenum C\n    Red\n",
        // Match with comments
        "func f(x)\n    match x\n        Some(y)\n            // yep\n            y\n        None\n            0\n",
        // String interp + UTF-8
        "func greet(n)\n    print(\"Olá {n} 🚀\")\n",
        // Char + escape
        "let nl = '\\n'\n",
        // Block comment nested
        "/* a /* b */ c */\nfunc h()\n    1\n",
    ];
    for fixture in fixtures {
        let tree = parse_lossless(fixture);
        assert_eq!(
            tree.to_source(),
            fixture,
            "byte-exact roundtrip must hold (semantic-equivalence guarantee)"
        );
    }
}

#[test]
fn lossless_qa_semantic_equivalence_documentation() {
    // Documentation test: prove that the reconstructed source would
    // parse identically. Since buff_lang_ast has no parser dep (cycle-
    // free), we assert the equivalent invariant: the reconstructed
    // source is byte-identical to the original, AND re-running the
    // lossless parse on it yields the same tree.
    let src = "func f()\n    // preserved\n    1\n";
    let t1 = parse_lossless(src);
    let reconstructed = t1.to_source();
    let t2 = parse_lossless(&reconstructed);
    assert_eq!(src.as_bytes(), reconstructed.as_bytes());
    assert_eq!(t1, t2);
}

// ---------------------------------------------------------------------------
// 5. Incremental reparse (v1.0-minimal — full rescan under a structured
//    API for future incremental use).
// ---------------------------------------------------------------------------

#[test]
fn lossless_reparse_simple_insert() {
    let src = "func f()\n    1\n";
    let t1 = parse_lossless(src);
    // Insert " // comment" AFTER the `1`. Compute `1`'s position
    // precisely (don't hard-code byte math).
    let one_idx = src.find('1').expect("fixture must contain '1'");
    let after_one = one_idx + 1;
    let t2 = t1.reparse(after_one, after_one, " // comment");
    let expected = "func f()\n    1 // comment\n";
    assert_eq!(t2.to_source(), expected);
    assert_eq!(t2.comment_count(), 1);
}

#[test]
fn lossless_reparse_delete_range() {
    let src = "func f()\n    // drop me\n    1\n";
    let t1 = parse_lossless(src);
    // Delete the comment line.
    let needle = "// drop me";
    let start = src.find(needle).expect("fixture must contain the comment");
    let end = start + needle.len();
    let t2 = t1.reparse(start, end, "");
    let new_src = t2.to_source();
    assert!(!new_src.contains("drop me"));
    assert_eq!(t2.comment_count(), 0);
    // The rest of the source is intact.
    assert!(new_src.contains("func f()"));
    assert!(new_src.contains("    1"));
}

#[test]
fn lossless_reparse_replace_token() {
    let src = "let x = 1\n";
    let t1 = parse_lossless(src);
    let one_pos = src.find('1').expect("fixture must contain '1'");
    let t2 = t1.reparse(one_pos, one_pos + 1, "42");
    assert_eq!(t2.to_source(), "let x = 42\n");
}

#[test]
fn lossless_reparse_clamps_out_of_bounds_silently() {
    let src = "abc\n";
    let t = parse_lossless(src);
    // start > end → clamped to (start.min, end.max.min) = (4, 4)
    let t2 = t.reparse(10, 5, "X");
    // After clamp: replace src[4..4] with "X" → "abc\nX"
    assert_eq!(t2.to_source(), "abc\nX");
    // start beyond len: same behaviour.
    let t3 = t.reparse(100, 100, "Y");
    assert_eq!(t3.to_source(), "abc\nY");
}

// ---------------------------------------------------------------------------
// 6. Piece metadata (byte ranges, kinds, lookup by offset).
// ---------------------------------------------------------------------------

#[test]
fn lossless_piece_ranges_contiguous_and_complete() {
    let src = "func f() // c\n";
    let tree = parse_lossless(src);
    // Walk pieces and verify contiguity.
    let mut cursor = 0;
    for p in tree.pieces() {
        assert_eq!(p.start(), cursor);
        assert_eq!(p.end() - p.start(), p.len());
        assert_eq!(p.text().len(), p.len());
        cursor = p.end();
    }
    assert_eq!(cursor, src.len());
}

#[test]
fn lossless_piece_at_finds_correct_piece() {
    let src = "func f()\n    1\n";
    let tree = parse_lossless(src);
    // Offset 0 → "func"
    let p0 = tree.piece_at(0).expect("offset 0 must hit a piece");
    assert_eq!(p0.text(), "func");
    // Offset 4 → whitespace
    let p4 = tree.piece_at(4).expect("offset 4 must hit a piece");
    assert!(p4.is_trivia());
    // Offset at the `1` → token "1"
    let one_off = src.find('1').expect("fixture must contain '1'");
    let p1 = tree
        .piece_at(one_off)
        .expect("offset of '1' must hit a piece");
    assert_eq!(p1.text(), "1");
}

#[test]
fn lossless_piece_at_returns_none_beyond_eof() {
    let tree = parse_lossless("abc");
    assert!(tree.piece_at(0).is_some());
    assert!(tree.piece_at(2).is_some());
    assert!(
        tree.piece_at(3).is_none(),
        "EOF offset is NOT inside any piece"
    );
}

#[test]
fn lossless_piece_token_count_matches_fixture_shape() {
    // `func f()` — the lossless scanner does NOT do semantic
    // tokenization; it groups maximal runs of "boring bytes" (no trivia
    // starters, no string/char starters) into one Token. So `func` and
    // `f()` (no whitespace inside) are 2 tokens. The single inter-token
    // space is ONE Whitespace trivia piece (maximal run, not split).
    let tree = parse_lossless("func f()");
    assert_eq!(tree.token_count(), 2);
    assert_eq!(tree.trivia_count(), 1);
    assert_eq!(tree.piece_count(), 3);
}

// ---------------------------------------------------------------------------
// 7. Whitespace/newline handling specifics.
// ---------------------------------------------------------------------------

#[test]
fn lossless_newline_crlf_preserved_as_single_piece() {
    let tree = parse_lossless("a\r\nb");
    // Pieces: a, \r\n (one Newline), b
    let nl = tree.pieces().iter().find(|p| {
        matches!(
            p,
            Piece::Trivia {
                kind: TriviaKind::Newline,
                ..
            }
        )
    });
    let nl = nl.expect("must find a Newline piece");
    assert_eq!(nl.text(), "\r\n");
    assert_eq!(nl.len(), 2);
}

#[test]
fn lossless_newline_lone_cr_preserved() {
    let tree = parse_lossless("a\rb");
    let nl = tree
        .pieces()
        .iter()
        .find(|p| {
            matches!(
                p,
                Piece::Trivia {
                    kind: TriviaKind::Newline,
                    ..
                }
            )
        })
        .expect("must find a Newline piece");
    assert_eq!(nl.text(), "\r");
}

#[test]
fn lossless_whitespace_run_grouped_as_single_piece() {
    // 5 spaces + 2 tabs should be ONE whitespace piece, not split.
    let tree = parse_lossless("a     \t\tx");
    let ws_count = tree
        .pieces()
        .iter()
        .filter(|p| {
            matches!(
                p,
                Piece::Trivia {
                    kind: TriviaKind::Whitespace,
                    ..
                }
            )
        })
        .count();
    assert_eq!(ws_count, 1);
}

// ---------------------------------------------------------------------------
// 8. Roundtrip stress: deterministic property — parse→to_source is a
//    fixed point.
// ---------------------------------------------------------------------------

#[test]
fn lossless_to_source_is_fixed_point() {
    // For ANY source s, parse_lossless(s).to_source() == s. Re-applying
    // the operation must not change anything.
    let srcs = [
        "",
        "x",
        "// c\n",
        "func f()\n    1\n",
        "/* block */ x /* inline */ y\n",
        "\"a // b\"\n",
        "let pat = /\\d+/\n",
    ];
    for s in srcs {
        let once = parse_lossless(s).to_source();
        let twice = parse_lossless(&once).to_source();
        assert_eq!(s, once, "first roundtrip must hold");
        assert_eq!(
            once, twice,
            "second roundtrip must equal first (fixed point)"
        );
    }
}
