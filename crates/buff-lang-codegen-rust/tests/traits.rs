//! T93 integration tests — Rust codegen for `trait Name [: Super] { ... }`
//! declarations with default methods and inheritance.
//!
//! Coverage:
//!
//! - `trait Greetable { fn name() -> String; fn greet() { ... } }`
//!   → emits `pub trait Greetable { fn name() -> String; fn greet() { ... } }`.
//!   Required methods are bodyless; default methods carry a body (Rust
//!   default-method syntax).
//! - Supertrait inheritance: `trait Pet : Animal { ... }`
//!   → emits `pub trait Pet: Animal { ... }`.
//! - End-to-end: the generated Rust re-parses as a valid `syn::File`.
//!
//! Each test builds a Buff AST by hand and runs it through
//! [`buff_lang_codegen_rust::generate_rust`], asserting on the resulting
//! Rust source. The parser is exercised separately in
//! `crates/buff-lang-parser/tests/traits.rs`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test traits
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{FuncDecl, MethodSig, TraitDecl};
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
fn param(name: &str, ty: &str) -> Param {
    Param {
        name: ident(name),
        ty: named_ty(ty),
        span: span(),
    }
}

/// Codegen a single TraitDecl.
fn codegen_trait(t: TraitDecl) -> String {
    generate_rust(&[Decl::TraitDecl(t)]).expect("trait codegen must succeed")
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// 1. Basic trait: required method is bodyless.
// ---------------------------------------------------------------------------

#[test]
fn traits_codegen_required_bodyless() {
    // `trait Greetable { fn name() -> String; }` — one required method.
    // The generated Rust must have `fn name() -> String;` (bodyless, with `;`).
    let t = TraitDecl {
        name: ident("Greetable"),
        supertraits: Vec::new(),
        required: vec![MethodSig {
            name: ident("name"),
            params: Vec::new(),
            return_type: Some(named_ty("String")),
            span: span(),
        }],
        defaults: Vec::new(),
        span: span(),
    };
    let src = codegen_trait(t);

    // Trait declaration present.
    assert!(
        src.contains("trait Greetable"),
        "expected `trait Greetable` in:\n{src}"
    );
    // Required method is bodyless (has `;`).
    assert!(
        src.contains("fn name() -> String;"),
        "expected bodyless `fn name() -> String;` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Default method carries a body.
// ---------------------------------------------------------------------------

#[test]
fn traits_codegen_default_body() {
    // `trait Greetable { fn greet() { return "hi" } }` — one default method.
    // The generated Rust must have `fn greet() { ... }` (with a body, no `;`).
    let t = TraitDecl {
        name: ident("Greetable"),
        supertraits: Vec::new(),
        required: Vec::new(),
        defaults: vec![FuncDecl {
            name: ident("greet"),
            params: Vec::new(),
            return_type: None,
            body: string_return_body("hi"),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: span(),
        }],
        span: span(),
    };
    let src = codegen_trait(t);

    assert!(
        src.contains("trait Greetable"),
        "expected `trait Greetable` in:\n{src}"
    );
    // Default method has a body (no trailing `;`, has `{`).
    assert!(
        src.contains("fn greet() {")
            || src.contains("fn greet()\n{")
            || src.contains("fn greet(){"),
        "expected `fn greet() {{` (default body) in:\n{src}"
    );
    // The body literal must appear.
    assert!(
        src.contains("\"hi\""),
        "expected body literal `\"hi\"` in:\n{src}"
    );
    // No bodyless `fn greet() ...;` should appear (the default has a body).
    assert!(
        !src.contains("fn greet();"),
        "default method must NOT be bodyless: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Mixed required + default (the spec's canonical example).
// ---------------------------------------------------------------------------

#[test]
fn traits_codegen_mixed_required_and_default() {
    // `trait Greetable { fn name() -> String; fn greet() { ... } }`
    let t = TraitDecl {
        name: ident("Greetable"),
        supertraits: Vec::new(),
        required: vec![MethodSig {
            name: ident("name"),
            params: Vec::new(),
            return_type: Some(named_ty("String")),
            span: span(),
        }],
        defaults: vec![FuncDecl {
            name: ident("greet"),
            params: Vec::new(),
            return_type: None,
            body: string_return_body("hello"),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: span(),
        }],
        span: span(),
    };
    let src = codegen_trait(t);

    assert!(src.contains("trait Greetable"));
    // Required: bodyless.
    assert!(src.contains("fn name() -> String;"));
    // Default: has body.
    assert!(
        src.contains("fn greet() {")
            || src.contains("fn greet()\n{")
            || src.contains("fn greet(){")
    );
    assert!(src.contains("\"hello\""));
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. Supertrait inheritance emits `: Super` in the trait header.
// ---------------------------------------------------------------------------

#[test]
fn traits_codegen_supertrait() {
    // `trait Pet : Animal { fn pet() { ... } }`
    let t = TraitDecl {
        name: ident("Pet"),
        supertraits: vec![named_ty("Animal")],
        required: Vec::new(),
        defaults: vec![FuncDecl {
            name: ident("pet"),
            params: Vec::new(),
            return_type: None,
            body: string_return_body("petting"),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: span(),
        }],
        span: span(),
    };
    let src = codegen_trait(t);

    // The trait header must include the supertrait.
    assert!(
        src.contains("trait Pet: Animal") || src.contains("trait Pet : Animal"),
        "expected `trait Pet: Animal` (or with spaces) in:\n{src}"
    );
    assert!(src.contains("fn pet"));
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. Multiple supertraits are `+`-separated in Rust.
// ---------------------------------------------------------------------------

#[test]
fn traits_codegen_multiple_supertraits() {
    // `trait A : B, C { fn m() { } }` → Rust `trait A: B + C { ... }`.
    let t = TraitDecl {
        name: ident("A"),
        supertraits: vec![named_ty("B"), named_ty("C")],
        required: Vec::new(),
        defaults: vec![FuncDecl {
            name: ident("m"),
            params: Vec::new(),
            return_type: None,
            body: string_return_body("x"),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: span(),
        }],
        span: span(),
    };
    let src = codegen_trait(t);

    // Both supertraits must appear in the header.
    assert!(
        src.contains("trait A:") || src.contains("trait A :"),
        "expected supertrait header in:\n{src}"
    );
    assert!(
        src.contains("B") && src.contains("C"),
        "expected both B and C supertraits in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 6. Required method with params + self receiver.
// ---------------------------------------------------------------------------

#[test]
fn traits_codegen_required_with_self() {
    // `trait Foo { fn greet(self) -> String; }` — required method with a
    // `self` receiver. The codegen rewrites `self: Self` → bare `self`.
    let t = TraitDecl {
        name: ident("Foo"),
        supertraits: Vec::new(),
        required: vec![MethodSig {
            name: ident("greet"),
            params: vec![param("self", "Self")],
            return_type: Some(named_ty("String")),
            span: span(),
        }],
        defaults: Vec::new(),
        span: span(),
    };
    let src = codegen_trait(t);

    assert!(src.contains("trait Foo"));
    // The `self` param is rewritten to a bare receiver.
    assert!(
        src.contains("fn greet(self) -> String;"),
        "expected `fn greet(self) -> String;` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 7. Empty trait (zero required + zero defaults) is still valid.
// (The parser rejects empty bodies, but codegen should handle the AST
// shape gracefully — a trait with zero methods is valid Rust.)
// ---------------------------------------------------------------------------

#[test]
fn traits_codegen_empty_trait_valid() {
    let t = TraitDecl {
        name: ident("Empty"),
        supertraits: Vec::new(),
        required: Vec::new(),
        defaults: Vec::new(),
        span: span(),
    };
    let src = codegen_trait(t);
    assert!(
        src.contains("trait Empty"),
        "expected `trait Empty` in:\n{src}"
    );
    must_reparse(&src);
}
