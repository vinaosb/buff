//! T75b integration tests — parser support for associated types in traits
//! and `impl Trait for Type { ... }` blocks.
//!
//! Coverage:
//!
//! - `trait Container { type Item; fn get(...) -> Item; }` parses with one
//!   associated type and one required method whose return type REFERENCES
//!   the associated type by name.
//! - Multiple associated types (`type Key; type Value;`) parse in any order
//!   relative to methods.
//! - Associated types with bounds (`type Item: Clone + Debug;`) parse.
//! - `impl Container for Box { type Item = Int; fn get(...) { ... } }`
//!   parses to `Decl::ImplBlock` with one type binding + one method.
//! - Empty impl body is a parse error.
//! - Existing trait/extend syntax STILL parses unchanged when an impl
//!   precedes or follows them.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser --test associated_types
//! ```

use buff_lang_ast::{AssociatedType, AssociatedTypeBinding, Decl, ImplBlock, TraitDecl, TypeRef};
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

fn sid() -> SourceId {
    SourceId(0)
}

/// Tokenize + parse `src` as a top-level program.
fn parse_program(src: &str) -> Vec<Decl> {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect("parser must succeed")
}

/// Tokenize + parse `src` as a top-level program, expecting FAILURE.
fn parse_program_err(src: &str) -> buff_lang_error::ParseError {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect_err("parser must fail")
}

// ---------------------------------------------------------------------------
// 1. Single associated type inside a trait body.
// ---------------------------------------------------------------------------

#[test]
fn associated_type_basic() {
    // The canonical T75b example: one associated type + one required method
    // whose return type references the associated type.
    let src = "trait Container {\n    type Item;\n    func get(index: Int) -> Item;\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1, "expected one decl");
    match &decls[0] {
        Decl::TraitDecl(TraitDecl {
            name,
            associated_types,
            required,
            defaults,
            supertraits,
            ..
        }) => {
            assert_eq!(name.name, "Container");
            assert!(supertraits.is_empty());
            assert_eq!(associated_types.len(), 1, "one associated type");
            assert_eq!(associated_types[0].name.name, "Item");
            assert!(
                associated_types[0].bounds.is_empty(),
                "no bounds in basic form"
            );
            assert_eq!(required.len(), 1, "one required method");
            assert!(defaults.is_empty());
            assert_eq!(required[0].name.name, "get");
            // The return type references the associated type by name.
            assert!(
                matches!(
                    &required[0].return_type,
                    Some(TypeRef::Named { name, .. }) if name.name == "Item"
                ),
                "expected return type Item, got {:?}",
                required[0].return_type
            );
        }
        other => panic!("expected TraitDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2. Multiple associated types.
// ---------------------------------------------------------------------------

#[test]
fn associated_type_multiple() {
    let src = "trait Map {\n    type Key;\n    type Value;\n    func get(k: Key) -> Value;\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1);
    if let Decl::TraitDecl(TraitDecl {
        associated_types, ..
    }) = &decls[0]
    {
        assert_eq!(associated_types.len(), 2, "two associated types");
        let names: Vec<&str> = associated_types
            .iter()
            .map(|a| a.name.name.as_str())
            .collect();
        assert_eq!(names, vec!["Key", "Value"]);
    } else {
        panic!("expected TraitDecl");
    }
}

// ---------------------------------------------------------------------------
// 3. Associated type with bounds.
// ---------------------------------------------------------------------------

#[test]
fn associated_type_with_bounds() {
    let src = "trait Container {\n    type Item: Clone + Debug;\n    func get() -> Item;\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1);
    if let Decl::TraitDecl(TraitDecl {
        associated_types, ..
    }) = &decls[0]
    {
        assert_eq!(associated_types.len(), 1);
        let at: &AssociatedType = &associated_types[0];
        assert_eq!(at.name.name, "Item");
        assert_eq!(at.bounds.len(), 2, "two bounds");
        let bound_names: Vec<&str> = at
            .bounds
            .iter()
            .map(|b| match b {
                TypeRef::Named { name, .. } => name.name.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(bound_names, vec!["Clone", "Debug"]);
    } else {
        panic!("expected TraitDecl");
    }
}

// ---------------------------------------------------------------------------
// 4. Associated type interleaved with methods (any order).
// ---------------------------------------------------------------------------

#[test]
fn associated_type_after_method() {
    // Methods and associated types may appear in any order.
    let src = "trait T {\n    func m1() -> Int;\n    type Item;\n    func m2() -> Item;\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1);
    if let Decl::TraitDecl(TraitDecl {
        associated_types,
        required,
        ..
    }) = &decls[0]
    {
        assert_eq!(associated_types.len(), 1);
        assert_eq!(required.len(), 2, "two required methods");
    } else {
        panic!("expected TraitDecl");
    }
}

// ---------------------------------------------------------------------------
// 5. Trait with ONLY an associated type (no methods) is valid.
// ---------------------------------------------------------------------------

#[test]
fn associated_type_only_no_methods() {
    let src = "trait Marker {\n    type Item;\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1);
    if let Decl::TraitDecl(TraitDecl {
        associated_types,
        required,
        defaults,
        ..
    }) = &decls[0]
    {
        assert_eq!(associated_types.len(), 1);
        assert!(required.is_empty());
        assert!(defaults.is_empty());
    } else {
        panic!("expected TraitDecl");
    }
}

// ---------------------------------------------------------------------------
// 6. Basic impl block with one type binding + one method.
// ---------------------------------------------------------------------------

#[test]
fn impl_block_basic() {
    let src = "impl Container for Box {\n    type Item = Int;\n    func get(index: Int) -> Int {\n        return self.value\n    }\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::ImplBlock(ImplBlock {
            trait_name,
            target,
            type_bindings,
            methods,
            ..
        }) => {
            assert!(
                matches!(trait_name, TypeRef::Named { name, .. } if name.name == "Container"),
                "trait name Container, got {trait_name:?}"
            );
            assert!(
                matches!(target, TypeRef::Named { name, .. } if name.name == "Box"),
                "target Box, got {target:?}"
            );
            assert_eq!(type_bindings.len(), 1, "one type binding");
            assert_eq!(type_bindings[0].name.name, "Item");
            assert!(
                matches!(&type_bindings[0].target, TypeRef::Named { name, .. } if name.name == "Int"),
                "Item = Int"
            );
            assert_eq!(methods.len(), 1, "one method");
            assert_eq!(methods[0].name.name, "get");
        }
        other => panic!("expected ImplBlock, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 7. Impl block with multiple type bindings.
// ---------------------------------------------------------------------------

#[test]
fn impl_block_multiple_bindings() {
    let src = "impl Map for Dict {\n    type Key = String;\n    type Value = Int;\n    func get(k: String) -> Int {\n        return 0\n    }\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1);
    if let Decl::ImplBlock(ImplBlock {
        type_bindings,
        methods,
        ..
    }) = &decls[0]
    {
        assert_eq!(type_bindings.len(), 2, "two type bindings");
        let names: Vec<(&str, &str)> = type_bindings
            .iter()
            .map(|b: &AssociatedTypeBinding| {
                let target_name = match &b.target {
                    TypeRef::Named { name, .. } => name.name.as_str(),
                    _ => "?",
                };
                (b.name.name.as_str(), target_name)
            })
            .collect();
        assert_eq!(names, vec![("Key", "String"), ("Value", "Int")]);
        assert_eq!(methods.len(), 1);
    } else {
        panic!("expected ImplBlock");
    }
}

// ---------------------------------------------------------------------------
// 8. Impl block with only methods (no type bindings) — valid when the
//    implemented trait declares no associated types.
// ---------------------------------------------------------------------------

#[test]
fn impl_block_methods_only() {
    let src =
        "impl Greetable for Person {\n    func name() -> String {\n        return \"x\"\n    }\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1);
    if let Decl::ImplBlock(ImplBlock {
        type_bindings,
        methods,
        ..
    }) = &decls[0]
    {
        assert!(type_bindings.is_empty(), "no type bindings");
        assert_eq!(methods.len(), 1, "one method");
    } else {
        panic!("expected ImplBlock");
    }
}

// ---------------------------------------------------------------------------
// 9. Error paths.
// ---------------------------------------------------------------------------

#[test]
fn impl_block_empty_body_errors() {
    let err = parse_program_err("impl T for U {\n}");
    assert!(
        err.diagnostic.message.contains("at least one") || err.diagnostic.message.contains("must"),
        "empty-body error should be descriptive: {}",
        err.diagnostic.message
    );
}

#[test]
fn impl_block_missing_for_errors() {
    let err = parse_program_err("impl T U {\n    func m() { }\n}");
    // Missing `for` surfaces as an unexpected-token error.
    assert!(
        !err.diagnostic.message.is_empty(),
        "missing-for should produce an error"
    );
}

#[test]
fn impl_block_type_binding_missing_eq_errors() {
    let err = parse_program_err("impl T for U {\n    type Item Int;\n}");
    assert!(
        !err.diagnostic.message.is_empty(),
        "missing-= in type binding should produce an error"
    );
}

// ---------------------------------------------------------------------------
// 10. Existing trait + extend syntax STILL parses unchanged when an impl
//     appears around them.
// ---------------------------------------------------------------------------

#[test]
fn impl_block_mixed_with_other_decls() {
    let src = "trait T {\n    type Item;\n    func m() -> Item;\n}\nstruct Box {\n    value: Int,\n}\nimpl T for Box {\n    type Item = Int;\n    func m() -> Int {\n        return self.value\n    }\n}\n";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 3);
    assert!(matches!(decls[0], Decl::TraitDecl(_)));
    assert!(matches!(decls[1], Decl::StructDecl(_)));
    assert!(matches!(decls[2], Decl::ImplBlock(_)));
}

#[test]
fn impl_block_with_extend_block() {
    // An extend block then an impl — both parse unchanged.
    let src = "extend String {\n    func shout(self) -> String {\n        return \"x\"\n    }\n}\nimpl T for U {\n    func m() {\n        return 0\n    }\n}\n";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 2);
    assert!(matches!(decls[0], Decl::ExtendBlock(_)));
    assert!(matches!(decls[1], Decl::ImplBlock(_)));
}

// ---------------------------------------------------------------------------
// 11. Display round-trip smoke.
// ---------------------------------------------------------------------------

#[test]
fn impl_block_display_round_trip() {
    let decls = parse_program("impl T for U {\n    func m() {\n        return 0\n    }\n}");
    let s = decls[0].to_string();
    assert!(
        s.contains("ImplBlock"),
        "Display should mention ImplBlock: {s}"
    );
    assert!(s.contains("T"), "Display should mention trait name: {s}");
    assert!(s.contains("U"), "Display should mention target: {s}");
}
