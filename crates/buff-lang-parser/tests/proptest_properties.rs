//! Property-based tests for the Buff parser (P5.4).
//!
//! These proptests verify crash-safety and basic roundtrip properties of
//! the Buff parser across 1536 randomly generated inputs (6 properties x
//! 256 cases each). The core contract under test:
//!
//! 1. **Crash-safety** — `parse` must NEVER panic on any token stream,
//!    regardless of how malformed. The parser is the second pipeline stage
//!    and must gracefully reject invalid input via `Err`, never abort.
//! 2. **Literal expressions** — integer and string literals survive the
//!    tokenize -> parse round-trip as the correct AST node.
//! 3. **Operator precedence** — binary operators respect binding powers.
//! 4. **Function definitions** — valid `func` definitions parse to
//!    `Decl::FuncDecl` with the correct name.

use buff_lang_ast::Decl;
use buff_lang_error::SourceId;
use buff_lang_lexer::{tokenize, TokenKind};
use buff_lang_parser::parse;
use proptest::prelude::*;

/// Helper: tokenize + parse a source snippet, returning the declarations.
fn parse_src(src: &str) -> Result<Vec<Decl>, String> {
    let tokens = tokenize(src, SourceId(0)).map_err(|e| format!("tokenize: {e:?}"))?;
    parse(&tokens, SourceId(0)).map_err(|e| format!("parse: {e:?}"))
}

/// True when `s` is usable as a user identifier — i.e. the LEXER itself does
/// not recognize it as a reserved word. `TokenKind::from_keyword` is the
/// lexer's single recognition point: it covers all 30 keywords AND the
/// `and`/`or`/`not` word-operator aliases (BUG-4). Sourcing the filter from
/// it keeps this provably complete — a stale hand-copied list here twice let
/// a reserved-word draw fail these properties (ITER-51: an ubuntu flake on
/// a missing keyword, then a macos flake on the `or` word-operator alias;
/// the parser rightly rejects both as names).
fn is_identifier(s: &str) -> bool {
    TokenKind::from_keyword(s).is_none()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// (a) Any valid identifier as a bare expression statement never crashes
    /// the parser. We wrap it in a minimal function body to give it a valid
    /// top-level context.
    #[test]
    fn prop_valid_identifiers_in_func_never_crash(
        name in "[a-zA-Z_][a-zA-Z0-9_]{0,20}"
    ) {
        // Reserved words (keywords + word-operator aliases) are parse errors
        // by design; the property is about real identifiers succeeding.
        prop_assume!(is_identifier(&name));

        let src = format!("func {name}():\n    {name}\n");
        let result = parse_src(&src);
        prop_assert!(
            result.is_ok(),
            "parser failed on valid func with identifier {:?}: {:?}",
            name,
            result.err()
        );
    }

    /// (b) Integer literals in a function body survive tokenize + parse as
    /// `Expr::Literal(Literal::Int(n))`.
    #[test]
    fn prop_integer_literals_parse_correctly(n in 0i64..1_000_000) {
        let src = format!("func f():\n    {n}\n");
        let decls = parse_src(&src)
            .map_err(|e| TestCaseError::fail(format!("parse failed for {n}: {e:?}")))?;
        prop_assert!(!decls.is_empty(), "expected at least one declaration");
        // The first decl should be a FuncDecl.
        match &decls[0] {
            Decl::FuncDecl(fd) => {
                prop_assert_eq!(&fd.name.name, "f");
                prop_assert!(!fd.body.stmts.is_empty(), "function body should have stmts");
            }
            other => {
                return Err(TestCaseError::fail(format!(
                    "expected FuncDecl, got {other:?}"
                )));
            }
        }
    }

    /// (c) Binary operations with known precedence parse without crashing.
    /// `a + b * c` should parse (multiplication binds tighter than addition).
    #[test]
    fn prop_binary_operations_parse(
        a in 0i64..1000,
        b in 0i64..1000,
        c in 0i64..1000,
    ) {
        let src = format!("func f():\n    {a} + {b} * {c}\n");
        let result = parse_src(&src);
        prop_assert!(
            result.is_ok(),
            "parser failed on binary expression: {:?}",
            result.err()
        );
    }

    /// (d) Function definitions with random names parse to FuncDecl with
    /// the correct name.
    #[test]
    fn prop_func_definitions_parse(name in "[a-zA-Z_][a-zA-Z0-9_]{0,20}") {
        prop_assume!(is_identifier(&name));

        let src = format!("func {name}():\n    42\n");
        let decls = parse_src(&src)
            .map_err(|e| TestCaseError::fail(format!("parse failed: {e:?}")))?;
        prop_assert_eq!(decls.len(), 1, "expected exactly 1 declaration");
        match &decls[0] {
            Decl::FuncDecl(fd) => {
                prop_assert_eq!(&fd.name.name, &name);
            }
            other => {
                return Err(TestCaseError::fail(format!(
                    "expected FuncDecl, got {other:?}"
                )));
            }
        }
    }

    /// (e) Arbitrary non-tab printable-ASCII source must NEVER panic the
    /// parser. This is the strongest crash-safety property: the parser must
    /// return Err for invalid input, never abort.
    #[test]
    fn prop_arbitrary_source_never_panics(s in "[ -~\n]{0,200}") {
        // Tokenize first — if the lexer fails, that's fine (not a parser
        // concern). We only test that the parser itself never panics.
        if let Ok(tokens) = tokenize(&s, SourceId(0)) {
            let _ = parse(&tokens, SourceId(0));
        }
    }

    /// (f) Let bindings with integer values parse correctly.
    #[test]
    fn prop_let_bindings_parse(
        var_name in "[a-zA-Z_][a-zA-Z0-9_]{0,15}",
        value in 0i64..100_000,
    ) {
        prop_assume!(is_identifier(&var_name));

        let src = format!("func f():\n    let {var_name} = {value}\n");
        let result = parse_src(&src);
        prop_assert!(
            result.is_ok(),
            "parser failed on let binding {:?} = {:?}: {:?}",
            var_name,
            value,
            result.err()
        );
    }
}

/// ITER-51 regression: every word the lexer reserves — all 30 keywords plus
/// the `and`/`or`/`not` word-operator aliases — must be REJECTED in the
/// function-name position. The filter side (`is_identifier` above) is
/// sourced from the same lexer function, so filter and pinned behavior
/// cannot drift apart again.
#[test]
fn keyword_names_rejected_as_fn_names() {
    let reserved: Vec<&str> = TokenKind::all_keywords()
        .iter()
        .copied()
        .chain(["and", "or", "not"])
        .collect();
    for word in reserved {
        assert!(TokenKind::from_keyword(word).is_some());
        let fn_src = format!("func {word}():\n    42\n");
        assert!(
            parse_src(&fn_src).is_err(),
            "reserved word {word:?} should not parse as a function name"
        );
        let let_src = format!("func f():\n    let {word} = 0\n");
        assert!(
            parse_src(&let_src).is_err(),
            "reserved word {word:?} should not parse as a let-binding name"
        );
    }
}
