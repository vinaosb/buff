//! T36 — Parser error recovery integration tests.
//!
//! Verifies the parser no longer FAILS FAST on the first syntax error in
//! [`buff_lang_parser::parse_recovering`]:
//!
//! - Multiple errors in the same file are all collected in one pass
//! - After an error the parser skips to a sync point (`func`, `let`, `match`,
//!   etc.) and CONTINUES parsing subsequent declarations
//! - The original fail-fast [`buff_lang_parser::parse`] still returns
//!   `Err(first_error)` for backwards compatibility.

use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse, parse_recovering};

fn sid() -> SourceId {
    SourceId(0)
}

#[test]
fn error_recovery_reports_two_errors_in_one_pass() {
    // Two stray `)` tokens at top level (each is a syntax error). The valid
    // `func` decls between them must still parse, AND both errors must be
    // reported (not just the first).
    let src = "\
func good():
    return 1
)
func second():
    return 2
)
func third():
    return 3
";
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let (decls, errors) = parse_recovering(&tokens, sid());

    // Two errors collected (one per stray `)`).
    assert!(
        errors.len() >= 2,
        "expected at least 2 errors, got {}: {errors:?}",
        errors.len()
    );
    // At least one valid `func good` should have parsed before the first
    // error; ideally all three good decls survive recovery.
    assert!(
        !decls.is_empty(),
        "expected at least one decl to survive recovery, got 0"
    );
}

#[test]
fn error_recovery_continues_after_error_to_parse_subsequent_decl() {
    // A single bad token (bare `123` at top level) followed by a valid func.
    // After flagging the bad token, the parser must sync forward past it and
    // parse `func later()`.
    let src = "\
123
func later():
    return 42
";
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let (decls, errors) = parse_recovering(&tokens, sid());

    assert!(
        !errors.is_empty(),
        "expected at least one error from the bad top-level token, got 0"
    );
    assert!(
        !decls.is_empty(),
        "expected `func later` to parse after recovery, got 0 decls"
    );
    // The recovered decl should be the `func later` function.
    let names: Vec<&str> = decls
        .iter()
        .filter_map(|d| match d {
            buff_lang_ast::Decl::FuncDecl(f) => Some(f.name.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        names.contains(&"later"),
        "expected `later` func to survive recovery; got names: {names:?}"
    );
}

#[test]
fn error_recovery_clean_input_produces_no_errors() {
    // Sanity: well-formed input must produce zero errors and the same set of
    // decls as the fail-fast parser.
    let src = "\
func one():
    return 1
func two():
    return 2
";
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let (decls, errors) = parse_recovering(&tokens, sid());

    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    assert_eq!(decls.len(), 2, "expected 2 decls, got {}", decls.len());

    // And the fail-fast parser should agree.
    let fast = parse(&tokens, sid()).expect("fail-fast parse should succeed");
    assert_eq!(fast.len(), 2);
}

#[test]
fn error_recovery_does_not_panic_on_completely_garbled_input() {
    // No sync tokens anywhere — just operators. Recovery should not loop
    // forever, panic, or return bogus decls.
    let src = "+++ === +++";
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let (decls, errors) = parse_recovering(&tokens, sid());

    // Either way: no crash. Some errors expected; no valid decls.
    assert!(decls.is_empty(), "expected 0 decls from garbled input");
    assert!(
        !errors.is_empty(),
        "expected at least one error from garbled input"
    );
}

#[test]
fn parse_fail_fast_still_returns_first_error_for_backwards_compat() {
    // The legacy entry point `parse()` must still return Err on the first
    // syntax error (its documented contract).
    let src = ")\nfunc good():\n    return 1\n";
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let result = parse(&tokens, sid());
    assert!(result.is_err(), "fail-fast parse should error on stray `)`");
}
