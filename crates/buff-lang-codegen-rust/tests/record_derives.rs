//! T107 integration tests — auto-derived record methods for user structs.
//!
//! Every user `struct` automatically derives `Clone`, `PartialEq`, `Debug`,
//! and (when ALL field types impl `Hash`) `Hash`. In addition, each field
//! gets a `copy_<field>(&self, <field>: <ty>) -> Self` immutable-update
//! method generated automatically — e.g. for `struct P { age: Int }` the
//! codegen emits:
//!
//! ```rust,ignore
//! impl P {
//!     pub fn copy_age(&self, age: i64) -> Self {
//!         let mut c = self.clone();
//!         c.age = age;
//!         c
//!     }
//! }
//! ```
//!
//! Coverage:
//!
//! - Struct with all Hash-safe fields (Int/String/Bool/...) → derives
//!   `#[derive(Clone, PartialEq, Hash, Debug)]`.
//! - Struct with a Float/Double field → derives
//!   `#[derive(Clone, PartialEq, Debug)]` (NO Hash — f32/f64 don't impl Hash).
//! - Struct with a Vector<T> field → no Hash (Vec<T> doesn't impl Hash).
//! - Struct with Option<Int> → derives Hash (Option<T>: Hash when T: Hash).
//! - Struct with Option<Float> → no Hash (recursive Hash-safety check).
//! - Struct with a Tuple of Hash-safe members → derives Hash.
//! - Empty struct → derives Hash (vacuously all fields Hash-safe).
//! - Transitive Hash-safety: struct A { b: B }, B { x: Float } → NEITHER
//!   derives Hash.
//! - Per-field `copy_<field>` method emitted with correct signature + body.
//! - Clone derive still present after copy method emission (no regression).
//!
//! Each test builds a Buff AST by hand and runs it through
//! [`buff_lang_codegen_rust::generate_rust`], asserting on the resulting
//! Rust source. The generated source is re-parsed via
//! `syn::parse_str::<syn::File>` to guarantee it's valid Rust.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust record_derives
//! ```

use buff_lang_ast::common::Ident;
use buff_lang_ast::decl::StructDecl;
use buff_lang_ast::{Decl, TypeRef};
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

fn option_ty(inner: TypeRef) -> TypeRef {
    TypeRef::Option(Box::new(inner), span())
}

fn tuple_ty(members: Vec<TypeRef>) -> TypeRef {
    TypeRef::Tuple(members, span())
}

fn generic_ty(base: &str, args: Vec<TypeRef>) -> TypeRef {
    TypeRef::Generic {
        base: Box::new(named_ty(base)),
        args,
        span: span(),
    }
}

/// Build a `struct` declaration AST node with the given name + fields.
fn struct_decl(name: &str, fields: Vec<(&str, TypeRef)>) -> StructDecl {
    StructDecl {
        name: ident(name),
        fields: fields.into_iter().map(|(n, t)| (ident(n), t)).collect(),
        traits: Vec::new(),
        span: span(),
    }
}

/// Generate Rust source from a slice of struct declarations.
fn codegen_structs(decls: Vec<StructDecl>) -> String {
    let decls: Vec<Decl> = decls.into_iter().map(Decl::StructDecl).collect();
    generate_rust(&decls).expect("struct codegen must succeed")
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// 1. Hash-safe fields → derive Clone, PartialEq, Hash, Debug
// ---------------------------------------------------------------------------

#[test]
fn record_derives_partial_eq_and_hash_for_hash_safe_struct() {
    // `struct Person { name: String, age: Int }` → all fields Hash-safe →
    // `#[derive(Clone, PartialEq, Hash, Debug)]`.
    let src = codegen_structs(vec![struct_decl(
        "Person",
        vec![("name", named_ty("String")), ("age", named_ty("Int"))],
    )]);
    assert!(
        src.contains("#[derive(Clone, PartialEq, Hash, Debug)]"),
        "expected `#[derive(Clone, PartialEq, Hash, Debug)]` for Hash-safe struct in:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_partial_eq_always_present() {
    // Even with a float field, PartialEq must always be derived.
    let src = codegen_structs(vec![struct_decl("S", vec![("x", named_ty("Float"))])]);
    assert!(
        src.contains("PartialEq"),
        "PartialEq must always be derived; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_empty_struct_gets_hash() {
    // An empty struct has all (zero) fields Hash-safe → derives Hash.
    let src = codegen_structs(vec![struct_decl("Empty", Vec::new())]);
    assert!(
        src.contains("#[derive(Clone, PartialEq, Hash, Debug)]"),
        "empty struct should derive Hash; got:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Non-Hash-safe fields → NO Hash derive (Clone, PartialEq, Debug only)
// ---------------------------------------------------------------------------

#[test]
fn record_derives_float_field_no_hash() {
    // `struct S { x: Float }` → f32 doesn't impl Hash → no Hash derive.
    let src = codegen_structs(vec![struct_decl("S", vec![("x", named_ty("Float"))])]);
    assert!(
        src.contains("#[derive(Clone, PartialEq, Debug)]"),
        "Float-field struct should derive `Clone, PartialEq, Debug` (no Hash); got:\n{src}"
    );
    assert!(
        !src.contains("Hash"),
        "Float-field struct must NOT derive Hash; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_double_field_no_hash() {
    // f64 also doesn't impl Hash.
    let src = codegen_structs(vec![struct_decl("S", vec![("y", named_ty("Double"))])]);
    assert!(
        !src.contains("Hash"),
        "Double-field struct must NOT derive Hash; got:\n{src}"
    );
    assert!(
        src.contains("#[derive(Clone, PartialEq, Debug)]"),
        "Double-field struct should derive `Clone, PartialEq, Debug`; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_vector_field_no_hash() {
    // Vec<T> doesn't impl Hash in std → no Hash derive.
    let src = codegen_structs(vec![struct_decl(
        "Bag",
        vec![("items", generic_ty("Vector", vec![named_ty("Int")]))],
    )]);
    assert!(
        !src.contains("Hash"),
        "Vector-field struct must NOT derive Hash; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_option_of_int_gets_hash() {
    // Option<T>: Hash when T: Hash → Option<Int> is Hash-safe.
    let src = codegen_structs(vec![struct_decl(
        "Opt",
        vec![("maybe", option_ty(named_ty("Int")))],
    )]);
    assert!(
        src.contains("#[derive(Clone, PartialEq, Hash, Debug)]"),
        "Option<Int> struct should derive Hash; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_option_of_float_no_hash() {
    // Option<Float> is NOT Hash-safe (recursive check fires).
    let src = codegen_structs(vec![struct_decl(
        "OptF",
        vec![("maybe", option_ty(named_ty("Float")))],
    )]);
    assert!(
        !src.contains("Hash"),
        "Option<Float> struct must NOT derive Hash; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_tuple_of_hash_safe_gets_hash() {
    // (Int, String) — both Hash-safe → struct derives Hash.
    let src = codegen_structs(vec![struct_decl(
        "Pair",
        vec![("tup", tuple_ty(vec![named_ty("Int"), named_ty("String")]))],
    )]);
    assert!(
        src.contains("#[derive(Clone, PartialEq, Hash, Debug)]"),
        "(Int, String) tuple struct should derive Hash; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_tuple_with_float_no_hash() {
    // (Int, Float) — Float is non-Hash → tuple is non-Hash → struct no Hash.
    let src = codegen_structs(vec![struct_decl(
        "PairF",
        vec![("tup", tuple_ty(vec![named_ty("Int"), named_ty("Float")]))],
    )]);
    assert!(
        !src.contains("Hash"),
        "(Int, Float) tuple struct must NOT derive Hash; got:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Transitive Hash-safety across user structs
// ---------------------------------------------------------------------------

#[test]
fn record_derives_transitive_hash_safety() {
    // struct A { b: B }
    // struct B { x: Float }
    //
    // B has a Float field → B can't derive Hash.
    // A's only field is b: B, and B is not Hash-safe → A can't derive Hash.
    // Neither struct should derive Hash.
    let src = codegen_structs(vec![
        struct_decl("A", vec![("b", named_ty("B"))]),
        struct_decl("B", vec![("x", named_ty("Float"))]),
    ]);
    // The struct decl lines look like `#[derive(...)] pub struct Name`.
    // Both A and B must NOT carry Hash.
    assert!(
        !src.contains("Hash"),
        "transitive: neither A nor B should derive Hash when B has a Float field; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_transitive_hash_safety_both_safe() {
    // struct A { b: B }
    // struct B { x: Int }
    //
    // B has all Hash-safe fields → B derives Hash.
    // A's only field b: B is Hash-safe → A derives Hash.
    let src = codegen_structs(vec![
        struct_decl("A", vec![("b", named_ty("B"))]),
        struct_decl("B", vec![("x", named_ty("Int"))]),
    ]);
    assert!(
        src.matches("Hash").count() == 2,
        "transitive: both A and B should derive Hash; got:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. copy_<field> method emission
// ---------------------------------------------------------------------------

#[test]
fn record_derives_copy_method_emitted() {
    // `struct P { age: Int }` → impl P { pub fn copy_age(...) -> Self { ... } }
    let src = codegen_structs(vec![struct_decl("P", vec![("age", named_ty("Int"))])]);
    assert!(
        src.contains("impl P"),
        "expected `impl P` block for copy methods in:\n{src}"
    );
    assert!(
        src.contains("fn copy_age"),
        "expected `fn copy_age` method in:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_copy_method_signature() {
    // The copy method signature must be:
    //   pub fn copy_<field>(&self, <field>: <rust_ty>) -> Self
    let src = codegen_structs(vec![struct_decl("P", vec![("age", named_ty("Int"))])]);
    assert!(
        src.contains("pub fn copy_age(&self, age: i64) -> Self"),
        "expected `pub fn copy_age(&self, age: i64) -> Self` in:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_copy_method_body() {
    // Body: `let mut c = self.clone(); c.<field> = <field>; c`
    let src = codegen_structs(vec![struct_decl("P", vec![("age", named_ty("Int"))])]);
    assert!(
        src.contains("let mut c = self.clone();"),
        "expected `let mut c = self.clone();` in copy body in:\n{src}"
    );
    assert!(
        src.contains("c.age = age;"),
        "expected `c.age = age;` field assignment in:\n{src}"
    );
    // Method must return the cloned value `c`.
    // The trailing-statement form is `c` (no semicolon — expression return).
    assert!(
        src.contains("c.age = age;\n        c\n") || src.contains("c.age = age;\n    c\n"),
        "expected copy method to return `c` after field assignment; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_copy_method_one_per_field() {
    // Struct with 2 fields → 2 copy methods.
    let src = codegen_structs(vec![struct_decl(
        "P",
        vec![("name", named_ty("String")), ("age", named_ty("Int"))],
    )]);
    assert!(
        src.contains("fn copy_name"),
        "expected `fn copy_name` in:\n{src}"
    );
    assert!(
        src.contains("fn copy_age"),
        "expected `fn copy_age` in:\n{src}"
    );
    // Both must be in the SAME impl block (one impl P { ... } containing both).
    let impl_start = src.find("impl P").expect("impl P present");
    let impl_body = &src[impl_start..];
    assert!(
        impl_body.contains("copy_name") && impl_body.contains("copy_age"),
        "both copy methods must be in the same `impl P` block; got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_copy_method_field_types_mapped() {
    // Field type uses the SAME primitive mapping (Float → f32, etc.).
    let src = codegen_structs(vec![struct_decl(
        "V",
        vec![("x", named_ty("Float")), ("y", named_ty("Double"))],
    )]);
    assert!(
        src.contains("pub fn copy_x(&self, x: f32) -> Self"),
        "expected copy_x with f32 param in:\n{src}"
    );
    assert!(
        src.contains("pub fn copy_y(&self, y: f64) -> Self"),
        "expected copy_y with f64 param in:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. Regression — Clone + Debug still present after copy method emission
// ---------------------------------------------------------------------------

#[test]
fn record_derives_copy_preserves_clone_and_debug() {
    // Copy-method emission MUST NOT remove Clone / Debug derives.
    let src = codegen_structs(vec![struct_decl(
        "P",
        vec![("name", named_ty("String")), ("age", named_ty("Int"))],
    )]);
    assert!(
        src.contains("Clone"),
        "Clone derive must still be present; got:\n{src}"
    );
    assert!(
        src.contains("Debug"),
        "Debug derive must still be present; got:\n{src}"
    );
    // The copy body relies on `self.clone()` — Clone MUST be derived.
    assert!(
        src.contains("self.clone()"),
        "copy body must call `self.clone()` (requires Clone derive); got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn record_derives_empty_struct_no_copy_methods() {
    // An empty struct has no fields → no copy methods emitted (but still
    // gets the derive attribute).
    let src = codegen_structs(vec![struct_decl("Empty", Vec::new())]);
    assert!(
        !src.contains("fn copy_"),
        "empty struct must NOT emit any copy methods; got:\n{src}"
    );
    assert!(
        !src.contains("impl Empty"),
        "empty struct must NOT emit an empty impl block; got:\n{src}"
    );
    must_reparse(&src);
}
