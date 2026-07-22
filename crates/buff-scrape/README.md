# buff-scrape

> HTML parsing + crawling for the **Buff** language. Pure-Rust MVP.

`buff-scrape` wraps the mature [`scraper`](https://crates.io/crates/scraper) crate (HTML parser + CSS selectors) and [`reqwest`](https://crates.io/crates/reqwest) (rustls-tls HTTP client) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses HTML via the `Document` / `Element` prelude types + crawls via `Crawler`:

```buff
doc = Document.from_html(html: "<html><body><h1>Hello</h1></body></html>")
heading = doc.select(css: "h1").get(0)
print(heading.text())

crawler = Crawler.new(seed_url: "https://example.com/")
visited = crawler.crawl(max_pages: 10)
print("crawled", len(visited), "pages")
```

**Status: experimental** (T43 v1.17 frameworks wave).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Document` / `Element` / `Crawler` prelude types.

For direct Rust use:

```bash
cargo add buff-scrape --path crates/buff-scrape
```

## Quick start

```rust
use buff_scrape::{Document, ScrapeError};

fn main() -> Result<(), ScrapeError> {
    let doc = Document::from_html("<html><body><h1>Hello</h1></body></html>")?;
    let heading = doc.select("h1")?.into_iter().next().expect("h1");
    assert_eq!(heading.text(), "Hello");
    Ok(())
}
```

## Public API

### `Document` — parsed HTML tree

| Method | Signature | Notes |
|---|---|---|
| `Document::from_html` | `(html) -> Result<Document, ScrapeError>` | Zero network I/O. `catch_unwind` boundary. |
| `doc.select` | `(css) -> Result<Vector<Element>, ScrapeError>` | CSS selector query. |
| `doc.text` | `() -> String` | Concatenated text nodes. |
| `doc.html` | `() -> String` | Round-trips the source string. |
| `doc.title` | `() -> Option<String>` | `<title>` text. |

### `Element` — a single selected node

| Method | Signature |
|---|---|
| `el.text` | `() -> String` |
| `el.attr` | `(name) -> Option<String>` |
| `el.html` | `() -> String` |
| `el.inner_html` | `() -> String` |
| `el.select` | `(css) -> Result<Vector<Element>, ScrapeError>` |

### `Crawler` — HTTP fetch + BFS crawl

| Method | Signature | Notes |
|---|---|---|
| `Crawler::new` | `(seed_url) -> Result<Crawler, ScrapeError>` | Configures UA + 15s timeout. |
| `crawler.seed` | `() -> String` | Round-trip accessor. |
| `crawler.fetch` | `(url) -> Result<Document, ScrapeError>` | GET + parse. |
| `crawler.crawl` | `(max_pages) -> Result<Vector<String>, ScrapeError>` | Same-host BFS, robots-aware. |
| `crawler.robots_allows` | `(url) -> Bool` | Fail-open on missing robots.txt. |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md):

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Document`, `Element`, `Crawler`, `ScrapeError`. No `*const` / `*mut`. |
| R2 — Ownership boundary | `select` returns owned `Vec<Element>`. `fetch` returns owned `Document`. |
| R3 — Error mapping | Every fallible op returns `Result<T, ScrapeError>`. `scraper::SelectorErrorKind` + `reqwest::Error` + `url::ParseError` auto-convert. |
| R4 — Thread safety | `Document` / `Element` / `Crawler` are `Send + Sync`. (`scraper::Html` itself is `!Send`; the wrapper caches the source `String` + re-parses per access.) |
| R5 — Lifetime hiding | No public lifetime parameters. All public types own their data. |
| R6 — Panic boundary | Every public function wraps its body in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-scrape
cargo clippy -p buff-scrape --all-targets -- -D warnings
cargo fmt -p buff-scrape --check
```

Tests are hermetic: HTML fixtures are inline constants; Crawler tests use `httpmock` so no real network is touched.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.
