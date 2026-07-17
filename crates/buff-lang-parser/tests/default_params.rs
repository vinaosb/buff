//! T106 integration tests — Parser for default parameter values.
//!
//! Verifies that `func fetch(url: String, timeout: Int = 30)` parses with the
//! `timeout` param carrying `default_value = Some(Literal::Int(30))`, that
//! multiple defaults parse, that mixed default/non-default params work, and
//! that params WITHOUT defaults still carry `default_value = None` (no
//! regression on existing fn-decl parsing).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser default_params
//! ```

use buff_lang_ast::{Expr, FuncDecl, Literal, Param};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::{parse_func_decl, TokenStream};

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse a function declaration. The source must start with `func`.
fn parse_func(src: &str) -> FuncDecl {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    let mut stream = TokenStream::new(&tokens, sid());
    parse_func_decl(&mut stream, Vec::new()).expect("parser should succeed")
}

/// Find a param by name in a func decl's param list (panics if absent).
fn param<'a>(f: &'a FuncDecl, name: &str) -> &'a Param {
    f.params
        .iter()
        .find(|p| p.name.name == name)
        .unwrap_or_else(|| panic!("no param named `{name}` in func `{}`", f.name.name))
}

// ---------------------------------------------------------------------------
// Single default param: `timeout: Int = 30`
// ---------------------------------------------------------------------------

#[test]
fn default_params_single() {
    // `func fetch(url: String, timeout: Int = 30): ...`
    // → timeout carries default_value = Some(Literal::Int(30)).
    let f = parse_func("func fetch(url: String, timeout: Int = 30):\n    return 0");
    assert_eq!(f.params.len(), 2, "expected two params");

    // url — no default.
    let url = param(&f, "url");
    assert!(
        url.default_value.is_none(),
        "`url` should have NO default, got {:?}",
        url.default_value
    );

    // timeout — default Int(30).
    let timeout = param(&f, "timeout");
    match &timeout.default_value {
        Some(Expr::Literal(Literal::Int(v), _)) => {
            assert_eq!(*v, 30, "timeout default should be Int(30)");
        }
        other => panic!("timeout default should be Some(Literal::Int(30))), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// String default: `greeting: String = "hi"`
// ---------------------------------------------------------------------------

#[test]
fn default_params_string_default() {
    let f = parse_func(
        r#"func greet(name: String, greeting: String = "hi"):
    return 0"#,
    );
    let greeting = param(&f, "greeting");
    match &greeting.default_value {
        Some(Expr::Literal(Literal::String(s), _)) => {
            assert_eq!(s, "hi", "greeting default should be String(\"hi\")");
        }
        other => panic!("greeting default should be Some(Literal::String(\"hi\"))), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Multiple default params.
// ---------------------------------------------------------------------------

#[test]
fn default_params_multiple() {
    // Both trailing params have defaults.
    let f = parse_func("func cfg(host: String, port: Int = 80, retries: Int = 3):\n    return 0");
    assert_eq!(f.params.len(), 3);

    let port = param(&f, "port");
    match &port.default_value {
        Some(Expr::Literal(Literal::Int(v), _)) => assert_eq!(*v, 80),
        other => panic!("port default should be Int(80), got {other:?}"),
    }
    let retries = param(&f, "retries");
    match &retries.default_value {
        Some(Expr::Literal(Literal::Int(v), _)) => assert_eq!(*v, 3),
        other => panic!("retries default should be Int(3), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// No defaults → all params carry default_value = None (regression guard).
// ---------------------------------------------------------------------------

#[test]
fn default_params_no_default_unchanged() {
    // A plain func decl with NO defaults — existing behaviour must hold.
    let f = parse_func("func add(a: Int, b: Int) -> Int:\n    return a + b");
    assert_eq!(f.params.len(), 2);
    for p in &f.params {
        assert!(
            p.default_value.is_none(),
            "param `{}` should have NO default, got {:?}",
            p.name.name,
            p.default_value
        );
    }
}

// ---------------------------------------------------------------------------
// Mixed: one required + one defaulted (the spec's canonical shape).
// ---------------------------------------------------------------------------

#[test]
fn default_params_mixed_required_and_default() {
    let f = parse_func("func fetch(url: String, timeout: Int = 30):\n    return 0");
    assert_eq!(f.params.len(), 2);
    // url required, timeout defaulted.
    assert!(param(&f, "url").default_value.is_none());
    assert!(param(&f, "timeout").default_value.is_some());
}

// ---------------------------------------------------------------------------
// Display impl: `name: Type = expr` renders with the default.
// ---------------------------------------------------------------------------

#[test]
fn default_params_display_includes_default() {
    let f = parse_func("func fetch(url: String, timeout: Int = 30):\n    return 0");
    let timeout = param(&f, "timeout");
    let s = timeout.to_string();
    assert!(
        s.contains("= "),
        "Param Display should include `= ` for defaults, got: {s}"
    );
    assert!(
        s.contains("30"),
        "Param Display should include the default value, got: {s}"
    );
}

// ---------------------------------------------------------------------------
// Zero-param and single-param funcs still parse (no `=` present).
// ---------------------------------------------------------------------------

#[test]
fn default_params_zero_param_func_parses() {
    let f = parse_func("func main():\n    return 0");
    assert!(f.params.is_empty(), "zero-param func should parse cleanly");
}

#[test]
fn default_params_bool_default() {
    let f = parse_func("func flag(enabled: Bool = true):\n    return 0");
    let enabled = param(&f, "enabled");
    match &enabled.default_value {
        Some(Expr::Literal(Literal::Bool(b), _)) => assert!(*b, "enabled default should be true"),
        other => panic!("enabled default should be Bool(true), got {other:?}"),
    }
}
