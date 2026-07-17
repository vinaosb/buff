//! T105 integration tests — Parser for named call arguments.
//!
//! Verifies that `create(host: "x", port: 80)` (and the reordered form
//! `create(port: 80, host: "x")`) parse to AST nodes carrying the names,
//! that mixed positional + named arg lists are supported, and that pure
//! positional calls are unchanged (no regression).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser named_args
//! ```

use buff_lang_ast::{Expr, Ident, Literal};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse_expression;

fn sid() -> SourceId {
    SourceId(0)
}

fn span() -> buff_lang_error::Span {
    buff_lang_error::Span::dummy()
}

fn int(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn str_lit(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

/// Tokenize + parse a single expression from `src`. Panics on failure.
fn parse(src: &str) -> Expr {
    let tokens = tokenize(src, sid()).expect("lexer should succeed");
    parse_expression(&tokens, sid()).expect("parser should succeed")
}

/// Compare two Exprs by their Display string (ignores span differences).
fn shape(e: &Expr) -> String {
    e.to_string()
}

/// Extract the (name, value-shape) pair from a NamedArg node.
fn named_pair(arg: &Expr) -> (&str, String) {
    match arg {
        Expr::NamedArg { name, value, .. } => (&name.name, shape(value.as_ref())),
        other => panic!("expected Expr::NamedArg, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Single named arg: `f(name: value)`
// ---------------------------------------------------------------------------

#[test]
fn named_args_single() {
    // `f(host: "x")` -> FuncCall { args: [NamedArg { name: "host", value: "x" }] }
    let e = parse(r#"f(host: "x")"#);
    match e {
        Expr::FuncCall { callee, args, .. } => {
            assert!(
                matches!(callee.as_ref(), Expr::Ident(name, _) if name.name == "f"),
                "callee should be `f`, got {:?}",
                callee
            );
            assert_eq!(args.len(), 1, "expected exactly one arg");
            let (n, v) = named_pair(&args[0]);
            assert_eq!(n, "host", "first named arg name should be `host`");
            assert_eq!(v, shape(&str_lit("x")), "first named arg value shape");
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Two named args, declaration order: `create(host: "x", port: 80)`
// ---------------------------------------------------------------------------

#[test]
fn named_args_declaration_order() {
    let e = parse(r#"create(host: "x", port: 80)"#);
    match e {
        Expr::FuncCall { args, .. } => {
            assert_eq!(args.len(), 2);
            let (n0, v0) = named_pair(&args[0]);
            let (n1, v1) = named_pair(&args[1]);
            assert_eq!(n0, "host");
            assert_eq!(v0, shape(&str_lit("x")));
            assert_eq!(n1, "port");
            assert_eq!(v1, shape(&int(80)));
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Reordered named args: `create(port: 80, host: "x")`
// (AST preserves source order; reorder happens at codegen time.)
// ---------------------------------------------------------------------------

#[test]
fn named_args_reordered() {
    let e = parse(r#"create(port: 80, host: "x")"#);
    match e {
        Expr::FuncCall { args, .. } => {
            assert_eq!(args.len(), 2);
            // Source order is preserved at parse time — `port` first.
            let (n0, v0) = named_pair(&args[0]);
            let (n1, v1) = named_pair(&args[1]);
            assert_eq!(n0, "port");
            assert_eq!(v0, shape(&int(80)));
            assert_eq!(n1, "host");
            assert_eq!(v1, shape(&str_lit("x")));
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Mixed positional + named: `f(1, name: "x")`
// ---------------------------------------------------------------------------

#[test]
fn named_args_mixed_positional_and_named() {
    let e = parse(r#"f(1, name: "x")"#);
    match e {
        Expr::FuncCall { args, .. } => {
            assert_eq!(args.len(), 2);
            // First arg is positional (Literal::Int(1)).
            assert_eq!(
                shape(&args[0]),
                shape(&int(1)),
                "first arg should be positional Int(1)"
            );
            // Second arg is named.
            let (n, v) = named_pair(&args[1]);
            assert_eq!(n, "name");
            assert_eq!(v, shape(&str_lit("x")));
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Pure positional regression: `f(1, 2)` (no NamedArg nodes; unchanged)
// ---------------------------------------------------------------------------

#[test]
fn named_args_positional_unchanged() {
    let e = parse("f(1, 2)");
    match e {
        Expr::FuncCall { args, .. } => {
            assert_eq!(args.len(), 2);
            // Both args must be positional (no NamedArg).
            assert_eq!(shape(&args[0]), shape(&int(1)));
            assert_eq!(shape(&args[1]), shape(&int(2)));
            assert!(
                !matches!(args[0], Expr::NamedArg { .. }),
                "positional arg must not be a NamedArg"
            );
            assert!(
                !matches!(args[1], Expr::NamedArg { .. }),
                "positional arg must not be a NamedArg"
            );
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Named arg carries the Ident (not a string) for the name.
// ---------------------------------------------------------------------------

#[test]
fn named_args_name_is_ident() {
    let e = parse(r#"f(my_param: 42)"#);
    match e {
        Expr::FuncCall { args, .. } => {
            assert_eq!(args.len(), 1);
            match &args[0] {
                Expr::NamedArg { name, value, .. } => {
                    // Name is an Ident (struct, not a String).
                    assert_eq!(name.name, "my_param");
                    // Value is boxed.
                    assert_eq!(shape(value.as_ref()), shape(&int(42)));
                }
                other => panic!("expected Expr::NamedArg, got {other:?}"),
            }
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Named arg with trailing comma: `f(host: "x", port: 80,)`
// ---------------------------------------------------------------------------

#[test]
fn named_args_trailing_comma_allowed() {
    let e = parse(r#"f(host: "x", port: 80,)"#);
    match e {
        Expr::FuncCall { args, .. } => {
            assert_eq!(args.len(), 2);
            assert_eq!(named_pair(&args[0]).0, "host");
            assert_eq!(named_pair(&args[1]).0, "port");
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Named arg with complex value: `f(idx: a + 1)`
// ---------------------------------------------------------------------------

#[test]
fn named_args_complex_value() {
    let e = parse("f(idx: a + 1)");
    match e {
        Expr::FuncCall { args, .. } => {
            assert_eq!(args.len(), 1);
            let (n, v) = named_pair(&args[0]);
            assert_eq!(n, "idx");
            // Value is a BinaryOp expression (NOT a literal).
            assert!(
                v.starts_with("BinaryOp("),
                "named arg value should be a BinaryOp shape, got {v}"
            );
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Named arg in method call: `obj.m(name: "x")`
// ---------------------------------------------------------------------------

#[test]
fn named_args_in_method_call() {
    let e = parse(r#"obj.m(name: "x")"#);
    match e {
        Expr::MethodCall { method, args, .. } => {
            assert_eq!(method.name, "m");
            assert_eq!(args.len(), 1);
            let (n, v) = named_pair(&args[0]);
            assert_eq!(n, "name");
            assert_eq!(v, shape(&str_lit("x")));
        }
        other => panic!("expected Expr::MethodCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Empty arg list still works (no regression).
// ---------------------------------------------------------------------------

#[test]
fn named_args_empty_args_unchanged() {
    let e = parse("f()");
    match e {
        Expr::FuncCall { args, .. } => {
            assert!(args.is_empty(), "empty call should have no args");
        }
        other => panic!("expected Expr::FuncCall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Display renders the named arg.
// ---------------------------------------------------------------------------

#[test]
fn named_args_display() {
    let e = parse(r#"f(host: "x")"#);
    let s = e.to_string();
    // Display should mention the name and the value.
    assert!(
        s.contains("host"),
        "display should include name `host`: {s}"
    );
    assert!(
        s.contains("x") || s.contains("\"x\""),
        "display should include value: {s}"
    );
}

// Suppress unused warnings for helpers used conditionally.
#[allow(dead_code)]
fn _unused() {
    let _ = Ident::new("x", span());
}
