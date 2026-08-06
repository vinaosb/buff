//! Property-based tests for the Buff lexer (P5.4).
//!
//! These proptests verify crash-safety and basic roundtrip properties of
//! [`buff_lang_lexer::tokenize`] across 1536 randomly generated inputs
//! (6 properties × 256 cases each). The core contract under test:
//!
//! 1. **Crash-safety** — `tokenize` must NEVER panic on any input. Malformed
//!    input returns `Err`; well-formed input returns `Ok`; but NO input
//!    causes an abort.
//! 2. **Literal round-trip** — integer literals survive tokenization with
//!    their exact value intact.
//! 3. **Comment skipping** — `//` line comments are removed from the token
//!    stream (they never appear as `StringLit` or `Ident` tokens).
//!
//! Tabs are deliberately excluded from all strategies because Buff mandates
//! 4-space indentation and rejects tabs at lex time.

use buff_lang_error::SourceId;
use buff_lang_lexer::{tokenize, TokenKind};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// (a) Any valid identifier `[a-zA-Z_][a-zA-Z0-9_]*` must tokenize without
    /// panicking. The lexer may return `Err` for pathological shapes but must
    /// never panic. A bare identifier (or keyword) always succeeds.
    #[test]
    fn prop_valid_identifiers_never_crash(s in "[a-zA-Z_][a-zA-Z0-9_]{0,30}") {
        let result = tokenize(&s, SourceId(0));
        prop_assert!(
            result.is_ok(),
            "lexer failed on valid identifier {:?}: {:?}",
            s,
            result.err()
        );
    }

    /// (b) Integer literals round-trip: tokenizing `n.to_string()` must yield
    /// exactly `[IntLit(n), Eof]`. Non-negative integers have no sign prefix
    /// so they produce a single numeric token.
    #[test]
    fn prop_integer_literals_roundtrip(n in 0i64..1_000_000_000) {
        let src = n.to_string();
        let tokens = tokenize(&src, SourceId(0))
            .map(|t| t.into_iter().map(|tok| tok.kind).collect::<Vec<_>>())
            .map_err(|e| TestCaseError::fail(format!("tokenize failed for {:?}: {:?}", src, e)))?;
        prop_assert_eq!(tokens.len(), 2, "expected [IntLit, Eof] for {:?}", src);
        prop_assert_eq!(&tokens[0], &TokenKind::IntLit(n));
        prop_assert_eq!(&tokens[1], &TokenKind::Eof);
    }

    /// (c) Float literals of the form `<int>.<frac>` never crash the lexer
    /// and produce a `FloatLit` (f32) token. The fractional part always has
    /// at least one digit so the lexer's `digit after dot` check passes.
    #[test]
    fn prop_float_literals_never_crash(
        int_part in 0u32..100_000,
        frac_part in 0u32..100_000,
    ) {
        let src = format!("{}.{}", int_part, frac_part);
        let result = tokenize(&src, SourceId(0));
        prop_assert!(
            result.is_ok(),
            "lexer failed on float literal {:?}: {:?}",
            src,
            result.err()
        );
        let kinds: Vec<TokenKind> = result
            .map(|t| t.into_iter().map(|tok| tok.kind).collect())
            .unwrap_or_default();
        prop_assert!(
            matches!(kinds.first(), Some(TokenKind::FloatLit(_))),
            "expected FloatLit for {:?}, got {:?}",
            src,
            kinds.first()
        );
    }

    /// (d) String literals containing safe characters (alphanumeric + spaces)
    /// wrapped in double quotes never crash the lexer and produce a
    /// `StringLit` token. The generated body contains no `"` or `\` so no
    /// escape processing or interpolation kicks in.
    #[test]
    fn prop_string_literals_never_crash(inner in "[a-zA-Z0-9 ]{0,40}") {
        let src = format!("\"{}\"", inner);
        let result = tokenize(&src, SourceId(0));
        prop_assert!(
            result.is_ok(),
            "lexer failed on string literal {:?}: {:?}",
            src,
            result.err()
        );
        let kinds: Vec<TokenKind> = result
            .map(|t| t.into_iter().map(|tok| tok.kind).collect())
            .unwrap_or_default();
        prop_assert!(
            matches!(kinds.first(), Some(TokenKind::StringLit(_)) | Some(TokenKind::StringStart)),
            "expected StringLit or StringStart for {:?}, got {:?}",
            src,
            kinds.first()
        );
    }

    /// (e) Arbitrary non-tab printable-ASCII source must NEVER panic the
    /// lexer. This is the strongest crash-safety property: the lexer is the
    /// first stage of the pipeline and untrusted input may reach it directly
    /// (REPL, LSP, playground). Tabs are excluded because Buff rejects them
    /// at lex time (they produce `Err`, which is fine, but the task spec
    /// forbids generating tab-containing programs).
    #[test]
    fn prop_arbitrary_source_never_panics(s in "[ -~\n]{0,200}") {
        // The assertion is implicit: if tokenize panics, the test process
        // aborts and proptest reports the failure. We discard the Result —
        // both Ok and Err are acceptable outcomes.
        let _ = tokenize(&s, SourceId(0));
    }

    /// (f) `//` line comments are skipped: tokenizing `// comment\n<ident>`
    /// must never leak the comment body as a `StringLit`, and the trailing
    /// identifier must be present in the output (as `Ident` or a keyword
    /// token). Buff uses `//` for line comments (NOT `#`).
    #[test]
    fn prop_comment_handling(
        comment in "[a-zA-Z0-9 ]{1,50}",
        ident in "[a-zA-Z_][a-zA-Z0-9_]{0,20}",
    ) {
        let src = format!("// {}\n{}", comment, ident);
        let result = tokenize(&src, SourceId(0));
        prop_assert!(
            result.is_ok(),
            "lexer failed on {:?}: {:?}",
            src,
            result.err()
        );
        let kinds: Vec<TokenKind> = result
            .map(|t| t.into_iter().map(|tok| tok.kind).collect())
            .unwrap_or_default();

        // The comment text must never appear as a string literal.
        let leaked = kinds.iter().any(|k| {
            if let TokenKind::StringLit(s) = k {
                s.contains(comment.as_str())
            } else {
                false
            }
        });
        prop_assert!(!leaked, "comment body leaked as StringLit in {:?}", src);

        // At least one Ident or keyword must survive (the trailing identifier).
        let has_token = kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Ident(_)) || k.is_keyword());
        prop_assert!(
            has_token,
            "no identifier/keyword token after comment in {:?}",
            src
        );
    }
}
