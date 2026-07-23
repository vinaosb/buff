//! HTML element wrapper — a single selected node from a [`crate::Document`].
//!
//! Wraps `scraper::ElementRef` (a borrowed view into the parsed
//! `scraper::Html`) at a safe FFI boundary per the T4 guide. The
//! element stores owned copies of its text / html / attributes so
//! it is `'static + Send + Sync + Clone` (FFI guide R4/R5).

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use scraper::{ElementRef, Selector};

use crate::error::ScrapeError;

/// A single HTML element returned by [`crate::Document::select`] or
/// [`Element::select`].
///
/// Constructed via [`crate::Document::select`]; never built directly
/// by user code. All accessors read from values cached at
/// construction time — the underlying `scraper::ElementRef` borrow
/// ends when the element is materialised as owned.
#[derive(Clone)]
pub struct Element {
    html: String,
    inner_html: String,
    text: String,
    attrs: BTreeMap<String, String>,
}

impl Element {
    pub(crate) fn from_ref(r: ElementRef<'_>) -> Self {
        let mut attrs = BTreeMap::new();
        for (name, value) in r.value().attrs() {
            attrs.insert((*name).to_string(), (*value).to_string());
        }
        Element {
            html: r.html(),
            inner_html: r.inner_html(),
            text: r.text().collect::<String>(),
            attrs,
        }
    }

    /// The text content of this element (concatenated descendant
    /// text nodes, no tags). Zero args. Returns String.
    pub fn text(&self) -> String {
        self.text.clone()
    }

    /// Read an attribute by name. Returns `None` when the attribute
    /// is absent (Buff's `String?` surface). One arg (String).
    pub fn attr(&self, name: &str) -> Option<String> {
        self.attrs.get(name).cloned()
    }

    /// The full HTML serialization of this element (opening tag +
    /// inner content + closing tag). Zero args. Returns String.
    pub fn html(&self) -> String {
        self.html.clone()
    }

    /// The inner HTML of this element (content WITHOUT the opening
    /// / closing tag). Zero args. Returns String.
    pub fn inner_html(&self) -> String {
        self.inner_html.clone()
    }

    /// Run a CSS selector against this element's descendants.
    /// Returns matching [`Element`]s as owned values. One arg
    /// (String). The cached `html()` is re-parsed for the query.
    pub fn select(&self, css: &str) -> Result<Vec<Element>, ScrapeError> {
        if css.is_empty() {
            return Err(ScrapeError::EmptyInput);
        }
        let css_owned = css.to_string();
        let html_owned = self.html.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let selector =
                Selector::parse(&css_owned).map_err(|e| ScrapeError::Selector(e.to_string()))?;
            let fragment = scraper::Html::parse_fragment(&html_owned);
            let elements: Vec<Element> =
                fragment.select(&selector).map(Element::from_ref).collect();
            Ok(elements)
        }));
        match result {
            Ok(inner) => inner,
            Err(_) => Err(ScrapeError::Panic),
        }
    }

    /// pub(crate): all attributes as an ordered map. Used by the
    /// crawler's link extractor + snapshot tests. NOT part of the
    /// stable Buff-visible surface (T43 caps public API at 20 fns).
    pub(crate) fn attrs(&self) -> &BTreeMap<String, String> {
        &self.attrs
    }
}

impl std::fmt::Debug for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let preview: String = self.html.chars().take(60).collect();
        write!(f, "Element({preview})")
    }
}

impl PartialEq for Element {
    fn eq(&self, other: &Self) -> bool {
        self.html == other.html
    }
}

impl Eq for Element {}

impl std::fmt::Display for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let preview: String = self.html.chars().take(40).collect();
        write!(f, "Element({preview})")
    }
}
