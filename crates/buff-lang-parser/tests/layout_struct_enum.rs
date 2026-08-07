//! BUG-3 + BUG-6 regression tests — layout-sensitive (indentation-based)
//! `struct` and `enum` declarations.
//!
//! Both `struct` and `enum` support TWO syntactic forms in Buff:
//!
//! 1. **Layout form** (primary — Python/F#-style indentation blocks):
//!
//! ```text
//! struct Point:
//!     x: Int
//!     y: Int
//!
//! enum Color:
//!     Red
//!     Green
//!     Blue
//! ```
//!
//! 2. **Brace form** (compact one-liners — backward-compat baseline):
//!
//! ```text
//! struct Point { x: Int, y: Int }
//! enum Color { Red, Green, Blue }
//! ```
//!
//! **BUG-3**: the layout arm of `parse_struct_decl` existed but was broken —
//! the `:` / `Newline` / `Indent` token consumption order was wrong, so every
//! layout-form struct was rejected with "expected newline after `struct Name:`".
//!
//! **BUG-6**: `parse_enum_decl` had NO layout arm at all — it hard-required
//! `{` via `stream.expect(TokenKind::LBrace)`, so layout-form enums were
//! impossible.
//!
//! This file pins BOTH fixes AND guards the brace-form backward compatibility.

use buff_lang_ast::Decl;
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

// ---------------------------------------------------------------------------
// BUG-3: layout-form struct declarations
// ---------------------------------------------------------------------------

#[test]
fn bug3_layout_struct_simple_parses() {
    //   struct Point:
    //       x: Int
    //       y: Int
    let src = "struct Point:\n    x: Int\n    y: Int";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1, "expected one top-level decl");
    let s = match &decls[0] {
        Decl::StructDecl(s) => s,
        other => panic!("expected StructDecl, got {other:?}"),
    };
    assert_eq!(s.name.name, "Point", "struct name");
    assert!(s.type_params.is_empty(), "non-generic struct");
    assert_eq!(s.fields.len(), 2, "two fields");
    assert_eq!(s.fields[0].0.name, "x", "first field name");
    assert_eq!(s.fields[1].0.name, "y", "second field name");
}

#[test]
fn bug3_layout_struct_generic_parses() {
    //   struct Pair<T, U>:
    //       x: T
    //       y: U
    let src = "struct Pair<T, U>:\n    x: T\n    y: U";
    let decls = parse_program(src);
    let s = match &decls[0] {
        Decl::StructDecl(s) => s,
        other => panic!("expected StructDecl, got {other:?}"),
    };
    assert_eq!(s.name.name, "Pair");
    assert_eq!(
        s.type_params
            .iter()
            .map(|tp| tp.name.name.as_str())
            .collect::<Vec<_>>(),
        vec!["T", "U"],
        "two generic params"
    );
    assert_eq!(s.fields.len(), 2);
    assert_eq!(s.fields[0].0.name, "x");
    assert_eq!(s.fields[1].0.name, "y");
}

#[test]
fn bug3_layout_struct_single_field_parses() {
    // A layout struct with only one field.
    let src = "struct Wrapper:\n    value: Int";
    let decls = parse_program(src);
    let s = match &decls[0] {
        Decl::StructDecl(s) => s,
        other => panic!("expected StructDecl, got {other:?}"),
    };
    assert_eq!(s.name.name, "Wrapper");
    assert_eq!(s.fields.len(), 1, "one field");
    assert_eq!(s.fields[0].0.name, "value");
}

// ---------------------------------------------------------------------------
// BUG-6: layout-form enum declarations
// ---------------------------------------------------------------------------

#[test]
fn bug6_layout_enum_simple_unit_variants_parses() {
    //   enum Color:
    //       Red
    //       Green
    //       Blue
    let src = "enum Color:\n    Red\n    Green\n    Blue";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 1, "expected one top-level decl");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Color", "enum name");
    assert!(e.type_params.is_empty(), "non-generic enum");
    assert_eq!(e.variants.len(), 3, "three variants");
    let names: Vec<&str> = e.variants.iter().map(|v| v.name.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Red", "Green", "Blue"],
        "variant names in order"
    );
    // All unit variants — `data` is None.
    for v in &e.variants {
        assert!(
            v.data.is_none(),
            "unit variant {:?} should have no payload",
            v.name
        );
    }
}

#[test]
fn bug6_layout_enum_with_payload_variants_parses() {
    //   enum Shape:
    //       Circle(Float)
    //       Square(Float)
    let src = "enum Shape:\n    Circle(Float)\n    Square(Float)";
    let decls = parse_program(src);
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Shape");
    assert_eq!(e.variants.len(), 2, "two variants");
    assert_eq!(e.variants[0].name.name, "Circle");
    let circle_data = e.variants[0].data.as_ref().expect("Circle has payload");
    assert_eq!(circle_data.len(), 1, "Circle has one payload type");
    assert_eq!(e.variants[1].name.name, "Square");
    assert!(e.variants[1].data.is_some(), "Square has payload");
}

#[test]
fn bug6_layout_enum_generic_parses() {
    //   enum Result<T, E>:
    //       Ok(T)
    //       Err(E)
    let src = "enum Result<T, E>:\n    Ok(T)\n    Err(E)";
    let decls = parse_program(src);
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Result");
    assert_eq!(
        e.type_params
            .iter()
            .map(|tp| tp.name.name.as_str())
            .collect::<Vec<_>>(),
        vec!["T", "E"],
        "two generic params"
    );
    assert_eq!(e.variants.len(), 2);
    assert_eq!(e.variants[0].name.name, "Ok");
    assert!(e.variants[0].data.is_some(), "Ok has payload");
    assert_eq!(e.variants[1].name.name, "Err");
    assert!(e.variants[1].data.is_some(), "Err has payload");
}

#[test]
fn bug6_layout_enum_mixed_unit_and_payload_parses() {
    //   enum Message:
    //       Quit
    //       Move(Int, Int)
    //       Write(String)
    let src = "enum Message:\n    Quit\n    Move(Int, Int)\n    Write(String)";
    let decls = parse_program(src);
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Message");
    assert_eq!(e.variants.len(), 3);
    // Quit — unit.
    assert_eq!(e.variants[0].name.name, "Quit");
    assert!(e.variants[0].data.is_none(), "Quit is unit");
    // Move(Int, Int) — two payload types.
    assert_eq!(e.variants[1].name.name, "Move");
    let move_data = e.variants[1].data.as_ref().expect("Move has payload");
    assert_eq!(move_data.len(), 2, "Move has two payload types");
    // Write(String) — one payload.
    assert_eq!(e.variants[2].name.name, "Write");
    assert!(e.variants[2].data.is_some(), "Write has payload");
}

// ---------------------------------------------------------------------------
// Backward compatibility: brace form MUST still work.
// ---------------------------------------------------------------------------

#[test]
fn backward_compat_brace_struct_parses() {
    let decls = parse_program("struct Point { x: Int, y: Int }");
    let s = match &decls[0] {
        Decl::StructDecl(s) => s,
        other => panic!("expected StructDecl, got {other:?}"),
    };
    assert_eq!(s.name.name, "Point");
    assert_eq!(s.fields.len(), 2);
    assert_eq!(s.fields[0].0.name, "x");
    assert_eq!(s.fields[1].0.name, "y");
}

#[test]
fn backward_compat_brace_struct_empty_parses() {
    let decls = parse_program("struct Empty { }");
    let s = match &decls[0] {
        Decl::StructDecl(s) => s,
        other => panic!("expected StructDecl, got {other:?}"),
    };
    assert_eq!(s.name.name, "Empty");
    assert!(s.fields.is_empty(), "empty struct");
}

#[test]
fn backward_compat_brace_enum_parses() {
    let decls = parse_program("enum Color { Red, Green, Blue }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Color");
    assert_eq!(e.variants.len(), 3);
}

#[test]
fn backward_compat_brace_enum_empty_parses() {
    let decls = parse_program("enum Empty { }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        other => panic!("expected EnumDecl, got {other:?}"),
    };
    assert_eq!(e.name.name, "Empty");
    assert!(e.variants.is_empty());
}

#[test]
fn backward_compat_brace_struct_with_trailing_comma_parses() {
    let decls = parse_program("struct P { x: Int, y: Int, }");
    let s = match &decls[0] {
        Decl::StructDecl(s) => s,
        _ => panic!("expected StructDecl"),
    };
    assert_eq!(s.fields.len(), 2, "trailing comma tolerated");
}

#[test]
fn backward_compat_brace_enum_with_trailing_comma_parses() {
    let decls = parse_program("enum C { Red, Green, Blue, }");
    let e = match &decls[0] {
        Decl::EnumDecl(e) => e,
        _ => panic!("expected EnumDecl"),
    };
    assert_eq!(e.variants.len(), 3, "trailing comma tolerated");
}

// ---------------------------------------------------------------------------
// Coexistence: a layout struct followed by a brace enum (and vice versa).
// ---------------------------------------------------------------------------

#[test]
fn layout_struct_and_brace_enum_coexist() {
    let src = "struct Point:\n    x: Int\n    y: Int\nenum Color { Red, Green, Blue }";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 2, "two top-level decls");
    assert!(matches!(decls[0], Decl::StructDecl(_)), "first is struct");
    assert!(matches!(decls[1], Decl::EnumDecl(_)), "second is enum");
}

#[test]
fn brace_struct_and_layout_enum_coexist() {
    let src = "struct Point { x: Int, y: Int }\nenum Color:\n    Red\n    Green\n    Blue";
    let decls = parse_program(src);
    assert_eq!(decls.len(), 2, "two top-level decls");
    assert!(matches!(decls[0], Decl::StructDecl(_)), "first is struct");
    assert!(matches!(decls[1], Decl::EnumDecl(_)), "second is enum");
}
