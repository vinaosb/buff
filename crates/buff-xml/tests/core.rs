//! Integration tests for the `buff-xml` crate.
//!
//! Covers all public functions per the T50 spec:
//! - Constructors: `XmlDocument::from_str`
//! - Accessors: `root`, `find`
//! - Element access: `name`, `attr`, `text`, `children`
//! - Serialization: `to_string`
//!
//! 10+ unit tests (per T50 acceptance criteria).

use buff_xml::{XmlDocument, XmlError};

#[test]
fn parse_simple_element() {
    let doc = XmlDocument::from_str("<root>hello</root>").expect("parse");
    assert_eq!(doc.root().name(), "root");
    assert_eq!(doc.root().text(), Some("hello"));
}

#[test]
fn parse_nested_elements() {
    let xml = "<root><child><grandchild>deep</grandchild></child></root>";
    let doc = XmlDocument::from_str(xml).expect("parse");
    assert_eq!(doc.root().name(), "root");
    assert_eq!(doc.root().children().len(), 1);
    assert_eq!(doc.root().children()[0].name(), "child");
    assert_eq!(doc.root().children()[0].children()[0].name(), "grandchild");
    assert_eq!(doc.root().children()[0].children()[0].text(), Some("deep"));
}

#[test]
fn parse_with_attributes() {
    let xml = r#"<root id="42" lang="en">text</root>"#;
    let doc = XmlDocument::from_str(xml).expect("parse");
    assert_eq!(doc.root().attr("id"), Some("42"));
    assert_eq!(doc.root().attr("lang"), Some("en"));
    assert_eq!(doc.root().attr("missing"), None);
}

#[test]
fn parse_self_closing_tag() {
    let xml = "<root><br /><hr /></root>";
    let doc = XmlDocument::from_str(xml).expect("parse");
    assert_eq!(doc.root().children().len(), 2);
    assert_eq!(doc.root().children()[0].name(), "br");
    assert_eq!(doc.root().children()[1].name(), "hr");
}

#[test]
fn parse_empty_input_returns_error() {
    let err = XmlDocument::from_str("").unwrap_err();
    assert!(matches!(err, XmlError::EmptyInput));
    let err = XmlDocument::from_str("   ").unwrap_err();
    assert!(matches!(err, XmlError::EmptyInput));
}

#[test]
fn parse_malformed_xml_returns_error() {
    let err = XmlDocument::from_str("<root><unclosed>").unwrap_err();
    assert!(matches!(err, XmlError::Parse(_)));
}

#[test]
fn find_xpath_matches_root() {
    let doc = XmlDocument::from_str("<root>data</root>").expect("parse");
    let found = doc.find("root").expect("find root");
    assert_eq!(found.text(), Some("data"));
}

#[test]
fn find_xpath_nested() {
    let xml = "<catalog><book><title>Buff Guide</title></book></catalog>";
    let doc = XmlDocument::from_str(xml).expect("parse");
    let title = doc.find("catalog/book/title").expect("find title");
    assert_eq!(title.text(), Some("Buff Guide"));
}

#[test]
fn find_xpath_no_match() {
    let doc = XmlDocument::from_str("<root><a/></root>").expect("parse");
    let err = doc.find("root/b").unwrap_err();
    assert!(matches!(err, XmlError::XPathNoMatch(_)));
}

#[test]
fn serialize_roundtrip() {
    let xml = r#"<root id="1"><child>hello</child><empty /></root>"#;
    let doc = XmlDocument::from_str(xml).expect("parse");
    let serialized = doc.to_string().expect("serialize");
    // Re-parse the serialized output.
    let re_doc = XmlDocument::from_str(&serialized).expect("re-parse");
    assert_eq!(re_doc.root().name(), "root");
    assert_eq!(re_doc.root().attr("id"), Some("1"));
    assert_eq!(re_doc.root().children().len(), 2);
    assert_eq!(re_doc.root().children()[0].name(), "child");
    assert_eq!(re_doc.root().children()[0].text(), Some("hello"));
    assert_eq!(re_doc.root().children()[1].name(), "empty");
}

#[test]
fn element_to_string() {
    let doc = XmlDocument::from_str("<item>value</item>").expect("parse");
    let s = doc.root().to_string();
    assert!(s.contains("<item>"));
    assert!(s.contains("value"));
    assert!(s.contains("</item>"));
}

#[test]
fn multiple_siblings() {
    let xml = "<root><a>1</a><a>2</a><a>3</a></root>";
    let doc = XmlDocument::from_str(xml).expect("parse");
    assert_eq!(doc.root().children().len(), 3);
    assert_eq!(doc.root().children()[0].text(), Some("1"));
    assert_eq!(doc.root().children()[1].text(), Some("2"));
    assert_eq!(doc.root().children()[2].text(), Some("3"));
}

#[test]
fn cdata_is_parsed_as_text() {
    let xml = "<root><![CDATA[hello <world>]]></root>";
    let doc = XmlDocument::from_str(xml).expect("parse");
    assert_eq!(doc.root().text(), Some("hello <world>"));
}

#[test]
fn default_document() {
    let doc = XmlDocument::default();
    assert_eq!(doc.root().name(), "root");
    assert!(doc.root().children().is_empty());
}

#[test]
fn display_format() {
    let doc = XmlDocument::from_str("<foo>bar</foo>").expect("parse");
    let display = format!("{}", doc);
    assert!(display.contains("XmlDocument"));
    assert!(display.contains("foo"));
}
