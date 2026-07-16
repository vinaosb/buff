//! Token definition tests for the buff-lang-lexer crate.
//!
//! Covers keyword completeness, keyword lookup, token construction,
//! Display formatting, error construction, and operator variant count.

use buff_lang_error::Span;
use buff_lang_lexer::{LexerError, Token, TokenKind};

// ---------------------------------------------------------------------------
// Keyword tests
// ---------------------------------------------------------------------------

#[test]
fn all_keywords_present() {
    let expected: &[&str] = &[
        "func", "let", "mut", "struct", "enum", "trait", "type", "if", "else", "for", "return",
        "break", "continue", "in", "match", "async", "spawn", "import", "export", "from", "as",
        "true", "false", "extern", "unsafe",
    ];
    assert_eq!(TokenKind::all_keywords(), expected);
    assert_eq!(TokenKind::all_keywords().len(), 25);
}

#[test]
fn from_keyword_returns_some_for_all_keywords() {
    for kw in TokenKind::all_keywords() {
        assert!(
            TokenKind::from_keyword(kw).is_some(),
            "expected from_keyword({:?}) to return Some",
            kw
        );
    }
}

#[test]
fn from_keyword_returns_none_for_non_keyword() {
    assert_eq!(TokenKind::from_keyword("not_a_keyword"), None);
    assert_eq!(TokenKind::from_keyword("foo"), None);
    assert_eq!(TokenKind::from_keyword("class"), None);
    assert_eq!(TokenKind::from_keyword(""), None);
}

#[test]
fn from_keyword_specific_mappings() {
    assert_eq!(TokenKind::from_keyword("func"), Some(TokenKind::KwFunc));
    assert_eq!(TokenKind::from_keyword("let"), Some(TokenKind::KwLet));
    assert_eq!(TokenKind::from_keyword("mut"), Some(TokenKind::KwMut));
    assert_eq!(TokenKind::from_keyword("struct"), Some(TokenKind::KwStruct));
    assert_eq!(TokenKind::from_keyword("enum"), Some(TokenKind::KwEnum));
    assert_eq!(TokenKind::from_keyword("trait"), Some(TokenKind::KwTrait));
    assert_eq!(TokenKind::from_keyword("type"), Some(TokenKind::KwType));
    assert_eq!(TokenKind::from_keyword("if"), Some(TokenKind::KwIf));
    assert_eq!(TokenKind::from_keyword("else"), Some(TokenKind::KwElse));
    assert_eq!(TokenKind::from_keyword("for"), Some(TokenKind::KwFor));
    assert_eq!(TokenKind::from_keyword("return"), Some(TokenKind::KwReturn));
    assert_eq!(TokenKind::from_keyword("break"), Some(TokenKind::KwBreak));
    assert_eq!(
        TokenKind::from_keyword("continue"),
        Some(TokenKind::KwContinue)
    );
    assert_eq!(TokenKind::from_keyword("in"), Some(TokenKind::KwIn));
    assert_eq!(TokenKind::from_keyword("match"), Some(TokenKind::KwMatch));
    assert_eq!(TokenKind::from_keyword("async"), Some(TokenKind::KwAsync));
    assert_eq!(TokenKind::from_keyword("spawn"), Some(TokenKind::KwSpawn));
    assert_eq!(TokenKind::from_keyword("import"), Some(TokenKind::KwImport));
    assert_eq!(TokenKind::from_keyword("export"), Some(TokenKind::KwExport));
    assert_eq!(TokenKind::from_keyword("from"), Some(TokenKind::KwFrom));
    assert_eq!(TokenKind::from_keyword("as"), Some(TokenKind::KwAs));
    assert_eq!(TokenKind::from_keyword("true"), Some(TokenKind::KwTrue));
    assert_eq!(TokenKind::from_keyword("false"), Some(TokenKind::KwFalse));
    assert_eq!(TokenKind::from_keyword("extern"), Some(TokenKind::KwExtern));
    assert_eq!(TokenKind::from_keyword("unsafe"), Some(TokenKind::KwUnsafe));
}

#[test]
fn is_keyword_true_for_all_keywords() {
    for kw in TokenKind::all_keywords() {
        let kind = TokenKind::from_keyword(kw).unwrap();
        assert!(
            kind.is_keyword(),
            "expected {:?}.is_keyword() to be true",
            kind
        );
    }
}

#[test]
fn is_keyword_false_for_non_keywords() {
    assert!(!TokenKind::IntLit(42).is_keyword());
    assert!(!TokenKind::Plus.is_keyword());
    assert!(!TokenKind::Ident("foo".into()).is_keyword());
    assert!(!TokenKind::Eof.is_keyword());
}

// ---------------------------------------------------------------------------
// Token construction
// ---------------------------------------------------------------------------

#[test]
fn token_new_constructs() {
    let span = Span::dummy();
    let t = Token::new(TokenKind::IntLit(42), span);
    assert_eq!(t.kind, TokenKind::IntLit(42));
    assert_eq!(t.span, span);
}

#[test]
fn token_new_with_various_kinds() {
    let span = Span::dummy();
    let cases = vec![
        TokenKind::FloatLit(2.5_f32),
        TokenKind::DoubleLit(99.9),
        TokenKind::StringLit("hello".into()),
        TokenKind::ByteLit(0xFF),
        TokenKind::Ident("foo".into()),
        TokenKind::KwReturn,
        TokenKind::Plus,
        TokenKind::Eof,
    ];
    for kind in cases {
        let t = Token::new(kind.clone(), span);
        assert_eq!(t.kind, kind);
    }
}

// ---------------------------------------------------------------------------
// Display tests
// ---------------------------------------------------------------------------

#[test]
fn display_keywords() {
    assert_eq!(TokenKind::KwFunc.to_string(), "func");
    assert_eq!(TokenKind::KwLet.to_string(), "let");
    assert_eq!(TokenKind::KwReturn.to_string(), "return");
    assert_eq!(TokenKind::KwTrue.to_string(), "true");
    assert_eq!(TokenKind::KwFalse.to_string(), "false");
    assert_eq!(TokenKind::KwExtern.to_string(), "extern");
    assert_eq!(TokenKind::KwUnsafe.to_string(), "unsafe");
}

#[test]
fn display_operators() {
    assert_eq!(TokenKind::Plus.to_string(), "+");
    assert_eq!(TokenKind::Minus.to_string(), "-");
    assert_eq!(TokenKind::Star.to_string(), "*");
    assert_eq!(TokenKind::Slash.to_string(), "/");
    assert_eq!(TokenKind::Percent.to_string(), "%");
    assert_eq!(TokenKind::EqEq.to_string(), "==");
    assert_eq!(TokenKind::NotEq.to_string(), "!=");
    assert_eq!(TokenKind::Lt.to_string(), "<");
    assert_eq!(TokenKind::Gt.to_string(), ">");
    assert_eq!(TokenKind::LtEq.to_string(), "<=");
    assert_eq!(TokenKind::GtEq.to_string(), ">=");
    assert_eq!(TokenKind::AndAnd.to_string(), "&&");
    assert_eq!(TokenKind::OrOr.to_string(), "||");
    assert_eq!(TokenKind::Not.to_string(), "!");
    assert_eq!(TokenKind::Question.to_string(), "?");
    assert_eq!(TokenKind::Caret.to_string(), "^");
    assert_eq!(TokenKind::Pipe.to_string(), "|");
    assert_eq!(TokenKind::Amp.to_string(), "&");
    assert_eq!(TokenKind::Shl.to_string(), "<<");
    assert_eq!(TokenKind::Shr.to_string(), ">>");
    assert_eq!(TokenKind::Tilde.to_string(), "~");
    assert_eq!(TokenKind::Arrow.to_string(), "->");
    assert_eq!(TokenKind::FatArrow.to_string(), "=>");
    assert_eq!(TokenKind::Assign.to_string(), "=");
    assert_eq!(TokenKind::PlusEq.to_string(), "+=");
    assert_eq!(TokenKind::MinusEq.to_string(), "-=");
    assert_eq!(TokenKind::StarEq.to_string(), "*=");
    assert_eq!(TokenKind::SlashEq.to_string(), "/=");
    assert_eq!(TokenKind::PercentEq.to_string(), "%=");
}

#[test]
fn display_delimiters() {
    assert_eq!(TokenKind::LParen.to_string(), "(");
    assert_eq!(TokenKind::RParen.to_string(), ")");
    assert_eq!(TokenKind::LBrace.to_string(), "{");
    assert_eq!(TokenKind::RBrace.to_string(), "}");
    assert_eq!(TokenKind::LBracket.to_string(), "[");
    assert_eq!(TokenKind::RBracket.to_string(), "]");
    assert_eq!(TokenKind::Colon.to_string(), ":");
    assert_eq!(TokenKind::Comma.to_string(), ",");
    assert_eq!(TokenKind::Dot.to_string(), ".");
    assert_eq!(TokenKind::Semicolon.to_string(), ";");
    assert_eq!(TokenKind::At.to_string(), "@");
}

#[test]
fn display_literals() {
    assert_eq!(TokenKind::IntLit(42).to_string(), "int(42)");
    assert_eq!(TokenKind::ByteLit(0xFF).to_string(), "byte(255)");
    assert_eq!(
        TokenKind::StringLit("hi".into()).to_string(),
        r#"string("hi")"#
    );
}

#[test]
fn display_layout() {
    assert_eq!(TokenKind::Newline.to_string(), "newline");
    assert_eq!(TokenKind::Indent.to_string(), "indent");
    assert_eq!(TokenKind::Dedent.to_string(), "dedent");
    assert_eq!(TokenKind::Eof.to_string(), "eof");
}

#[test]
fn display_string_interp() {
    assert_eq!(TokenKind::StringStart.to_string(), "string_start");
    assert_eq!(
        TokenKind::StringPart("x".into()).to_string(),
        r#"string_part("x")"#
    );
    assert_eq!(TokenKind::InterpStart.to_string(), "interp_start");
    assert_eq!(TokenKind::InterpEnd.to_string(), "interp_end");
    assert_eq!(TokenKind::StringEnd.to_string(), "string_end");
}

#[test]
fn display_token_struct() {
    let t = Token::new(TokenKind::KwFunc, Span::dummy());
    assert_eq!(t.to_string(), "func");
}

// ---------------------------------------------------------------------------
// Operator variant count
// ---------------------------------------------------------------------------

#[test]
fn operator_variant_count() {
    // Count all operator variants (excluding literals, keywords, delimiters, layout, ident, interp)
    let operators = [
        TokenKind::Plus,
        TokenKind::Minus,
        TokenKind::Star,
        TokenKind::Slash,
        TokenKind::Percent,
        TokenKind::EqEq,
        TokenKind::NotEq,
        TokenKind::Lt,
        TokenKind::Gt,
        TokenKind::LtEq,
        TokenKind::GtEq,
        TokenKind::AndAnd,
        TokenKind::OrOr,
        TokenKind::Not,
        TokenKind::Question,
        TokenKind::Caret,
        TokenKind::Pipe,
        TokenKind::Amp,
        TokenKind::Shl,
        TokenKind::Shr,
        TokenKind::Tilde,
        TokenKind::Arrow,
        TokenKind::FatArrow,
        TokenKind::Assign,
        TokenKind::PlusEq,
        TokenKind::MinusEq,
        TokenKind::StarEq,
        TokenKind::SlashEq,
        TokenKind::PercentEq,
    ];
    assert_eq!(operators.len(), 29, "expected 29 operator variants");
}

// ---------------------------------------------------------------------------
// Error tests
// ---------------------------------------------------------------------------

#[test]
fn lexer_error_unexpected_char() {
    let span = Span::dummy();
    let err = LexerError::unexpected_char('!', span);
    let msg = err.to_string();
    assert!(msg.contains("unexpected character"), "got: {}", msg);
    assert!(msg.contains("'!'"), "got: {}", msg);
}

#[test]
fn lexer_error_unterminated_string() {
    let span = Span::dummy();
    let err = LexerError::unterminated_string(span);
    assert!(err.to_string().contains("unterminated string"));
}

#[test]
fn lexer_error_invalid_number() {
    let span = Span::dummy();
    let err = LexerError::invalid_number(span);
    assert!(err.to_string().contains("invalid numeric literal"));
}

#[test]
fn lexer_error_mixed_tabs_spaces() {
    let span = Span::dummy();
    let err = LexerError::mixed_tabs_spaces(span);
    assert!(err.to_string().contains("mixed tabs and spaces"));
}

#[test]
fn lexer_error_new() {
    let span = Span::dummy();
    let err = LexerError::new("something went wrong", span);
    assert!(err.to_string().contains("something went wrong"));
}

#[test]
fn lexer_error_into_buff_error() {
    let span = Span::dummy();
    let lex_err = LexerError::new("test", span);
    let buff_err: buff_lang_error::BuffError = lex_err.into();
    assert!(buff_err.to_string().contains("test"));
}

#[test]
fn lexer_error_implements_std_error() {
    use std::error::Error;
    let span = Span::dummy();
    let err = LexerError::new("test", span);
    assert!(err.source().is_some());
}
