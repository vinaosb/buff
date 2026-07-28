//! T34 integration tests â€” closures / lambdas codegen.
//!
//! Builds on T23's minimal closure support (`{ params => expr }` â†’
//! `|params| expr`) and T34's **variable capture** analysis.
//!
//! Coverage:
//!
//! - `{ x => x * 2 }` â†’ `|x| x * 2` (single-param)
//! - `{ x, y => x + y }` â†’ `|x, y| x + y` (multi-param)
//! - closure captures external Copy var: `let f = 10; { x => x + f }` â†’ `|x| x + f`
//! - closure in `.map()`: `[1,2,3].map({ x => x + f })` captures `f`
//! - closure in `.filter()`: `[1,2,3].filter({ x => x > f })`
//! - nested closure: `{ x => { y => x + y } }`
//! - closure returning a computed expr
//! - captured non-Copy var used multiple times inside a closure body does
//!   NOT get a spurious `.clone()` (T34 capture-aware codegen)
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test closures
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

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
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

/// Binary op shorthand.
fn binary_op(op: buff_lang_ast::op::BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
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

/// Build a closure whose body is itself a closure (nested).
fn nested_closure(outer_params: &[&str], inner_params: &[&str], inner_body: Expr) -> Expr {
    let inner = closure(inner_params, inner_body);
    let outer_params: Vec<Param> = outer_params
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
        params: outer_params,
        body: Block {
            stmts: vec![Stmt::ExprStmt(inner, span())],
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
        type_params: Vec::new(),
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
// 1. Single-param closure: `{ x => x * 2 }` â†’ `|x| x * 2`
// ---------------------------------------------------------------------------

#[test]
fn closures_single_param() {
    // { x => x * 2 } â†’ |x| x * 2
    let body = binary_op(
        buff_lang_ast::op::BinaryOp::Mul,
        ident_expr("x"),
        int_expr(2),
    );
    let src = codegen_one_expr(closure(&["x"], body));
    assert!(src.contains("|x| x * 2"), "expected `|x| x * 2` in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Multi-param closure: `{ x, y => x + y }` â†’ `|x, y| x + y`
// ---------------------------------------------------------------------------

#[test]
fn closures_multi_param() {
    // { x, y => x + y } â†’ |x, y| x + y
    let body = binary_op(
        buff_lang_ast::op::BinaryOp::Add,
        ident_expr("x"),
        ident_expr("y"),
    );
    let src = codegen_one_expr(closure(&["x", "y"], body));
    assert!(
        src.contains("|x, y| x + y"),
        "expected `|x, y| x + y` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Closure captures external Copy variable: `let f = 10; { x => x + f }`
// ---------------------------------------------------------------------------

#[test]
fn closures_capture_external_copy_var() {
    // let f = 10
    // { x => x + f } â†’ |x| x + f   (f is captured; Copy â†’ no clone)
    let body = binary_op(
        buff_lang_ast::op::BinaryOp::Add,
        ident_expr("x"),
        ident_expr("f"),
    );
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("f"),
            value: int_expr(10),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(closure(&["x"], body), span()),
    ];
    let src = codegen_stmts(stmts);
    assert!(
        src.contains("|x| x + f"),
        "expected `|x| x + f` (capture f) in: {src}"
    );
    // f is Copy (Int literal) â€” no .clone() should appear.
    assert!(
        !src.contains("f.clone()"),
        "Copy capture should NOT get .clone(): {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. Closure in `.map()` with capture (the QA scenario)
// ---------------------------------------------------------------------------

#[test]
fn closures_map_with_capture() {
    // let f = 10
    // let r = [1, 2, 3].map({ x => x + f })
    // â†’ vec![1, 2, 3].into_iter().map(|x| x + f).collect::<Vec<_>>()
    let body = binary_op(
        buff_lang_ast::op::BinaryOp::Add,
        ident_expr("x"),
        ident_expr("f"),
    );
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("f"),
            value: int_expr(10),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::LetDecl {
            name: ident("r"),
            value: method_call(
                array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]),
                "map",
                vec![closure(&["x"], body)],
            ),
            mutable: false,
            ty: None,
            span: span(),
        },
    ];
    let src = codegen_stmts(stmts);
    assert!(
        src.contains(".map(|x| x + f).collect::<Vec<_>>()"),
        "expected map-collect capturing f in: {src}"
    );
    assert!(
        !src.contains("f.clone()"),
        "Copy capture f should NOT get .clone(): {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. Closure in `.filter()` with capture
// ---------------------------------------------------------------------------

#[test]
fn closures_filter_with_capture() {
    // let threshold = 0
    // [1, 2, 3].filter({ x => x > threshold })
    // â†’ vec![...].into_iter().filter(|x| x > threshold).collect::<Vec<_>>()
    let body = binary_op(
        buff_lang_ast::op::BinaryOp::Gt,
        ident_expr("x"),
        ident_expr("threshold"),
    );
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("threshold"),
            value: int_expr(0),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(
            method_call(
                array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]),
                "filter",
                vec![closure(&["x"], body)],
            ),
            span(),
        ),
    ];
    let src = codegen_stmts(stmts);
    assert!(
        src.contains(".filter(|x| x > threshold).collect::<Vec<_>>()"),
        "expected filter-collect capturing threshold in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 6. Nested closure: `{ x => { y => x + y } }`
// ---------------------------------------------------------------------------

#[test]
fn closures_nested() {
    // { x => { y => x + y } } â†’ |x| |y| x + y
    let inner_body = binary_op(
        buff_lang_ast::op::BinaryOp::Add,
        ident_expr("x"),
        ident_expr("y"),
    );
    let src = codegen_one_expr(nested_closure(&["x"], &["y"], inner_body));
    // The outer closure's param x is captured by the inner closure.
    // Both closures lower to Rust closures; the body should contain
    // `|x| |y| x + y` (possibly with slight prettyplease spacing).
    assert!(
        src.contains("|x|") && src.contains("|y|"),
        "expected nested closures |x| ... |y| in: {src}"
    );
    assert!(src.contains("x + y"), "expected `x + y` body in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 7. Closure returning a computed expression
// ---------------------------------------------------------------------------

#[test]
fn closures_returning_computed_expr() {
    // { x => x * x + x } â†’ |x| x * x + x
    let body = binary_op(
        buff_lang_ast::op::BinaryOp::Add,
        binary_op(
            buff_lang_ast::op::BinaryOp::Mul,
            ident_expr("x"),
            ident_expr("x"),
        ),
        ident_expr("x"),
    );
    let src = codegen_one_expr(closure(&["x"], body));
    assert!(
        src.contains("x * x + x"),
        "expected `x * x + x` body in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 8. Captured non-Copy var used multiple times in closure body â€” NO spurious clone
//    (T34 capture-aware codegen: the key interaction with MoveAnalyzer)
// ---------------------------------------------------------------------------

#[test]
fn closures_captured_non_copy_no_spurious_clone_inside_body() {
    // let s = "hi"
    // [1].map({ x => x + s.len() + s.len() })
    //
    // s is non-Copy (String) and captured by the closure. It's used
    // TWICE inside the closure body. Without T34's capture-aware codegen,
    // the MoveAnalyzer would insert `.clone()` on the second use (seeing
    // it as "use after move"). With T34, the capture stack tells the
    // ident-lowering path to emit s plainly both times â€” Rust handles
    // the capture (by reference, since .len() only reads).
    let s_len = || method_call(ident_expr("s"), "len", vec![]);
    let body = binary_op(
        buff_lang_ast::op::BinaryOp::Add,
        binary_op(buff_lang_ast::op::BinaryOp::Add, ident_expr("x"), s_len()),
        s_len(),
    );
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("s"),
            value: string_expr("hi"),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(
            method_call(
                array_lit(vec![int_expr(1)]),
                "map",
                vec![closure(&["x"], body)],
            ),
            span(),
        ),
    ];
    let src = codegen_stmts(stmts);
    // The closure body should contain s.len() twice with NO .clone().
    assert!(
        !src.contains("s.clone()"),
        "captured non-Copy var used inside closure body should NOT get \
         spurious .clone(): {src}"
    );
    assert!(src.contains(".map(|x|"), "expected map closure in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 9. Captured non-Copy var used once inside closure + once after â€” the
//    after-use is OUTSIDE the closure so it goes through normal MoveAnalyzer.
//    This verifies the capture stack is correctly scoped (only affects
//    uses INSIDE the closure body).
// ---------------------------------------------------------------------------

#[test]
fn closures_capture_stack_scoped_to_body() {
    // let s = "hi"
    // [1].map({ x => x + s.len() })   // s captured, used once inside
    // print(s)                        // s used AFTER closure â€” normal path
    //
    // The closure-body use of s is a capture (no clone). The post-closure
    // use of s goes through the normal MoveAnalyzer path. Since s is
    // non-Copy and used once before (inside closure), the post-closure use
    // is the "second use" â†’ .clone() MAY be inserted (conservative: Rust
    // might have captured by ref, making the clone spurious but sound).
    // The key assertion: NO clone inside the closure body.
    let body = binary_op(
        buff_lang_ast::op::BinaryOp::Add,
        ident_expr("x"),
        method_call(ident_expr("s"), "len", vec![]),
    );
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("s"),
            value: string_expr("hi"),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(
            method_call(
                array_lit(vec![int_expr(1)]),
                "map",
                vec![closure(&["x"], body)],
            ),
            span(),
        ),
        Stmt::ExprStmt(call_expr("print", vec![ident_expr("s")]), span()),
    ];
    let src = codegen_stmts(stmts);
    // Inside the map closure, s should appear plainly (no clone).
    // The map closure body is `.map(|x| x + s.len())`.
    assert!(
        src.contains("x + s.len()"),
        "expected `x + s.len()` (no clone inside closure) in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 10. Zero-param closure (captures everything, takes no args)
//     { => f } â†’ || f   (edge case: no params, only captures)
// ---------------------------------------------------------------------------

#[test]
fn closures_zero_param_capture_only() {
    // let f = 42
    // { => f } â†’ || f
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("f"),
            value: int_expr(42),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(closure(&[], ident_expr("f")), span()),
    ];
    let src = codegen_stmts(stmts);
    // A zero-param closure lowers to `|| body`. Rust syntax allows `|| f`.
    assert!(
        src.contains("|| f"),
        "expected `|| f` zero-param closure in: {src}"
    );
    must_reparse(&src);
}
