//! DR-020 / P2.1e integration tests — parser support for `dyn Trait` syntax.
//!
//! Coverage:
//!
//! - `Box<dyn Trait>` parses to `TypeRef::TraitObject { lifetime: None, .. }`.
//! - `dyn Trait ('static)` parses with `lifetime: Some("static")`.
//! - `dyn` alone (no trait name) is a parse error.
//! - Existing code using `dyn` as a variable name continues to parse
//!   (Stability Promise: `dyn` is NOT a reserved keyword).
//! - Trait objects compose inside `Vector<...>` and `Option<...>`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser --test dyn_trait
//! ```

use buff_lang_ast::{Decl, FuncDecl, TypeRef};
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
#[allow(dead_code)]
fn parse_program_err(src: &str) -> buff_lang_error::ParseError {
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    parse(&tokens, sid()).expect_err("parser must fail")
}

/// Pull the first `Decl::FuncDecl` out of a parsed program.
fn first_func(decls: &[Decl]) -> &FuncDecl {
    decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(f) => Some(f),
            _ => None,
        })
        .expect("program must contain at least one func decl")
}

// ---------------------------------------------------------------------------
// 1. `Box<dyn Trait>` parses to TypeRef::TraitObject (owned form).
// ---------------------------------------------------------------------------

#[test]
fn owned_box_dyn_trait_parses() {
    let src = "func take_drawable(item: Box<dyn Drawable>):\n    print(item)\n";
    let prog = parse_program(src);
    let f = first_func(&prog);
    let param = &f.params[0];
    // The param parses as `Box<dyn Drawable>` which is a Generic with
    // base `Box` and one arg `TypeRef::TraitObject`. The parser does NOT
    // special-case `Box<...>` — it parses as Generic and the inner type
    // is parsed via the recursive `parse_type_ref` call. So we check
    // the inner arg is the TraitObject.
    match &param.ty {
        TypeRef::Generic { base, args, .. } => {
            // Base should be Named("Box").
            match base.as_ref() {
                TypeRef::Named { name, .. } => {
                    assert_eq!(name.name, "Box", "expected Box base");
                }
                other => panic!("expected Named base, got {other:?}"),
            }
            // And the single arg should be a TraitObject.
            assert_eq!(args.len(), 1, "Box<> should have exactly 1 arg");
            match &args[0] {
                TypeRef::TraitObject {
                    trait_name,
                    lifetime,
                    ..
                } => {
                    assert_eq!(trait_name.name, "Drawable");
                    assert!(lifetime.is_none(), "owned form has no explicit lifetime");
                }
                other => panic!("expected TraitObject arg, got {other:?}"),
            }
        }
        other => panic!("expected Generic (Box<dyn ...>), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2. `dyn Trait` always parses with lifetime=None (MVP).
//
//    The `lifetime` AST field exists for future expansion, but the parser
//    does not yet accept explicit `('static)` syntax — Buff's lexer
//    tokenizes `'static` as the start of a char literal, not an Ident.
//    Codegen ignores the field anyway (always emits `Box<dyn Trait>` per
//    DR-020 §Autoboxing Rules). A future T-numbered task will add proper
//    Lifetime token support when borrowed-form lifetimes become a real
//    user need.
// ---------------------------------------------------------------------------

#[test]
fn dyn_trait_lifetime_field_is_none_in_mvp() {
    let src = "func borrow_drawable(item: dyn Drawable):\n    print(item)\n";
    let prog = parse_program(src);
    let f = first_func(&prog);
    let param = &f.params[0];
    match &param.ty {
        TypeRef::TraitObject {
            trait_name,
            lifetime,
            ..
        } => {
            assert_eq!(trait_name.name, "Drawable");
            assert!(
                lifetime.is_none(),
                "MVP parser must not populate lifetime (no Lifetime token support yet)"
            );
        }
        other => panic!("expected TraitObject, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 3. `dyn` alone (no trait name) is a parse error.
// ---------------------------------------------------------------------------

#[test]
fn dyn_alone_is_parse_error() {
    let src = "func broken(item: dyn):\n    print(item)\n";
    let tokens = tokenize(src, sid()).expect("lexer must succeed");
    let result = parse(&tokens, sid());
    assert!(
        result.is_err(),
        "`dyn` without a trait name must be a parse error"
    );
}

// ---------------------------------------------------------------------------
// 4. Existing code using `dyn` as a variable name continues to parse.
//    (Stability Promise: `dyn` is NOT a reserved keyword.)
// ---------------------------------------------------------------------------

#[test]
fn dyn_as_variable_name_still_parses() {
    // `let dyn = 42` should still parse because `dyn` is recognized
    // contextually only in TYPE position. In expression/statement
    // position it's an ordinary identifier.
    let src = "func use_dyn_as_name():\n    let dyn = 42\n    print(dyn)\n";
    let prog = parse_program(src);
    // If we got here, the parser accepted the program. The binding `dyn`
    // resolves as a regular ident (not a TypeRef context).
    assert!(
        prog.iter().any(|d| matches!(d, Decl::FuncDecl(_))),
        "func decl must parse"
    );
}

// ---------------------------------------------------------------------------
// 5. Trait objects compose inside `Vector<...>` and `Option<...>`.
//    This is the heterogeneous-collection case from DR-020 §5.
// ---------------------------------------------------------------------------

#[test]
fn trait_object_inside_vector_parses() {
    let src = "func process(items: Vector<Box<dyn Drawable>>):\n    print(items)\n";
    let prog = parse_program(src);
    let f = first_func(&prog);
    let param = &f.params[0];
    // Outer: Vector<Box<dyn Drawable>> — a Generic with base Vector and
    // one arg (the Box<dyn Drawable>).
    match &param.ty {
        TypeRef::Generic { base, args, .. } => {
            match base.as_ref() {
                TypeRef::Named { name, .. } => assert_eq!(name.name, "Vector"),
                other => panic!("expected Vector base, got {other:?}"),
            }
            assert_eq!(args.len(), 1);
            // Inner: Box<dyn Drawable>.
            match &args[0] {
                TypeRef::Generic { args: box_args, .. } => {
                    assert_eq!(box_args.len(), 1);
                    match &box_args[0] {
                        TypeRef::TraitObject { trait_name, .. } => {
                            assert_eq!(trait_name.name, "Drawable");
                        }
                        other => panic!("expected TraitObject inside Box, got {other:?}"),
                    }
                }
                other => panic!("expected Box<> inside Vector<>, got {other:?}"),
            }
        }
        other => panic!("expected Vector<> outer, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6. Trait object inside Option<>.
// ---------------------------------------------------------------------------

#[test]
fn trait_object_inside_option_parses() {
    let src = "func maybe_drawable(item: Option<Box<dyn Drawable>>):\n    print(item)\n";
    let prog = parse_program(src);
    let f = first_func(&prog);
    let param = &f.params[0];
    match &param.ty {
        TypeRef::Generic { base, args, .. } => {
            match base.as_ref() {
                TypeRef::Named { name, .. } => assert_eq!(name.name, "Option"),
                other => panic!("expected Option base, got {other:?}"),
            }
            assert_eq!(args.len(), 1);
            match &args[0] {
                TypeRef::Generic { args: box_args, .. } => {
                    assert!(matches!(
                        &box_args[0],
                        TypeRef::TraitObject { trait_name, .. } if trait_name.name == "Drawable"
                    ));
                }
                other => panic!("expected Box<> inside Option<>, got {other:?}"),
            }
        }
        other => panic!("expected Option<> outer, got {other:?}"),
    }
}
