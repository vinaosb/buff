//! T21 integration tests — string interpolation, Char literals, and the
//! string-method family.
//!
//! These tests cover the T21 surface:
//!
//! - `"Hello {name}!"` → `format!("Hello {}!", name)` interpolation codegen
//! - `'A'` → Char literal codegen (`'A'`)
//! - `.char_count()`, `.byte_len()`, `.chars()`, `.bytes()`, `.graphemes()`
//! - `.first()`, `.last()`, `.slice(a, b)`
//! - Multi-line `"""..."""` raw strings (lexed as plain String literals)
//!
//! Each test builds a Buff AST by hand, runs it through
//! [`buff_lang_codegen_rust::generate_rust`], and asserts properties of the
//! resulting Rust source. Snapshots use inline `assert_snapshot!(x, @"...")`.
//!
//! Run the whole module via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust string_methods
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, InterpPart, Literal, Stmt};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn char_expr(c: char) -> Expr {
    Expr::Literal(Literal::Char(c), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

/// Build `receiver.method(args...)` as an AST node.
fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: ident(method),
        args,
        span: span(),
    }
}

/// Build `Expr::StringInterp` from a flat list of parts.
fn interp(parts: Vec<InterpPart>) -> Expr {
    Expr::StringInterp {
        parts,
        span: span(),
    }
}

/// Wrap a single expression-statement in a no-arg function called `f` and
/// lower it to Rust source. Returns the generated source.
fn codegen_one_stmt(stmt: Stmt) -> String {
    let func = FuncDecl {
        name: ident("f"),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts: vec![stmt],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    generate_rust(&[Decl::FuncDecl(func)]).expect("codegen must succeed")
}

/// Like [`codegen_one_stmt`] but the function takes one `String` parameter
/// named `s` — used for method-receiver tests.
fn codegen_with_string_param(stmt: Stmt) -> String {
    let func = FuncDecl {
        name: ident("f"),
        params: vec![buff_lang_ast::common::Param {
            name: ident("s"),
            ty: buff_lang_ast::TypeRef::Named {
                name: ident("String"),
                span: span(),
            },
            default_value: None,
            is_comptime: false,
            span: span(),
        }],
        return_type: None,
        body: Block {
            stmts: vec![stmt],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    generate_rust(&[Decl::FuncDecl(func)]).expect("codegen must succeed")
}

// ---------------------------------------------------------------------------
// 1. String interpolation — `"Hello {name}!"` → `format!(...)`
// ---------------------------------------------------------------------------

#[test]
fn string_methods_interp_single_expression() {
    // `"Hello {name}!"` → `format!("Hello {}!", name)`
    let e = interp(vec![
        InterpPart::Literal("Hello ".to_string()),
        InterpPart::Expr(Box::new(ident_expr("name")), None),
        InterpPart::Literal("!".to_string()),
    ]);
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("greeting"),
        value: e,
        mutable: false,
        ty: None,
        span: span(),
    });
    assert!(
        src.contains(r#"format!("Hello {}!", name)"#),
        "expected format! call in: {src}"
    );
}

#[test]
fn string_methods_interp_multiple_expressions() {
    // `"{a} + {b} = {c}"` → `format!("{} + {} = {}", a, b, c)`
    let e = interp(vec![
        InterpPart::Expr(Box::new(ident_expr("a")), None),
        InterpPart::Literal(" + ".to_string()),
        InterpPart::Expr(Box::new(ident_expr("b")), None),
        InterpPart::Literal(" = ".to_string()),
        InterpPart::Expr(Box::new(ident_expr("c")), None),
    ]);
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("equation"),
        value: e,
        mutable: false,
        ty: None,
        span: span(),
    });
    assert!(
        src.contains(r#"format!("{} + {} = {}", a, b, c)"#),
        "expected multi-arg format! in: {src}"
    );
}

#[test]
fn string_methods_interp_with_arithmetic_expression() {
    // `"total = {price * qty}"` → `format!("total = {}", price * qty)`
    let inner = Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Mul,
        lhs: Box::new(ident_expr("price")),
        rhs: Box::new(ident_expr("qty")),
        span: span(),
    };
    let e = interp(vec![
        InterpPart::Literal("total = ".to_string()),
        InterpPart::Expr(Box::new(inner), None),
    ]);
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("msg"),
        value: e,
        mutable: false,
        ty: None,
        span: span(),
    });
    assert!(
        src.contains(r#"format!("total = {}", price * qty)"#),
        "expected arithmetic-arg format! in: {src}"
    );
}

#[test]
fn string_methods_interp_escapes_braces() {
    // `"use {x} literally {{not a slot}}"` — the literal `{`/`}` in a part
    // must be escaped to `{{`/`}}` in the format string.
    let e = interp(vec![
        InterpPart::Literal("literal braces: {".to_string()),
        InterpPart::Expr(Box::new(ident_expr("x")), None),
        InterpPart::Literal("}".to_string()),
    ]);
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("s"),
        value: e,
        mutable: false,
        ty: None,
        span: span(),
    });
    // The format string should escape the literal `{`/`}` as `{{`/`}}`.
    // `{{` = literal `{`, `{}` = slot, `}}` = literal `}` — so the output is
    // `{{{}}}` which when expanded with `x` gives `{<x>}`.
    assert!(
        src.contains(r#"format!("literal braces: {{{}}}", x)"#),
        "expected escaped braces in: {src}"
    );
}

#[test]
fn string_methods_interp_only_expression_no_literal() {
    // `"{x}"` → `format!("{}", x)` — no surrounding literal text.
    let e = interp(vec![InterpPart::Expr(Box::new(ident_expr("x")), None)]);
    let src = codegen_one_stmt(Stmt::ExprStmt(e, span()));
    assert!(
        src.contains(r#"format!("{}", x)"#),
        "expected bare-expr format! in: {src}"
    );
}

// ---------------------------------------------------------------------------
// 2. Char literal — `'A'` → Rust `'A'`
// ---------------------------------------------------------------------------

#[test]
fn string_methods_char_ascii_literal() {
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("c"),
        value: char_expr('A'),
        mutable: false,
        ty: None,
        span: span(),
    });
    assert!(src.contains("let c: char = 'A'"), "src = {src}");
}

#[test]
fn string_methods_char_multibyte_latin() {
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("c"),
        value: char_expr('é'),
        mutable: false,
        ty: None,
        span: span(),
    });
    assert!(src.contains("let c: char = 'é'"), "src = {src}");
}

#[test]
fn string_methods_char_emoji_literal() {
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("c"),
        value: char_expr('🚀'),
        mutable: false,
        ty: None,
        span: span(),
    });
    assert!(src.contains("let c: char = '🚀'"), "src = {src}");
}

#[test]
fn string_methods_char_escape_literal() {
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("nl"),
        value: char_expr('\n'),
        mutable: false,
        ty: None,
        span: span(),
    });
    assert!(src.contains("let nl: char = '\\n'"), "src = {src}");
}

// ---------------------------------------------------------------------------
// 3. String methods — `.char_count()`, `.byte_len()`, `.chars()`, `.bytes()`
// ---------------------------------------------------------------------------

#[test]
fn string_methods_char_count_maps_to_chars_count() {
    // `s.char_count()` → `s.chars().count()`
    let e = method_call(ident_expr("s"), "char_count", vec![]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(
        src.contains("s.chars().count()"),
        "expected `s.chars().count()` in: {src}"
    );
}

#[test]
fn string_methods_byte_len_maps_to_len() {
    // `s.byte_len()` → `s.len()`
    let e = method_call(ident_expr("s"), "byte_len", vec![]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(src.contains("s.len()"), "expected `s.len()` in: {src}");
}

#[test]
fn string_methods_chars_maps_to_chars() {
    // `s.chars()` → `s.chars()` (Rust has the same name)
    let e = method_call(ident_expr("s"), "chars", vec![]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(
        src.contains("s.chars()") && !src.contains("s.chars()."),
        "expected bare `s.chars()` (not chained) in: {src}"
    );
}

#[test]
fn string_methods_bytes_maps_to_bytes() {
    // `s.bytes()` → `s.bytes()` (Rust has the same name)
    let e = method_call(ident_expr("s"), "bytes", vec![]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(src.contains("s.bytes()"), "expected `s.bytes()` in: {src}");
}

// ---------------------------------------------------------------------------
// 4. `.first()`, `.last()`, `.slice(a, b)`
// ---------------------------------------------------------------------------

#[test]
fn string_methods_first_maps_to_chars_next() {
    // `s.first()` → `s.chars().next()`
    let e = method_call(ident_expr("s"), "first", vec![]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(
        src.contains("s.chars().next()"),
        "expected `s.chars().next()` in: {src}"
    );
}

#[test]
fn string_methods_last_maps_to_chars_last() {
    // `s.last()` → `s.chars().last()`
    let e = method_call(ident_expr("s"), "last", vec![]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(
        src.contains("s.chars().last()"),
        "expected `s.chars().last()` in: {src}"
    );
}

#[test]
fn string_methods_slice_two_args() {
    // `s.slice(0, 5)` → `s.chars().skip(0).take(5 - 0).collect::<String>()`
    let e = method_call(ident_expr("s"), "slice", vec![int_expr(0), int_expr(5)]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(
        src.contains("s.chars().skip(0).take(5 - 0).collect::<String>()"),
        "expected char-safe slice in: {src}"
    );
}

#[test]
fn string_methods_slice_one_arg() {
    // `s.slice(2)` → `s.chars().skip(2).collect::<String>()`
    let e = method_call(ident_expr("s"), "slice", vec![int_expr(2)]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(
        src.contains("s.chars().skip(2).collect::<String>()"),
        "expected char-safe slice (no take) in: {src}"
    );
}

// ---------------------------------------------------------------------------
// 5. `.graphemes()` — unicode-segmentation wiring
// ---------------------------------------------------------------------------

#[test]
fn string_methods_graphemes_maps_to_unicode_segmentation() {
    // `s.graphemes()` → `unicode_segmentation::UnicodeSegmentation::graphemes(&s, true).collect::<String>()`
    let e = method_call(ident_expr("s"), "graphemes", vec![]);
    let src = codegen_with_string_param(Stmt::ExprStmt(e, span()));
    assert!(
        src.contains("unicode_segmentation::UnicodeSegmentation::graphemes(&s, true)"),
        "expected fully-qualified graphemes call in: {src}"
    );
    assert!(
        src.contains(".collect::<String>()"),
        "expected `.collect::<String>()` in: {src}"
    );
}

// ---------------------------------------------------------------------------
// 6. Multi-line raw strings `"""..."""` (parsed as plain String literals)
// ---------------------------------------------------------------------------

#[test]
fn string_methods_multiline_raw_string_preserves_content() {
    // A triple-quoted string lexes/parses as Literal::String with newlines
    // preserved. Codegen should emit a Rust string literal whose content
    // matches.
    let raw = "line1\nline2";
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("s"),
        value: string_expr(raw),
        mutable: false,
        ty: None,
        span: span(),
    });
    // prettyplease prints Rust string literals with `\n` escapes for
    // embedded newlines, so we check for the escaped form.
    assert!(
        src.contains(r#""line1\nline2""#),
        "expected escaped-newline string literal in: {src}"
    );
}

// ---------------------------------------------------------------------------
// 7. End-to-end snapshot — interpolation + a string method
// ---------------------------------------------------------------------------

#[test]
fn string_methods_combined_interp_and_method_snapshot() {
    // `let len = "Hello {name}!".char_count()` — mix interpolation and
    // method call on the resulting interpolated string.
    let interp_expr = interp(vec![
        InterpPart::Literal("Hello ".to_string()),
        InterpPart::Expr(Box::new(ident_expr("name")), None),
        InterpPart::Literal("!".to_string()),
    ]);
    let method = method_call(interp_expr, "char_count", vec![]);
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("len"),
        value: method,
        mutable: false,
        ty: None,
        span: span(),
    });
    // The method chain should wrap the format! call:
    //   `format!("Hello {}!", name).chars().count()`
    assert!(
        src.contains(r#"format!("Hello {}!", name).chars().count()"#),
        "expected nested format!().chars().count() in: {src}"
    );
    // Verify the generated source re-parses as valid Rust.
    syn::parse_str::<syn::File>(&src).expect("generated source must re-parse");
}

// ---------------------------------------------------------------------------
// 8. Unicode behaviour — `"café".chars()` yields 4 chars
// ---------------------------------------------------------------------------

#[test]
fn string_methods_unicode_cafe_yields_four_chars() {
    // This is a documentation-style test: the generated code is correct
    // because `.chars().count()` counts scalar values (é is one). We assert
    // the codegen shape; the runtime behaviour is Rust's std guarantee.
    let s = string_expr("café");
    let e = method_call(s, "char_count", vec![]);
    let src = codegen_one_stmt(Stmt::LetDecl {
        name: ident("n"),
        value: e,
        mutable: false,
        ty: None,
        span: span(),
    });
    // `"café".chars().count()` — prettyplease keeps the non-ASCII bytes.
    assert!(
        src.contains(r#""café".chars().count()"#),
        "expected café chars().count() in: {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("unicode test must re-parse");
}
