//! Integration tests for the `buff-lang-lexer` crate's `tokenize()` function.
//!
//! Covers tokenization of identifiers, keywords, literals (int/float/double/
//! byte), operators (single- and multi-char), comments (line + nested block),
//! indentation tracking (Indent/Dedent emission, mixed-tab error), string
//! interpolation, CRLF normalization, span correctness, and snapshots of the
//! T5 fixture files.

use buff_lang_error::{SourceId, Span};
use buff_lang_lexer::{tokenize, Token, TokenKind};

/// Run the lexer and return only the token kinds.
fn kinds(src: &str) -> Vec<TokenKind> {
    tokenize(src, SourceId(0))
        .unwrap_or_else(|e| panic!("lexer failed on {src:?}: {e}"))
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

/// Run the lexer and return tokens with spans.
fn full(src: &str) -> Vec<Token> {
    tokenize(src, SourceId(0)).unwrap_or_else(|e| panic!("lexer failed on {src:?}: {e}"))
}

/// Run the lexer and expect it to fail.
fn err(src: &str) -> buff_lang_lexer::LexerError {
    tokenize(src, SourceId(0))
        .err()
        .unwrap_or_else(|| panic!("expected lexer error on {src:?}, got success"))
}

// ---------------------------------------------------------------------------
// Basic tokenization
// ---------------------------------------------------------------------------

#[test]
fn test_tokenize_hello_world() {
    let src = "func main():\n    print(\"Olá, Buff!\")";
    let tokens = kinds(src);
    assert_eq!(
        tokens,
        vec![
            TokenKind::KwFunc,
            TokenKind::Ident("main".into()),
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::Colon,
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Ident("print".into()),
            TokenKind::LParen,
            TokenKind::StringStart,
            TokenKind::StringPart("Olá, Buff!".into()),
            TokenKind::StringEnd,
            TokenKind::RParen,
            TokenKind::Dedent,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_empty_input() {
    assert_eq!(kinds(""), vec![TokenKind::Eof]);
}

#[test]
fn test_whitespace_only() {
    assert_eq!(kinds("   \n  "), vec![TokenKind::Eof]);
    assert_eq!(kinds("\n\n\n"), vec![TokenKind::Eof]);
}

#[test]
fn test_eof_at_end() {
    let tokens = kinds("x");
    assert!(matches!(tokens.last(), Some(TokenKind::Eof)));
}

// ---------------------------------------------------------------------------
// Indentation
// ---------------------------------------------------------------------------

#[test]
fn test_indent_increase() {
    let tokens = kinds("x\n    y");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Ident("y".into()),
            TokenKind::Dedent,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_indent_decrease() {
    let tokens = kinds("x\n    y\nz");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Ident("y".into()),
            TokenKind::Newline,
            TokenKind::Dedent,
            TokenKind::Ident("z".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_indent_no_change() {
    let tokens = kinds("x\ny\nz");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
            TokenKind::Ident("y".into()),
            TokenKind::Newline,
            TokenKind::Ident("z".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_indent_nested_two_levels() {
    let tokens = kinds("a\n    b\n        c\n    d\ne");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("a".into()),
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Ident("b".into()),
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Ident("c".into()),
            TokenKind::Newline,
            TokenKind::Dedent,
            TokenKind::Ident("d".into()),
            TokenKind::Newline,
            TokenKind::Dedent,
            TokenKind::Ident("e".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_mixed_tabs_spaces_error() {
    let e = err("x\n    \ty");
    assert!(e.to_string().contains("mixed tabs and spaces"), "got: {e}");
}

#[test]
fn test_blank_lines_do_not_emit_newline() {
    let tokens = kinds("x\n\n\ny");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
            TokenKind::Ident("y".into()),
            TokenKind::Eof,
        ]
    );
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

#[test]
fn test_string_simple() {
    let tokens = kinds("\"hello\"");
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart("hello".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_string_empty() {
    let tokens = kinds("\"\"");
    assert_eq!(
        tokens,
        vec![TokenKind::StringStart, TokenKind::StringEnd, TokenKind::Eof]
    );
}

#[test]
fn test_string_interp_single() {
    let tokens = kinds("\"valor {x} fim\"");
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart("valor ".into()),
            TokenKind::InterpStart,
            TokenKind::Ident("x".into()),
            TokenKind::InterpEnd,
            TokenKind::StringPart(" fim".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_string_interp_complex() {
    let tokens = kinds("\"a {b + c} d {e} f\"");
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart("a ".into()),
            TokenKind::InterpStart,
            TokenKind::Ident("b".into()),
            TokenKind::Plus,
            TokenKind::Ident("c".into()),
            TokenKind::InterpEnd,
            TokenKind::StringPart(" d ".into()),
            TokenKind::InterpStart,
            TokenKind::Ident("e".into()),
            TokenKind::InterpEnd,
            TokenKind::StringPart(" f".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_string_unterminated() {
    let e = err("\"hello");
    assert!(e.to_string().contains("unterminated string"), "got: {e}");
}

#[test]
fn test_string_escape_backslash() {
    // Raw escape bytes are preserved verbatim in the StringPart; the parser
    // or codegen is responsible for interpretation.
    let tokens = kinds(r#""line\nbreak""#);
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart(r"line\nbreak".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_string_escape_quote() {
    let tokens = kinds(r#""say \"hi\"""#);
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart(r#"say \"hi\""#.into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_unicode_string() {
    let tokens = kinds("\"olá mundo ✓\"");
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart("olá mundo ✓".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_unicode_identifier_not_supported_in_v01() {
    // Design decision for v0.1: identifiers are ASCII-only. The lexer
    // stops the identifier scan at the first non-ASCII byte and then
    // errors on that byte. UTF-8 inside STRING literals is fully
    // supported (see test_unicode_string).
    let result = tokenize("naïve", SourceId(0));
    assert!(
        result.is_err(),
        "v0.1 lexer should reject non-ASCII identifiers, but got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

#[test]
fn test_int_literal() {
    let tokens = kinds("42");
    assert_eq!(tokens, vec![TokenKind::IntLit(42), TokenKind::Eof]);
}

#[test]
fn test_int_zero() {
    let tokens = kinds("0");
    assert_eq!(tokens, vec![TokenKind::IntLit(0), TokenKind::Eof]);
}

#[test]
fn test_int_large() {
    let tokens = kinds("9999999999");
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0], TokenKind::IntLit(9_999_999_999)));
}

#[test]
fn test_float_literal() {
    let tokens = kinds("2.5");
    assert_eq!(tokens.len(), 2);
    match tokens[0] {
        TokenKind::FloatLit(f) => assert!((f - 2.5_f32).abs() < 1e-6),
        ref other => panic!("expected FloatLit, got {other:?}"),
    }
}

#[test]
fn test_double_literal_d_suffix() {
    let tokens = kinds("99.9d");
    assert_eq!(tokens.len(), 2);
    match tokens[0] {
        TokenKind::DoubleLit(d) => assert!((d - 99.9_f64).abs() < 1e-9),
        ref other => panic!("expected DoubleLit, got {other:?}"),
    }
}

#[test]
fn test_double_literal_capital_d_suffix() {
    let tokens = kinds("1.5D");
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0], TokenKind::DoubleLit(_)));
}

// T20: the `m`/`M` decimal suffix is now SUPPORTED (it was rejected in
// v0.1). `3.14m` lexes to a `DecimalLit("3.14")` token — the raw digit text
// is carried verbatim so the value never rounds through f64.
#[test]
fn test_decimal_m_suffix_now_supported() {
    let tokens = kinds("3.14m");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0], TokenKind::DecimalLit("3.14".into()));
    assert_eq!(tokens[1], TokenKind::Eof);

    // Capital `M` is equivalent.
    let tokens = kinds("3.14M");
    assert_eq!(tokens[0], TokenKind::DecimalLit("3.14".into()));
}

#[test]
fn test_byte_literal_hex_uppercase() {
    let tokens = kinds("0xFF");
    assert_eq!(tokens, vec![TokenKind::ByteLit(255), TokenKind::Eof]);
}

#[test]
fn test_byte_literal_hex_lowercase() {
    let tokens = kinds("0xff");
    assert_eq!(tokens, vec![TokenKind::ByteLit(255), TokenKind::Eof]);
}

#[test]
fn test_byte_literal_binary() {
    let tokens = kinds("0b1010");
    assert_eq!(tokens, vec![TokenKind::ByteLit(10), TokenKind::Eof]);
}

#[test]
fn test_byte_literal_zero() {
    let tokens = kinds("0b0");
    assert_eq!(tokens, vec![TokenKind::ByteLit(0), TokenKind::Eof]);
}

#[test]
fn test_decimal_not_split_into_int_dot_int() {
    let tokens = kinds("3.14");
    assert_eq!(tokens.len(), 2); // FloatLit + Eof, NOT Int(3) Dot Int(14) + Eof
    assert!(matches!(tokens[0], TokenKind::FloatLit(_)));
}

#[test]
fn test_int_then_dot_no_fraction() {
    // `42.` is an integer followed by a dot — not a float (no fractional
    // digits after the dot).
    let tokens = kinds("42.");
    assert_eq!(
        tokens,
        vec![TokenKind::IntLit(42), TokenKind::Dot, TokenKind::Eof]
    );
}

// ---------------------------------------------------------------------------
// Keywords and operators
// ---------------------------------------------------------------------------

#[test]
fn test_all_25_keywords_tokenize() {
    // T73 added `guard` (26 keywords); T75 added `extend` (27 keywords).
    // The test name keeps the original "25" for historical traceability;
    // the actual count is 27.
    let src = "func let mut struct enum trait type if else for return break continue in match async spawn import export from as true false extern unsafe guard extend";
    let tokens = kinds(src);
    // Expect exactly 27 keyword tokens followed by EOF.
    assert_eq!(tokens.len(), 28);
    for (i, k) in tokens.iter().enumerate() {
        if i < 27 {
            assert!(k.is_keyword(), "expected keyword at index {i}, got {k:?}");
        }
    }
}

#[test]
fn test_keyword_distinct_from_identifier() {
    // `funcx` should be an identifier, not `func` + `x`.
    let tokens = kinds("funcx");
    assert_eq!(
        tokens,
        vec![TokenKind::Ident("funcx".into()), TokenKind::Eof]
    );
}

#[test]
fn test_all_multi_char_operators() {
    let src = "== != <= >= && || << >> -> => += -= *= /= %=";
    let tokens = kinds(src);
    assert_eq!(
        tokens,
        vec![
            TokenKind::EqEq,
            TokenKind::NotEq,
            TokenKind::LtEq,
            TokenKind::GtEq,
            TokenKind::AndAnd,
            TokenKind::OrOr,
            TokenKind::Shl,
            TokenKind::Shr,
            TokenKind::Arrow,
            TokenKind::FatArrow,
            TokenKind::PlusEq,
            TokenKind::MinusEq,
            TokenKind::StarEq,
            TokenKind::SlashEq,
            TokenKind::PercentEq,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_all_single_char_operators() {
    let src = "+ - * / % < > ! ? ^ | & ~ =";
    let tokens = kinds(src);
    assert_eq!(
        tokens,
        vec![
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::Lt,
            TokenKind::Gt,
            TokenKind::Not,
            TokenKind::Question,
            TokenKind::Caret,
            TokenKind::Pipe,
            TokenKind::Amp,
            TokenKind::Tilde,
            TokenKind::Assign,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_all_delimiters() {
    let src = "( ) { } [ ] : , . ; @";
    let tokens = kinds(src);
    assert_eq!(
        tokens,
        vec![
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::LBracket,
            TokenKind::RBracket,
            TokenKind::Colon,
            TokenKind::Comma,
            TokenKind::Dot,
            TokenKind::Semicolon,
            TokenKind::At,
            TokenKind::Eof,
        ]
    );
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

#[test]
fn test_line_comment_at_line_start() {
    let tokens = kinds("// comment\nx");
    assert_eq!(tokens, vec![TokenKind::Ident("x".into()), TokenKind::Eof]);
}

#[test]
fn test_line_comment_trailing() {
    let tokens = kinds("x // trailing comment\n");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_block_comment_simple() {
    let tokens = kinds("/* comment */ x");
    assert_eq!(tokens, vec![TokenKind::Ident("x".into()), TokenKind::Eof]);
}

#[test]
fn test_block_comment_multiline() {
    let tokens = kinds("/* line1\nline2 */ y");
    assert_eq!(tokens, vec![TokenKind::Ident("y".into()), TokenKind::Eof]);
}

#[test]
fn test_nested_block_comment() {
    let tokens = kinds("/* outer /* inner */ still outer */ x");
    assert_eq!(tokens, vec![TokenKind::Ident("x".into()), TokenKind::Eof]);
}

#[test]
fn test_nested_block_comment_deeply() {
    let tokens = kinds("/* a /* b /* c */ b */ a */ z");
    assert_eq!(tokens, vec![TokenKind::Ident("z".into()), TokenKind::Eof]);
}

#[test]
fn test_unterminated_block_comment_errors() {
    let e = err("/* never closed");
    let msg = e.to_string();
    assert!(msg.contains("unterminated block comment"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// Span correctness
// ---------------------------------------------------------------------------

#[test]
fn test_span_for_identifier() {
    let tokens = full("abc");
    assert_eq!(tokens[0].span, Span::new(0, 3, SourceId(0)));
}

#[test]
fn test_span_for_integer() {
    let tokens = full("42");
    assert_eq!(tokens[0].span, Span::new(0, 2, SourceId(0)));
}

#[test]
fn test_span_for_string_tokens() {
    let tokens = full("\"hi\"");
    assert_eq!(tokens[0].span, Span::new(0, 1, SourceId(0))); // StringStart
    assert_eq!(tokens[1].span, Span::new(1, 3, SourceId(0))); // StringPart("hi")
    assert_eq!(tokens[2].span, Span::new(3, 4, SourceId(0))); // StringEnd
}

#[test]
fn test_span_for_two_char_operator() {
    let tokens = full("==");
    assert_eq!(tokens[0].span, Span::new(0, 2, SourceId(0)));
}

#[test]
fn test_span_for_interp_tokens() {
    let tokens = full("\"a{x}b\"");
    // StringStart 0..1, StringPart("a") 1..2, InterpStart 2..3, Ident(x) 3..4,
    // InterpEnd 4..5, StringPart("b") 5..6, StringEnd 6..7
    assert_eq!(tokens[0].span, Span::new(0, 1, SourceId(0)));
    assert_eq!(tokens[1].span, Span::new(1, 2, SourceId(0)));
    assert_eq!(tokens[2].span, Span::new(2, 3, SourceId(0)));
    assert_eq!(tokens[3].span, Span::new(3, 4, SourceId(0)));
    assert_eq!(tokens[4].span, Span::new(4, 5, SourceId(0)));
    assert_eq!(tokens[5].span, Span::new(5, 6, SourceId(0)));
    assert_eq!(tokens[6].span, Span::new(6, 7, SourceId(0)));
}

// ---------------------------------------------------------------------------
// Newline normalization
// ---------------------------------------------------------------------------

#[test]
fn test_newline_crlf_normalized() {
    let tokens = kinds("x\r\ny");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
            TokenKind::Ident("y".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_newline_cr_normalized() {
    let tokens = kinds("x\ry");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
            TokenKind::Ident("y".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_newline_lf_unchanged() {
    let tokens = kinds("x\ny");
    assert_eq!(
        tokens,
        vec![
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
            TokenKind::Ident("y".into()),
            TokenKind::Eof,
        ]
    );
}

// ---------------------------------------------------------------------------
// T104: Raw string literals `r"..."` — no escape processing.
// ---------------------------------------------------------------------------

#[test]
fn test_raw_strings_simple() {
    // `r"hello"` → StringStart, StringPart("hello"), StringEnd
    let tokens = kinds(r#"r"hello""#);
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart("hello".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_raw_strings_backslash_preserved() {
    // `r"\n"` → content is literal backslash-n (NOT newline)
    let tokens = kinds(r#"r"\n""#);
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart(r"\n".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_raw_strings_windows_path() {
    // `r"C:\path"` → backslashes preserved
    let tokens = kinds(r#"r"C:\path""#);
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart(r"C:\path".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_raw_strings_regex() {
    // `r"\d+"` → literal `\d+`
    let tokens = kinds(r#"r"\d+""#);
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart(r"\d+".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn test_raw_strings_empty() {
    // `r""` → empty raw string
    let tokens = kinds(r#"r"""#);
    assert_eq!(
        tokens,
        vec![TokenKind::StringStart, TokenKind::StringEnd, TokenKind::Eof]
    );
}

#[test]
fn test_raw_strings_identifier_r_not_followed_by_quote() {
    // `r` as a normal identifier (NOT followed by `"`) must still work.
    let tokens = kinds("r");
    assert_eq!(tokens, vec![TokenKind::Ident("r".into()), TokenKind::Eof]);
}

#[test]
fn test_raw_strings_identifier_rain() {
    // `rain` starts with `r` but is NOT `r"` — must lex as identifier.
    let tokens = kinds("rain");
    assert_eq!(
        tokens,
        vec![TokenKind::Ident("rain".into()), TokenKind::Eof]
    );
}

#[test]
fn test_raw_strings_unterminated() {
    // `r"abc` with no closing quote → error.
    let e = err(r#"r"abc"#);
    assert!(e.to_string().contains("unterminated string"), "got: {e}");
}

#[test]
fn test_raw_strings_no_interpolation() {
    // `{expr}` is literal text inside a raw string.
    let tokens = kinds(r#"r"x {y} z""#);
    assert_eq!(
        tokens,
        vec![
            TokenKind::StringStart,
            TokenKind::StringPart("x {y} z".into()),
            TokenKind::StringEnd,
            TokenKind::Eof,
        ]
    );
}

// ---------------------------------------------------------------------------
// Unexpected character handling
// ---------------------------------------------------------------------------

#[test]
fn test_unexpected_char_error() {
    // '#' is not a valid Buff token.
    let e = err("#");
    let msg = e.to_string();
    assert!(msg.contains("unexpected character"), "got: {msg}");
}

// ---------------------------------------------------------------------------
// T19: Byte (Bits<8>) support — named test for acceptance criteria
// ---------------------------------------------------------------------------

#[test]
fn hex_binary_literals() {
    // 0xFF infers as Byte (u8)
    let tokens = kinds("0xFF");
    assert_eq!(tokens, vec![TokenKind::ByteLit(255), TokenKind::Eof]);

    // 0b1010 infers as Byte
    let tokens = kinds("0b1010");
    assert_eq!(tokens, vec![TokenKind::ByteLit(10), TokenKind::Eof]);

    // 0x00 (zero)
    let tokens = kinds("0x00");
    assert_eq!(tokens, vec![TokenKind::ByteLit(0), TokenKind::Eof]);

    // 0b0 (zero)
    let tokens = kinds("0b0");
    assert_eq!(tokens, vec![TokenKind::ByteLit(0), TokenKind::Eof]);

    // 0xFF (max byte)
    let tokens = kinds("0xFF");
    assert_eq!(tokens, vec![TokenKind::ByteLit(255), TokenKind::Eof]);

    // 0x100 (overflow) should error
    let e = err("0x100");
    assert!(
        e.to_string().contains("invalid numeric literal"),
        "got: {e}"
    );
}

// ---------------------------------------------------------------------------
// Proptest: arbitrary identifier-ish input never panics.
// ---------------------------------------------------------------------------

proptest::proptest! {
    #[test]
    fn prop_identifiers_never_crash(s in "[a-zA-Z_][a-zA-Z0-9_ ]*") {
        let _ = tokenize(&s, SourceId(0));
    }

    #[test]
    fn prop_integers_parse_roundtrip(n in 0i64..1_000_000) {
        let src = n.to_string();
        let tokens = tokenize(&src, SourceId(0)).unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntLit(n));
    }
}

// ---------------------------------------------------------------------------
// Snapshots (uses insta; `.snap.new` files can be reviewed with cargo-insta)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_ola_buff() {
    let src = include_str!("../../../tests/fixtures/valid/ola.buff");
    let tokens = tokenize(src, SourceId(0)).expect("ola.buff should tokenize cleanly");
    let lines: Vec<String> = tokens.iter().map(|t| t.to_string()).collect();
    insta::assert_snapshot!(lines.join("\n"));
}

#[test]
fn test_snapshot_arithmetic_buff() {
    let src = include_str!("../../../tests/fixtures/valid/arithmetic.buff");
    let tokens = tokenize(src, SourceId(0)).expect("arithmetic.buff should tokenize cleanly");
    let lines: Vec<String> = tokens.iter().map(|t| t.to_string()).collect();
    insta::assert_snapshot!(lines.join("\n"));
}
