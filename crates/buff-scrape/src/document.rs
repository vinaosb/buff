//! HTML document wrapper — parsed HTML tree + CSS selector entry point.
//!
//! `scraper::Html` is `!Send + !Sync` (internal `tendril` /
//! `html5ever` borrow graph), so this type caches only the source
//! string and re-parses per access (mirrors the `Element` design).
//! `Document` is `'static + Send + Sync + Clone`.

use std::panic::{catch_unwind, AssertUnwindSafe};

use scraper::Selector;

use crate::element::Element;
use crate::error::ScrapeError;

/// A parsed HTML document. Constructed via [`Document::from_html`]
/// (zero network I/O — `Document` is purely an in-memory parse).
/// For HTTP fetch, see [`crate::Crawler::fetch`].
///
/// Stores the source HTML as an owned `String`; the `scraper::Html`
/// tree is rebuilt on each `select()` / `text()` / `title()` call
/// (parsing is fast — `scraper::Html::parse_document` does a single
/// pass; the `String` cache keeps clone cheap). Matches the
/// `Element` design rationale: stay `Send + Sync` at the cost of a
/// re-parse per access.
#[derive(Clone)]
pub struct Document {
    source: String,
}

impl Document {
    /// Parse a `&str` of HTML into a [`Document`]. Body wrapped in
    /// `catch_unwind` per FFI guide R6.
    pub fn from_html(html: &str) -> Result<Self, ScrapeError> {
        if html.is_empty() {
            return Err(ScrapeError::EmptyInput);
        }
        let html_owned = html.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = scraper::Html::parse_document(&html_owned);
        }));
        match result {
            Ok(()) => Ok(Document { source: html_owned }),
            Err(_) => Err(ScrapeError::Panic),
        }
    }

    /// pub(crate): wrap a verified source string into a Document.
    /// Used internally by [`crate::Crawler::fetch`] after a
    /// successful HTTP body read + `from_html` validation.
    pub(crate) fn from_source(source: String) -> Self {
        Document { source }
    }

    fn parsed(&self) -> scraper::Html {
        scraper::Html::parse_document(&self.source)
    }

    /// Run a CSS selector against the whole document. Returns
    /// matching [`Element`]s as owned values. One arg (String).
    pub fn select(&self, css: &str) -> Result<Vec<Element>, ScrapeError> {
        if css.is_empty() {
            return Err(ScrapeError::EmptyInput);
        }
        let css_owned = css.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let selector = Selector::parse(&css_owned)
                .map_err(|e| ScrapeError::Selector(e.to_string()))?;
            let document = self.parsed();
            let elements: Vec<Element> =
                document.select(&selector).map(Element::from_ref).collect();
            Ok(elements)
        }));
        match result {
            Ok(inner) => inner,
            Err(_) => Err(ScrapeError::Panic),
        }
    }

    /// The full text content of the document (concatenated text
    /// nodes from the root downward, no tags). Zero args.
    pub fn text(&self) -> String {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.parsed().root_element().text().collect::<String>()
        }));
        result.unwrap_or_default()
    }

    /// The full HTML serialization of the document. Zero args.
    /// Returns the original source string passed to `from_html`.
    pub fn html(&self) -> String {
        self.source.clone()
    }

    /// The `<title>` element's text content. Zero args. Returns
    /// `None` when the document has no `<title>` element (Buff's
    /// `String?` surface).
    pub fn title(&self) -> Option<String> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let selector = Selector::parse("title").ok()?;
            let document = self.parsed();
            document
                .select(&selector)
                .next()
                .map(|e| e.text().collect::<String>())
        }));
        result.ok().flatten()
    }
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Document({} bytes)", self.source.len())
    }
}

impl std::fmt::Display for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Document({} bytes)", self.source.len())
    }
}

impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for Document {}

impl Default for Document {
    fn default() -> Self {
        Document {
            source: "<html></html>".to_string(),
        }
    }
}
