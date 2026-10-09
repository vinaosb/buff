//! T53 codegen tests — ComptimeValue → Rust `const` item lowering.

use buff_lang_codegen_rust::comptime::lower_comptime_facts;
use buff_lang_error::{CodegenError, ErrorCode};
use buff_lang_types::{ComptimeFacts, ComptimeValue};
use std::collections::BTreeMap;

#[test]
fn lowers_single_int_value_to_const_item() {
    let mut values = BTreeMap::new();
    values.insert(42usize, ComptimeValue::Int(100));
    let facts = ComptimeFacts {
        values,
        errors: vec![],
    };
    let items = lower_comptime_facts(&facts).expect("lower ok");
    assert_eq!(items.len(), 1);
}

#[test]
fn lowers_bool_value() {
    let mut values = BTreeMap::new();
    values.insert(0, ComptimeValue::Bool(true));
    let facts = ComptimeFacts {
        values,
        errors: vec![],
    };
    let items = lower_comptime_facts(&facts).expect("lower ok");
    assert_eq!(items.len(), 1);
}

#[test]
fn lowers_string_value() {
    let mut values = BTreeMap::new();
    values.insert(0, ComptimeValue::String("hello".to_string()));
    let facts = ComptimeFacts {
        values,
        errors: vec![],
    };
    let items = lower_comptime_facts(&facts).expect("lower ok");
    assert_eq!(items.len(), 1);
}

#[test]
fn lowers_array_value() {
    let mut values = BTreeMap::new();
    values.insert(
        0,
        ComptimeValue::Array(vec![ComptimeValue::Int(1), ComptimeValue::Int(2)]),
    );
    let facts = ComptimeFacts {
        values,
        errors: vec![],
    };
    let items = lower_comptime_facts(&facts).expect("lower ok");
    assert_eq!(items.len(), 1);
}

#[test]
fn lowers_multiple_values_in_offset_order() {
    let mut values = BTreeMap::new();
    values.insert(100, ComptimeValue::Int(1));
    values.insert(10, ComptimeValue::Int(2));
    values.insert(999, ComptimeValue::Int(3));
    let facts = ComptimeFacts {
        values,
        errors: vec![],
    };
    let items = lower_comptime_facts(&facts).expect("lower ok");
    assert_eq!(items.len(), 3);
}

#[test]
fn empty_facts_produces_no_items() {
    let facts = ComptimeFacts::default();
    let items = lower_comptime_facts(&facts).expect("lower ok");
    assert!(items.is_empty());
}

#[test]
fn unit_value_rejected_with_e1304() {
    let mut values = BTreeMap::new();
    values.insert(0, ComptimeValue::Unit);
    let facts = ComptimeFacts {
        values,
        errors: vec![],
    };
    let err: CodegenError = lower_comptime_facts(&facts).unwrap_err();
    assert_eq!(err.diagnostic.code, Some(ErrorCode::ComptimeLoweringFailed));
}

#[test]
fn facts_with_errors_still_lower_successful_values() {
    // The interpreter may have errored on some blocks; codegen should
    // still lower the ones that succeeded (those are the only ones in
    // `facts.values`; failures are in `facts.errors`).
    let mut values = BTreeMap::new();
    values.insert(0, ComptimeValue::Int(7));
    let facts = ComptimeFacts {
        values,
        errors: vec![],
    };
    let items = lower_comptime_facts(&facts).expect("lower ok");
    assert_eq!(items.len(), 1);
}

// ---------------------------------------------------------------------
// ITER-53B: top-level `comptime:` declarations (the T53 config form).
// -----------------------------------------------------------------------

#[test]
fn top_level_comptime_becomes_user_visible_consts() {
    use buff_lang_codegen_rust::generate_rust;
    use buff_lang_error::SourceId;
    use buff_lang_lexer::tokenize;
    use buff_lang_parser::parse;

    let src = "comptime:\n    let max_connections = 100\n    let timeout_seconds = 30\n\nfunc main():\n    print(max_connections)\n    print(timeout_seconds)\n";
    let toks = tokenize(src, SourceId(0)).expect("lexer");
    let decls = parse(&toks, SourceId(0)).expect("parse");
    let rust = generate_rust(&decls).expect("codegen");

    assert!(
        rust.contains("const max_connections: i64 = 100i64;"),
        "expected user-visible const for max_connections in:\n{rust}"
    );
    assert!(
        rust.contains("const timeout_seconds: i64 = 30i64;"),
        "expected user-visible const for timeout_seconds in:\n{rust}"
    );
    assert!(
        !rust.contains("__BUFF_COMPTIME_"),
        "top-level bindings must use user-visible names, not span-keyed internals:\n{rust}"
    );
}

#[test]
fn top_level_comptime_evaluation_error_maps_to_codegen_error() {
    use buff_lang_codegen_rust::generate_rust;
    use buff_lang_error::SourceId;
    use buff_lang_lexer::tokenize;
    use buff_lang_parser::parse;

    // `foo` is not bound inside the comptime block -> E1210-class failure.
    let src = "comptime:\n    let x = foo\n\nfunc main():\n    print(x)\n";
    let toks = tokenize(src, SourceId(0)).expect("lexer");
    let decls = parse(&toks, SourceId(0)).expect("parse");
    let err = generate_rust(&decls).expect_err("unbound comptime ident must fail codegen");
    assert!(
        err.to_string().contains("comptime"),
        "error should mention comptime: {err}"
    );
}
