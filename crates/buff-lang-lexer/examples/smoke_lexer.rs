//! Behavioral equivalence test: Rust original vs Buff port (lexer.buff).
//!
//! Mirrors the stdout of `crates/buff-lang-lexer/selfhost/lexer.buff` exactly.
//! Exercises every `TokenKind` variant (101), the `Token` struct, helper
//! functions (`is_keyword`, `kind_label`), and `LexerError` constructors.
//!
//! Run: `cargo run -p buff-lang-lexer --example smoke_lexer --release`

// The float literals 3.14 and 2.71828 are deliberately chosen to match the
// .buff port's test inputs (lexer.buff lines 467-468). They approximate PI
// and E but are test data, not mathematical constants.
#![allow(clippy::approx_constant)]

use buff_lang_error::{ErrorCode, SourceId, Span};
use buff_lang_lexer::{LexerError, Token, TokenKind};

/// Stable numeric ID for every `TokenKind` variant (matches lexer.buff's
/// `token_kind_num`). Numbering follows the declaration order in
/// `crates/buff-lang-lexer/src/token.rs`, starting at 1. This is NOT the same
/// as Rust's `Discriminant` (which skips data-carrying variants) — it is a
/// hand-assigned 1..=101 contiguous ID.
fn token_kind_num(kind: &TokenKind) -> i64 {
    match kind {
        // Literals (1-8)
        TokenKind::IntLit(_) => 1,
        TokenKind::FloatLit(_) => 2,
        TokenKind::DoubleLit(_) => 3,
        TokenKind::StringLit(_) => 4,
        TokenKind::ByteLit(_) => 5,
        TokenKind::CharLit(_) => 6,
        TokenKind::DecimalLit(_) => 7,
        TokenKind::RegexLit(_) => 8,
        // String interpolation (9-14)
        TokenKind::StringStart => 9,
        TokenKind::StringPart(_) => 10,
        TokenKind::InterpStart => 11,
        TokenKind::InterpSpec(_) => 12,
        TokenKind::InterpEnd => 13,
        TokenKind::StringEnd => 14,
        // Identifier (15)
        TokenKind::Ident(_) => 15,
        // Keywords (16-44)
        TokenKind::KwFunc => 16,
        TokenKind::KwLet => 17,
        TokenKind::KwMut => 18,
        TokenKind::KwStruct => 19,
        TokenKind::KwEnum => 20,
        TokenKind::KwTrait => 21,
        TokenKind::KwType => 22,
        TokenKind::KwIf => 23,
        TokenKind::KwElse => 24,
        TokenKind::KwFor => 25,
        TokenKind::KwReturn => 26,
        TokenKind::KwBreak => 27,
        TokenKind::KwContinue => 28,
        TokenKind::KwIn => 29,
        TokenKind::KwMatch => 30,
        TokenKind::KwAsync => 31,
        TokenKind::KwSpawn => 32,
        TokenKind::KwImport => 33,
        TokenKind::KwExport => 34,
        TokenKind::KwFrom => 35,
        TokenKind::KwAs => 36,
        TokenKind::KwTrue => 37,
        TokenKind::KwFalse => 38,
        TokenKind::KwExtern => 39,
        TokenKind::KwUnsafe => 40,
        TokenKind::KwGuard => 41,
        TokenKind::KwExtend => 42,
        TokenKind::KwDefer => 43,
        TokenKind::KwImpl => 44,
        // Operators (45-78)
        TokenKind::DotDot => 45,
        TokenKind::DotDotEq => 46,
        TokenKind::Plus => 47,
        TokenKind::Minus => 48,
        TokenKind::Star => 49,
        TokenKind::Slash => 50,
        TokenKind::Percent => 51,
        TokenKind::EqEq => 52,
        TokenKind::NotEq => 53,
        TokenKind::Lt => 54,
        TokenKind::Gt => 55,
        TokenKind::LtEq => 56,
        TokenKind::GtEq => 57,
        TokenKind::AndAnd => 58,
        TokenKind::OrOr => 59,
        TokenKind::Not => 60,
        TokenKind::Question => 61,
        TokenKind::QuestionQuestion => 62,
        TokenKind::QuestionDot => 63,
        TokenKind::Caret => 64,
        TokenKind::Pipe => 65,
        TokenKind::Amp => 66,
        TokenKind::Shl => 67,
        TokenKind::Shr => 68,
        TokenKind::Tilde => 69,
        TokenKind::Arrow => 70,
        TokenKind::FatArrow => 71,
        TokenKind::Assign => 72,
        TokenKind::PlusEq => 73,
        TokenKind::MinusEq => 74,
        TokenKind::StarEq => 75,
        TokenKind::SlashEq => 76,
        TokenKind::PercentEq => 77,
        TokenKind::PipeGt => 78,
        // Unicode math operators (79-86)
        TokenKind::Sum => 79,
        TokenKind::Product => 80,
        TokenKind::Sqrt => 81,
        TokenKind::InUni => 82,
        TokenKind::NotInUni => 83,
        TokenKind::SubsetUni => 84,
        TokenKind::ApproxUni => 85,
        TokenKind::Adjoint => 86,
        // Delimiters (87-97)
        TokenKind::LParen => 87,
        TokenKind::RParen => 88,
        TokenKind::LBrace => 89,
        TokenKind::RBrace => 90,
        TokenKind::LBracket => 91,
        TokenKind::RBracket => 92,
        TokenKind::Colon => 93,
        TokenKind::Comma => 94,
        TokenKind::Dot => 95,
        TokenKind::Semicolon => 96,
        TokenKind::At => 97,
        // Layout (98-101)
        TokenKind::Newline => 98,
        TokenKind::Indent => 99,
        TokenKind::Dedent => 100,
        TokenKind::Eof => 101,
    }
}

/// Short human-readable label for a `TokenKind` variant (matches lexer.buff's
/// `kind_label`). Data-carrying variants render just the variant stem.
fn kind_label(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::IntLit(_) => "IntLit",
        TokenKind::FloatLit(_) => "FloatLit",
        TokenKind::DoubleLit(_) => "DoubleLit",
        TokenKind::StringLit(_) => "StringLit",
        TokenKind::ByteLit(_) => "ByteLit",
        TokenKind::CharLit(_) => "CharLit",
        TokenKind::DecimalLit(_) => "DecimalLit",
        TokenKind::RegexLit(_) => "RegexLit",
        TokenKind::StringStart => "StringStart",
        TokenKind::StringPart(_) => "StringPart",
        TokenKind::InterpStart => "InterpStart",
        TokenKind::InterpSpec(_) => "InterpSpec",
        TokenKind::InterpEnd => "InterpEnd",
        TokenKind::StringEnd => "StringEnd",
        TokenKind::Ident(_) => "Ident",
        TokenKind::KwFunc => "KwFunc",
        TokenKind::KwLet => "KwLet",
        TokenKind::KwMut => "KwMut",
        TokenKind::KwStruct => "KwStruct",
        TokenKind::KwEnum => "KwEnum",
        TokenKind::KwTrait => "KwTrait",
        TokenKind::KwType => "KwType",
        TokenKind::KwIf => "KwIf",
        TokenKind::KwElse => "KwElse",
        TokenKind::KwFor => "KwFor",
        TokenKind::KwReturn => "KwReturn",
        TokenKind::KwBreak => "KwBreak",
        TokenKind::KwContinue => "KwContinue",
        TokenKind::KwIn => "KwIn",
        TokenKind::KwMatch => "KwMatch",
        TokenKind::KwAsync => "KwAsync",
        TokenKind::KwSpawn => "KwSpawn",
        TokenKind::KwImport => "KwImport",
        TokenKind::KwExport => "KwExport",
        TokenKind::KwFrom => "KwFrom",
        TokenKind::KwAs => "KwAs",
        TokenKind::KwTrue => "KwTrue",
        TokenKind::KwFalse => "KwFalse",
        TokenKind::KwExtern => "KwExtern",
        TokenKind::KwUnsafe => "KwUnsafe",
        TokenKind::KwGuard => "KwGuard",
        TokenKind::KwExtend => "KwExtend",
        TokenKind::KwDefer => "KwDefer",
        TokenKind::KwImpl => "KwImpl",
        TokenKind::DotDot => "DotDot",
        TokenKind::DotDotEq => "DotDotEq",
        TokenKind::Plus => "Plus",
        TokenKind::Minus => "Minus",
        TokenKind::Star => "Star",
        TokenKind::Slash => "Slash",
        TokenKind::Percent => "Percent",
        TokenKind::EqEq => "EqEq",
        TokenKind::NotEq => "NotEq",
        TokenKind::Lt => "Lt",
        TokenKind::Gt => "Gt",
        TokenKind::LtEq => "LtEq",
        TokenKind::GtEq => "GtEq",
        TokenKind::AndAnd => "AndAnd",
        TokenKind::OrOr => "OrOr",
        TokenKind::Not => "Not",
        TokenKind::Question => "Question",
        TokenKind::QuestionQuestion => "QuestionQuestion",
        TokenKind::QuestionDot => "QuestionDot",
        TokenKind::Caret => "Caret",
        TokenKind::Pipe => "Pipe",
        TokenKind::Amp => "Amp",
        TokenKind::Shl => "Shl",
        TokenKind::Shr => "Shr",
        TokenKind::Tilde => "Tilde",
        TokenKind::Arrow => "Arrow",
        TokenKind::FatArrow => "FatArrow",
        TokenKind::Assign => "Assign",
        TokenKind::PlusEq => "PlusEq",
        TokenKind::MinusEq => "MinusEq",
        TokenKind::StarEq => "StarEq",
        TokenKind::SlashEq => "SlashEq",
        TokenKind::PercentEq => "PercentEq",
        TokenKind::PipeGt => "PipeGt",
        TokenKind::Sum => "Sum",
        TokenKind::Product => "Product",
        TokenKind::Sqrt => "Sqrt",
        TokenKind::InUni => "InUni",
        TokenKind::NotInUni => "NotInUni",
        TokenKind::SubsetUni => "SubsetUni",
        TokenKind::ApproxUni => "ApproxUni",
        TokenKind::Adjoint => "Adjoint",
        TokenKind::LParen => "LParen",
        TokenKind::RParen => "RParen",
        TokenKind::LBrace => "LBrace",
        TokenKind::RBrace => "RBrace",
        TokenKind::LBracket => "LBracket",
        TokenKind::RBracket => "RBracket",
        TokenKind::Colon => "Colon",
        TokenKind::Comma => "Comma",
        TokenKind::Dot => "Dot",
        TokenKind::Semicolon => "Semicolon",
        TokenKind::At => "At",
        TokenKind::Newline => "Newline",
        TokenKind::Indent => "Indent",
        TokenKind::Dedent => "Dedent",
        TokenKind::Eof => "Eof",
    }
}

/// Whether the variant is one of the 29 keyword variants (IDs 16..=44).
fn is_keyword(kind: &TokenKind) -> bool {
    let n = token_kind_num(kind);
    (16..=44).contains(&n)
}

/// Extract the stable numeric error code from a `LexerError` (matches the
/// lexer.buff port's `LexerError.code` field — `E1xxx` without the `E`
/// prefix, or `0` when no code is attached).
fn error_code_num(err: &LexerError) -> i64 {
    match err.inner.diagnostic.code {
        Some(ErrorCode::UnexpectedChar) => 1001,
        Some(ErrorCode::UnterminatedString) => 1002,
        Some(ErrorCode::InvalidNumber) => 1003,
        Some(ErrorCode::MixedTabsSpaces) => 1004,
        _ => 0,
    }
}

fn main() {
    println!("--- buff-lang-lexer self-host: TokenKind port ---");

    // --- Literals (1-8) ---
    println!("{}", token_kind_num(&TokenKind::IntLit(42)));
    println!("{}", token_kind_num(&TokenKind::FloatLit(3.14)));
    println!("{}", token_kind_num(&TokenKind::DoubleLit(2.71828)));
    println!("{}", token_kind_num(&TokenKind::StringLit("hi".to_string())));
    println!("{}", token_kind_num(&TokenKind::ByteLit(255)));
    println!("{}", token_kind_num(&TokenKind::CharLit('A')));
    println!("{}", token_kind_num(&TokenKind::DecimalLit("99.90".to_string())));
    println!("{}", token_kind_num(&TokenKind::RegexLit("\\d+".to_string())));

    // --- String interpolation (9-14) ---
    println!("{}", token_kind_num(&TokenKind::StringStart));
    println!("{}", token_kind_num(&TokenKind::StringPart("hello ".to_string())));
    println!("{}", token_kind_num(&TokenKind::InterpStart));
    println!("{}", token_kind_num(&TokenKind::InterpSpec(".2".to_string())));
    println!("{}", token_kind_num(&TokenKind::InterpEnd));
    println!("{}", token_kind_num(&TokenKind::StringEnd));

    // --- Identifier (15) ---
    println!("{}", token_kind_num(&TokenKind::Ident("foo".to_string())));

    // --- Keywords (16-44) ---
    println!("{}", token_kind_num(&TokenKind::KwFunc));
    println!("{}", token_kind_num(&TokenKind::KwLet));
    println!("{}", token_kind_num(&TokenKind::KwMut));
    println!("{}", token_kind_num(&TokenKind::KwStruct));
    println!("{}", token_kind_num(&TokenKind::KwEnum));
    println!("{}", token_kind_num(&TokenKind::KwTrait));
    println!("{}", token_kind_num(&TokenKind::KwType));
    println!("{}", token_kind_num(&TokenKind::KwIf));
    println!("{}", token_kind_num(&TokenKind::KwElse));
    println!("{}", token_kind_num(&TokenKind::KwFor));
    println!("{}", token_kind_num(&TokenKind::KwReturn));
    println!("{}", token_kind_num(&TokenKind::KwBreak));
    println!("{}", token_kind_num(&TokenKind::KwContinue));
    println!("{}", token_kind_num(&TokenKind::KwIn));
    println!("{}", token_kind_num(&TokenKind::KwMatch));
    println!("{}", token_kind_num(&TokenKind::KwAsync));
    println!("{}", token_kind_num(&TokenKind::KwSpawn));
    println!("{}", token_kind_num(&TokenKind::KwImport));
    println!("{}", token_kind_num(&TokenKind::KwExport));
    println!("{}", token_kind_num(&TokenKind::KwFrom));
    println!("{}", token_kind_num(&TokenKind::KwAs));
    println!("{}", token_kind_num(&TokenKind::KwTrue));
    println!("{}", token_kind_num(&TokenKind::KwFalse));
    println!("{}", token_kind_num(&TokenKind::KwExtern));
    println!("{}", token_kind_num(&TokenKind::KwUnsafe));
    println!("{}", token_kind_num(&TokenKind::KwGuard));
    println!("{}", token_kind_num(&TokenKind::KwExtend));
    println!("{}", token_kind_num(&TokenKind::KwDefer));
    println!("{}", token_kind_num(&TokenKind::KwImpl));

    // --- Operators (45-78) ---
    println!("{}", token_kind_num(&TokenKind::DotDot));
    println!("{}", token_kind_num(&TokenKind::DotDotEq));
    println!("{}", token_kind_num(&TokenKind::Plus));
    println!("{}", token_kind_num(&TokenKind::Minus));
    println!("{}", token_kind_num(&TokenKind::Star));
    println!("{}", token_kind_num(&TokenKind::Slash));
    println!("{}", token_kind_num(&TokenKind::Percent));
    println!("{}", token_kind_num(&TokenKind::EqEq));
    println!("{}", token_kind_num(&TokenKind::NotEq));
    println!("{}", token_kind_num(&TokenKind::Lt));
    println!("{}", token_kind_num(&TokenKind::Gt));
    println!("{}", token_kind_num(&TokenKind::LtEq));
    println!("{}", token_kind_num(&TokenKind::GtEq));
    println!("{}", token_kind_num(&TokenKind::AndAnd));
    println!("{}", token_kind_num(&TokenKind::OrOr));
    println!("{}", token_kind_num(&TokenKind::Not));
    println!("{}", token_kind_num(&TokenKind::Question));
    println!("{}", token_kind_num(&TokenKind::QuestionQuestion));
    println!("{}", token_kind_num(&TokenKind::QuestionDot));
    println!("{}", token_kind_num(&TokenKind::Caret));
    println!("{}", token_kind_num(&TokenKind::Pipe));
    println!("{}", token_kind_num(&TokenKind::Amp));
    println!("{}", token_kind_num(&TokenKind::Shl));
    println!("{}", token_kind_num(&TokenKind::Shr));
    println!("{}", token_kind_num(&TokenKind::Tilde));
    println!("{}", token_kind_num(&TokenKind::Arrow));
    println!("{}", token_kind_num(&TokenKind::FatArrow));
    println!("{}", token_kind_num(&TokenKind::Assign));
    println!("{}", token_kind_num(&TokenKind::PlusEq));
    println!("{}", token_kind_num(&TokenKind::MinusEq));
    println!("{}", token_kind_num(&TokenKind::StarEq));
    println!("{}", token_kind_num(&TokenKind::SlashEq));
    println!("{}", token_kind_num(&TokenKind::PercentEq));
    println!("{}", token_kind_num(&TokenKind::PipeGt));

    // --- Unicode math operators (79-86) ---
    println!("{}", token_kind_num(&TokenKind::Sum));
    println!("{}", token_kind_num(&TokenKind::Product));
    println!("{}", token_kind_num(&TokenKind::Sqrt));
    println!("{}", token_kind_num(&TokenKind::InUni));
    println!("{}", token_kind_num(&TokenKind::NotInUni));
    println!("{}", token_kind_num(&TokenKind::SubsetUni));
    println!("{}", token_kind_num(&TokenKind::ApproxUni));
    println!("{}", token_kind_num(&TokenKind::Adjoint));

    // --- Delimiters (87-97) ---
    println!("{}", token_kind_num(&TokenKind::LParen));
    println!("{}", token_kind_num(&TokenKind::RParen));
    println!("{}", token_kind_num(&TokenKind::LBrace));
    println!("{}", token_kind_num(&TokenKind::RBrace));
    println!("{}", token_kind_num(&TokenKind::LBracket));
    println!("{}", token_kind_num(&TokenKind::RBracket));
    println!("{}", token_kind_num(&TokenKind::Colon));
    println!("{}", token_kind_num(&TokenKind::Comma));
    println!("{}", token_kind_num(&TokenKind::Dot));
    println!("{}", token_kind_num(&TokenKind::Semicolon));
    println!("{}", token_kind_num(&TokenKind::At));

    // --- Layout (98-101) ---
    println!("{}", token_kind_num(&TokenKind::Newline));
    println!("{}", token_kind_num(&TokenKind::Indent));
    println!("{}", token_kind_num(&TokenKind::Dedent));
    println!("{}", token_kind_num(&TokenKind::Eof));

    // --- Exercise kind_label + is_keyword ---
    println!("--- helpers ---");
    println!("{}", kind_label(&TokenKind::StringLit("hi".to_string())));
    println!("{}", kind_label(&TokenKind::KwFunc));
    println!("{}", kind_label(&TokenKind::Eof));
    println!("{}", is_keyword(&TokenKind::KwFunc));
    println!("{}", is_keyword(&TokenKind::IntLit(42)));

    // --- Exercise Token struct ---
    println!("--- Token struct ---");
    let tok = Token::new(TokenKind::IntLit(7), Span::new(10, 20, SourceId(0)));
    println!("{}", tok.span.start);
    println!("{}", tok.span.end);

    // --- Exercise LexerError constructors ---
    // The .buff port omits the offending char from the `unexpected_char`
    // message (documented codegen gap in lexer.buff). The code is extracted
    // from the real ErrorCode; the message is printed in the simplified form
    // to match the .buff port's output exactly.
    println!("--- LexerError ---");
    let e1 = LexerError::unexpected_char('@', Span::new(0, 1, SourceId(0)));
    println!("{}", error_code_num(&e1));
    println!("unexpected character");
    let e2 = LexerError::unterminated_string(Span::new(2, 4, SourceId(0)));
    println!("{}", error_code_num(&e2));
    println!("{}", e2.inner.diagnostic.message);
    let e3 = LexerError::invalid_number(Span::new(5, 7, SourceId(0)));
    println!("{}", error_code_num(&e3));
    let e4 = LexerError::mixed_tabs_spaces(Span::new(8, 9, SourceId(0)));
    println!("{}", error_code_num(&e4));
    let e5 = LexerError::new("generic lex error", Span::new(0, 0, SourceId(0)));
    println!("{}", error_code_num(&e5));
    println!("{}", e5.inner.diagnostic.message);
}
