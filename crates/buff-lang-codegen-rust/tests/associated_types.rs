//! T75b integration tests — Rust codegen for associated types in traits and
//! `impl Trait for Type { ... }` blocks.
//!
//! Coverage:
//!
//! - `trait Container { type Item; fn get() -> Item; }` → emits a Rust
//!   trait with `type Item;` (bodyless associated-type declaration).
//! - `trait Container { type Item: Clone + Debug; ... }` → emits
//!   `type Item: Clone + Debug;` (Rust `+`-separated bound syntax).
//! - `impl Container for Box { type Item = Int; }` → emits a Rust trait
//!   impl with `type Item = i64;` binding.
//! - `impl Greetable for Person { fn greet() { ... } }` → emits a Rust
//!   trait impl with a method body.
//! - End-to-end: every generated Rust source re-parses as a valid
//!   `syn::File`.
//!
//! Each test builds a Buff AST by hand and runs it through
//! [`buff_lang_codegen_rust::generate_rust`], asserting on the resulting
//! Rust source. The parser is exercised separately in
//! `crates/buff-lang-parser/tests/associated_types.rs`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test associated_types
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{
    AssociatedType, AssociatedTypeBinding, FuncDecl, ImplBlock, MethodSig, TraitDecl,
};
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

/// Build an empty-body fn with a single `return "X"` statement.
fn string_return_body(text: &str) -> Block {
    Block {
        stmts: vec![Stmt::Return(
            Some(Expr::Literal(Literal::String(text.to_string()), span())),
            span(),
        )],
        span: span(),
    }
}

/// Build a Param from name + type name.
#[allow(dead_code)]
fn param(name: &str, ty: &str) -> Param {
    Param {
        name: ident(name),
        ty: named_ty(ty),
        default_value: None,
        is_comptime: false,
        span: span(),
    }
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

/// Codegen a slice of Decls and return the resulting Rust source.
fn codegen(decls: &[Decl]) -> String {
    generate_rust(decls).expect("codegen must succeed")
}

// ---------------------------------------------------------------------------
// 1. Trait with associated type → `type Item;` bodyless declaration.
// ---------------------------------------------------------------------------

#[test]
fn codegen_trait_associated_type_basic() {
    // `trait Container { type Item; fn get() -> Item; }`
    let t = TraitDecl {
        name: ident("Container"),
        supertraits: Vec::new(),
        associated_types: vec![AssociatedType {
            name: ident("Item"),
            bounds: Vec::new(),
            span: span(),
        }],
        required: vec![MethodSig {
            name: ident("get"),
            params: Vec::new(),
            return_type: Some(named_ty("Item")),
            span: span(),
        }],
        defaults: Vec::new(),
        span: span(),
    };
    let src = codegen(&[Decl::TraitDecl(t)]);
    assert!(
        src.contains("trait Container"),
        "expected `trait Container` in:\n{src}"
    );
    // The associated-type declaration `type Item;` MUST appear bodyless.
    assert!(
        src.contains("type Item;"),
        "expected bodyless `type Item;` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Bounded associated type → `type Item: Clone + Debug;`.
// ---------------------------------------------------------------------------

#[test]
fn codegen_trait_associated_type_with_bounds() {
    let t = TraitDecl {
        name: ident("Container"),
        supertraits: Vec::new(),
        associated_types: vec![AssociatedType {
            name: ident("Item"),
            bounds: vec![named_ty("Clone"), named_ty("Debug")],
            span: span(),
        }],
        required: Vec::new(),
        defaults: Vec::new(),
        span: span(),
    };
    let src = codegen(&[Decl::TraitDecl(t)]);
    assert!(
        src.contains("type Item: Clone + Debug;"),
        "expected `type Item: Clone + Debug;` (Rust `+`-separated bound syntax) in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Multiple associated types — all render.
// ---------------------------------------------------------------------------

#[test]
fn codegen_trait_multiple_associated_types() {
    let t = TraitDecl {
        name: ident("Map"),
        supertraits: Vec::new(),
        associated_types: vec![
            AssociatedType {
                name: ident("Key"),
                bounds: Vec::new(),
                span: span(),
            },
            AssociatedType {
                name: ident("Value"),
                bounds: Vec::new(),
                span: span(),
            },
        ],
        required: Vec::new(),
        defaults: Vec::new(),
        span: span(),
    };
    let src = codegen(&[Decl::TraitDecl(t)]);
    assert!(
        src.contains("type Key;") && src.contains("type Value;"),
        "expected both `type Key;` and `type Value;` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. Impl block with a type binding → `impl T for U { type X = Y; }`.
// ---------------------------------------------------------------------------

#[test]
fn codegen_impl_block_with_type_binding() {
    let i = ImplBlock {
        trait_name: named_ty("Container"),
        target: named_ty("Box"),
        type_bindings: vec![AssociatedTypeBinding {
            name: ident("Item"),
            target: named_ty("Int"),
            span: span(),
        }],
        methods: Vec::new(),
        span: span(),
    };
    let src = codegen(&[Decl::ImplBlock(i)]);
    assert!(
        src.contains("impl Container for Box"),
        "expected `impl Container for Box` in:\n{src}"
    );
    // The associated-type binding `type Item = i64;` (Buff Int → Rust i64).
    assert!(
        src.contains("type Item = i64;"),
        "expected `type Item = i64;` binding in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. Impl block with a method body — body literal survives lowering.
// ---------------------------------------------------------------------------

#[test]
fn codegen_impl_block_with_method_body() {
    let i = ImplBlock {
        trait_name: named_ty("Greetable"),
        target: named_ty("Person"),
        type_bindings: Vec::new(),
        methods: vec![FuncDecl {
            name: ident("greet"),
            params: Vec::new(),
            return_type: Some(named_ty("String")),
            body: string_return_body("hello"),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            type_params: Vec::new(),
            span: span(),
        }],
        span: span(),
    };
    let src = codegen(&[Decl::ImplBlock(i)]);
    assert!(
        src.contains("impl Greetable for Person"),
        "expected `impl Greetable for Person` in:\n{src}"
    );
    assert!(
        src.contains("\"hello\""),
        "expected body literal `\"hello\"` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 6. Full pair: trait declaration with assoc type + matching impl block.
//    (Smoke: both items land in the same generated source.)
// ---------------------------------------------------------------------------

#[test]
fn codegen_trait_decl_and_impl_pair() {
    let trait_decl = TraitDecl {
        name: ident("Container"),
        supertraits: Vec::new(),
        associated_types: vec![AssociatedType {
            name: ident("Item"),
            bounds: Vec::new(),
            span: span(),
        }],
        required: vec![MethodSig {
            name: ident("get"),
            params: Vec::new(),
            return_type: Some(named_ty("Item")),
            span: span(),
        }],
        defaults: Vec::new(),
        span: span(),
    };
    let impl_block = ImplBlock {
        trait_name: named_ty("Container"),
        target: named_ty("Box"),
        type_bindings: vec![AssociatedTypeBinding {
            name: ident("Item"),
            target: named_ty("Int"),
            span: span(),
        }],
        methods: Vec::new(),
        span: span(),
    };
    let src = codegen(&[Decl::TraitDecl(trait_decl), Decl::ImplBlock(impl_block)]);
    // Both the trait declaration AND the impl appear in the output.
    assert!(
        src.contains("trait Container") && src.contains("impl Container for Box"),
        "expected both `trait Container` and `impl Container for Box` in:\n{src}"
    );
    must_reparse(&src);
}
