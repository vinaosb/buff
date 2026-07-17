//! T75 integration tests — Rust codegen for `extend TYPE { fn ...; ... }`
//! extension-method blocks.
//!
//! Coverage:
//!
//! - `extend String { fn shout(self) -> String { self.to_uppercase() } }`
//!   → emits BOTH a `trait BuffExtString { fn shout(self) -> String; }`
//!   AND a `impl BuffExtString for String { fn shout(self) -> String {
//!   ... } }`. The method call site `"x".shout()` resolves because the
//!   trait is in scope (the codegen emits the impl, which Rust picks up
//!   automatically when the trait is `use`d — for single-file codegen the
//!   trait + impl are both top-level items in the same module, so
//!   resolution works without an explicit `use`).
//! - Multiple methods per block: `extend T { fn m1(self) { ... } fn m2()
//!   { ... } }` → trait with two signatures + impl with two bodies.
//! - Trait-name scheme: `BuffExt{Type}` (BuffExtString, BuffExtInt).
//! - End-to-end: the generated Rust re-parses as a valid `syn::File`.
//!
//! Each test builds a Buff AST by hand and runs it through
//! [`buff_lang_codegen_rust::generate_rust`], asserting on the resulting
//! Rust source. The parser is exercised separately in
//! `crates/buff-lang-parser/tests/extensions.rs`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test extensions
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{ExtendBlock, FuncDecl};
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn named_ty(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build a `self: Self`-style param (the receiver of an extension method).
fn self_param(target: &str) -> Param {
    Param {
        name: ident("self"),
        ty: named_ty(target),
        span: span(),
    }
}

/// Build an empty-body fn with a single `return "X"` statement, used as
/// the body of an extension method.
fn string_return_body(text: &str) -> Block {
    Block {
        stmts: vec![Stmt::Return(
            Some(Expr::Literal(Literal::String(text.to_string()), span())),
            span(),
        )],
        span: span(),
    }
}

/// Build a one-method `extend String { ... }` block.
fn extend_string_one_method(name: &str, body: Block, ret: TypeRef) -> ExtendBlock {
    ExtendBlock {
        target: named_ty("String"),
        methods: vec![FuncDecl {
            name: ident(name),
            params: vec![self_param("String")],
            return_type: Some(ret),
            body,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: span(),
        }],
        span: span(),
    }
}

/// Codegen a single ExtendBlock.
fn codegen_extend(e: ExtendBlock) -> String {
    generate_rust(&[Decl::ExtendBlock(e)]).expect("extend codegen must succeed")
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// Suppress unused warning for Param import (used implicitly via helpers).
#[allow(dead_code)]
fn _param_smoke() -> Param {
    Param {
        name: ident("_"),
        ty: named_ty("Int"),
        span: span(),
    }
}

// ---------------------------------------------------------------------------
// 1. Trait + impl pair — basic shape.
// ---------------------------------------------------------------------------

#[test]
fn extensions_trait_and_impl() {
    // `extend String { fn shout(self) -> String { ... } }` →
    // BOTH `trait BuffExtString { fn shout(self) -> String; }` AND
    //      `impl BuffExtString for String { fn shout(self) -> String { ... } }`.
    let block = extend_string_one_method("shout", string_return_body("SHOUT"), named_ty("String"));
    let src = codegen_extend(block);

    // Trait name follows the `BuffExt{Type}` scheme.
    assert!(
        src.contains("trait BuffExtString"),
        "expected trait BuffExtString in:\n{src}"
    );
    // The trait declares the method SIGNATURE (no body).
    assert!(
        src.contains("fn shout(self) -> String;"),
        "expected trait signature `fn shout(self) -> String;` in:\n{src}"
    );
    // The impl block implements the trait for String.
    assert!(
        src.contains("impl BuffExtString for String"),
        "expected `impl BuffExtString for String` in:\n{src}"
    );
    // The impl provides the method BODY (no trailing `;`).
    // We check that a `fn shout(self) -> String {` line appears (the body
    // block follows).
    assert!(
        src.contains("fn shout(self) -> String {"),
        "expected impl fn `fn shout(self) -> String {{` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Method body preserved verbatim from the FuncDecl.
// ---------------------------------------------------------------------------

#[test]
fn extensions_method_body() {
    // Body content must survive into the impl block.
    let block = extend_string_one_method(
        "shout",
        string_return_body("SHOUTED_BODY"),
        named_ty("String"),
    );
    let src = codegen_extend(block);

    // The body's return literal must appear in the impl side (not the
    // trait side, which has no body).
    assert!(
        src.contains("\"SHOUTED_BODY\""),
        "expected body literal `\"SHOUTED_BODY\"` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Multiple methods per extend block.
// ---------------------------------------------------------------------------

#[test]
fn extensions_multiple_methods() {
    let block = ExtendBlock {
        target: named_ty("String"),
        methods: vec![
            FuncDecl {
                name: ident("shout"),
                params: vec![self_param("String")],
                return_type: Some(named_ty("String")),
                body: string_return_body("SHOUT"),
                is_async: false,
                is_unsafe: false,
                is_extern: false,
                attributes: Vec::new(),
                span: span(),
            },
            FuncDecl {
                name: ident("whisper"),
                params: vec![self_param("String")],
                return_type: Some(named_ty("String")),
                body: string_return_body("whisper"),
                is_async: false,
                is_unsafe: false,
                is_extern: false,
                attributes: Vec::new(),
                span: span(),
            },
        ],
        span: span(),
    };
    let src = codegen_extend(block);

    // Both method signatures in the trait.
    assert!(src.contains("fn shout(self) -> String;"));
    assert!(src.contains("fn whisper(self) -> String;"));
    // Both method bodies in the impl.
    assert!(src.contains("fn shout(self) -> String {"));
    assert!(src.contains("fn whisper(self) -> String {"));
    assert!(src.contains("\"SHOUT\""));
    assert!(src.contains("\"whisper\""));
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. Trait-name scheme: BuffExt{Type}.
// ---------------------------------------------------------------------------

#[test]
fn extensions_trait_name_scheme_for_int() {
    // `extend Int { fn squared(self) -> Int { ... } }` → BuffExtInt.
    let block = ExtendBlock {
        target: named_ty("Int"),
        methods: vec![FuncDecl {
            name: ident("squared"),
            params: vec![self_param("Int")],
            return_type: Some(named_ty("Int")),
            body: Block {
                stmts: vec![Stmt::Return(
                    Some(Expr::Literal(Literal::Int(42), span())),
                    span(),
                )],
                span: span(),
            },
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: span(),
        }],
        span: span(),
    };
    let src = codegen_extend(block);
    assert!(
        src.contains("trait BuffExtInt"),
        "expected trait BuffExtInt in:\n{src}"
    );
    assert!(
        src.contains("impl BuffExtInt for i64"),
        "expected `impl BuffExtInt for i64` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. End-to-end: extend block + caller fn compiles to valid Rust.
// ---------------------------------------------------------------------------

#[test]
fn extensions_end_to_end_with_caller() {
    // `extend String { fn shout(self) -> String { return "SHOUT" } }`
    // followed by
    // `func caller(): { return "x".shout() }`
    let extend = Decl::ExtendBlock(extend_string_one_method(
        "shout",
        string_return_body("SHOUT"),
        named_ty("String"),
    ));
    let caller = Decl::FuncDecl(FuncDecl {
        name: ident("caller"),
        params: Vec::new(),
        return_type: Some(named_ty("String")),
        body: Block {
            stmts: vec![Stmt::Return(
                Some(Expr::MethodCall {
                    receiver: Box::new(Expr::Literal(Literal::String("x".to_string()), span())),
                    method: ident("shout"),
                    args: Vec::new(),
                    span: span(),
                }),
                span(),
            )],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    });
    let src = generate_rust(&[extend, caller]).expect("codegen must succeed");
    assert!(src.contains("trait BuffExtString"));
    assert!(src.contains("impl BuffExtString for String"));
    assert!(src.contains("fn caller() -> String"));
    must_reparse(&src);
}
