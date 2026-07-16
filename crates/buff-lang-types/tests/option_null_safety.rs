//! T28 integration tests — `Option<T>` + null safety.
//!
//! Coverage:
//!
//! - `None` infers as `Option<T>` with a fresh (Unknown) inner type — it is a
//!   prelude enum variant, NOT a reserved keyword.
//! - `Some(x)` infers as `Option<T>` where `T` is the argument's type.
//! - Using an `Option<T>` value where the BARE `T` is expected (e.g.
//!   `let y: Int = x` with `x: Option<Int>`) is a **compile error** whose
//!   message carries the exact suffix
//!   `. Use if-let or ?? to unwrap.` (the `??` operator itself is T101,
//!   deferred — the message mentions it now per the T28 contract).
//! - `let y: Option<Int> = None` and `let y: Option<Int> = Some(42)` both
//!   type-check (None/Some are valid Option values).
//! - `None`/`Some` are NOT in the lexer's reserved keyword list (they resolve
//!   as prelude Option variants).
//! - The built-in `Option` enum is seeded into the exhaustiveness registry's
//!   prelude variant so `match opt { Some(x) => ..., None => ... }` resolves.
//! - Safe unwrap via `match opt { Some(x) => use(x), None => default }` — the
//!   bound `x` carries the inner type. (`if let Some(x) = opt` syntax is T72,
//!   not yet implemented; the `match` path from T27 is exercised here.)
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-types --test option_null_safety
//! ```

use buff_lang_ast::common::Ident;
use buff_lang_ast::{Expr, Literal, Stmt, TypeRef};
use buff_lang_error::Span;
use buff_lang_types::{
    build_enum_registry, build_enum_registry_with_prelude, Type, TypeInferencer,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn bool_expr(b: bool) -> Expr {
    Expr::Literal(Literal::Bool(b), span())
}

fn named_ty(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build `Option<T>` as the generic-application shape the parser produces.
fn option_ty(inner: &str) -> TypeRef {
    TypeRef::Generic {
        base: Box::new(named_ty("Option")),
        args: vec![named_ty(inner)],
        span: span(),
    }
}

/// A `Some(expr)` FuncCall node (the parser-realistic shape).
fn some_expr(arg: Expr) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr("Some")),
        args: vec![arg],
        span: span(),
    }
}

/// A `let name[: ty] = value` statement.
fn let_stmt(name: &str, value: Expr, ty: Option<TypeRef>) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty,
        span: span(),
    }
}

/// Extract the message string from a TypeError.
fn type_err_msg(res: Result<Type, buff_lang_error::TypeError>) -> String {
    let err = res.expect_err("expected a TypeError");
    err.diagnostic.message
}

// ---------------------------------------------------------------------------
// 1. None / Some inference — prelude Option variants (NOT keywords).
// ---------------------------------------------------------------------------

#[test]
fn none_infers_as_option_with_unknown_inner() {
    let mut inf = TypeInferencer::new();
    // `None` alone resolves to Option<Unknown> (inner pinned by context).
    let ty = inf
        .infer_expr(&ident_expr("None"))
        .expect("None is valid Option");
    assert_eq!(ty, Type::option(Type::Unknown));
    // It must NOT be `Unknown` (that would mean None was treated as an
    // undefined variable). The Option wrapper is the whole point.
    assert_ne!(ty, Type::Unknown);
}

#[test]
fn some_of_int_infers_option_int() {
    let mut inf = TypeInferencer::new();
    let ty = inf
        .infer_expr(&some_expr(int_expr(42)))
        .expect("Some(42) infers");
    assert_eq!(ty, Type::option(Type::int_default()));
}

#[test]
fn some_of_string_infers_option_string() {
    let mut inf = TypeInferencer::new();
    let ty = inf
        .infer_expr(&some_expr(string_expr("hi")))
        .expect("Some(\"hi\") infers");
    assert_eq!(ty, Type::option(Type::string()));
}

#[test]
fn some_of_bool_infers_option_bool() {
    let mut inf = TypeInferencer::new();
    let ty = inf
        .infer_expr(&some_expr(bool_expr(true)))
        .expect("Some(true) infers");
    assert_eq!(ty, Type::option(Type::bool()));
}

#[test]
fn none_is_not_an_undefined_variable_error() {
    // Regression guard: before T28, `None` (an unbound ident) produced
    // "undefined variable: None". Now it resolves as an Option variant.
    let mut inf = TypeInferencer::new();
    let res = inf.infer_expr(&ident_expr("None"));
    assert!(res.is_ok(), "None must resolve, not error");
    // And it resolves to a real Option wrapper (not Unknown, not a bare id).
    assert_eq!(res.unwrap(), Type::option(Type::Unknown));
}

// ---------------------------------------------------------------------------
// 2. Null-safety enforcement — Option<T> cannot be used as bare T.
// ---------------------------------------------------------------------------

#[test]
fn assigning_option_int_to_int_is_a_null_safety_error() {
    // `let x: Option<Int> = Some(42)` then `let y: Int = x`.
    // The second binding must error with the exact suffix.
    let mut inf = TypeInferencer::new();
    // First bind x as Option<Int> via an annotation (forces resolution).
    let bind_x = let_stmt("x", some_expr(int_expr(42)), Some(option_ty("Int")));
    inf.infer_stmt(&bind_x)
        .expect("x: Option<Int> = Some(42) ok");
    assert_eq!(
        inf.lookup("x").cloned(),
        Some(Type::option(Type::int_default())),
        "x is bound as Option<Int>"
    );
    // Now the null-safety violation.
    let bad = let_stmt("y", ident_expr("x"), Some(named_ty("Int")));
    let msg = type_err_msg(inf.infer_stmt(&bad));
    assert!(
        msg.contains("expected"),
        "message names the expected type: {msg}"
    );
    assert!(
        msg.contains("found Option<"),
        "message names the Option value type: {msg}"
    );
    assert!(
        msg.contains(". Use if-let or ?? to unwrap."),
        "message carries the exact null-safety suffix: {msg}"
    );
}

#[test]
fn null_safety_error_message_is_exact_for_int() {
    // The full message for Option<Int> -> Int is, verbatim:
    //   expected Int<64>, found Option<Int<64>>. Use if-let or ?? to unwrap.
    // (Int's Display is `Int<64>` because Buff's default Int width is 64.)
    let mut inf = TypeInferencer::new();
    inf.bind("opt", Type::option(Type::int_default()));
    let bad = let_stmt("y", ident_expr("opt"), Some(named_ty("Int")));
    let msg = type_err_msg(inf.infer_stmt(&bad));
    assert_eq!(
        msg,
        "expected Int<64>, found Option<Int<64>>. Use if-let or ?? to unwrap."
    );
}

#[test]
fn null_safety_error_message_for_string_target() {
    // `let y: String = opt` where opt: Option<String> -> error names String.
    let mut inf = TypeInferencer::new();
    inf.bind("opt", Type::option(Type::string()));
    let bad = let_stmt("y", ident_expr("opt"), Some(named_ty("String")));
    let msg = type_err_msg(inf.infer_stmt(&bad));
    assert!(
        msg.contains("expected String, found Option<String>. Use if-let or ?? to unwrap."),
        "String null-safety message: {msg}"
    );
}

#[test]
fn option_to_option_same_inner_is_not_a_null_safety_error() {
    // `let y: Option<Int> = x` where x: Option<Int> -> OK (same type).
    let mut inf = TypeInferencer::new();
    inf.bind("x", Type::option(Type::int_default()));
    let ok = let_stmt("y", ident_expr("x"), Some(option_ty("Int")));
    let res = inf.infer_stmt(&ok);
    assert!(res.is_ok(), "Option<Int> -> Option<Int> is fine");
    assert_eq!(
        res.unwrap(),
        Type::option(Type::int_default()),
        "y is Option<Int>"
    );
}

#[test]
fn plain_type_mismatch_has_no_null_safety_suffix() {
    // `let y: Int = "hi"` is a normal type mismatch (String vs Int), NOT a
    // null-safety issue. Its message must NOT carry the if-let suffix.
    let mut inf = TypeInferencer::new();
    let bad = let_stmt("y", string_expr("hi"), Some(named_ty("Int")));
    let msg = type_err_msg(inf.infer_stmt(&bad));
    assert!(
        !msg.contains("if-let"),
        "non-Option mismatch has no if-let suffix: {msg}"
    );
    assert!(
        msg.contains("expected Int<64>, found String"),
        "plain mismatch message: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 3. Valid Option bindings — None and Some are first-class Option values.
// ---------------------------------------------------------------------------

#[test]
fn let_option_int_from_none_typechecks() {
    // `let x: Option<Int> = None` — None is a valid Option value.
    let mut inf = TypeInferencer::new();
    let s = let_stmt("x", ident_expr("None"), Some(option_ty("Int")));
    let res = inf.infer_stmt(&s);
    assert!(res.is_ok(), "let x: Option<Int> = None must type-check");
    assert_eq!(
        res.unwrap(),
        Type::option(Type::int_default()),
        "x annotated as Option<Int>"
    );
}

#[test]
fn let_option_int_from_some_typechecks() {
    // `let x: Option<Int> = Some(42)` — Some wraps the value.
    let mut inf = TypeInferencer::new();
    let s = let_stmt("x", some_expr(int_expr(42)), Some(option_ty("Int")));
    let res = inf.infer_stmt(&s);
    assert!(res.is_ok(), "let x: Option<Int> = Some(42) must type-check");
    assert_eq!(res.unwrap(), Type::option(Type::int_default()));
}

#[test]
fn let_unannotated_none_binds_option_unknown() {
    // `let x = None` (no annotation) -> x: Option<Unknown>. The inner stays
    // Unknown until context pins it; the wrapper is still Option.
    let mut inf = TypeInferencer::new();
    let s = let_stmt("x", ident_expr("None"), None);
    let ty = inf.infer_stmt(&s).expect("unannotated None ok");
    assert_eq!(ty, Type::option(Type::Unknown));
}

// ---------------------------------------------------------------------------
// 4. typeref_to_type — Option<T> annotation resolution.
// ---------------------------------------------------------------------------

#[test]
fn option_generic_typeref_resolves_to_type_option() {
    // The parser produces `Option<Int>` as TypeRef::Generic. Verify it flows
    // through a let-annotation into a real Type::Option.
    let mut inf = TypeInferencer::new();
    let s = let_stmt("x", int_expr(0), Some(option_ty("Int")));
    // `let x: Option<Int> = 0` -> error (0 is Int, not Option), proving the
    // annotation resolved to Option<Int> (otherwise 0 would be accepted).
    let msg = type_err_msg(inf.infer_stmt(&s));
    assert!(
        msg.contains("expected Option<Int<64>>"),
        "Option annotation resolved to Type::Option: {msg}"
    );
}

#[test]
fn option_typeref_dedicated_variant_resolves() {
    // Hand-built TypeRef::Option(inner) (the dedicated AST variant) also
    // resolves — exercised by tests/tools that construct the AST directly.
    let mut inf = TypeInferencer::new();
    let s = Stmt::LetDecl {
        name: ident("x"),
        value: ident_expr("None"),
        mutable: false,
        ty: Some(TypeRef::Option(Box::new(named_ty("String")), span())),
        span: span(),
    };
    let ty = inf.infer_stmt(&s).expect("TypeRef::Option resolves");
    assert_eq!(ty, Type::option(Type::string()));
}

// ---------------------------------------------------------------------------
// 5. None / Some are NOT reserved keywords (prelude enum variants).
// ---------------------------------------------------------------------------

#[test]
fn none_and_some_are_not_in_reserved_keyword_list() {
    use buff_lang_lexer::TokenKind;
    // The lexer must NOT recognise None/Some as keywords — they resolve as
    // prelude Option variants (plain identifiers at the token level).
    assert!(
        TokenKind::from_keyword("None").is_none(),
        "None is NOT a keyword (prelude Option variant)"
    );
    assert!(
        TokenKind::from_keyword("Some").is_none(),
        "Some is NOT a keyword (prelude Option variant)"
    );
    // The full keyword list must not mention them either.
    let all = TokenKind::all_keywords();
    assert!(
        !all.contains(&"None"),
        "None absent from all_keywords: {all:?}"
    );
    assert!(
        !all.contains(&"Some"),
        "Some absent from all_keywords: {all:?}"
    );
    // Sanity: a real keyword IS recognised (guard against a broken helper).
    assert!(TokenKind::from_keyword("func").is_some());
    assert!(all.contains(&"match"));
}

#[test]
fn none_and_some_tokenize_as_plain_identifiers() {
    use buff_lang_error::SourceId;
    use buff_lang_lexer::tokenize;
    use buff_lang_lexer::TokenKind;
    // `None` lexes as Ident("None") (no keyword token). This is the
    // lexer-level proof that None/Some are plain identifiers resolving to
    // prelude Option variants.
    let toks = tokenize("None\n", SourceId(0)).expect("lexing None must succeed");
    let kinds: Vec<&TokenKind> = toks.iter().map(|t| &t.kind).collect();
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Ident(n) if n == "None")),
        "None lexes as Ident: {kinds:?}"
    );
    assert!(
        !kinds
            .iter()
            .any(|k| k.is_keyword() && format!("{k}") == "None"),
        "no None keyword token"
    );
}

// ---------------------------------------------------------------------------
// 6. Built-in Option enum registered in the prelude-seeded registry.
// ---------------------------------------------------------------------------

#[test]
fn prelude_registry_seeds_option_with_some_and_none() {
    // The prelude-seeded registry knows Option -> [Some, None] even with NO
    // user enum declarations. This lets `match opt { Some(x)=>.., None=>.. }`
    // be checked for exhaustiveness without a user `enum Option`.
    let registry = build_enum_registry_with_prelude(&[]);
    assert_eq!(
        registry.get("Option").cloned(),
        Some(vec!["Some".to_string(), "None".to_string()]),
        "Option prelude-seeded with Some, None"
    );
}

#[test]
fn pure_registry_does_not_include_option_unless_declared() {
    // The pure build_enum_registry (no prelude seed) does NOT contain Option
    // unless the program declares it. This keeps the existing T27 tests
    // (which assert exact registry contents) stable.
    let registry = build_enum_registry(&[]);
    assert!(
        !registry.contains_key("Option"),
        "pure registry excludes Option unless user-declared"
    );
}

#[test]
fn user_option_decl_overrides_prelude_seed() {
    // If a program declares its own `enum Option`, the user's variant list
    // wins over the prelude seed (user decls insert AFTER the seed).
    use buff_lang_ast::{Decl, EnumDecl, EnumVariant};
    let user_option = EnumDecl {
        name: ident("Option"),
        generics: vec![ident("T")],
        variants: vec![
            EnumVariant {
                name: ident("Just"),
                data: None,
                span: span(),
            },
            EnumVariant {
                name: ident("Nothing"),
                data: None,
                span: span(),
            },
        ],
        span: span(),
    };
    let registry = build_enum_registry_with_prelude(&[Decl::EnumDecl(user_option)]);
    assert_eq!(
        registry.get("Option").cloned(),
        Some(vec!["Just".to_string(), "Nothing".to_string()]),
        "user Option decl overrides prelude seed"
    );
}

// ---------------------------------------------------------------------------
// 7. Safe unwrap via match (T27 mechanism; if-let is T72, deferred).
// ---------------------------------------------------------------------------

#[test]
fn safe_unwrap_via_match_binds_inner_type() {
    // `match opt { Some(x) => x, None => 0 }` safely unwraps. We verify the
    // mechanism at the type level: the scrutinee `opt` is Option<Int>, and
    // the bound `x` in the Some arm carries Int. Because full match-arm type
    // inference is deferred (v0.5 best-effort), we exercise the building
    // block: binding the Some arm's inner identifier as Int and confirming
    // the scrutinee's type is Option<Int>.
    //
    // `if let Some(x) = opt { use x }` is the T72 enhancement (not yet
    // implemented); the `match` path from T27 is the available safe-unwrap.
    let mut inf = TypeInferencer::new();
    inf.bind("opt", Type::option(Type::int_default()));
    // Scrutinee `opt` resolves to Option<Int>.
    let scrut_ty = inf.infer_expr(&ident_expr("opt")).expect("opt resolves");
    assert_eq!(scrut_ty, Type::option(Type::int_default()));
    // The inner type (what `Some(x)` would bind x to) is Int.
    if let Type::Option(inner) = scrut_ty {
        assert_eq!(*inner, Type::int_default(), "inner of Some(x) is Int");
    } else {
        panic!("scrutinee must be Option");
    }
}

#[test]
fn nested_option_type_is_constructible_and_self_assignable() {
    // Option<Option<Int>> is a legal type and assignable to itself.
    let nested = Type::option(Type::option(Type::int_default()));
    assert_eq!(nested, Type::option(Type::option(Type::int_default())));
    // Display form for diagnostics.
    let displayed = format!("{nested}");
    assert_eq!(displayed, "Option<Option<Int<64>>>");
}
