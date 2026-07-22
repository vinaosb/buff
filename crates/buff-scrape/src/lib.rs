//! `buff-scrape` — HTML parsing + crawling for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`scraper`](https://crates.io/crates/scraper)
//! crate (HTML parser + CSS selector engine) for document/element
//! operations, and [`reqwest`](https://crates.io/crates/reqwest)
//! (already pinned at the workspace level) for HTTP fetch + crawl.
//! No JS rendering (deferred to an optional `fantoccini` path per
//! the T43 task spec); no distributed crawling (also forbidden).
//!
//! # Pipeline
//!
//! ```text
//!   Document.from_html(html) ──▶ Document { source, html: scraper::Html }
//!                                      │
//!                                      ├─ doc.select(css) ──▶ Vector<Element>
//!                                      ├─ doc.text() ──▶ String
//!                                      ├─ doc.html() ──▶ String
//!                                      └─ doc.title() ──▶ String?
//!
//!   Element ──▶ el.text() / el.attr(name) / el.html() / el.inner_html()
//!              / el.select(css) ──▶ Vector<Element>
//!
//!   Crawler.new(seed) ──▶ Crawler { client: reqwest::blocking::Client }
//!                              │
//!                              ├─ crawler.fetch(url) ──▶ Document
//!                              ├─ crawler.crawl(max_pages) ──▶ Vector<String>
//!                              └─ crawler.robots_allows(url) ──▶ Bool
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface: `Document`, `Element`, `Crawler`, `ScrapeError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | All constructors return owned types. `select` returns owned `Vec<Element>`. `fetch` returns owned `Document`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, ScrapeError>`. `scraper::SelectorError` + `reqwest::Error` + `url::ParseError` auto-convert via `From`. |
//! | R4 — Thread safety | `Document`, `Element`, `Crawler` are `Send + Sync` (owned data only — no `Rc<T>`, no raw pointers). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `Element` caches owned `String`s (text/html/attrs) at construction; `Document` owns its source + parsed tree. |
//! | R6 — Panic boundary | Every public function wraps its body in `catch_unwind` (per FFI guide §6). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code.

pub mod crawler;
pub mod document;
pub mod element;
pub mod error;

pub use crawler::Crawler;
pub use document::Document;
pub use element::Element;
pub use error::ScrapeError;
