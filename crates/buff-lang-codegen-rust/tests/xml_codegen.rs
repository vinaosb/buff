//! T50 integration tests - buff-xml prelude types codegen.
//!
//! Verifies that the Rust codegen lowers the T50 xml surface:
//!
//! - **Xml** namespace (`Xml.from_str(xml) -> XmlDocument`) — one-arg
//!   associated function. Wraps `buff_xml::XmlDocument::from_str(&xml)
//!   .unwrap_or_default()` (panic-free on empty / parse failure —
//!   XmlDocument impls Default as a root-only document).
//! - **XmlDocument** instance methods (`doc.root() -> XmlElement`,
//!   `doc.find(xpath) -> Option<XmlElement>`, `doc.to_string()
//!   -> String`).
//! - **XmlElement** namespace (`XmlElement.new(name, text, attrs)
//!   -> XmlElement`) — three-arg associated function. Wraps
//!   `buff_xml::XmlElement::new(&name.to_string(), &text.to_string(),
//!   attrs.into_iter().map(...).collect())` (the conversion accepts
//!   any IntoIterator yielding string-like tuples — Buff Map literal
//!   codegens to `HashMap<&str, &str>`).
//! - **XmlElement** instance methods (`el.name() -> String`,
//!   `el.attr(name) -> Option<String>`, `el.text() -> Option<String>`,
//!   `el.children() -> Vector<XmlElement>`).
//!
//! Each method wraps the `buff_xml` crate's safe API. `from_str` is
//! fallible but wrapped with `.unwrap_or_default()` (panic-free per
//! the no-panic hard rule). Instance methods lift `&str` /
//! `Option<&str>` / `&[XmlElement]` to owned Buff values (`String` /
//! `Option<String>` / `Vec<XmlElement>`) per FFI guide R2.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test xml_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All types here are prelude types (associated functions + instance
//! methods), so source parsing requires no new keyword / AST node —
//! the existing `MethodCall` shape handles them. We construct ASTs by
//! hand here for the same reasons `nlp_codegen.rs` (T46),
//! `geo_codegen.rs` (T45), `crypto_codegen.rs` (T124k),
//! `fs_codegen.rs` (T124j), `format_codegen.rs` (T124i),
//! `web_codegen.rs` (T124h), `system_codegen.rs` (T124g),
//! `regex_codegen.rs` (T124d), `toml_codegen.rs` (T124e), and
//! `utility_codegen.rs` (T124f) do: direct AST construction decouples
//! the codegen-pinning snapshots from any future parser-restructuring
//! work, and lets us test specific edge cases (e.g. wrong arity,
//! ident vs literal arg) without writing Buff source that the parser
//! may reject for orthogonal reasons.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::{generate_rust, RustCodegen};
use buff_lang_error::Span;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build a free-function decl `func <name>(<params...>) { <body> }`.
fn func_decl(name: &str, params: &[(&str, &str)], body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl { name: ident(name),
    params: params
        .iter()
        .map(|(n, t)| Param {
            name: ident(n),
            ty: named_type(t),
            default_value: None,
            is_comptime: false,
            span: span(),
        })
        .collect(),
    return_type: None,
    body: Block {
        stmts: body_stmts,
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), })
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

fn let_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty: None,
        span: span(),
    }
}

/// `<namespace>.<method>(args...)` AST node (associated-function call
/// shape). The receiver is the bare namespace Ident (e.g. `Xml`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
/// The receiver is a variable Ident (e.g. `doc`).
fn instance_call(recv: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(recv)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// Generate Rust for a single helper function `f` containing `stmts`.
fn codegen_stmts_in(name: &str, stmts: Vec<Stmt>) -> String {
    let func = func_decl(name, &[], stmts);
    generate_rust(&[func]).expect("codegen must succeed")
}

/// Generate Rust for a single helper function `f` containing one expr stmt.
fn codegen_one_expr_in(name: &str, expr: Expr) -> String {
    codegen_stmts_in(name, vec![expr_stmt(expr)])
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ===========================================================================
// 1. Xml.from_str — one-arg assoc fn returning XmlDocument.
// ===========================================================================

#[test]
fn xml_codegen_from_str_with_literal_arg() {
    // Xml.from_str("<root>...</root>")
    //   -> buff_xml::XmlDocument::from_str(&"<root>...</root>")
    //         .unwrap_or_default()
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Xml", "from_str", vec![string_expr("<root>hello</root>")]),
    );
    assert!(
        src.contains("buff_xml::XmlDocument::from_str"),
        "expected `buff_xml::XmlDocument::from_str(` in: {src}"
    );
    assert!(
        src.contains("&"),
        "expected `&` (borrow for from_str) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free on parse failure) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn xml_codegen_from_str_with_ident_arg() {
    // Xml.from_str(xml) where xml is a variable. The arg should
    // splice through as a borrow of the bare ident.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Xml", "from_str", vec![ident_expr("xml")]),
    );
    assert!(
        src.contains("buff_xml::XmlDocument::from_str"),
        "expected `buff_xml::XmlDocument::from_str(` in: {src}"
    );
    assert!(
        src.contains("&xml"),
        "expected `&xml` (borrow of ident arg) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. XmlDocument instance methods — root / find / to_string.
// ===========================================================================

#[test]
fn xml_codegen_doc_root_lowers_correctly() {
    // let doc = Xml.from_str("...")
    // doc.root()
    //   -> buff_xml::XmlDocument::root(&doc).clone()
    // (.clone() lifts &XmlElement -> XmlElement per FFI guide R2.)
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "doc",
                ns_assoc_call("Xml", "from_str", vec![string_expr("<root>hi</root>")]),
            ),
            expr_stmt(instance_call("doc", "root", vec![])),
        ],
    );
    assert!(
        src.contains("buff_xml::XmlDocument::root"),
        "expected `buff_xml::XmlDocument::root(` in: {src}"
    );
    assert!(
        src.contains(".clone()"),
        "expected `.clone()` (lift &XmlElement -> XmlElement) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn xml_codegen_doc_find_lowers_correctly() {
    // let doc = Xml.from_str("...")
    // doc.find("a")
    //   -> buff_xml::XmlDocument::find(&doc, &"a").ok().cloned()
    // (.ok() turns Result<&XmlElement, XmlError> into Option<&XmlElement>;
    //  .cloned() lifts Option<&XmlElement> -> Option<XmlElement>.)
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "doc",
                ns_assoc_call("Xml", "from_str", vec![string_expr("<root><a/></root>")]),
            ),
            expr_stmt(instance_call("doc", "find", vec![string_expr("root/a")])),
        ],
    );
    assert!(
        src.contains("buff_xml::XmlDocument::find"),
        "expected `buff_xml::XmlDocument::find(` in: {src}"
    );
    assert!(
        src.contains(".ok()"),
        "expected `.ok()` (Result -> Option) in: {src}"
    );
    assert!(
        src.contains(".cloned()"),
        "expected `.cloned()` (lift Option<&XmlElement> -> Option<XmlElement>) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn xml_codegen_doc_to_string_lowers_correctly() {
    // let doc = Xml.from_str("...")
    // doc.to_string()
    //   -> buff_xml::XmlDocument::to_string(&doc).unwrap_or_default()
    // (.unwrap_or_default() panic-free on serialize failure.)
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "doc",
                ns_assoc_call("Xml", "from_str", vec![string_expr("<root>hi</root>")]),
            ),
            expr_stmt(instance_call("doc", "to_string", vec![])),
        ],
    );
    assert!(
        src.contains("buff_xml::XmlDocument::to_string"),
        "expected `buff_xml::XmlDocument::to_string(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free on serialize failure) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. XmlElement.new — three-arg assoc fn returning XmlElement.
// ===========================================================================

#[test]
fn xml_codegen_element_new_with_literal_args() {
    // XmlElement.new("test", "hello", {"key": "val"})
    //   -> buff_xml::XmlElement::new(
    //          &"test".to_string(),
    //          &"hello".to_string(),
    //          {"key": "val"}.into_iter()
    //              .map(|(k, v)| (k.to_string(), v.to_string()))
    //              .collect::<Vec<(String, String)>>(),
    //      )
    let map = Expr::MapLit {
        entries: vec![(string_expr("key"), string_expr("val"))],
        span: span(),
    };
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "XmlElement",
            "new",
            vec![string_expr("test"), string_expr("hello"), map],
        ),
    );
    assert!(
        src.contains("buff_xml::XmlElement::new"),
        "expected `buff_xml::XmlElement::new(` in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (name/text conversion to String) in: {src}"
    );
    assert!(
        src.contains(".into_iter()"),
        "expected `.into_iter()` (attrs conversion) in: {src}"
    );
    assert!(
        src.contains(".collect::<Vec<(String, String)>>()"),
        "expected `.collect::<Vec<(String, String)>>()` (attrs -> Vec) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn xml_codegen_element_new_with_ident_args() {
    // XmlElement.new(name, text, attrs) where all three are variables.
    let map = Expr::MapLit {
        entries: vec![(string_expr("k"), string_expr("v"))],
        span: span(),
    };
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "XmlElement",
            "new",
            vec![ident_expr("name"), ident_expr("text"), map],
        ),
    );
    assert!(
        src.contains("buff_xml::XmlElement::new"),
        "expected `buff_xml::XmlElement::new(` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn xml_codegen_element_new_rejects_wrong_arity() {
    // XmlElement.new("only-one-arg") should fail with arity error.
    let result = generate_rust(&[func_decl(
        "f",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "XmlElement",
            "new",
            vec![string_expr("only-name")],
        ))],
    )]);
    assert!(
        result.is_err(),
        "expected XmlElement.new with 1 arg to fail codegen, got: {result:?}"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("3 args") || err.contains("arity"),
        "expected arity error mentioning 3 args, got: {err}"
    );
}

// ===========================================================================
// 4. XmlElement instance methods — name / attr / text / children.
// ===========================================================================

#[test]
fn xml_codegen_element_name_lowers_correctly() {
    // let el = XmlElement.new("test", "hello", {})
    // el.name()
    //   -> el.name().to_string()
    // (.to_string() lifts &str -> String per FFI guide R2.)
    let empty_map = Expr::MapLit {
        entries: vec![],
        span: span(),
    };
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "el",
                ns_assoc_call(
                    "XmlElement",
                    "new",
                    vec![string_expr("test"), string_expr("hello"), empty_map],
                ),
            ),
            expr_stmt(instance_call("el", "name", vec![])),
        ],
    );
    assert!(
        src.contains(".name().to_string()"),
        "expected `.name().to_string()` (lift &str -> String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn xml_codegen_element_text_lowers_correctly() {
    // let el = XmlElement.new("test", "hello", {})
    // el.text()
    //   -> el.text().map(|s| s.to_string())
    // (.map(|s| s.to_string()) lifts Option<&str> -> Option<String>.)
    let empty_map = Expr::MapLit {
        entries: vec![],
        span: span(),
    };
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "el",
                ns_assoc_call(
                    "XmlElement",
                    "new",
                    vec![string_expr("test"), string_expr("hello"), empty_map],
                ),
            ),
            expr_stmt(instance_call("el", "text", vec![])),
        ],
    );
    assert!(
        src.contains(".text().map(|s| s.to_string())"),
        "expected `.text().map(|s| s.to_string())` (lift Option<&str> -> Option<String>) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn xml_codegen_element_attr_lowers_correctly() {
    // let el = XmlElement.new("test", "hi", {})
    // el.attr("key")
    //   -> el.attr(&"key").map(|s| s.to_string())
    let empty_map = Expr::MapLit {
        entries: vec![],
        span: span(),
    };
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "el",
                ns_assoc_call(
                    "XmlElement",
                    "new",
                    vec![string_expr("test"), string_expr("hi"), empty_map],
                ),
            ),
            expr_stmt(instance_call("el", "attr", vec![string_expr("key")])),
        ],
    );
    assert!(
        src.contains(".attr(&"),
        "expected `.attr(&...)` (borrow for attr lookup) in: {src}"
    );
    assert!(
        src.contains(".map(|s| s.to_string())"),
        "expected `.map(|s| s.to_string())` (lift Option<&str> -> Option<String>) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn xml_codegen_element_children_lowers_correctly() {
    // let el = XmlElement.new("test", "hi", {})
    // el.children()
    //   -> el.children().to_vec()
    // (.to_vec() lifts &[XmlElement] -> Vec<XmlElement> per FFI guide R2.)
    let empty_map = Expr::MapLit {
        entries: vec![],
        span: span(),
    };
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "el",
                ns_assoc_call(
                    "XmlElement",
                    "new",
                    vec![string_expr("test"), string_expr("hi"), empty_map],
                ),
            ),
            expr_stmt(instance_call("el", "children", vec![])),
        ],
    );
    assert!(
        src.contains(".children().to_vec()"),
        "expected `.children().to_vec()` (lift &[XmlElement] -> Vec<XmlElement>) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. extern_crates registration (narrow walker).
// ===========================================================================

#[test]
fn xml_codegen_registers_buff_xml_for_xml_namespace() {
    // A program with Xml.from_str(...) registers buff-xml + quick-xml.
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "doc",
            ns_assoc_call("Xml", "from_str", vec![string_expr("<root/>")]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-xml"),
        "extern_crates should contain `buff-xml`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("quick-xml"),
        "extern_crates should contain `quick-xml`, got: {:?}",
        extern_crates
    );
}

#[test]
fn xml_codegen_registers_buff_xml_for_xmlelement_namespace() {
    // A program with XmlElement.new(...) also registers buff-xml +
    // quick-xml (the walker fires on either Xml.* or XmlElement.*).
    let map = Expr::MapLit {
        entries: vec![(string_expr("k"), string_expr("v"))],
        span: span(),
    };
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "el",
            ns_assoc_call(
                "XmlElement",
                "new",
                vec![string_expr("test"), string_expr("hi"), map],
            ),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-xml"),
        "extern_crates should contain `buff-xml` (XmlElement.new), got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("quick-xml"),
        "extern_crates should contain `quick-xml` (XmlElement.new), got: {:?}",
        extern_crates
    );
}

#[test]
fn xml_codegen_no_extern_crate_when_unused() {
    // A program with no Xml.* / XmlElement.* calls should not register
    // buff-xml / quick-xml.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![ident_expr("hi")],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("buff-xml"),
        "extern_crates should NOT contain `buff-xml` when xml types are unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("quick-xml"),
        "extern_crates should NOT contain `quick-xml` when xml types are unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 6. Full program snapshot — pins the end-to-end codegen shape.
// ===========================================================================

#[test]
fn xml_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises the full xml
    // surface from the task spec's acceptance criteria.
    let attrs_map = Expr::MapLit {
        entries: vec![(string_expr("key"), string_expr("val"))],
        span: span(),
    };
    let empty_map = Expr::MapLit {
        entries: vec![],
        span: span(),
    };
    let main = func_decl(
        "main",
        &[],
        vec![
            // Xml.from_str
            let_stmt(
                "doc",
                ns_assoc_call(
                    "Xml",
                    "from_str",
                    vec![string_expr("<root><a>hello</a></root>")],
                ),
            ),
            // XmlDocument.root
            let_stmt("root", instance_call("doc", "root", vec![])),
            // XmlElement.name (on root)
            expr_stmt(instance_call("root", "name", vec![])),
            // XmlElement.text (on root)
            expr_stmt(instance_call("root", "text", vec![])),
            // XmlDocument.find
            let_stmt(
                "found",
                instance_call("doc", "find", vec![string_expr("root/a")]),
            ),
            // XmlDocument.to_string
            expr_stmt(instance_call("doc", "to_string", vec![])),
            // XmlElement.new
            let_stmt(
                "el",
                ns_assoc_call(
                    "XmlElement",
                    "new",
                    vec![string_expr("test"), string_expr("hello"), attrs_map],
                ),
            ),
            // XmlElement.attr (on el)
            expr_stmt(instance_call("el", "attr", vec![string_expr("key")])),
            // XmlElement.children (on el)
            expr_stmt(instance_call("el", "children", vec![])),
            // Use empty_map to verify the empty-Map codegen path too.
            let_stmt(
                "el2",
                ns_assoc_call(
                    "XmlElement",
                    "new",
                    vec![string_expr("x"), string_expr("y"), empty_map],
                ),
            ),
            expr_stmt(instance_call("el2", "name", vec![])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}
