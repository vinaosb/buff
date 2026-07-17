//! T93 integration tests — parser support for `trait Name [: Super] { ... }`
//! declarations with default methods and inheritance.
//!
//! Coverage:
//!
//! - `trait Greetable { fn name() -> String; fn greet() { ... } }` parses
//!   to `Decl::TraitDecl` with one REQUIRED method (`name`, bodyless) and
//!   one DEFAULT method (`greet`, has body).
//! - Trait inheritance: `trait Pet : Animal { fn pet() { ... } }` parses
//!   with one supertrait.
//! - Multiple supertraits: `trait A : B, C { ... }`.
//! - Empty body `trait T { }` is a parse error.
//! - Existing top-level decls (func / enum / extend) STILL parse unchanged
//!   when a trait precedes or follows them.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-parser --test traits
//! ```

use buff_lang_ast::{Decl, MethodSig, TraitDecl, TypeRef};
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
// 1. Required method (bodyless signature) parses correctly.
// ---------------------------------------------------------------------------

#[test]
fn traits_required_method() {
    // `trait Greetable { fn name() -> String; }` — one required method,
    // no defaults, no supertraits. The `;` after the signature marks it
    // as a REQUIRED (bodyless) method.
    let decls = parse_program("trait Greetable {\n    func name() -> String;\n}");
    assert_eq!(decls.len(), 1, "expected one decl");
    match &decls[0] {
        Decl::TraitDecl(TraitDecl {
            name,
            supertraits,
            required,
            defaults,
            ..
        }) => {
            assert_eq!(name.name, "Greetable");
            assert!(supertraits.is_empty(), "expected no supertraits");
            assert_eq!(required.len(), 1, "expected one required method");
            assert!(defaults.is_empty(), "expected no default methods");
            // Check the required method's signature.
            let m: &MethodSig = &required[0];
            assert_eq!(m.name.name, "name");
            assert!(m.params.is_empty());
            assert!(
                matches!(
                    &m.return_type,
                    Some(TypeRef::Named { name, .. }) if name.name == "String"
                ),
                "expected return type String"
            );
        }
        other => panic!("expected TraitDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2. Default method (signature + body) parses correctly.
// ---------------------------------------------------------------------------

#[test]
fn traits_default_method() {
    // `trait Greetable { fn greet() { return "hi" } }` — one default
    // method, no required, no supertraits.
    let decls =
        parse_program("trait Greetable {\n    func greet() {\n        return \"hi\"\n    }\n}");
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::TraitDecl(TraitDecl {
            name,
            supertraits,
            required,
            defaults,
            ..
        }) => {
            assert_eq!(name.name, "Greetable");
            assert!(supertraits.is_empty());
            assert!(required.is_empty(), "expected no required methods");
            assert_eq!(defaults.len(), 1, "expected one default method");
            let d = &defaults[0];
            assert_eq!(d.name.name, "greet");
            assert!(d.params.is_empty());
            assert!(d.return_type.is_none(), "greet has no return type");
            // Body should be non-empty.
            assert!(!d.body.stmts.is_empty(), "default method must have a body");
        }
        other => panic!("expected TraitDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 3. Trait inheritance via supertrait (`trait Pet : Animal`).
// ---------------------------------------------------------------------------

#[test]
fn traits_inheritance_supertrait() {
    // `trait Pet : Animal { fn pet() { return "pet" } }` — one supertrait.
    let decls =
        parse_program("trait Pet : Animal {\n    func pet() {\n        return \"pet\"\n    }\n}");
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::TraitDecl(TraitDecl {
            name,
            supertraits,
            required,
            defaults,
            ..
        }) => {
            assert_eq!(name.name, "Pet");
            assert_eq!(supertraits.len(), 1, "expected one supertrait");
            assert!(
                matches!(
                    &supertraits[0],
                    TypeRef::Named { name, .. } if name.name == "Animal"
                ),
                "expected supertrait Animal, got {:?}",
                supertraits[0]
            );
            assert!(required.is_empty());
            assert_eq!(defaults.len(), 1);
            assert_eq!(defaults[0].name.name, "pet");
        }
        other => panic!("expected TraitDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4. Multiple comma-separated supertraits.
// ---------------------------------------------------------------------------

#[test]
fn traits_multiple_supertraits() {
    // `trait A : B, C { fn m() { } }` — two supertraits.
    let decls = parse_program("trait A : B, C {\n    func m() {\n        return 0\n    }\n}");
    assert_eq!(decls.len(), 1);
    if let Decl::TraitDecl(TraitDecl { supertraits, .. }) = &decls[0] {
        assert_eq!(supertraits.len(), 2, "expected two supertraits");
        let names: Vec<&str> = supertraits
            .iter()
            .map(|st| match st {
                TypeRef::Named { name, .. } => name.name.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(names, vec!["B", "C"]);
    } else {
        panic!("expected TraitDecl");
    }
}

// ---------------------------------------------------------------------------
// 5. Mixed required + default methods (the spec's canonical example).
// ---------------------------------------------------------------------------

#[test]
fn traits_mixed_required_and_default() {
    // `trait Greetable { fn name() -> String; fn greet() { print(name()) } }`
    // — the canonical example from the spec: one required (`name`, `;`) + one
    // default (`greet`, body) that calls the required method.
    let src = "trait Greetable {\n    func name() -> String;\n    func greet() {\n        return \"hi\"\n    }\n}";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1);
    match &decls[0] {
        Decl::TraitDecl(TraitDecl {
            required, defaults, ..
        }) => {
            assert_eq!(required.len(), 1, "one required method");
            assert_eq!(defaults.len(), 1, "one default method");
            assert_eq!(required[0].name.name, "name");
            assert_eq!(defaults[0].name.name, "greet");
        }
        other => panic!("expected TraitDecl, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6. Error paths.
// ---------------------------------------------------------------------------

#[test]
fn traits_empty_body_errors() {
    let err = parse_program_err("trait Foo {\n}");
    assert!(
        err.diagnostic.message.contains("at least one method"),
        "error should explain the empty-trait rule: {}",
        err.diagnostic.message
    );
}

#[test]
fn traits_missing_name_errors() {
    let err = parse_program_err("trait {\n    func m() { }\n}");
    assert!(
        err.diagnostic.message.contains("expected trait name")
            || err.diagnostic.message.contains("identifier"),
        "error should mention trait name: {}",
        err.diagnostic.message
    );
}

#[test]
fn traits_missing_close_brace_errors() {
    let err = parse_program_err("trait Foo {\n    func m() { }");
    assert!(
        err.diagnostic.message.contains("}")
            || err.diagnostic.message.contains("brace")
            || err.diagnostic.message.contains("input"),
        "error should mention missing `}}`: {}",
        err.diagnostic.message
    );
}

// ---------------------------------------------------------------------------
// 7. Default method with params + return type.
// ---------------------------------------------------------------------------

#[test]
fn traits_default_method_with_params_and_return() {
    // `trait Foo { fn greet(self) -> String { return "x" } }` — default
    // method with a `self` receiver and a return type.
    let decls = parse_program(
        "trait Foo {\n    func greet(self) -> String {\n        return \"x\"\n    }\n}",
    );
    assert_eq!(decls.len(), 1);
    if let Decl::TraitDecl(TraitDecl { defaults, .. }) = &decls[0] {
        assert_eq!(defaults.len(), 1);
        let d = &defaults[0];
        assert_eq!(d.name.name, "greet");
        assert_eq!(d.params.len(), 1);
        assert_eq!(d.params[0].name.name, "self");
        assert!(
            matches!(
                &d.return_type,
                Some(TypeRef::Named { name, .. }) if name.name == "String"
            ),
            "expected return type String"
        );
    } else {
        panic!("expected TraitDecl");
    }
}

// ---------------------------------------------------------------------------
// 8. Required method with params.
// ---------------------------------------------------------------------------

#[test]
fn traits_required_method_with_params() {
    // `trait Foo { fn add(a: Int, b: Int) -> Int; }` — required method
    // with two params and a return type. The `;` marks it as bodyless.
    let decls = parse_program("trait Foo {\n    func add(a: Int, b: Int) -> Int;\n}");
    assert_eq!(decls.len(), 1);
    if let Decl::TraitDecl(TraitDecl { required, .. }) = &decls[0] {
        assert_eq!(required.len(), 1);
        let m = &required[0];
        assert_eq!(m.name.name, "add");
        assert_eq!(m.params.len(), 2);
        assert_eq!(m.params[0].name.name, "a");
        assert_eq!(m.params[1].name.name, "b");
        assert!(
            matches!(
                &m.return_type,
                Some(TypeRef::Named { name, .. }) if name.name == "Int"
            ),
            "expected return type Int"
        );
    } else {
        panic!("expected TraitDecl");
    }
}

// ---------------------------------------------------------------------------
// 9. Existing decls still parse unchanged around a trait.
// ---------------------------------------------------------------------------

#[test]
fn traits_parse_mixed_with_other_decls() {
    let src = "trait Foo {\n    func m() -> Int;\n}\nfunc helper():\n    return 1\n";
    let decls = parse_program(src);
    // Two decls: a TraitDecl then a FuncDecl.
    assert_eq!(decls.len(), 2);
    assert!(matches!(decls[0], Decl::TraitDecl(_)));
    assert!(matches!(decls[1], Decl::FuncDecl(_)));
}

#[test]
fn traits_parse_trait_after_func() {
    let src = "func helper():\n    return 1\ntrait Foo {\n    func m() -> Int;\n}\n";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 2);
    assert!(matches!(decls[0], Decl::FuncDecl(_)));
    assert!(matches!(decls[1], Decl::TraitDecl(_)));
}

#[test]
fn traits_parse_trait_after_extend() {
    // An extend block then a trait — both parse unchanged.
    let src = "extend String {\n    func shout(self) -> String {\n        return \"x\"\n    }\n}\ntrait Foo {\n    func m() -> Int;\n}\n";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 2);
    assert!(matches!(decls[0], Decl::ExtendBlock(_)));
    assert!(matches!(decls[1], Decl::TraitDecl(_)));
}

// ---------------------------------------------------------------------------
// 10. Display impl round-trip (smoke).
// ---------------------------------------------------------------------------

#[test]
fn traits_display_round_trip() {
    let decls = parse_program("trait Greetable {\n    func name() -> String;\n}");
    let s = decls[0].to_string();
    assert!(
        s.contains("TraitDecl"),
        "Display should mention TraitDecl: {s}"
    );
    assert!(
        s.contains("Greetable"),
        "Display should mention the trait name: {s}"
    );
}
