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
    // The core T58 capability: 2+ impls of the same Buff name each
    // become a unique Rust free function via mangling.
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
    ];
    let src = generate_rust(&decls).unwrap();
    assert!(
        src.contains("fn combine_int_int("),
        "expected mangled name combine_int_int; source was:\n{src}"
    );
    assert!(
        src.contains("fn combine_float_float("),
        "expected mangled name combine_float_float; source was:\n{src}"
    );
    assert!(
        !src.contains("fn combine("),
        "no impl should emit the unmangled name; source was:\n{src}"
    );
}

#[test]
fn t58_c4_mixed_single_and_multi_dispatch_coexist() {
    // A single-impl `foo` and a multi-impl `bar` should coexist:
    // `foo` stays unmangled, `bar`'s impls get mangled.
    let decls = vec![
        func("foo", &[("a", named("Int"))]),
        func("bar", &[("a", named("Int"))]),
        func("bar", &[("a", named("Float"))]),
    ];
    let src = generate_rust(&decls).unwrap();
    assert!(src.contains("fn foo("), "single-impl `foo` unmangled");
    assert!(
        src.contains("fn bar_int(") && src.contains("fn bar_float("),
        "multi-impl `bar` mangled; source was:\n{src}"
    );
    assert!(
        !src.contains("fn bar("),
        "no unmangled `bar` should remain; source was:\n{src}"
    );
}

#[test]
fn t58_c5_call_site_uses_mangled_name() {
    // A multi-dispatch CALL SITE should emit the mangled callee name
    // selected by inferred arg types. `combine(1, 2)` infers as
    // `(Int, Int)` so the int_impl is selected.
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
        span: sp(),
    };
    let decls = vec![
        func("combine", &[("a", named("Int")), ("b", named("Int"))]),
        func("combine", &[("a", named("Float")), ("b", named("Float"))]),
        Decl::FuncDecl(main_fn),
    ];
    let src = generate_rust(&decls).unwrap();
    assert!(
        src.contains("combine_int_int("),
        "call site `combine(1, 2)` should lower to `combine_int_int`; source was:\n{src}"
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
    assert!(src1.contains("combine_int_int"));
    let decls2 = vec![func("only_one", &[("a", named("Int"))])];
    let file2 = codegen.generate(&decls2).unwrap();
    let src2 = format_file(&file2);
    assert!(
        src2.contains("fn only_one(") && !src2.contains("combine"),
        "second generate() should rebuild the table; source was:\n{src2}"
    );
}
