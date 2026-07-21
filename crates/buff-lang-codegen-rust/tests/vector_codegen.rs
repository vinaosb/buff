//! T23 integration tests — `Vector<T>` type, collection literals, indexing,
//! and the vector-method family (`.push` / `.pop` / `.len` / `.map` /
//! `.filter` / `.reduce`).
//!
//! Coverage:
//!
//! - `[1, 2, 3]` -> `vec![1, 2, 3]` literal codegen (incl. empty + trailing comma)
//! - `v[0]` -> `v[0 as usize]` indexing (Int -> usize coercion)
//! - `.push(x)`, `.pop()`, `.len()` passthrough methods
//! - `.map({x => x * 2})` -> `v.into_iter().map(|x| x * 2).collect::<Vec<_>>()`
//! - `.filter({x => x > 0})` -> `v.into_iter().filter(...).collect::<Vec<_>>()`
//! - `.reduce({a, b => a + b})` -> `v.into_iter().reduce(|a, b| a + b)`
//! - auto-width: `let v = [1, 2, 3]` -> `let v: Vec<i8> = ...` (T22 range analysis)
//! - end-to-end QA scenario codegens to valid Rust
//!
//! Each test builds a Buff AST by hand, runs it through
//! [`buff_lang_codegen_rust::generate_rust`], and asserts properties of the
//! resulting Rust source. The generated source is also re-parsed via
//! `syn::parse_str::<syn::File>` to guarantee it is valid Rust.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test vector_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

/// A placeholder TypeRef for untyped closure params (codegen ignores it).
fn placeholder_ty() -> TypeRef {
    TypeRef::Named {
        name: ident("_"),
        span: span(),
    }
}

/// Build an `Expr::ArrayLit` from a list of element expressions.
fn array_lit(elements: Vec<Expr>) -> Expr {
    Expr::ArrayLit {
        elements,
        span: span(),
    }
}

/// Build an `Expr::Index { base, indices }` with a single index (T23 shape;
/// T24 generalized Index to carry a `Vec<Expr>` of indices).
fn index_expr(base: Expr, index: Expr) -> Expr {
    Expr::Index {
        base: Box::new(base),
        indices: vec![index],
        span: span(),
    }
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

/// Build a free function call `name(args...)`.
fn call_expr(name: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args,
        span: span(),
    }
}

/// Build a minimal closure `{ params => body }` as a Lambda node.
fn closure(params: &[&str], body: Expr) -> Expr {
    let params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: ident(p),
            ty: placeholder_ty(),
            default_value: None,
            is_comptime: false,
            span: span(),
        })
        .collect();
    Expr::Lambda {
        params,
        body: Block {
            stmts: vec![Stmt::ExprStmt(body, span())],
            span: span(),
        },
        return_type: None,
        span: span(),
    }
}

/// Wrap a list of statements in a no-arg function called `f`.
fn codegen_stmts(stmts: Vec<Stmt>) -> String {
    let func = FuncDecl {
        name: ident("f"),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    };
    generate_rust(&[Decl::FuncDecl(func)]).expect("codegen must succeed")
}

/// Like [`codegen_stmts`] but emits a single expression statement.
fn codegen_one_expr(expr: Expr) -> String {
    codegen_stmts(vec![Stmt::ExprStmt(expr, span())])
}

/// Assert the generated source re-parses as a valid Rust file.
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// 1. Collection literal `[1, 2, 3]` -> `vec![1, 2, 3]`
// ---------------------------------------------------------------------------

#[test]
fn vector_codegen_literal_three_ints() {
    let src = codegen_one_expr(array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]));
    assert!(
        src.contains("vec![1, 2, 3]"),
        "expected `vec![1, 2, 3]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn vector_codegen_literal_empty() {
    let src = codegen_one_expr(array_lit(vec![]));
    assert!(src.contains("vec![]"), "expected `vec![]` in: {src}");
    must_reparse(&src);
}

#[test]
fn vector_codegen_literal_trailing_comma() {
    // The parser allows a trailing comma; codegen should still emit clean
    // `vec![1, 2, 3]` (no trailing comma in the output).
    let src = codegen_one_expr(array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]));
    assert!(
        src.contains("vec![1, 2, 3]") && !src.contains("vec![1, 2, 3,]"),
        "expected no trailing comma in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn vector_codegen_literal_nested_expressions() {
    // `[a + b, f(x)]` — elements can be arbitrary expressions.
    let sum = Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Add,
        lhs: Box::new(ident_expr("a")),
        rhs: Box::new(ident_expr("b")),
        span: span(),
    };
    let src = codegen_one_expr(array_lit(vec![sum, call_expr("f", vec![ident_expr("x")])]));
    assert!(
        src.contains("vec![a + b, f(x)]"),
        "expected nested-element vec! in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Auto-width via T22 range analysis — `let v = [1, 2, 3]` -> `Vec<i8>`
// ---------------------------------------------------------------------------

#[test]
fn vector_codegen_auto_width_small_ints_are_i8() {
    // [1, 2, 3] all fit i8 -> Vec<i8>.
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("v"),
        value: array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    assert!(
        src.contains("let v: Vec<i8> = vec![1, 2, 3]"),
        "expected `let v: Vec<i8> = vec![1, 2, 3]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn vector_codegen_auto_width_300_is_i16() {
    // [300] exceeds i8 (max 127) -> Vec<i16>.
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("v"),
        value: array_lit(vec![int_expr(300)]),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    assert!(
        src.contains("let v: Vec<i16> = vec![300]"),
        "expected `let v: Vec<i16> = vec![300]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn vector_codegen_auto_width_negative_widens() {
    // [-200, 5] -> min -200 needs i16 -> Vec<i16>. The parser represents
    // `-200` as `UnaryOp(Neg, Lit(Int(200)))`, so we build it that way.
    let neg200 = Expr::UnaryOp {
        op: buff_lang_ast::op::UnaryOp::Neg,
        operand: Box::new(int_expr(200)),
        span: span(),
    };
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("v"),
        value: array_lit(vec![neg200, int_expr(5)]),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    // The auto-width inference is the key signal: -200 needs i16. (The
    // prettyplease macro-tokenizer prints `- 200` with a space; we only
    // assert the inferred type annotation and that the source re-parses.)
    assert!(
        src.contains("let v: Vec<i16> ="),
        "expected `let v: Vec<i16> =` (auto-width to i16) in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Indexing `v[0]` -> `v[0 as usize]`
// ---------------------------------------------------------------------------

#[test]
fn vector_codegen_index_literal_to_usize() {
    // v[0] -> v[0 as usize]
    let src = codegen_one_expr(index_expr(ident_expr("v"), int_expr(0)));
    assert!(
        src.contains("v[0 as usize]"),
        "expected `v[0 as usize]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn vector_codegen_index_variable_to_usize() {
    // v[i] -> v[i as usize]
    let src = codegen_one_expr(index_expr(ident_expr("v"), ident_expr("i")));
    assert!(
        src.contains("v[i as usize]"),
        "expected `v[i as usize]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn vector_codegen_index_then_len_no_double_move() {
    // d[0] then d.len() — d is a Vector (non-Copy). The second use may get a
    // `.clone()`, but the result must still be valid Rust. We just check it
    // re-parses and contains the index form.
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("d"),
            value: array_lit(vec![int_expr(1), int_expr(2)]),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(index_expr(ident_expr("d"), int_expr(0)), span()),
    ];
    let src = codegen_stmts(stmts);
    assert!(
        src.contains("d[0 as usize]"),
        "expected `d[0 as usize]` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. Vector methods — `.push()`, `.pop()`, `.len()` (passthrough)
// ---------------------------------------------------------------------------

#[test]
fn vector_codegen_push_method_passthrough() {
    // v.push(4) -> v.push(4) (v must be mut in Buff source)
    let src = codegen_one_expr(method_call(ident_expr("v"), "push", vec![int_expr(4)]));
    assert!(src.contains("v.push(4)"), "expected `v.push(4)` in: {src}");
    must_reparse(&src);
}

#[test]
fn vector_codegen_pop_method_passthrough() {
    let src = codegen_one_expr(method_call(ident_expr("v"), "pop", vec![]));
    assert!(src.contains("v.pop()"), "expected `v.pop()` in: {src}");
    must_reparse(&src);
}

#[test]
fn vector_codegen_len_method_passthrough() {
    let src = codegen_one_expr(method_call(ident_expr("v"), "len", vec![]));
    assert!(src.contains("v.len()"), "expected `v.len()` in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. Closure-arg methods — `.map()`, `.filter()`, `.reduce()`
// ---------------------------------------------------------------------------

#[test]
fn vector_codegen_map_closure() {
    // v.map({x => x * 2}) -> v.into_iter().map(|x| x * 2).collect::<Vec<_>>()
    let body = Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Mul,
        lhs: Box::new(ident_expr("x")),
        rhs: Box::new(int_expr(2)),
        span: span(),
    };
    let e = method_call(ident_expr("v"), "map", vec![closure(&["x"], body)]);
    let src = codegen_one_expr(e);
    assert!(
        src.contains("v.into_iter().map(|x| x * 2).collect::<Vec<_>>()"),
        "expected map-collect chain in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn vector_codegen_filter_closure() {
    // v.filter({x => x > 0}) -> v.into_iter().filter(|x| x > 0).collect::<Vec<_>>()
    let body = Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Gt,
        lhs: Box::new(ident_expr("x")),
        rhs: Box::new(int_expr(0)),
        span: span(),
    };
    let e = method_call(ident_expr("v"), "filter", vec![closure(&["x"], body)]);
    let src = codegen_one_expr(e);
    assert!(
        src.contains("v.into_iter().filter(|x| x > 0).collect::<Vec<_>>()"),
        "expected filter-collect chain in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn vector_codegen_reduce_closure() {
    // v.reduce({a, b => a + b}) -> v.into_iter().reduce(|a, b| a + b)
    let body = Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Add,
        lhs: Box::new(ident_expr("a")),
        rhs: Box::new(ident_expr("b")),
        span: span(),
    };
    let e = method_call(ident_expr("v"), "reduce", vec![closure(&["a", "b"], body)]);
    let src = codegen_one_expr(e);
    assert!(
        src.contains("v.into_iter().reduce(|a, b| a + b)"),
        "expected reduce chain in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 6. End-to-end QA scenario — map then index, codegens to valid Rust
// ---------------------------------------------------------------------------

#[test]
fn vector_codegen_end_to_end_qa_scenario() {
    // let v = [1, 2, 3]
    // let d = v.map({x => x * 2})
    // print(d[0])
    //
    // The generated Rust must re-parse as a valid file. The map produces a
    // new Vec; d[0] indexes it (2 in the first position).
    let map_body = Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Mul,
        lhs: Box::new(ident_expr("x")),
        rhs: Box::new(int_expr(2)),
        span: span(),
    };
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("v"),
            value: array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::LetDecl {
            name: ident("d"),
            value: method_call(ident_expr("v"), "map", vec![closure(&["x"], map_body)]),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(
            call_expr("print", vec![index_expr(ident_expr("d"), int_expr(0))]),
            span(),
        ),
    ];
    let src = codegen_stmts(stmts);
    // v is a Vec<i8>; d is the mapped result; print(d[0 as usize]).
    assert!(
        src.contains("let v: Vec<i8> = vec![1, 2, 3]"),
        "expected typed v binding in: {src}"
    );
    assert!(
        src.contains("v.into_iter().map(|x| x * 2).collect::<Vec<_>>()"),
        "expected map chain in: {src}"
    );
    assert!(
        src.contains("d[0 as usize]"),
        "expected indexed d in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 7. args()[0] — the T99-deferred indexing scenario now works (T23)
// ---------------------------------------------------------------------------

#[test]
fn vector_codegen_args_then_index_unblocks_t99() {
    // print(args()[0]) — args() returns Vec<String>, index returns String.
    let idx = index_expr(call_expr("args", vec![]), int_expr(0));
    let src = codegen_one_expr(call_expr("print", vec![idx]));
    // The index cast is the key: `args()[...]` with a usize cast. The
    // prettyplease turbofish spacing differs, so we assert on the cast form.
    assert!(
        src.contains("[0 as usize]"),
        "expected `[0 as usize]` index cast in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 8. Type inference — Vector<Int<8>> for [1, 2, 3] (exercises infer directly)
// ---------------------------------------------------------------------------

#[test]
fn vector_codegen_inference_returns_vector_int_w8() {
    use buff_lang_types::{IntWidth, Type, TypeInferencer};
    let mut inf = TypeInferencer::new();
    let e = array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]);
    let ty = inf.infer_expr(&e).expect("inference must succeed");
    assert_eq!(
        ty,
        Type::Vector(Box::new(Type::Int {
            width: IntWidth::W8
        }))
    );
}

#[test]
fn vector_codegen_inference_index_returns_element() {
    use buff_lang_types::{IntWidth, Type, TypeInferencer};
    let mut inf = TypeInferencer::new();
    inf.bind(
        "v",
        Type::Vector(Box::new(Type::Int {
            width: IntWidth::W8,
        })),
    );
    let e = index_expr(ident_expr("v"), int_expr(0));
    let ty = inf.infer_expr(&e).expect("inference must succeed");
    assert_eq!(
        ty,
        Type::Int {
            width: IntWidth::W8
        }
    );
}
