//! T53 AST tests — ComptimeBlock variant + is_comptime Param field.

use buff_lang_ast::{
    common::{Block, Ident, Param},
    stmt::Stmt,
    ty::TypeRef,
    Expr, Literal,
};
use buff_lang_error::{ErrorCode, SourceId, Span};

fn span_at(start: usize, end: usize) -> Span {
    Span::new(start, end, SourceId(1))
}

fn dummy_span() -> Span {
    Span::dummy()
}

#[test]
fn comptime_block_variant_constructs() {
    let body = Block::empty(dummy_span());
    let stmt = Stmt::ComptimeBlock {
        body,
        span: span_at(10, 30),
    };
    assert!(matches!(stmt, Stmt::ComptimeBlock { .. }));
}

#[test]
fn comptime_block_display_renders_body() {
    let body = Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::Literal(Literal::Int(42), dummy_span()),
            dummy_span(),
        )],
        span: dummy_span(),
    };
    let stmt = Stmt::ComptimeBlock {
        body,
        span: dummy_span(),
    };
    let s = format!("{stmt}");
    assert!(s.starts_with("Comptime("));
    assert!(s.contains("42"));
}

#[test]
fn param_is_comptime_field_defaults_via_plain_constructor() {
    let p = Param::plain(
        "x",
        TypeRef::Named {
            name: Ident::new("Int", dummy_span()),
            span: dummy_span(),
        },
        dummy_span(),
    );
    assert!(!p.is_comptime);
    assert!(p.default_value.is_none());
}

#[test]
fn param_with_is_comptime_displays_prefix() {
    let p = Param {
        name: Ident::new("T", dummy_span()),
        ty: TypeRef::Named {
            name: Ident::new("Type", dummy_span()),
            span: dummy_span(),
        },
        default_value: None,
        is_comptime: true,
        span: dummy_span(),
    };
    assert_eq!(p.to_string(), "comptime T: Type");
}

#[test]
fn param_without_is_comptime_no_prefix() {
    let p = Param {
        name: Ident::new("x", dummy_span()),
        ty: TypeRef::Named {
            name: Ident::new("Int", dummy_span()),
            span: dummy_span(),
        },
        default_value: None,
        is_comptime: false,
        span: dummy_span(),
    };
    assert_eq!(p.to_string(), "x: Int");
}

#[test]
fn error_code_e1110_is_stable() {
    assert_eq!(ErrorCode::MalformedComptime.code_str(), "E1110");
    assert!(!ErrorCode::MalformedComptime.title().is_empty());
    assert!(ErrorCode::MalformedComptime.explanation().ends_with('.'));
}

#[test]
fn error_codes_e1210_e1211_e1212_e1304_all_distinct_and_stable() {
    let codes = [
        ErrorCode::ComptimeEvaluationFailed,
        ErrorCode::ComptimeIoForbidden,
        ErrorCode::ComptimeReflectionForbidden,
        ErrorCode::ComptimeLoweringFailed,
    ];
    let strs: Vec<&'static str> = codes.iter().map(|c| c.code_str()).collect();
    let mut deduped = strs.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(deduped.len(), codes.len(), "codes must be unique: {strs:?}");
    assert_eq!(ErrorCode::ComptimeEvaluationFailed.code_str(), "E1210");
    assert_eq!(ErrorCode::ComptimeIoForbidden.code_str(), "E1211");
    assert_eq!(ErrorCode::ComptimeReflectionForbidden.code_str(), "E1212");
    assert_eq!(ErrorCode::ComptimeLoweringFailed.code_str(), "E1304");
}

#[test]
fn error_code_all_includes_comptime_variants() {
    let all = ErrorCode::all();
    assert!(all.contains(&ErrorCode::MalformedComptime));
    assert!(all.contains(&ErrorCode::ComptimeEvaluationFailed));
    assert!(all.contains(&ErrorCode::ComptimeIoForbidden));
    assert!(all.contains(&ErrorCode::ComptimeReflectionForbidden));
    assert!(all.contains(&ErrorCode::ComptimeLoweringFailed));
}
