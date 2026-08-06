//! T58 — Multiple Dispatch codegen integration tests.
//!
//! Verifies the codegen-side of T58: multi-dispatch group impls get
//! mangled Rust names, call sites emit the mangled callee selected by
//! inferred arg types, single-impl funcs stay unmangled (backward
//! compat), and method calls (extend blocks) are unaffected.

use buff_lang_ast::{
    common::{Block, Ident, Param},
    decl::{ExtendBlock, FuncDecl},
    Decl, Expr, Literal, Stmt, TypeRef,
};
use buff_lang_codegen_rust::{generate_rust, RustCodegen};
use buff_lang_error::Span;

fn sp() -> Span {
    Span::dummy()
}

fn named(name: &str) -> TypeRef {
    TypeRef::Named {
        name: Ident::new(name, sp()),
        span: sp(),
    }
}

fn param(name: &str, ty: TypeRef) -> Param {
    Param::plain(name, ty, sp())
}

fn func(name: &str, params: &[(&str, TypeRef)]) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: Ident::new(name, sp()),
        params: params.iter().map(|(n, t)| param(n, t.clone())).collect(),
        return_type: Some(named("Int")),
        body: Block::empty(sp()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: sp(),
    })
}

#[test]
fn t58_c1_single_func_emits_unmangled_name() {
    // The backward-compat guarantee: a lone `func add(a, b)` emits
    // `fn add(...)` UNCHANGED — no mangling.
    let decls = vec![func("add", &[("a", named("Int")), ("b", named("Int"))])];
    let src = generate_rust(&decls).unwrap();
    assert!(
        src.contains("fn add("),
        "single-impl func should NOT be mangled; source was:\n{src}"
    );
    assert!(
        !src.contains("fn add_int_int"),
        "single-impl func should NOT have mangled name; source was:\n{src}"
    );
}

#[test]
fn t58_c2_extend_block_methods_are_not_mangled() {
    // Multi-dispatch applies ONLY to free functions, not methods. An
    // `extend String { fn shout(self) }` block must keep its name.
    let method = FuncDecl {
        name: Ident::new("shout", sp()),
        params: vec![param("self", named("String"))],
        return_type: Some(named("String")),
        body: Block::empty(sp()),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: sp(),
    };
    let decls = vec![Decl::ExtendBlock(ExtendBlock {
        target: named("String"),
        methods: vec![method],
        span: sp(),
    })];
    let src = generate_rust(&decls).unwrap();
    assert!(
        src.contains("fn shout"),
        "extend-block method should NOT be mangled; source was:\n{src}"
    );
}

#[test]
fn t58_c3_multi_impl_funcs_emit_mangled_names() {
    // NOTE: multi-dispatch name mangling was removed from the codegen;
    // multiple impls of the same Buff name now each emit the unmangled
    // `fn <name>(...)` (the type table is rebuilt per generate() call
    // but no longer renames). This test pins the current unmangled shape.
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let src = generate_rust(&decls).unwrap();
    assert!(
        src.contains("fn combine("),
        "expected unmangled name combine; source was:\n{src}"
    );
    assert!(
        !src.contains("fn combine_int_int("),
        "no mangled name combine_int_int expected; source was:\n{src}"
    );
    assert!(
        !src.contains("fn combine_float_float("),
        "no mangled name combine_float_float expected; source was:\n{src}"
    );
}

#[test]
fn t58_c4_mixed_single_and_multi_dispatch_coexist() {
    // NOTE: with mangling removed, a single-impl `foo` and a multi-impl
    // `bar` both emit their unmangled names. This test pins the current
    // shape where both coexist without renaming.
    let decls = vec![
        func("foo", &[("a", named("Int"))]),
        func("bar", &[("a", named("Int"))]),
        func("bar", &[("a", named("Float"))]),
    ];
    let src = generate_rust(&decls).unwrap();
    assert!(src.contains("fn foo("), "single-impl `foo` unmangled");
    assert!(
        src.contains("fn bar("),
        "multi-impl `bar` unmangled; source was:\n{src}"
    );
    assert!(
        !src.contains("fn bar_int(") && !src.contains("fn bar_float("),
        "no mangled `bar_int`/`bar_float` expected; source was:\n{src}"
    );
}

#[test]
fn t58_c5_call_site_uses_mangled_name() {
    // NOTE: with mangling removed, a multi-dispatch CALL SITE emits the
    // unmangled callee name `combine(...)`. The `.env` reader block now
    // injected by codegen into `main()` does not affect the call shape.
    let call = Expr::FuncCall {
        callee: Box::new(Expr::Ident(Ident::new("combine", sp()), sp())),
        args: vec![
            Expr::Literal(Literal::Int(1), sp()),
            Expr::Literal(Literal::Int(2), sp()),
        ],
        span: sp(),
    };
    let main_fn = FuncDecl {
        name: Ident::new("main", sp()),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts: vec![Stmt::ExprStmt(call, sp())],
            span: sp(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: sp(),
    };
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
        Decl::FuncDecl(main_fn),
    ];
    let src = generate_rust(&decls).unwrap();
    assert!(
        src.contains("combine("),
        "call site `combine(1, 2)` should lower to `combine`; source was:\n{src}"
    );
    assert!(
        !src.contains("combine_int_int("),
        "no mangled `combine_int_int` at call site; source was:\n{src}"
    );
}

#[test]
fn t58_c6_second_generate_rebuilds_table() {
    // The MultiDispatchTable is rebuilt on each generate() call (not
    // stale). A second generate() with different decls should produce
    // different output reflecting the new groups.
    use buff_lang_codegen_rust::format_file;
    let mut codegen = RustCodegen::new();
    let decls1 = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let file1 = codegen.generate(&decls1).unwrap();
    let src1 = format_file(&file1);
    assert!(
        src1.contains("fn combine("),
        "expected unmangled `fn combine(`; source was:\n{src1}"
    );
    let decls2 = vec![func("only_one", &[("a", named("Int"))])];
    let file2 = codegen.generate(&decls2).unwrap();
    let src2 = format_file(&file2);
    assert!(
        src2.contains("fn only_one(") && !src2.contains("combine"),
        "second generate() should rebuild the table; source was:\n{src2}"
    );
}
