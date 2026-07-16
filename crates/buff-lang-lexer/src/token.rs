//! Token types for the Buff lexer.
//!
//! Defines [`TokenKind`] (all token variants), [`Token`] (kind + span),
//! keyword helpers, and [`Display`] formatting.

use std::fmt;

use buff_lang_error::Span;

/// All token kinds produced by the Buff lexer.
///
/// NOTE: Does not derive `Eq` because [`FloatLit`](TokenKind::FloatLit) and
/// [`DoubleLit`](TokenKind::DoubleLit) contain `f32`/`f64` which do not
/// implement `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // --- Literals ---
    IntLit(i64),
    FloatLit(f32),
    DoubleLit(f64),
    StringLit(String),
    ByteLit(u8),

    // --- String interpolation tokens ---
    StringStart,
    StringPart(String),
    InterpStart,
    InterpEnd,
    StringEnd,

    // --- Identifiers and keywords ---
    Ident(String),

    // 25 keywords
    KwFunc,
    KwLet,
    KwMut,
    KwStruct,
    KwEnum,
    KwTrait,
    KwType,
    KwIf,
    KwElse,
    KwFor,
    KwReturn,
    KwBreak,
    KwContinue,
    KwIn,
    KwMatch,
    KwAsync,
    KwSpawn,
    KwImport,
    KwExport,
    KwFrom,
    KwAs,
    KwTrue,
    KwFalse,
    KwExtern,
    KwUnsafe,

    // --- Operators ---
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AndAnd,
    OrOr,
    Not,
    Question,
    Caret,
    Pipe,
    Amp,
    Shl,
    Shr,
    Tilde,
    Arrow,
    FatArrow,
    Assign,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,

    // --- Delimiters ---
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Dot,
    Semicolon,
    At,

    // --- Layout tokens ---
    Newline,
    Indent,
    Dedent,
    Eof,
}

impl TokenKind {
    /// Returns `Some(keyword_token)` if `s` is a reserved keyword, else `None`.
    pub fn from_keyword(s: &str) -> Option<TokenKind> {
        match s {
            "func" => Some(Self::KwFunc),
            "let" => Some(Self::KwLet),
            "mut" => Some(Self::KwMut),
            "struct" => Some(Self::KwStruct),
            "enum" => Some(Self::KwEnum),
            "trait" => Some(Self::KwTrait),
            "type" => Some(Self::KwType),
            "if" => Some(Self::KwIf),
            "else" => Some(Self::KwElse),
            "for" => Some(Self::KwFor),
            "return" => Some(Self::KwReturn),
            "break" => Some(Self::KwBreak),
            "continue" => Some(Self::KwContinue),
            "in" => Some(Self::KwIn),
            "match" => Some(Self::KwMatch),
            "async" => Some(Self::KwAsync),
            "spawn" => Some(Self::KwSpawn),
            "import" => Some(Self::KwImport),
            "export" => Some(Self::KwExport),
            "from" => Some(Self::KwFrom),
            "as" => Some(Self::KwAs),
            "true" => Some(Self::KwTrue),
            "false" => Some(Self::KwFalse),
            "extern" => Some(Self::KwExtern),
            "unsafe" => Some(Self::KwUnsafe),
            _ => None,
        }
    }

    /// Is this a reserved keyword token?
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            Self::KwFunc
                | Self::KwLet
                | Self::KwMut
                | Self::KwStruct
                | Self::KwEnum
                | Self::KwTrait
                | Self::KwType
                | Self::KwIf
                | Self::KwElse
                | Self::KwFor
                | Self::KwReturn
                | Self::KwBreak
                | Self::KwContinue
                | Self::KwIn
                | Self::KwMatch
                | Self::KwAsync
                | Self::KwSpawn
                | Self::KwImport
                | Self::KwExport
                | Self::KwFrom
                | Self::KwAs
                | Self::KwTrue
                | Self::KwFalse
                | Self::KwExtern
                | Self::KwUnsafe
        )
    }

    /// All 25 reserved keywords as a slice of string literals.
    pub fn all_keywords() -> &'static [&'static str] {
        &[
            "func", "let", "mut", "struct", "enum", "trait", "type", "if", "else", "for", "return",
            "break", "continue", "in", "match", "async", "spawn", "import", "export", "from", "as",
            "true", "false", "extern", "unsafe",
        ]
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Literals
            Self::IntLit(v) => write!(f, "int({})", v),
            Self::FloatLit(v) => write!(f, "float({})", v),
            Self::DoubleLit(v) => write!(f, "double({})", v),
            Self::StringLit(v) => write!(f, "string({:?})", v),
            Self::ByteLit(v) => write!(f, "byte({})", v),
            // String interpolation
            Self::StringStart => write!(f, "string_start"),
            Self::StringPart(v) => write!(f, "string_part({:?})", v),
            Self::InterpStart => write!(f, "interp_start"),
            Self::InterpEnd => write!(f, "interp_end"),
            Self::StringEnd => write!(f, "string_end"),
            // Identifiers
            Self::Ident(v) => write!(f, "ident({})", v),
            // Keywords
            Self::KwFunc => write!(f, "func"),
            Self::KwLet => write!(f, "let"),
            Self::KwMut => write!(f, "mut"),
            Self::KwStruct => write!(f, "struct"),
            Self::KwEnum => write!(f, "enum"),
            Self::KwTrait => write!(f, "trait"),
            Self::KwType => write!(f, "type"),
            Self::KwIf => write!(f, "if"),
            Self::KwElse => write!(f, "else"),
            Self::KwFor => write!(f, "for"),
            Self::KwReturn => write!(f, "return"),
            Self::KwBreak => write!(f, "break"),
            Self::KwContinue => write!(f, "continue"),
            Self::KwIn => write!(f, "in"),
            Self::KwMatch => write!(f, "match"),
            Self::KwAsync => write!(f, "async"),
            Self::KwSpawn => write!(f, "spawn"),
            Self::KwImport => write!(f, "import"),
            Self::KwExport => write!(f, "export"),
            Self::KwFrom => write!(f, "from"),
            Self::KwAs => write!(f, "as"),
            Self::KwTrue => write!(f, "true"),
            Self::KwFalse => write!(f, "false"),
            Self::KwExtern => write!(f, "extern"),
            Self::KwUnsafe => write!(f, "unsafe"),
            // Operators
            Self::Plus => write!(f, "+"),
            Self::Minus => write!(f, "-"),
            Self::Star => write!(f, "*"),
            Self::Slash => write!(f, "/"),
            Self::Percent => write!(f, "%"),
            Self::EqEq => write!(f, "=="),
            Self::NotEq => write!(f, "!="),
            Self::Lt => write!(f, "<"),
            Self::Gt => write!(f, ">"),
            Self::LtEq => write!(f, "<="),
            Self::GtEq => write!(f, ">="),
            Self::AndAnd => write!(f, "&&"),
            Self::OrOr => write!(f, "||"),
            Self::Not => write!(f, "!"),
            Self::Question => write!(f, "?"),
            Self::Caret => write!(f, "^"),
            Self::Pipe => write!(f, "|"),
            Self::Amp => write!(f, "&"),
            Self::Shl => write!(f, "<<"),
            Self::Shr => write!(f, ">>"),
            Self::Tilde => write!(f, "~"),
            Self::Arrow => write!(f, "->"),
            Self::FatArrow => write!(f, "=>"),
            Self::Assign => write!(f, "="),
            Self::PlusEq => write!(f, "+="),
            Self::MinusEq => write!(f, "-="),
            Self::StarEq => write!(f, "*="),
            Self::SlashEq => write!(f, "/="),
            Self::PercentEq => write!(f, "%="),
            // Delimiters
            Self::LParen => write!(f, "("),
            Self::RParen => write!(f, ")"),
            Self::LBrace => write!(f, "{{"),
            Self::RBrace => write!(f, "}}"),
            Self::LBracket => write!(f, "["),
            Self::RBracket => write!(f, "]"),
            Self::Colon => write!(f, ":"),
            Self::Comma => write!(f, ","),
            Self::Dot => write!(f, "."),
            Self::Semicolon => write!(f, ";"),
            Self::At => write!(f, "@"),
            // Layout
            Self::Newline => write!(f, "newline"),
            Self::Indent => write!(f, "indent"),
            Self::Dedent => write!(f, "dedent"),
            Self::Eof => write!(f, "eof"),
        }
    }
}

/// A single token produced by the lexer.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}
