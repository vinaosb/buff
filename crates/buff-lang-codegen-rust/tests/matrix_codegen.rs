//! T24 integration tests — `Matrix<T>` builtin type, flat-storage codegen,
//! and 2-D indexing `m[row, col]`.
//!
//! Coverage:
//!
//! - `Matrix.new(2, 3)` -> `Matrix::new(2, 3)` associated-fn call
//! - the builtin `Matrix<T>` struct is emitted ON-DEMAND when `Matrix.new`
//!   appears, and carries flat `data: Vec<T>` storage (NO `Vec<Vec<T>>`)
//! - `m[1, 2]` -> `m.data[(1 * m.cols + 2) as usize]` (literal flat index)
//! - `m[r, c]` -> `m.data[(r * m.cols + c) as usize]` (variable flat index)
//! - the Matrix struct is NOT emitted when the program never uses Matrix
//! - single-index `v[0]` still lowers to `v[0 as usize]` (T23 regression)
//! - inference: `Matrix.new(...)` -> `Type::Matrix(_)`
//! - inference: `m[r, c]` on `Matrix<T>` -> `T`
//! - end-to-end: construct + index in one program, re-parses as valid Rust
//!
//! Each test builds a Buff AST by hand, runs it through
//! [`buff_lang_codegen_rust::generate_rust`], and asserts properties of the
//! resulting Rust source. The generated source is also re-parsed via
//! `syn::parse_str::<syn::File>` to guarantee it is valid Rust (syn doesn't
//! type-check, but it pins the syntactic correctness).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test matrix_codegen
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

/// Build a minimal closure `{ params => body }` as a Lambda node.
#[allow(dead_code)]
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

/// Build `receiver.method(args...)` as an AST node (the canonical Buff
/// constructor `Type.new(...)` shape — `Matrix.new(2, 3)` parses this way).
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

/// Build an `Expr::Index` carrying a single index (1-D Vector path).
fn index1(base: Expr, index: Expr) -> Expr {
    Expr::Index {
        base: Box::new(base),
        indices: vec![index],
        span: span(),
    }
}

/// Build an `Expr::Index` carrying two indices (2-D Matrix path, T24).
fn index2(base: Expr, row: Expr, col: Expr) -> Expr {
    Expr::Index {
        base: Box::new(base),
        indices: vec![row, col],
        span: span(),
    }
}

/// `Matrix.new(rows, cols)` constructor call.
fn matrix_new(rows: i64, cols: i64) -> Expr {
    method_call(
        ident_expr("Matrix"),
        "new",
        vec![int_expr(rows), int_expr(cols)],
    )
}

/// Wrap a list of statements in a no-arg function called `f`.
fn codegen_stmts(stmts: Vec<Stmt>) -> String {
    let func = FuncDecl { name: ident("f"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts,
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    generate_rust(&[Decl::FuncDecl(func)]).expect("codegen must succeed")
}

/// Like [`codegen_stmts`] but emits a single expression statement.
fn codegen_one_expr(expr: Expr) -> String {
    codegen_stmts(vec![Stmt::ExprStmt(expr, span())])
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// 1. Matrix.new(rows, cols) -> Matrix::new(rows, cols)
// ---------------------------------------------------------------------------

#[test]
fn matrix_codegen_new_two_by_three() {
    // Matrix.new(2, 3) -> Matrix::new(2, 3)
    let src = codegen_one_expr(matrix_new(2, 3));
    assert!(
        src.contains("Matrix::new(2, 3)"),
        "expected `Matrix::new(2, 3)` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn matrix_codegen_new_three_by_three_square() {
    // Matrix.new(3, 3) -> Matrix::new(3, 3) — square matrix.
    let src = codegen_one_expr(matrix_new(3, 3));
    assert!(
        src.contains("Matrix::new(3, 3)"),
        "expected `Matrix::new(3, 3)` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn matrix_codegen_new_variable_dims() {
    // Matrix.new(rows, cols) with variable args -> Matrix::new(rows, cols).
    let src = codegen_one_expr(method_call(
        ident_expr("Matrix"),
        "new",
        vec![ident_expr("rows"), ident_expr("cols")],
    ));
    assert!(
        src.contains("Matrix::new(rows, cols)"),
        "expected `Matrix::new(rows, cols)` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Matrix<T> struct is emitted on-demand, flat (Vec, no Vec<Vec>)
// ---------------------------------------------------------------------------

#[test]
fn matrix_codegen_emits_flat_struct_on_demand() {
    // A program using Matrix.new must get the builtin struct prepended.
    let src = codegen_one_expr(matrix_new(2, 3));
    assert!(
        src.contains("struct Matrix<T>"),
        "expected `struct Matrix<T>` declaration in: {src}"
    );
    assert!(
        src.contains("data: Vec<T>"),
        "expected flat `data: Vec<T>` field in: {src}"
    );
    assert!(
        src.contains("rows: usize"),
        "expected `rows: usize` field in: {src}"
    );
    assert!(
        src.contains("cols: usize"),
        "expected `cols: usize` field in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn matrix_codegen_storage_is_flat_no_nesting() {
    // GPU-readiness: storage must be ONE contiguous Vec<T>, not Vec<Vec<T>>
    // (which would fragment and not be directly uploadable to a WGSL buffer).
    let src = codegen_one_expr(matrix_new(2, 3));
    assert!(
        !src.contains("Vec<Vec"),
        "Matrix storage must NOT be nested (`Vec<Vec<...>>`); found nesting in: {src}"
    );
    assert!(
        src.contains("data: Vec<T>"),
        "expected single flat `data: Vec<T>` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn matrix_codegen_emits_new_impl() {
    // The `new(rows, cols)` associated function must be emitted so
    // `Matrix::new(2, 3)` resolves at rustc time.
    let src = codegen_one_expr(matrix_new(2, 3));
    assert!(
        src.contains("impl<T: Default + Clone> Matrix<T>"),
        "expected `impl<T: Default + Clone> Matrix<T>` in: {src}"
    );
    assert!(
        src.contains("fn new(rows: usize, cols: usize)"),
        "expected `fn new(rows: usize, cols: usize)` in: {src}"
    );
    assert!(
        src.contains("vec![T::default(); rows * cols]"),
        "expected flat-fill `vec![T::default(); rows * cols]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn matrix_codegen_not_emitted_when_unused() {
    // A program with NO Matrix reference must NOT get the struct injected —
    // on-demand emission keeps non-Matrix programs clean.
    let src = codegen_one_expr(int_expr(42));
    assert!(
        !src.contains("struct Matrix"),
        "Matrix struct leaked into a non-Matrix program: {src}"
    );
    assert!(
        !src.contains("Matrix::new"),
        "Matrix::new leaked into a non-Matrix program: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. 2-D indexing m[row, col] -> m.data[(row * m.cols + col) as usize]
// ---------------------------------------------------------------------------

#[test]
fn matrix_codegen_index_literal_flat_formula() {
    // m[1, 2] -> m.data[(1 * m.cols + 2) as usize]
    // The exact flat-index formula from the T24 spec.
    let src = codegen_one_expr(index2(ident_expr("m"), int_expr(1), int_expr(2)));
    assert!(
        src.contains("m.data[(1 * m.cols + 2) as usize]"),
        "expected `m.data[(1 * m.cols + 2) as usize]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn matrix_codegen_index_variable_flat_formula() {
    // m[r, c] -> m.data[(r * m.cols + c) as usize]
    let src = codegen_one_expr(index2(ident_expr("m"), ident_expr("r"), ident_expr("c")));
    assert!(
        src.contains("m.data[(r * m.cols + c) as usize]"),
        "expected `m.data[(r * m.cols + c) as usize]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn matrix_codegen_index_zero_zero() {
    // m[0, 0] -> m.data[(0 * m.cols + 0) as usize] — the top-left element.
    let src = codegen_one_expr(index2(ident_expr("m"), int_expr(0), int_expr(0)));
    assert!(
        src.contains("m.data[(0 * m.cols + 0) as usize]"),
        "expected `m.data[(0 * m.cols + 0) as usize]` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. Single-index regression — v[0] still lowers to v[0 as usize] (T23)
// ---------------------------------------------------------------------------

#[test]
fn matrix_codegen_single_index_unchanged() {
    // The T24 Index generalization must not regress 1-D Vector indexing.
    // v[0] -> v[0 as usize] (no `.data` field access — that's Matrix-only).
    let src = codegen_one_expr(index1(ident_expr("v"), int_expr(0)));
    assert!(
        src.contains("v[0 as usize]"),
        "expected 1-D `v[0 as usize]` (unchanged from T23) in: {src}"
    );
    assert!(
        !src.contains(".data["),
        "1-D index must NOT use the Matrix `.data` path; found in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. Type inference — Matrix.new -> Type::Matrix, m[r,c] -> element
// ---------------------------------------------------------------------------

#[test]
fn matrix_codegen_inference_matrix_new_returns_matrix_type() {
    use buff_lang_types::{Type, TypeInferencer};
    let mut inf = TypeInferencer::new();
    let e = matrix_new(2, 3);
    let ty = inf.infer_expr(&e).expect("inference must succeed");
    assert!(
        matches!(ty, Type::Matrix(_)),
        "expected Type::Matrix(_), got {ty}"
    );
}

#[test]
fn matrix_codegen_inference_2d_index_returns_element() {
    use buff_lang_types::{IntWidth, Type, TypeInferencer};
    let mut inf = TypeInferencer::new();
    // Bind m: Matrix<Int<32>>, then m[r, c] should infer Int<32>.
    inf.bind(
        "m",
        Type::matrix(Type::Int {
            width: IntWidth::W32,
        }),
    );
    let e = index2(ident_expr("m"), ident_expr("r"), ident_expr("c"));
    let ty = inf.infer_expr(&e).expect("inference must succeed");
    assert_eq!(
        ty,
        Type::Int {
            width: IntWidth::W32
        },
        "m[r, c] on Matrix<Int<32>> should yield Int<32>, got {ty}"
    );
}

// ---------------------------------------------------------------------------
// 6. End-to-end QA scenario — construct + index, valid Rust
// ---------------------------------------------------------------------------

#[test]
fn matrix_codegen_end_to_end_qa_scenario() {
    // let m = Matrix.new(2, 3)
    // print(m[1, 2])
    //
    // The generated Rust must re-parse as a valid file. The Matrix struct
    // is emitted, Matrix::new constructs it, and the 2-D index lowers to
    // the flat-storage access. The print path is the prelude fast path.
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("m"),
            value: matrix_new(2, 3),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::ExprStmt(
            call_expr(
                "print",
                vec![index2(ident_expr("m"), int_expr(1), int_expr(2))],
            ),
            span(),
        ),
    ];
    let src = codegen_stmts(stmts);
    assert!(
        src.contains("struct Matrix<T>"),
        "expected Matrix struct emission in: {src}"
    );
    assert!(
        src.contains("Matrix::new(2, 3)"),
        "expected Matrix::new(2, 3) in: {src}"
    );
    assert!(
        src.contains("m.data[(1 * m.cols + 2) as usize]"),
        "expected flat-index access in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn matrix_codegen_struct_emitted_once_for_multiple_uses() {
    // A program with two Matrix.new calls must still emit the struct exactly
    // ONCE (the on-demand scan is program-level, not per-statement).
    let stmts = vec![
        Stmt::LetDecl {
            name: ident("a"),
            value: matrix_new(2, 2),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::LetDecl {
            name: ident("b"),
            value: matrix_new(3, 3),
            mutable: false,
            ty: None,
            span: span(),
        },
    ];
    let src = codegen_stmts(stmts);
    let struct_count = src.matches("struct Matrix<T>").count();
    assert_eq!(
        struct_count, 1,
        "Matrix struct must be emitted exactly once (got {struct_count}) in: {src}"
    );
    let impl_count = src.matches("impl<T: Default + Clone> Matrix<T>").count();
    assert_eq!(
        impl_count, 1,
        "Matrix impl must be emitted exactly once (got {impl_count}) in: {src}"
    );
    must_reparse(&src);
}
