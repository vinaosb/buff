//! T92 integration tests — struct embedding + auto-delegation codegen.
//!
//! When a struct `Employee` has a field whose type is another DECLARED
//! struct `Person` (`person: Person`), and `Person` has methods (via an
//! `extend Person { fn ... }` block), then a method call
//! `employee.name()` should auto-delegate to `employee.person.name()`.
//! The compiler generates the forwarding `impl Employee` block
//! automatically — no user-written delegation.
//!
//! Coverage:
//!
//! - Single embedded field, single method: `extend Person { fn name(self)
//!   -> String {...} }` + `struct Employee { person: Person, salary: Float }`
//!   → emits `impl Employee { fn name(self) -> String { self.person.name() } }`.
//! - Multiple methods on the embedded type: each is promoted as a separate
//!   delegation method.
//! - No methods on the embedded type → no delegation impl emitted at all.
//! - Method with extra params (beyond `self`): forwarded in order.
//! - Field whose type is NOT a declared struct (e.g. `salary: Float`) → no
//!   delegation (primitive types have no extend-block methods here).
//! - End-to-end: generated source re-parses as a valid `syn::File`.
//!
//! Each test builds a Buff AST by hand and runs it through
//! [`buff_lang_codegen_rust::generate_rust`], asserting on the resulting
//! Rust source.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust embedding
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{ExtendBlock, FuncDecl, StructDecl};
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

/// Build a `self: Target`-style param (the receiver of an extension method).
fn self_param(target: &str) -> Param {
    Param {
        name: ident("self"),
        ty: named_ty(target),
        default_value: None,
        is_comptime: false,
        span: span(),
    }
}

/// Build a `name: Ty` param.
fn typed_param(name: &str, ty: &str) -> Param {
    Param {
        name: ident(name),
        ty: named_ty(ty),
        default_value: None,
        is_comptime: false,
        span: span(),
    }
}

/// Build an empty-body fn whose body is `return "X"`.
fn string_return_body(text: &str) -> Block {
    Block {
        stmts: vec![Stmt::Return(
            Some(Expr::Literal(Literal::String(text.to_string()), span())),
            span(),
        )],
        span: span(),
    }
}

/// Build a `struct Name { fields... }` declaration.
fn struct_decl(name: &str, fields: Vec<(&str, TypeRef)>) -> StructDecl {
    StructDecl { name: ident(name),
    fields: fields.into_iter().map(|(n, t)| (ident(n), t)).collect(), traits: Vec::new(), type_params: Vec::new(), span: span(), }
}

/// Build a single-method `extend Target { fn name(self) -> Ret {...} }`.
fn extend_one_method(target: &str, name: &str, body: Block, ret: TypeRef) -> ExtendBlock {
    ExtendBlock {
        target: named_ty(target),
        methods: vec![FuncDecl {
            name: ident(name),
            params: vec![self_param(target)],
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

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// 1. Single embedded field, single method — the spec QA case.
// ---------------------------------------------------------------------------

#[test]
fn embedding_single_field_delegates() {
    // struct Person { name: String }
    // extend Person { fn name(self) -> String { return "alice" } }
    // struct Employee { person: Person, salary: Float }
    //
    // Expected delegation:
    //   impl Employee { fn name(self) -> String { self.person.name() } }
    let person = Decl::StructDecl(struct_decl("Person", vec![("name", named_ty("String"))]));
    let extend = Decl::ExtendBlock(extend_one_method(
        "Person",
        "name",
        string_return_body("alice"),
        named_ty("String"),
    ));
    let employee = Decl::StructDecl(struct_decl(
        "Employee",
        vec![
            ("person", named_ty("Person")),
            ("salary", named_ty("Float")),
        ],
    ));
    let src = generate_rust(&[person, extend, employee]).expect("codegen must succeed");

    // The delegation impl must target Employee.
    assert!(
        src.contains("impl Employee"),
        "expected `impl Employee` delegation block in:\n{src}"
    );
    // The delegation method `name` must appear (taking bare `self`).
    assert!(
        src.contains("fn name(self) -> String"),
        "expected delegation `fn name(self) -> String` in:\n{src}"
    );
    // The body must forward through self.person.
    assert!(
        src.contains("self.person.name()"),
        "expected delegation body `self.person.name()` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Multiple methods on the embedded type — each is promoted.
// ---------------------------------------------------------------------------

#[test]
fn embedding_multiple_methods() {
    // struct Person { name: String }
    // extend Person { fn name(self) -> String {...} ; fn age(self) -> Int {...} }
    // struct Employee { person: Person }
    let person = Decl::StructDecl(struct_decl("Person", vec![("name", named_ty("String"))]));
    let extend = Decl::ExtendBlock(ExtendBlock {
        target: named_ty("Person"),
        methods: vec![
            FuncDecl {
                name: ident("name"),
                params: vec![self_param("Person")],
                return_type: Some(named_ty("String")),
                body: string_return_body("bob"),
                is_async: false,
                is_unsafe: false,
                is_extern: false,
                attributes: Vec::new(),
                span: span(),
            },
            FuncDecl {
                name: ident("age"),
                params: vec![self_param("Person")],
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
            },
        ],
        span: span(),
    });
    let employee = Decl::StructDecl(struct_decl(
        "Employee",
        vec![("person", named_ty("Person"))],
    ));
    let src = generate_rust(&[person, extend, employee]).expect("codegen must succeed");

    // Both methods promoted.
    assert!(
        src.contains("fn name(self) -> String"),
        "expected `fn name(self) -> String` in:\n{src}"
    );
    assert!(
        src.contains("fn age(self) -> i64"),
        "expected `fn age(self) -> i64` in:\n{src}"
    );
    // Both bodies forward through self.person.
    assert!(
        src.contains("self.person.name()"),
        "expected `self.person.name()` in:\n{src}"
    );
    assert!(
        src.contains("self.person.age()"),
        "expected `self.person.age()` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. No methods on the embedded type — no delegation impl emitted.
// ---------------------------------------------------------------------------

#[test]
fn embedding_no_methods_no_delegation() {
    // struct Person { name: String }   (no extend block → no methods)
    // struct Employee { person: Person }
    let person = Decl::StructDecl(struct_decl("Person", vec![("name", named_ty("String"))]));
    let employee = Decl::StructDecl(struct_decl(
        "Employee",
        vec![("person", named_ty("Person"))],
    ));
    let src = generate_rust(&[person, employee]).expect("codegen must succeed");

    // No DELEGATION body for Employee — the absence of an `extend Person`
    // block means no methods to forward, so no `self.person.<method>()`
    // forwarding appears in any impl block.
    //
    // NOTE: T107 emits per-field `copy_<field>` methods on every non-empty
    // struct (so `impl Employee { pub fn copy_person(..) .. }` IS present),
    // but those are NOT delegation — they're record-update methods. The
    // delegation-specific assertion is on the forwarding-body pattern.
    assert!(
        !src.contains("self.person."),
        "expected NO delegation body `self.person.<method>()` when Person has no methods, got:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. Method with extra params (beyond self) — forwarded in order.
// ---------------------------------------------------------------------------

#[test]
fn embedding_method_with_extra_params_forwarded() {
    // struct Person { name: String }
    // extend Person { fn greet(self, other: String) -> String {...} }
    // struct Employee { person: Person }
    let person = Decl::StructDecl(struct_decl("Person", vec![("name", named_ty("String"))]));
    let extend = Decl::ExtendBlock(ExtendBlock {
        target: named_ty("Person"),
        methods: vec![FuncDecl {
            name: ident("greet"),
            params: vec![self_param("Person"), typed_param("other", "String")],
            return_type: Some(named_ty("String")),
            body: string_return_body("hi"),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: span(),
        }],
        span: span(),
    });
    let employee = Decl::StructDecl(struct_decl(
        "Employee",
        vec![("person", named_ty("Person"))],
    ));
    let src = generate_rust(&[person, extend, employee]).expect("codegen must succeed");

    // Delegation signature keeps `other: String` after self.
    assert!(
        src.contains("fn greet(self, other: String) -> String"),
        "expected delegation `fn greet(self, other: String) -> String` in:\n{src}"
    );
    // Body forwards `other` to self.person.greet.
    assert!(
        src.contains("self.person.greet(other)"),
        "expected delegation body `self.person.greet(other)` in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. Primitive field type — never delegates (Float has no extend methods).
// ---------------------------------------------------------------------------

#[test]
fn embedding_primitive_field_not_delegated() {
    // struct Employee { salary: Float }   (Float is not a declared struct)
    let employee = Decl::StructDecl(struct_decl("Employee", vec![("salary", named_ty("Float"))]));
    let src = generate_rust(&[employee]).expect("codegen must succeed");
    // No DELEGATION forwarding through `self.salary.<method>()` — Float is
    // a primitive, not a declared struct, so it has no embeddable methods.
    //
    // NOTE: T107 still emits `impl Employee { pub fn copy_salary(..) .. }`
    // (record-update method), but that's NOT delegation. The delegation-
    // specific assertion is on the forwarding-body pattern.
    assert!(
        !src.contains("self.salary."),
        "expected NO delegation body `self.salary.<method>()` for primitive-only fields, got:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 6. End-to-end: delegation + caller compiles to valid Rust.
// ---------------------------------------------------------------------------

#[test]
fn embedding_end_to_end_with_caller() {
    // struct Person { name: String }
    // extend Person { fn name(self) -> String { return "zoe" } }
    // struct Employee { person: Person }
    // func caller(): { return Employee { person: Person { name: "zoe" } }.name() }
    let person = Decl::StructDecl(struct_decl("Person", vec![("name", named_ty("String"))]));
    let extend = Decl::ExtendBlock(extend_one_method(
        "Person",
        "name",
        string_return_body("zoe"),
        named_ty("String"),
    ));
    let employee = Decl::StructDecl(struct_decl(
        "Employee",
        vec![("person", named_ty("Person"))],
    ));
    let caller = Decl::FuncDecl(FuncDecl { name: ident("caller"),
    params: Vec::new(),
    return_type: Some(named_ty("String")),
    body: Block {
        stmts: vec![Stmt::Return(
            Some(Expr::MethodCall {
                receiver: Box::new(Expr::StructInit {
                    type_name: ident("Employee"),
                    fields: vec![(
                        ident("person"),
                        Expr::StructInit {
                            type_name: ident("Person"),
                            fields: vec![(
                                ident("name"),
                                Expr::Literal(Literal::String("zoe".to_string()), span()),
                            )],
                            span: span(),
                        },
                    )],
                    span: span(),
                }),
                method: ident("name"),
                args: Vec::new(),
                span: span(),
            }),
            span(),
        )],
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), });
    let src = generate_rust(&[person, extend, employee, caller]).expect("codegen must succeed");
    assert!(src.contains("impl Employee"));
    assert!(src.contains("self.person.name()"));
    assert!(src.contains("fn caller() -> String"));
    must_reparse(&src);
}
