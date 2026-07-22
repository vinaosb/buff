# buff-scrape

HTML parsing + crawling for the Buff language. Wraps the mature [`scraper`](https://crates.io/crates/scraper) crate (pure-Rust HTML parser + CSS selectors built on `html5ever` + `selectors` + `cssparser`) for document/element operations, and [`reqwest`](https://crates.io/crates/reqwest) (rustls-tls, already pinned at workspace level) for HTTP fetch + crawl. Safe FFI boundary per the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T43 v1.17 frameworks wave).

## STRUCTURE

```
buff-scrape/
├── Cargo.toml            # scraper + reqwest + thiserror + insta + httpmock deps
├── src/
│   ├── lib.rs            # Module root + FFI safety table + pipeline doc (~85 LOC)
│   ├── document.rs       # Document + re-parse-on-access design (~135 LOC)
│   ├── element.rs        # Element with cached text/html/attrs (~125 LOC)
│   ├── crawler.rs        # Crawler + robots.txt parser + BFS crawl (~310 LOC)
│   └── error.rs          # ScrapeError enum (~60 LOC)
├── examples/
│   ├── scrape_parse.rs        # Document + Element smoke test (hermetic)
│   ├── scrape_select.rs       # nested Element.select drill-down
│   ├── scrape_crawl.rs        # Crawler.crawl against local httpmock
│   └── scrape/
│       ├── parse.buff         # Buff-side forward-decl (matches .rs)
│       ├── select.buff        # Buff-side forward-decl (matches .rs)
│       └── crawl.buff         # Buff-side forward-decl (matches .rs)
└── tests/
    └── core.rs           # 23 tests + 3 insta snapshots (~310 LOC)
```

Total: ~725 LOC (well under the 2500 LOC T43 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new Document/Element method | `src/document.rs` / `src/element.rs` (add `pub fn`) + test in `tests/core.rs` |
| Add a new Crawler method | `src/crawler.rs` |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (15 functions, ≤20 cap)

### `Document` (5 functions)
- Constructors: `from_html` (assoc fn)
- Query: `select(css) -> Vector<Element>`
- Accessors: `text()`, `html()`, `title() -> String?`

### `Element` (5 functions)
- Query: `select(css) -> Vector<Element>` (descendant query)
- Accessors: `text()`, `attr(name) -> String?`, `html()`, `inner_html()`

### `Crawler` (5 functions)
- Constructors: `new(seed_url)` (assoc fn)
- Accessors: `seed() -> String`
- Fetch: `fetch(url) -> Document`
- Crawl: `crawl(max_pages) -> Vector<String>` (BFS, same-origin only)
- Robots: `robots_allows(url) -> Bool`

## CONVENTIONS

- **Pure-Rust only**: `scraper` depends only on `html5ever` + `selectors` + `cssparser` (all pure-Rust). `reqwest` uses rustls-tls (NOT native-tls). No C shims, no cc-rs.
- **No JS rendering**: the MVP explicitly defers JavaScript-rendered DOM extraction to an optional `fantoccini` path (per T43 spec). Static HTML only.
- **No distributed crawling**: single-host BFS only (per T43 spec). Multi-host orchestration deferred to v1.22+.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` in non-test code. Every fallible op returns `Result<T, ScrapeError>`.
- **`catch_unwind` boundary**: every public function wraps its body in `catch_unwind` per FFI guide R6.
- **Send + Sync**: `Document`, `Element`, `Crawler` are all `Send + Sync`. `scraper::Html` is NOT (internal `tendril` borrow graph), so the wrapper caches the source `String` and re-parses per access. The Buff-visible boundary never leaks the `!Send` underlying types.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `scraper` | Upstream HTML parser + CSS selector engine. `buff-scrape` is a safe wrapper; never re-exports `scraper::*` types directly. |
| `reqwest` | Upstream HTTP client (rustls-tls). Consumed by `Crawler` for fetch + crawl. Same workspace pin already used by `buff-http-client` (T33) + `buff-auth` (T34). |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Document` + `PreludeType::Crawler` + assoc-fn + instance-fn variants + return types. `ty.rs` has `Type::Document` + `Type::Crawler` variants + `is_prelude_document()` / `is_prelude_crawler()` predicates. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Document => "buff_scrape::Document"` + `Type::Crawler => "buff_scrape::Crawler"` arms. `lower_prelude_type_assoc_fn` has the `(Document, FromHtml)` + `(Crawler, New)` arms. `lower_prelude_type_instance_fn` has all 9 instance-method arms (5 Document + 4 Crawler). `program_uses_namespace("Document" / "Crawler")` records `buff-scrape` + `scraper` + `reqwest` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **`scraper::Html` is `!Send + !Sync`**: the internal `tendril` / `html5ever` borrow graph prevents `Send + Sync`. `buff-scrape::Document` works around this by caching the source `String` + re-parsing per `select()` / `text()` / `title()` call. Parsing is a single O(N) pass and fast (~1ms per 100KB HTML), so the trade-off is acceptable for MVP. If profiling shows re-parse is a hot path, a thread-local ` Rc<RefCell<Html>>` cache is the v1.18+ follow-up.
- **`Element` caches eagerly**: text + html + inner_html + attrs are computed once at construction (from the `ElementRef` borrow). This makes `Element::clone()` an owned-String clone (cheap relative to the original select query).
- **robots.txt parser is minimal**: supports `User-agent: *` + `Allow:` / `Disallow:` lines + trailing `*` wildcards + `/` (matches everything). Group-scoped rules (per `User-agent: buff-scrape`) are also honoured. Crawl-delay, sitemaps, and per-agent precedence are deferred to v1.18+.
- **Crawler fails open on robots.txt fetch errors**: if `/robots.txt` returns 404 / connection refused / 5xx, the crawler treats every URL as allowed (per the Robots Exclusion Protocol's "fail open" guidance).
- **Single-host crawl**: `crawler.crawl(max_pages)` follows only links whose host matches the seed URL's host. External links are extracted but never visited. Multi-host orchestration is explicitly forbidden by the T43 spec.
- **MSVC host note**: `cargo test -p buff-scrape` works on this Windows host (no native C deps). `scraper` + `reqwest` + `httpmock` are all pure-Rust with rustls-tls.
