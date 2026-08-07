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
    /// A single Unicode scalar value literal, e.g. `'A'`, `'é'`, `'🚀'` (T21).
    ///
    /// Stored as a Rust [`char`] (a Unicode scalar value). Multi-byte UTF-8
    /// sequences up to one scalar value are supported; combining marks and
    /// grapheme clusters are NOT (those are Strings).
    CharLit(char),
    /// A 128-bit fixed-point decimal literal, e.g. `99.90m` (T20).
    ///
    /// Stores the **raw source text** of the digits (including the decimal
    /// point but **excluding** the trailing `m`/`M` suffix), e.g. `"99.90"`.
    /// Carrying the raw text avoids any rounding through `f32`/`f64` so the
    /// exact value survives to the `rust_decimal_macros::dec!()` codegen.
    DecimalLit(String),
    /// A regex literal, e.g. `/\d+/`, `/\d{3}-\d{4}/` (T79).
    ///
    /// Stores the **raw source text between the slashes** (excluding both
    /// delimiters), so backslash classes survive verbatim (`/\d+/` →
    /// `RegexLit("\\d+")`). An escaped slash `\/` inside the pattern does NOT
    /// terminate the literal — the `\` escapes the next byte in the scanner,
    /// so `a\/b` is captured as `a\/b` (the backslash is preserved in the
    /// stored text). Flags (e.g. `/abc/gi`) are NOT supported in v0.5 and are
    /// deferred — only `/pattern/` is lexed today.
    ///
    /// The `/`-disambiguation (division vs regex) happens in the lexer via a
    /// JS/Perl-style "previous significant token" heuristic: a regex is valid
    /// where an expression primary is expected (after `(`, `,`, `=`,
    /// operators, keywords like `return`/`if`, or at the start of input);
    /// division is valid between two expressions.
    RegexLit(String),

    // --- String interpolation tokens ---
    StringStart,
    StringPart(String),
    InterpStart,
    /// A format specifier inside `${expr:spec}` (T81).
    ///
    /// Emitted by the string-interpolation scanner when it finds a `:` at
    /// brace-depth 0 between the opening `{` and the matching `}`. The
    /// payload is the raw spec text (everything between `:` and `}`),
    /// e.g. `.2`, `?`, `>10`, `x`, `b`, `o`, `e`, `05`.
    ///
    /// The parser attaches this to the preceding `InterpPart::Expr` and
    /// codegen passes it through to Rust's `format!("{spec}", expr)`.
    InterpSpec(String),
    InterpEnd,
    StringEnd,

    // --- Identifiers and keywords ---
    Ident(String),

    // 28 keywords
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
    /// `while cond { body }` conditional loop (BUG-9). Mirrors
    /// [`Stmt::ForWhile`](buff_lang_ast::Stmt::ForWhile) (the existing
    /// `for cond { body }` form) but spells the loop with the conventional
    /// `while` keyword so users don't get confusing "unexpected `while`"
    /// errors when they reach for the natural spelling.
    KwWhile,
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
    /// `guard` early-return statement (T73).
    ///
    /// `guard <cond>[, <cond>...] else { <diverging-block> }` runs the
    /// else-block (which must diverge via `return`/`break`/`continue`) when
    /// ANY condition fails; otherwise execution continues. A `let PATTERN =
    /// expr` condition additionally binds the pattern's identifiers in the
    /// enclosing scope (lowered to Rust's let-else).
    KwGuard,
    /// `extend TYPE { fn ...; ... }` extension-method block (T75).
    ///
    /// Adds methods to an existing type (primitive or user-defined) by
    /// lowering to a Rust "extension trait" + blanket-free impl. The trait
    /// name is derived from the target type as `BuffExt{Type}` (e.g.
    /// `extend String { ... }` → `trait BuffExtString { ... }`).
    KwExtend,
    /// `defer EXPR` deferred-execution statement (T100).
    ///
    /// Schedules `EXPR` to run when the ENCLOSING FUNCTION exits — on ANY
    /// exit path (return, fall-through end). Multiple defers run LIFO
    /// (last-registered first). The codegen collects deferred expressions
    /// during lowering and emits them in reverse order at every function
    /// exit point (each `return` and the implicit fall-through at the body
    /// end). A single expression is deferred in v0.5; a deferred BLOCK is a
    /// future extension.
    KwDefer,
    /// `impl Trait for Type { ... }` trait-implementation block (T75b —
    /// associated types in traits).
    ///
    /// Implements a declared [`buff_lang_ast::TraitDecl`] for a target type,
    /// supplying bodies for required methods and bindings for associated
    /// types. The body uses braces (same as `trait`/`extend`) and contains
    /// a mix of:
    ///
    /// - `type Item = T;` — associated-type bindings (one per associated
    ///   type declared by the trait).
    /// - `func name(...) -> Ret { body }` — method implementations (one
    ///   per required method; default methods may be overridden).
    ///
    /// Lowers to a Rust `syn::ItemImpl` with `trait_` set to
    /// `Some((None, Path, For))` so it is a trait-impl (not an inherent
    /// impl). Associated-type bindings become `syn::ImplItem::AssocType`
    /// entries; method impls become `syn::ImplItem::Fn`.
    KwImpl,

    // --- Operators ---
    DotDot,
    DotDotEq,
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
    QuestionQuestion,
    /// The null-conditional operator `?.` (T70).
    ///
    /// `receiver ?. name` desugars in the parser to
    /// `receiver.and_then(|x| x.name)` — an `Option`-chain with short-circuit
    /// semantics. Chaining `a?.b?.c` nests left-associatively. The desugar
    /// happens entirely in the parser (no new AST variant), so this token
    /// never reaches codegen. Lexed as a 2-char operator BEFORE the single
    /// `?` ([`TokenKind::Question`]) so `?.` is matched greedily instead of
    /// splitting into `?` + `.` (which would parse as `Try` then a stray
    /// field access).
    QuestionDot,
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
    /// The pipeline operator `|>` (T69).
    ///
    /// `LHS |> f(args...)` desugars to `f(LHS, args...)` — the left operand
    /// is inserted as the FIRST argument of the right-hand call. The desugar
    /// happens entirely in the parser (no new AST variant), so this token
    /// never reaches codegen. Lexed as a 2-char operator BEFORE the single
    /// `|` ([`TokenKind::Pipe`]) so `|>` is matched greedily instead of
    /// splitting into `|` + `>` (which would parse as `Try` then a stray
    /// field access).
    PipeGt,

    // --- T57: Unicode mathematical operators (scientific edition) ---
    //
    // These are ALWAYS lexed (the lexer is edition-agnostic), but the parser
    // accepts them ONLY when `edition = "scientific"` is active. In the
    // default Standard edition, encountering one is a parse error pointing
    // the user at the edition opt-in. ASCII alternatives are documented per
    // variant — Buff never FORCES users to type Unicode.
    /// `∑` (U+2211 N-ARY SUMMATION). ASCII alternative: `sum(...)`. Parser
    /// desugars `∑ expr` to `sum(expr)` (a free prelude function call).
    Sum,
    /// `∏` (U+220F N-ARY PRODUCT). ASCII alternative: `product(...)`. Parser
    /// desugars `∏ expr` to `product(expr)`.
    Product,
    /// `√` (U+221A SQUARE ROOT). ASCII alternative: `sqrt(...)`. Parser
    /// desugars `√ expr` to `sqrt(expr)`.
    Sqrt,
    /// `∈` (U+2208 ELEMENT OF). ASCII alternative: the `in` keyword. Aliases
    /// `KwIn` at parse time.
    InUni,
    /// `∉` (U+2209 NOT AN ELEMENT OF). ASCII alternative: `not in`. Parser
    /// treats it as the negated membership test.
    NotInUni,
    /// `⊂` (U+2282 SUBSET OF). ASCII alternative: `.is_subset(...)` method
    /// call. Parser desugars `a ⊂ b` to `a.is_subset(b)`.
    SubsetUni,
    /// `≈` (U+2248 ALMOST EQUAL TO). ASCII alternative: `==` (with tolerance
    /// applied at the type-system level — currently a direct alias of `EqEq`
    /// semantics). Parser lowers to `BinaryOp::Eq`.
    ApproxUni,
    /// Postfix adjoint/transpose operator `'` (U+0027 APOSTROPHE) — T57.
    ///
    /// The lexer is context-sensitive about `'`: when the previous
    /// significant token is expression-ending (Ident, literal, `)`, `]`),
    /// the apostrophe is emitted as `Adjoint`; otherwise it begins a
    /// [`CharLit`](TokenKind::CharLit). ASCII alternative: `.transpose()`
    /// method call. Parser desugars `A'` to `A.transpose()`.
    Adjoint,

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
            "while" => Some(Self::KwWhile),
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
            "guard" => Some(Self::KwGuard),
            "extend" => Some(Self::KwExtend),
            "defer" => Some(Self::KwDefer),
            "impl" => Some(Self::KwImpl),
            // BUG-4: word aliases for the symbolic logic operators. `and`,
            // `or`, `not` map to the EXISTING operator tokens (AndAnd/OrOr/
            // Not) so the Pratt parser and Rust codegen handle them with
            // zero changes — they produce the identical AST and identical
            // Rust output (`&&`/`||`/`!`). They are deliberately NOT in
            // [`all_keywords`](Self::all_keywords) / [`is_keyword`](Self::is_keyword):
            // they are operator aliases (Python/SQL-style), not reserved
            // keyword token kinds. Precedence mirrors the symbolic forms and
            // Python: `not` (unary, tightest) > `and` (binary) > `or` (binary,
            // loosest). The lexer always emits the operator token for these
            // words, so a user cannot use `and`/`or`/`not` as identifiers.
            "and" => Some(Self::AndAnd),
            "or" => Some(Self::OrOr),
            "not" => Some(Self::Not),
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
                | Self::KwWhile
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
                | Self::KwGuard
                | Self::KwExtend
                | Self::KwDefer
                | Self::KwImpl
        )
    }

    /// All reserved keywords as a slice of string literals.
    pub fn all_keywords() -> &'static [&'static str] {
        &[
            "func", "let", "mut", "struct", "enum", "trait", "type", "if", "else", "for", "while",
            "return", "break", "continue", "in", "match", "async", "spawn", "import", "export",
            "from", "as", "true", "false", "extern", "unsafe", "guard", "extend", "defer", "impl",
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
            Self::CharLit(c) => write!(f, "char({:?})", c),
            Self::DecimalLit(v) => write!(f, "decimal({:?})", v),
            // T79: render the raw pattern double-quoted, consistent with
            // StringLit / DecimalLit so the diagnostic is unambiguous.
            Self::RegexLit(v) => write!(f, "regex({:?})", v),
            // String interpolation
            Self::StringStart => write!(f, "string_start"),
            Self::StringPart(v) => write!(f, "string_part({:?})", v),
            Self::InterpStart => write!(f, "interp_start"),
            Self::InterpSpec(v) => write!(f, "interp_spec({:?})", v),
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
            Self::KwWhile => write!(f, "while"),
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
            Self::KwGuard => write!(f, "guard"),
            Self::KwExtend => write!(f, "extend"),
            Self::KwDefer => write!(f, "defer"),
            Self::KwImpl => write!(f, "impl"),
            // Operators
            Self::DotDot => write!(f, ".."),
            Self::DotDotEq => write!(f, "..="),
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
            Self::QuestionQuestion => write!(f, "??"),
            // T70: null-conditional operator `?.`.
            Self::QuestionDot => write!(f, "?."),
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
            // T69: pipeline operator `|>`.
            Self::PipeGt => write!(f, "|>"),
            // T57: Unicode mathematical operators.
            Self::Sum => write!(f, "∑"),
            Self::Product => write!(f, "∏"),
            Self::Sqrt => write!(f, "√"),
            Self::InUni => write!(f, "∈"),
            Self::NotInUni => write!(f, "∉"),
            Self::SubsetUni => write!(f, "⊂"),
            Self::ApproxUni => write!(f, "≈"),
            Self::Adjoint => write!(f, "'"),
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
