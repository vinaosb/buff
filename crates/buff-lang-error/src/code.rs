//! Stable error code registry (T124).
//!
//! Every user-facing Buff diagnostic may carry an [`ErrorCode`] that identifies
//! the failure mode with a short, stable string of the form `E1xxx` (mirroring
//! `rustc`'s `E0xxx` scheme). The code is rendered alongside the message —
//! e.g. `[Error] error[E1001]: unexpected character: '@'` — and a static site
//! under `docs/errors/` documents every code with a longer explanation, an
//! example that triggers it, and a fix recipe.
//!
//! # Numbering scheme
//!
//! Codes are grouped by compiler phase so that reading a code alone tells the
//! user which part of the pipeline produced it:
//!
//! | Range      | Phase       | Source crate(s)                                  |
//! |------------|-------------|--------------------------------------------------|
//! | `E10xx`    | Lexing      | `buff-lang-lexer`                                |
//! | `E11xx`    | Parsing     | `buff-lang-parser`                               |
//! | `E12xx`    | Type-check  | `buff-lang-types`                                |
//! | `E13xx`    | Codegen     | `buff-lang-codegen-rust`                         |
//! | `E14xx`    | Runtime     | `buff-lang-runtime` (reserved — unused today)    |
//!
//! # Stability guarantee
//!
//! **Error codes are stable across releases.** Once an `E1xxx` code ships, it
//! is never renumbered, never reused, and never silently removed. Retiring a
//! code (because the underlying error becomes impossible to trigger) leaves a
//! tombstone in this enum and on the static site; the code is never recycled
//! for a different meaning. New codes are appended at the end of their phase
//! block. This mirrors the guarantee `rustc` gives for its `E0xxx` codes —
//! see [`buff-conventions.md`] §19 for the full policy.
//!
//! [`buff-conventions.md`]: ../../.sisyphus/plans/buff-conventions.md

/// A stable error code identifying a class of Buff diagnostic.
///
/// Variants are grouped by compiler phase (see the [module docs](self)) and
/// expose three views via [`code_str`](Self::code_str),
/// [`title`](Self::title), and [`explanation`](Self::explanation). Codes are
/// intentionally a small closed enum — only failure modes the compiler
/// actually emits get a code (see T124 spec: "no speculative/aspirational
/// codes").
///
/// `ErrorCode` is `Copy` so it can be passed by value and stored alongside
/// `Diagnostic` without lifetimes or heap allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    // -----------------------------------------------------------------------
    // E10xx — Lexing (buff-lang-lexer)
    // -----------------------------------------------------------------------
    /// E1001 — Lexing hit a character it cannot start a token with.
    UnexpectedChar,
    /// E1002 — A `"..."` or `'...'` literal reached end of input without a
    /// closing quote.
    UnterminatedString,
    /// E1003 — A numeric literal failed to parse (overflow, malformed radix,
    /// bad decimal, etc.).
    InvalidNumber,
    /// E1004 — Indentation mixes tabs and spaces on the same line. Buff
    /// mandates 4-space indentation; tabs are rejected.
    MixedTabsSpaces,
    /// E1005 — A `dedent` landed on an indentation level that does not match
    /// any previously-seen scope (inconsistent dedent).
    InconsistentIndent,
    /// E1006 — A `/* ... */` block comment was opened but never closed.
    UnterminatedBlockComment,
    /// E1007 — A `/.../ ` regex literal reached end of input or end of line
    /// without a closing `/`.
    UnterminatedRegex,
    /// E1008 — A regex literal has no body (`//`).
    EmptyRegex,
    /// E1009 — A `'...'` char literal was opened but never closed.
    UnterminatedCharLiteral,
    /// E1010 — A char literal has no body (`''`).
    EmptyCharLiteral,
    /// E1011 — A character escape (`\n`, `\t`, `\u{...}`, …) inside a char or
    /// string literal is malformed or unknown.
    InvalidCharEscape,
    /// E1012 — A `\u{...}` escape's hex digits do not form a valid Unicode
    /// scalar value.
    InvalidUnicodeEscape,
    /// E1013 — A `}` appeared inside a `"..."` literal without a matching `{`.
    UnexpectedBraceInString,
    /// E1014 — A `{` interpolation inside a `"..."` literal was opened but
    /// never closed before end of input.
    UnterminatedInterpolation,

    // -----------------------------------------------------------------------
    // E11xx — Parsing (buff-lang-parser)
    // -----------------------------------------------------------------------
    /// E1101 — The parser expected a specific token but found another (or
    /// EOF). Covers the common `expected X, found Y` family.
    ExpectedToken,
    /// E1102 — The parser found a token it cannot use in the current
    /// position. Covers the `unexpected X: ...` family.
    UnexpectedToken,
    /// E1103 — After a `:` that opens a layout block, the parser did not find
    /// a required trailing newline.
    ExpectedLayoutNewline,
    /// E1104 — After a `:` that opens a layout block, the parser did not find
    /// the required indented block.
    ExpectedIndentedBlock,
    /// E1105 — A `func` declaration appeared inside a nested block; Buff
    /// requires top-level function declarations.
    FuncMustBeTopLevel,
    /// E1106 — An import, export, attribute, or extern declaration did not
    /// find the required identifier at the cursor.
    ExpectedIdentifier,
    /// E1107 — A delimited list (`{...}`, `(...)`, `<...>`) reached end of
    /// input without its closing delimiter.
    UnterminatedList,
    /// E1108 — An `extern "ABI" func ...` declaration used an ABI other than
    /// `"C"` (the only ABI supported in v1.3).
    UnsupportedAbi,
    /// E1109 — An `extern "C" func ...` declaration was given generic
    /// parameters, which Buff rejects on extern functions.
    ExternGenericsUnsupported,

    // -----------------------------------------------------------------------
    // E12xx — Type-checking (buff-lang-types)
    // -----------------------------------------------------------------------
    /// E1201 — The name was not found in scope (undefined variable,
    /// function, or symbol).
    UndefinedVariable,
    /// E1202 — A binary operator was applied to incompatible operand types
    /// (e.g. `Int + String`).
    BinaryOpTypeMismatch,
    /// E1203 — An assignment / variable binding has incompatible types on
    /// its two sides.
    AssignTypeMismatch,
    /// E1204 — A unary operator (`-`, `!`, `~`) was applied to an operand
    /// whose type the operator does not accept.
    InvalidUnaryOperand,
    /// E1205 — The condition of an `if` expression is not `Bool`.
    IfConditionMustBeBool,
    /// E1206 — The two branches of an `if`/`else` produce incompatible types.
    IfBranchTypeMismatch,
    /// E1207 — A `match` expression does not cover all possible values of
    /// its scrutinee type.
    NonExhaustiveMatch,
    /// E1208 — A function marked `@prefer(gpu)` is recursive; recursion is
    /// not allowed on GPU-bound functions.
    PreferGpuOnRecursiveFunction,
    /// E1209 — A module/import error: file not found, circular import,
    /// missing export, or unsupported stdlib path.
    ModuleError,

    // -----------------------------------------------------------------------
    // E13xx — Codegen (buff-lang-codegen-rust)
    // -----------------------------------------------------------------------
    /// E1301 — The Buff AST node has no Rust codegen implementation yet
    /// (unsupported language feature in the current version).
    UnsupportedCodegen,
    /// E1302 — Codegen produced a Rust token stream that `syn` refused to
    /// parse back into an AST. This is an internal compiler error surfaced
    /// as a user diagnostic for triage.
    CodegenParseError,
    /// E1303 — `block()` was called inside an `async func`. This is a
    /// *warning* — codegen still emits the call, but it can deadlock the
    /// single-threaded async runtime.
    AsyncBlockDeadlock,
}

impl ErrorCode {
    /// The canonical short string for this code, e.g. `"E1001"`.
    ///
    /// Stable across releases — see the [module docs](self) for the policy.
    pub fn code_str(self) -> &'static str {
        match self {
            // E10xx — Lexing
            ErrorCode::UnexpectedChar => "E1001",
            ErrorCode::UnterminatedString => "E1002",
            ErrorCode::InvalidNumber => "E1003",
            ErrorCode::MixedTabsSpaces => "E1004",
            ErrorCode::InconsistentIndent => "E1005",
            ErrorCode::UnterminatedBlockComment => "E1006",
            ErrorCode::UnterminatedRegex => "E1007",
            ErrorCode::EmptyRegex => "E1008",
            ErrorCode::UnterminatedCharLiteral => "E1009",
            ErrorCode::EmptyCharLiteral => "E1010",
            ErrorCode::InvalidCharEscape => "E1011",
            ErrorCode::InvalidUnicodeEscape => "E1012",
            ErrorCode::UnexpectedBraceInString => "E1013",
            ErrorCode::UnterminatedInterpolation => "E1014",
            // E11xx — Parsing
            ErrorCode::ExpectedToken => "E1101",
            ErrorCode::UnexpectedToken => "E1102",
            ErrorCode::ExpectedLayoutNewline => "E1103",
            ErrorCode::ExpectedIndentedBlock => "E1104",
            ErrorCode::FuncMustBeTopLevel => "E1105",
            ErrorCode::ExpectedIdentifier => "E1106",
            ErrorCode::UnterminatedList => "E1107",
            ErrorCode::UnsupportedAbi => "E1108",
            ErrorCode::ExternGenericsUnsupported => "E1109",
            // E12xx — Type-checking
            ErrorCode::UndefinedVariable => "E1201",
            ErrorCode::BinaryOpTypeMismatch => "E1202",
            ErrorCode::AssignTypeMismatch => "E1203",
            ErrorCode::InvalidUnaryOperand => "E1204",
            ErrorCode::IfConditionMustBeBool => "E1205",
            ErrorCode::IfBranchTypeMismatch => "E1206",
            ErrorCode::NonExhaustiveMatch => "E1207",
            ErrorCode::PreferGpuOnRecursiveFunction => "E1208",
            ErrorCode::ModuleError => "E1209",
            // E13xx — Codegen
            ErrorCode::UnsupportedCodegen => "E1301",
            ErrorCode::CodegenParseError => "E1302",
            ErrorCode::AsyncBlockDeadlock => "E1303",
        }
    }

    /// A short, single-line human description of this code (no trailing
    /// period, no `E1xxx` prefix — the prefix is added by
    /// [`code_str`](Self::code_str)).
    pub fn title(self) -> &'static str {
        match self {
            ErrorCode::UnexpectedChar => "unexpected character",
            ErrorCode::UnterminatedString => "unterminated string literal",
            ErrorCode::InvalidNumber => "invalid numeric literal",
            ErrorCode::MixedTabsSpaces => "mixed tabs and spaces in indentation",
            ErrorCode::InconsistentIndent => "inconsistent indentation level",
            ErrorCode::UnterminatedBlockComment => "unterminated block comment",
            ErrorCode::UnterminatedRegex => "unterminated regex literal",
            ErrorCode::EmptyRegex => "empty regex literal",
            ErrorCode::UnterminatedCharLiteral => "unterminated char literal",
            ErrorCode::EmptyCharLiteral => "empty char literal",
            ErrorCode::InvalidCharEscape => "invalid character escape",
            ErrorCode::InvalidUnicodeEscape => "invalid unicode escape",
            ErrorCode::UnexpectedBraceInString => "unexpected closing brace in string literal",
            ErrorCode::UnterminatedInterpolation => "unterminated interpolation in string literal",
            ErrorCode::ExpectedToken => "expected a different token",
            ErrorCode::UnexpectedToken => "unexpected token in this position",
            ErrorCode::ExpectedLayoutNewline => "expected newline after `:` for layout block",
            ErrorCode::ExpectedIndentedBlock => "expected indented block after `:`",
            ErrorCode::FuncMustBeTopLevel => "function declarations must be top-level",
            ErrorCode::ExpectedIdentifier => "expected an identifier",
            ErrorCode::UnterminatedList => "unterminated delimited list",
            ErrorCode::UnsupportedAbi => "unsupported ABI in `extern` declaration",
            ErrorCode::ExternGenericsUnsupported => {
                "generics are not supported on `extern` functions"
            }
            ErrorCode::UndefinedVariable => "undefined variable",
            ErrorCode::BinaryOpTypeMismatch => "binary operator applied to incompatible types",
            ErrorCode::AssignTypeMismatch => "assignment type mismatch",
            ErrorCode::InvalidUnaryOperand => "unary operator applied to invalid operand type",
            ErrorCode::IfConditionMustBeBool => "`if` condition must be `Bool`",
            ErrorCode::IfBranchTypeMismatch => "`if` and `else` branches have different types",
            ErrorCode::NonExhaustiveMatch => "non-exhaustive `match`",
            ErrorCode::PreferGpuOnRecursiveFunction => {
                "`@prefer(gpu)` is not allowed on recursive functions"
            }
            ErrorCode::ModuleError => "module / import resolution error",
            ErrorCode::UnsupportedCodegen => "unsupported language feature in code generation",
            ErrorCode::CodegenParseError => {
                "codegen produced invalid rust (internal compiler error)"
            }
            ErrorCode::AsyncBlockDeadlock => "`block()` inside an async function can deadlock",
        }
    }

    /// A longer, multi-sentence explanation suitable for a documentation
    /// page. Covers what triggers the error, why Buff rejects it, and how to
    /// fix it.
    pub fn explanation(self) -> &'static str {
        match self {
            ErrorCode::UnexpectedChar => "The Buff lexer encountered a character it cannot start any token with. Buff's syntax uses a small alphabet: ASCII letters and digits, the operators listed in the reference, the layout characters (newline and ASCII space), and the string/char/regex quote characters. Any other byte — a stray `@` outside attribute position, a non-breakable space, a CJK punctuation mark copied from documentation — produces this error. Fix: delete the character or replace it with its ASCII equivalent. If you are pasting code from a blog post or PDF, retype the offending line by hand.",
            ErrorCode::UnterminatedString => "A string literal (`\"...\"`) or char literal (`'...'`) was opened but never closed before the end of the line or the end of the file. String literals in Buff cannot span lines unless you interpolate (`\"{expr}\"`) or escape (`\\n`) the newline. Fix: add the missing closing quote on the same line, or split the literal across several literals joined by interpolation.",
            ErrorCode::InvalidNumber => "A numeric literal did not parse as a valid Buff number. Buff accepts decimal integers (`42`), hex (`0xff`), binary (`0b1010`), octal (`0o17`), decimals (`3.14`), scientific notation (`1e10`), and byte literals (`b'A'`). This error fires when a literal overflows its type, mixes radix prefixes incorrectly, has a malformed decimal point, or uses an unsupported suffix. Fix: re-read the literal as a single number, or split compound expressions.",
            ErrorCode::MixedTabsSpaces => "A single indentation line mixes ASCII tabs and spaces. Buff's layout rule (Python/Haskell-style offside) needs indentation to be a strict stack of levels, and mixing tabs and spaces makes the level ambiguous. The lexer accepts spaces only — the convention is exactly 4 spaces per level. Fix: convert tabs to spaces in your editor (most editors have a setting to insert spaces when you press Tab).",
            ErrorCode::InconsistentIndent => "A dedent landed on an indentation level that does not match any scope that is currently open. For example, opening three nested blocks at 4, 8, and 12 spaces, then dedenting directly to 6 spaces, produces this error — the lexer cannot tell which scope you meant to exit. Fix: dedent one level at a time (4, 8, 12, then 8, then 4, then 0).",
            ErrorCode::UnterminatedBlockComment => "A `/* ... */` block comment was opened but the matching `*/` was never seen before end of file. Block comments in Buff nest — `/* /* */ */` is balanced — so the lexer tracks depth. Fix: add the missing `*/`, or convert the comment to a series of line comments (`// ...`).",
            ErrorCode::UnterminatedRegex => "A regex literal `/.../ ` was opened but the closing `/` was not found before end of line or end of input. Regex literals in Buff cannot span lines. Fix: add the closing `/` on the same line, or break a long pattern into a string + `Regex.compile` call.",
            ErrorCode::EmptyRegex => "A regex literal has an empty body (`//`). An empty regex pattern is almost always a typo. Fix: write the pattern between the slashes, or use `Regex.compile(\"\")` if an empty pattern is genuinely intended.",
            ErrorCode::UnterminatedCharLiteral => "A char literal `'...'` was opened but never closed before end of input. Char literals in Buff contain exactly one Unicode scalar value, optionally escaped (`'\\n'`, `'\\u{1F600}'`). Fix: add the closing `'` and ensure the body is exactly one scalar value.",
            ErrorCode::EmptyCharLiteral => "A char literal `''` has no body. Buff char literals must contain exactly one Unicode scalar value. For an empty string use `\"\"` (a `String`, not a char). Fix: put exactly one character (or one escape) between the quotes.",
            ErrorCode::InvalidCharEscape => "A backslash escape inside a char or string literal is not recognised. Buff supports the same escapes as Rust: `\\n`, `\\r`, `\\t`, `\\\\`, `\\0`, `\\'`, `\\\"`, `\\x{HH}`, and `\\u{HHHH}`. An escape like `\\d` or `\\w` (regex-style) is a syntax error. Fix: use a supported escape, or write the literal character directly.",
            ErrorCode::InvalidUnicodeEscape => "A `\\u{...}` escape's hex digits do not form a valid Unicode scalar value. Either the hex was malformed (non-hex characters), the braces were missing, or the resulting code point is outside the valid range (`0x0000`–`0x10FFFF`, excluding the surrogate block `0xD800`–`0xDFFF`). Fix: recheck the code point (the Unicode code charts are authoritative) and use uppercase or lowercase hex with braces.",
            ErrorCode::UnexpectedBraceInString => "A `}` appeared inside a `\"...\"` string literal without a matching `{`. Inside a string, `{` opens an interpolation and `}` closes it; a bare `}` is a syntax error. Fix: write `\\}` to insert a literal `}`, or open an interpolation with `{` first.",
            ErrorCode::UnterminatedInterpolation => "A `{` opening a string interpolation was never closed with `}` before the end of the line or end of input. String interpolations cannot span lines. Fix: add the closing `}` on the same line, or break the expression out into a `let` binding before the string.",
            ErrorCode::ExpectedToken => "The parser expected a specific token but found another token, or end of input. This is the most common parse error — it fires anywhere a particular keyword, punctuation, or name is required by the grammar. The message includes both what was expected and what was found. Fix: read the message, then either insert the expected token at the cursor, or delete the offending token.",
            ErrorCode::UnexpectedToken => "The parser found a token that cannot legally appear in the current position. Unlike `ExpectedToken` (which names the missing token), `UnexpectedToken` fires when the parser sees something that breaks the grammar outright. The message describes what was being parsed and what showed up. Fix: delete the unexpected token or replace it with the construct the grammar expects.",
            ErrorCode::ExpectedLayoutNewline => "After a `:` that opens a layout block (function body, `if` body, `for` body, etc.), Buff requires a newline before the indented statements. Writing `func f(): x = 1` on one line is a syntax error. Fix: press Enter after the `:`, then indent the body.",
            ErrorCode::ExpectedIndentedBlock => "After a `:` and its newline, Buff expects at least one statement indented deeper than the surrounding scope. An empty body or a body at the same indentation as the header produces this error. Fix: indent the body (4 spaces per level), or — if the body is genuinely empty — replace it with a `pass`-style placeholder (`return ()` works for unit-returning functions).",
            ErrorCode::FuncMustBeTopLevel => "Buff requires function declarations (`func name():`) to live at the top level of a file, not nested inside another block. Nested function declarations are deferred. If you need a local helper, use a closure: `let helper = { x => ... }`.",
            ErrorCode::ExpectedIdentifier => "An `import`, `export`, `attribute`, `let`, `func`, or similar declaration requires a name (identifier) at the cursor, but none was found. Usually this means a typo, a stray keyword in name position, or end of input. Fix: write the identifier the construct expects.",
            ErrorCode::UnterminatedList => "A delimited list (`{...}`, `(...)`, `<...>`) reached end of input without its closing delimiter. This usually means a missing `)`, `}`, or `>` at the end of a long declaration. The error message includes which list was being parsed. Fix: add the missing closing delimiter at the end of the construct.",
            ErrorCode::UnsupportedAbi => "An `extern \"ABI\" func ...` declaration used an ABI string other than `\"C\"`. Buff v1.3 supports only the C ABI for cross-language stability (T119 spec); other ABIs (Rust, system, stdcall, fastcall, …) are deferred. Fix: declare the extern function with `extern \"C\"`, or wrap the foreign call in a C shim.",
            ErrorCode::ExternGenericsUnsupported => "An `extern \"C\" func ...` declaration was given generic type parameters (`<T>`). Extern functions lower to raw C symbols, which cannot be monomorphised. Fix: declare one concrete extern function per type you need, and have each call a typed Rust wrapper on the other side.",
            ErrorCode::UndefinedVariable => "A name used in an expression is not in scope. The type-checker could not find it as a local, parameter, function, struct/enum/trait name, or prelude builtin. Fix: check the spelling, import the module that exports it, or — for builtins like `print` — confirm the prelude is loaded (it is implicit; you do not need to import it).",
            ErrorCode::BinaryOpTypeMismatch => "A binary operator (`+`, `-`, `*`, `/`, `<`, `==`, `and`, `or`, `|`, `&`, `^`, …) was applied to operands whose types it does not accept. For example, `Int + String`, or `Int < Bool`. The message names the two operand types. Fix: cast one side explicitly, or rethink the operation — Buff's numerics follow Rust's, so integer division and unsigned/signed mix are common surprises.",
            ErrorCode::AssignTypeMismatch => "An assignment (`x = expr`) or `let x = expr` has a right-hand side whose type is not assignable to the left-hand side's type. The message names both types. Fix: annotate the binding with a different type, transform the RHS (e.g. `Int.from(s)` instead of bare `s`), or change the type of the variable.",
            ErrorCode::InvalidUnaryOperand => "A unary operator (`-`, `!`, or `~`) was applied to an operand whose type it does not accept. `-` requires a numeric type, `!` requires `Bool`, `~` requires an integer. Fix: convert the operand to the expected type first.",
            ErrorCode::IfConditionMustBeBool => "The condition expression of an `if` is not `Bool`. Buff (like Rust) requires `if` conditions to evaluate to exactly `Bool`; truthy/falsy coercion is not supported. Fix: change the condition to a boolean expression (`x == 0` instead of `x`, `opt.is_some()` instead of `opt`).",
            ErrorCode::IfBranchTypeMismatch => "The two branches of an `if`/`else` expression produce values of different types, but `if` is an expression in Buff and all branches must agree on the result type. Fix: make both branches return the same type, or convert one branch (`Ok(value)` vs `Err(())` are different types — wrap appropriately).",
            ErrorCode::NonExhaustiveMatch => "A `match` expression does not list arms for every possible value of its scrutinee type. For enums this means every variant must appear (or be covered by `_`). For `Bool` both `true` and `false` must appear. The error message names a value that is not covered. Fix: add an arm for the missing value, or add a `_` catch-all.",
            ErrorCode::PreferGpuOnRecursiveFunction => "A function marked `@prefer(gpu)` calls itself (directly or transitively). GPU shaders cannot recurse — the WGSL execution model has no call stack. Fix: remove `@prefer(gpu)` and let the runtime dispatch to CPU, or refactor the recursion into an iterative loop.",
            ErrorCode::ModuleError => "An `import` or `export` declaration failed to resolve. Sub-causes: the target file does not exist; an import cycle was detected; a name you tried to import is not in the module's `export` list; or the import path is a stdlib path that is not yet wired up in v0.5. The message distinguishes these. Fix: check the path spelling, break import cycles, ensure the name is exported, or — for stdlib imports — wait for the v1.0 stdlib rollout.",
            ErrorCode::UnsupportedCodegen => "The Buff AST node you wrote has no Rust codegen implementation yet in this version of the compiler. This is a feature-gated rejection, not a syntax or type error — the front-end accepted your code but codegen cannot lower it. The message names the construct. Fix: rewrite the construct using a supported equivalent, or wait for the feature in a later version.",
            ErrorCode::CodegenParseError => "Codegen produced a Rust token stream that `syn` refused to parse back into an AST. This is always an internal compiler error — the user's Buff program is well-formed; the bug is in the codegen pass. The message includes the `syn` parse error for triage. Fix: report the bug with a minimal reproducer; as a workaround, rewrite the offending construct using a simpler equivalent.",
            ErrorCode::AsyncBlockDeadlock => "`block()` was called inside an `async func`. `block_on` parks the current worker thread, which can prevent any future scheduled on the same worker from running — a deadlock. Codegen still emits the call (so you can see what you wrote), but treats it as a warning. Fix: remove `block()` and let the async fn `return` the future directly, or restructure so the blocking work happens in a non-async function.",
        }
    }

    /// Every defined `ErrorCode`, sorted ascending by numeric code.
    ///
    /// Use this for deterministic enumeration (e.g. generating the static
    /// error index page, or asserting that every code has a doc page).
    pub fn all() -> &'static [ErrorCode] {
        &[
            // E10xx — Lexing
            ErrorCode::UnexpectedChar,
            ErrorCode::UnterminatedString,
            ErrorCode::InvalidNumber,
            ErrorCode::MixedTabsSpaces,
            ErrorCode::InconsistentIndent,
            ErrorCode::UnterminatedBlockComment,
            ErrorCode::UnterminatedRegex,
            ErrorCode::EmptyRegex,
            ErrorCode::UnterminatedCharLiteral,
            ErrorCode::EmptyCharLiteral,
            ErrorCode::InvalidCharEscape,
            ErrorCode::InvalidUnicodeEscape,
            ErrorCode::UnexpectedBraceInString,
            ErrorCode::UnterminatedInterpolation,
            // E11xx — Parsing
            ErrorCode::ExpectedToken,
            ErrorCode::UnexpectedToken,
            ErrorCode::ExpectedLayoutNewline,
            ErrorCode::ExpectedIndentedBlock,
            ErrorCode::FuncMustBeTopLevel,
            ErrorCode::ExpectedIdentifier,
            ErrorCode::UnterminatedList,
            ErrorCode::UnsupportedAbi,
            ErrorCode::ExternGenericsUnsupported,
            // E12xx — Type-checking
            ErrorCode::UndefinedVariable,
            ErrorCode::BinaryOpTypeMismatch,
            ErrorCode::AssignTypeMismatch,
            ErrorCode::InvalidUnaryOperand,
            ErrorCode::IfConditionMustBeBool,
            ErrorCode::IfBranchTypeMismatch,
            ErrorCode::NonExhaustiveMatch,
            ErrorCode::PreferGpuOnRecursiveFunction,
            ErrorCode::ModuleError,
            // E13xx — Codegen
            ErrorCode::UnsupportedCodegen,
            ErrorCode::CodegenParseError,
            ErrorCode::AsyncBlockDeadlock,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorCode;

    /// Every variant must produce a non-empty `"E1"` + digits string.
    #[test]
    fn code_str_well_formed() {
        for &code in ErrorCode::all() {
            let s = code.code_str();
            assert!(
                s.len() == 5 && s.starts_with('E') && s[1..].chars().all(|c| c.is_ascii_digit()),
                "code_str {s:?} is not well-formed `E1xxx`"
            );
        }
    }

    /// Codes must be unique — no two variants share a code string.
    #[test]
    fn code_str_unique() {
        let mut seen: Vec<&'static str> = ErrorCode::all().iter().map(|c| c.code_str()).collect();
        seen.sort_unstable();
        let len_before = seen.len();
        seen.dedup();
        assert_eq!(
            seen.len(),
            len_before,
            "duplicate error code detected after dedup"
        );
    }

    /// `all()` must be sorted ascending by numeric value, so deterministic
    /// enumeration is automatic for callers.
    #[test]
    fn all_sorted_ascending() {
        let codes: Vec<usize> = ErrorCode::all()
            .iter()
            .map(|c| c.code_str()[1..].parse::<usize>().expect("numeric"))
            .collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted, "ErrorCode::all() must be sorted ascending");
    }

    /// Title and explanation must be non-empty for every variant — no
    /// placeholder content.
    #[test]
    fn title_and_explanation_non_empty() {
        for &code in ErrorCode::all() {
            assert!(!code.title().is_empty(), "empty title for {code:?}");
            assert!(
                !code.explanation().is_empty(),
                "empty explanation for {code:?}"
            );
            // No trailing periods in titles (convention §4).
            assert!(
                !code.title().ends_with('.'),
                "title for {code:?} has trailing period"
            );
            assert!(
                code.explanation().ends_with('.'),
                "explanation for {code:?} should end with a period"
            );
        }
    }
}
