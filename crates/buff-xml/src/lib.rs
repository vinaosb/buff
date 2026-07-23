//! `buff-xml` — XML parsing for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`quick-xml`](https://docs.rs/quick-xml)
//! crate. Provides streaming XML parsing with a simple DOM-like API:
//! parse a string into a [`XmlDocument`], query elements via XPath-like
//! paths, read attributes and text content, and serialize back to XML.
//!
//! # Pipeline
//!
//! ```text
//!   XmlDocument::from_str(xml) ──▶ XmlDocument { root: XmlElement }
//!                                      │
//!                                      ├─ doc.root() -> &XmlElement
//!                                      ├─ doc.find(xpath) -> Result<&XmlElement>
//!                                      └─ doc.to_string() -> String
//!                                              │
//!                                      XmlElement
//!                                      ├─ el.name() -> &str
//!                                      ├─ el.attr(name) -> Option<&str>
//!                                      ├─ el.text() -> Option<&str>
//!                                      ├─ el.children() -> &[XmlElement]
//!                                      └─ el.to_string() -> String
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `XmlDocument`, `XmlElement`, `XmlError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `from_str` returns owned `XmlDocument`. `to_string` returns owned `String`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, XmlError>`. `quick_xml::Error` mapped via `From`. |
//! | R4 — Thread safety | `XmlDocument` is `Send + Sync` (wraps owned `Vec<XmlElement>` — no interior mutability). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `XmlDocument` owns all its data. |
//! | R6 — Panic boundary | `from_str` wraps its body in `catch_unwind` (per FFI guide §6). |

pub mod error;

pub use error::XmlError;

use std::panic::{catch_unwind, AssertUnwindSafe};

/// A parsed XML document.
///
/// Constructed via [`XmlDocument::from_str`] (parse an XML string).
/// The document owns a tree of [`XmlElement`] nodes. Query the root
/// element via [`XmlDocument::root`], find nested elements via
/// [`XmlDocument::find`] (simple XPath-like path), or serialize back
/// to XML via [`XmlDocument::to_string`].
#[derive(Debug, Clone, PartialEq)]
pub struct XmlDocument {
    root: XmlElement,
}

/// A single XML element with a name, attributes, text content, and
/// child elements.
///
/// Constructed internally by [`XmlDocument::from_str`]. Access the
/// element's name via [`XmlElement::name`], attributes via
/// [`XmlElement::attr`], text content via [`XmlElement::text`], and
/// child elements via [`XmlElement::children`].
#[derive(Debug, Clone, PartialEq)]
pub struct XmlElement {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<XmlElement>,
}

impl XmlDocument {
    /// Parse an XML string into a [`XmlDocument`].
    ///
    /// Returns [`XmlError::EmptyInput`] for empty input,
    /// [`XmlError::Parse`] for malformed XML, and
    /// [`XmlError::NoRootElement`] if the document has no root element.
    ///
    /// The body is wrapped in `catch_unwind` per T4 FFI guide R6 so a
    /// panic in the parser becomes a stable `Err(XmlError::Panic)`.
    pub fn from_str(xml: &str) -> Result<Self, XmlError> {
        if xml.trim().is_empty() {
            return Err(XmlError::EmptyInput);
        }
        let xml_owned = xml.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| parse_document(&xml_owned)));
        match result {
            Ok(Ok(doc)) => Ok(doc),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(XmlError::Panic),
        }
    }

    /// Borrow the root element of this document.
    pub fn root(&self) -> &XmlElement {
        &self.root
    }

    /// Find the first element matching a simple XPath-like path.
    ///
    /// The path is a `/`-separated sequence of element names, e.g.
    /// `"root/child/grandchild"`. Returns the first matching element
    /// at each level. Returns [`XmlError::XPathNoMatch`] if any
    /// segment does not match.
    pub fn find(&self, xpath: &str) -> Result<&XmlElement, XmlError> {
        let parts: Vec<&str> = xpath.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(XmlError::XPathNoMatch(xpath.to_string()));
        }
        // First segment must match the root element name.
        if self.root.name != parts[0] {
            return Err(XmlError::XPathNoMatch(xpath.to_string()));
        }
        let mut current = &self.root;
        for segment in &parts[1..] {
            let found = current.children.iter().find(|c| c.name == *segment);
            match found {
                Some(child) => current = child,
                None => return Err(XmlError::XPathNoMatch(xpath.to_string())),
            }
        }
        Ok(current)
    }

    /// Serialize this document back to an XML string.
    pub fn to_string(&self) -> Result<String, XmlError> {
        let result = catch_unwind(AssertUnwindSafe(|| serialize_element(&self.root, 0)));
        match result {
            Ok(s) => Ok(s),
            Err(_) => Err(XmlError::Panic),
        }
    }
}

impl XmlElement {
    /// Construct a new `XmlElement` with the given name, text content,
    /// and attributes. The element has no children.
    ///
    /// Used by the Buff `XmlElement.new(name, text, attrs)` constructor
    /// (T50 prelude wiring). The `attrs` arg is a `Vec<(String, String)>`
    /// because Buff's map-literal codegen produces a `HashMap<String,
    /// String>` and the codegen inserts `.into_iter().collect()` to
    /// satisfy this signature (works for any IntoIterator yielding
    /// `(String, String)`).
    pub fn new(name: &str, text: &str, attrs: Vec<(String, String)>) -> Self {
        XmlElement {
            name: name.to_string(),
            attrs,
            text: text.to_string(),
            children: Vec::new(),
        }
    }

    /// The element's tag name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the value of an attribute by name. Returns `None` if the
    /// attribute does not exist.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The concatenated text content of this element (excluding child
    /// elements). Returns `None` if the element has no text content.
    pub fn text(&self) -> Option<&str> {
        if self.text.is_empty() {
            None
        } else {
            Some(self.text.as_str())
        }
    }

    /// The child elements of this element.
    pub fn children(&self) -> &[XmlElement] {
        &self.children
    }

    /// Serialize this element and its children to an XML string.
    pub fn to_string(&self) -> String {
        serialize_element(self, 0)
    }
}

// ---- Internal parsing logic ------------------------------------------------

/// Parse an XML string into a [`XmlDocument`] using `quick_xml`.
fn parse_document(xml: &str) -> Result<XmlDocument, XmlError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut stack: Vec<XmlElement> = Vec::new();
    let mut current_text = String::new();
    let mut root: Option<XmlElement> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                // Flush any accumulated text into the parent before pushing a new element.
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attrs = parse_attributes(e);
                if let Some(parent) = stack.last_mut() {
                    if !current_text.trim().is_empty() {
                        parent.text = current_text.trim().to_string();
                        current_text.clear();
                    }
                }
                stack.push(XmlElement {
                    name,
                    attrs,
                    text: String::new(),
                    children: Vec::new(),
                });
            }
            Ok(Event::End(ref _e)) => {
                if let Some(mut child) = stack.pop() {
                    // Flush accumulated text into the element being closed.
                    if !current_text.trim().is_empty() {
                        child.text = current_text.trim().to_string();
                        current_text.clear();
                    }
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(child);
                    } else {
                        // This was the root element.
                        root = Some(child);
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(text) = e.unescape() {
                    current_text.push_str(&text);
                }
            }
            Ok(Event::CData(ref e)) => {
                if let Ok(text) = e.decode() {
                    current_text.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Ok(Event::Empty(ref e)) => {
                // Self-closing tag: <tag attr="val" />
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attrs = parse_attributes(e);
                let el = XmlElement {
                    name,
                    attrs,
                    text: String::new(),
                    children: Vec::new(),
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(el);
                } else {
                    root = Some(el);
                }
            }
            Err(e) => return Err(XmlError::from(e)),
            _ => {} // Skip comments, PIs, DTD, etc.
        }
        buf.clear();
    }

    match root {
        Some(r) => Ok(XmlDocument { root: r }),
        None => Err(XmlError::NoRootElement),
    }
}

/// Parse attributes from a `quick_xml` start/empty event.
fn parse_attributes(e: &quick_xml::events::BytesStart) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    for attr_result in e.attributes() {
        if let Ok(attr) = attr_result {
            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
            let value = String::from_utf8_lossy(&attr.value).to_string();
            attrs.push((key, value));
        }
    }
    attrs
}

/// Serialize an [`XmlElement`] and its children to an XML string.
fn serialize_element(el: &XmlElement, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let mut out = String::new();

    out.push_str(&indent);
    out.push('<');
    out.push_str(&el.name);

    for (k, v) in &el.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&escape_xml_attr(v));
        out.push('"');
    }

    if el.children.is_empty() && el.text.is_empty() {
        out.push_str(" />\n");
        return out;
    }

    out.push('>');

    if !el.children.is_empty() {
        out.push('\n');
        for child in &el.children {
            out.push_str(&serialize_element(child, depth + 1));
        }
        out.push_str(&indent);
        out.push_str("</");
        out.push_str(&el.name);
        out.push_str(">\n");
    } else {
        out.push_str(&escape_xml_text(&el.text));
        out.push_str("</");
        out.push_str(&el.name);
        out.push_str(">\n");
    }

    out
}

/// Escape special XML characters in attribute values.
fn escape_xml_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape special XML characters in text content.
fn escape_xml_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Default for XmlDocument {
    fn default() -> Self {
        XmlDocument {
            root: XmlElement {
                name: String::from("root"),
                attrs: Vec::new(),
                text: String::new(),
                children: Vec::new(),
            },
        }
    }
}

impl std::fmt::Display for XmlDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XmlDocument(root={})", self.root.name)
    }
}

impl std::fmt::Display for XmlElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<{}>{} children, {} attrs",
            self.name,
            self.children.len(),
            self.attrs.len()
        )
    }
}
