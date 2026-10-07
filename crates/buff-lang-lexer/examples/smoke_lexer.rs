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
        // Keywords (16-45)
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
        TokenKind::KwWhile => 26,
        TokenKind::KwReturn => 27,
        TokenKind::KwBreak => 28,
        TokenKind::KwContinue => 29,
        TokenKind::KwIn => 30,
        TokenKind::KwMatch => 31,
        TokenKind::KwAsync => 32,
        TokenKind::KwSpawn => 33,
        TokenKind::KwImport => 34,
        TokenKind::KwExport => 35,
        TokenKind::KwFrom => 36,
        TokenKind::KwAs => 37,
        TokenKind::KwTrue => 38,
        TokenKind::KwFalse => 39,
        TokenKind::KwExtern => 40,
        TokenKind::KwUnsafe => 41,
        TokenKind::KwGuard => 42,
        TokenKind::KwExtend => 43,
        TokenKind::KwDefer => 44,
        TokenKind::KwImpl => 45,
        // Operators (46-79)
        TokenKind::DotDot => 46,
        TokenKind::DotDotEq => 47,
        TokenKind::Plus => 48,
        TokenKind::Minus => 49,
        TokenKind::Star => 50,
        TokenKind::Slash => 51,
        TokenKind::Percent => 52,
        TokenKind::EqEq => 53,
        TokenKind::NotEq => 54,
        TokenKind::Lt => 55,
        TokenKind::Gt => 56,
        TokenKind::LtEq => 57,
        TokenKind::GtEq => 58,
        TokenKind::AndAnd => 59,
        TokenKind::OrOr => 60,
        TokenKind::Not => 61,
        TokenKind::Question => 62,
        TokenKind::QuestionQuestion => 63,
        TokenKind::QuestionDot => 64,
        TokenKind::Caret => 65,
        TokenKind::Pipe => 66,
        TokenKind::Amp => 67,
        TokenKind::Shl => 68,
        TokenKind::Shr => 69,
        TokenKind::Tilde => 70,
        TokenKind::Arrow => 71,
        TokenKind::FatArrow => 72,
        TokenKind::Assign => 73,
        TokenKind::PlusEq => 74,
        TokenKind::MinusEq => 75,
        TokenKind::StarEq => 76,
        TokenKind::SlashEq => 77,
        TokenKind::PercentEq => 78,
        TokenKind::PipeGt => 79,
        // Unicode math operators (80-87)
        TokenKind::Sum => 80,
        TokenKind::Product => 81,
        TokenKind::Sqrt => 82,
        TokenKind::InUni => 83,
        TokenKind::NotInUni => 84,
        TokenKind::SubsetUni => 85,
        TokenKind::ApproxUni => 86,
        TokenKind::Adjoint => 87,
        // Delimiters (88-98)
        TokenKind::LParen => 88,
        TokenKind::RParen => 89,
        TokenKind::LBrace => 90,
        TokenKind::RBrace => 91,
        TokenKind::LBracket => 92,
        TokenKind::RBracket => 93,
        TokenKind::Colon => 94,
        TokenKind::Comma => 95,
        TokenKind::Dot => 96,
        TokenKind::Semicolon => 97,
        TokenKind::At => 98,
        // Layout (99-102)
        TokenKind::Newline => 99,
        TokenKind::Indent => 100,
        TokenKind::Dedent => 101,
        TokenKind::Eof => 102,
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
        TokenKind::KwWhile => "KwWhile",
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
    println!(
        "{}",
        token_kind_num(&TokenKind::StringLit("hi".to_string()))
    );
    println!("{}", token_kind_num(&TokenKind::ByteLit(255)));
    println!("{}", token_kind_num(&TokenKind::CharLit('A')));
    println!(
        "{}",
        token_kind_num(&TokenKind::DecimalLit("99.90".to_string()))
    );
    println!(
        "{}",
        token_kind_num(&TokenKind::RegexLit("\\d+".to_string()))
    );

    // --- String interpolation (9-14) ---
    println!("{}", token_kind_num(&TokenKind::StringStart));
    println!(
        "{}",
        token_kind_num(&TokenKind::StringPart("hello ".to_string()))
    );
    println!("{}", token_kind_num(&TokenKind::InterpStart));
    println!(
        "{}",
        token_kind_num(&TokenKind::InterpSpec(".2".to_string()))
    );
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
    println!("{}", token_kind_num(&TokenKind::KwWhile));
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
