//! Integration tests for the `buff-scrape` crate.
//!
//! Covers all 14 public functions per the T43 spec:
//! - Document: `from_html`, `select`, `text`, `html`, `title` (5)
//! - Element: `text`, `attr`, `html`, `inner_html`, `select` (5)
//! - Crawler: `new`, `seed`, `fetch`, `crawl`, `robots_allows` (5)
//!
//! Crawler tests use `httpmock` for hermetic HTTP mocking (no real
//! network — matches the buff-http-client test pattern).

use buff_scrape::{Crawler, Document, Element, ScrapeError};

const SAMPLE_HTML: &str = r#"<!DOCTYPE html>
<html>
<head><title>Sample Page</title></head>
<body>
    <h1>Hello, Buff!</h1>
    <p class="greeting">Welcome to <a href="/about">the about page</a>.</p>
    <ul>
        <li class="item"><a href="/page1">Page 1</a></li>
        <li class="item"><a href="/page2">Page 2</a></li>
        <li class="item"><a href="https://external.example.com/">External</a></li>
    </ul>
    <p data-id="42">tagged paragraph</p>
</body>
</html>"#;

#[test]
fn document_from_html_parses_valid_html() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    assert_eq!(doc.title(), Some("Sample Page".to_string()));
}

#[test]
fn document_from_html_rejects_empty_input() {
    let err = Document::from_html("").unwrap_err();
    assert!(matches!(err, ScrapeError::EmptyInput));
}

#[test]
fn document_select_returns_matching_elements() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let items = doc.select("li.item").expect("select");
    assert_eq!(items.len(), 3);
    let hrefs: Vec<String> = items
        .iter()
        .filter_map(|el| el.attr("class"))
        .collect();
    assert_eq!(hrefs, vec!["item".to_string(); 3]);
}

#[test]
fn document_select_rejects_empty_css() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let err = doc.select("").unwrap_err();
    assert!(matches!(err, ScrapeError::EmptyInput));
}

#[test]
fn document_select_rejects_invalid_css() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let err = doc.select("!!!invalid!!!").unwrap_err();
    assert!(matches!(err, ScrapeError::Selector(_)));
}

#[test]
fn document_text_collects_all_text_nodes() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let text = doc.text();
    assert!(text.contains("Hello, Buff!"));
    assert!(text.contains("Page 1"));
    assert!(text.contains("tagged paragraph"));
}

#[test]
fn document_html_round_trips_source() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    assert_eq!(doc.html(), SAMPLE_HTML);
}

#[test]
fn element_text_and_attr_and_html() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let link = doc.select("a").expect("select").into_iter().next().expect("first <a>");
    assert_eq!(link.text(), "the about page");
    assert_eq!(link.attr("href"), Some("/about".to_string()));
    assert!(link.html().contains("<a"));
    assert!(link.inner_html().contains("the about page"));
}

#[test]
fn element_attr_returns_none_for_missing() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let h1 = doc.select("h1").expect("select").into_iter().next().expect("h1");
    assert_eq!(h1.attr("class"), None);
    assert_eq!(h1.text(), "Hello, Buff!");
}

#[test]
fn element_select_drills_into_descendants() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let ul = doc.select("ul").expect("select").into_iter().next().expect("ul");
    let links: Vec<String> = ul
        .select("a")
        .expect("ul.select(a)")
        .into_iter()
        .map(|a| a.attr("href").unwrap_or_default())
        .collect();
    assert_eq!(
        links,
        vec![
            "/page1".to_string(),
            "/page2".to_string(),
            "https://external.example.com/".to_string()
        ]
    );
}

#[test]
fn element_equality_and_clone() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let a = doc.select("h1").expect("select").into_iter().next().expect("h1");
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn element_partial_order_debug_format() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let h1 = doc.select("h1").expect("select").into_iter().next().expect("h1");
    let debug = format!("{h1:?}");
    assert!(debug.starts_with("Element("));
    let display = format!("{h1}");
    assert!(display.starts_with("Element("));
}

// ---- Crawler (httpmock-backed) ------------------------------------------

fn make_mock_server() -> httpmock::MockServer {
    httpmock::MockServer::start()
}

#[test]
fn crawler_new_rejects_empty_seed() {
    let err = Crawler::new("").unwrap_err();
    assert!(matches!(err, ScrapeError::EmptyInput));
}

#[test]
fn crawler_seed_round_trip() {
    let crawler = Crawler::new("https://example.com/").expect("new");
    assert_eq!(crawler.seed(), "https://example.com/");
}

#[test]
fn crawler_fetch_returns_document() {
    let server = make_mock_server();
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/page.html");
        then.status(200)
            .header("content-type", "text/html")
            .body("<html><head><title>Fetched</title></head><body><p>hi</p></body></html>");
    });

    let crawler = Crawler::new(&server.base_url()).expect("new");
    let doc = crawler
        .fetch(&server.url("/page.html"))
        .expect("fetch ok");
    assert_eq!(doc.title(), Some("Fetched".to_string()));
}

#[test]
fn crawler_fetch_surfaces_http_error_on_non_2xx() {
    let server = make_mock_server();
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/missing");
        then.status(404);
    });

    let crawler = Crawler::new(&server.base_url()).expect("new");
    let err = crawler.fetch(&server.url("/missing")).unwrap_err();
    assert!(matches!(err, ScrapeError::Http(_)));
}

#[test]
fn crawler_fetch_rejects_empty_url() {
    let crawler = Crawler::new("https://example.com/").expect("new");
    let err = crawler.fetch("").unwrap_err();
    assert!(matches!(err, ScrapeError::EmptyInput));
}

#[test]
fn crawler_crawl_zero_pages_returns_empty() {
    let crawler = Crawler::new("https://example.com/").expect("new");
    let visited = crawler.crawl(0).expect("crawl ok");
    assert!(visited.is_empty());
}

#[test]
fn crawler_crawl_negative_returns_empty() {
    let crawler = Crawler::new("https://example.com/").expect("new");
    let visited = crawler.crawl(-5).expect("crawl ok");
    assert!(visited.is_empty());
}

#[test]
fn crawler_crawl_bfs_visits_seed_and_linked_pages() {
    let server = make_mock_server();
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/");
        then.status(200).body(
            r#"<html><body>
               <a href="/a.html">A</a>
               <a href="/b.html">B</a>
               <a href="https://other.example.com/external">External</a>
               </body></html>"#,
        );
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/a.html");
        then.status(200).body(r#"<html><body><a href="/c.html">C</a></body></html>"#);
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/b.html");
        then.status(200).body(r#"<html><body>B content</body></html>"#);
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/c.html");
        then.status(200).body(r#"<html><body>C content</body></html>"#);
    });
    // robots.txt: allow everything (no Disallow rules).
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/robots.txt");
        then.status(200).body("User-agent: *\nAllow: /\n");
    });

    let base = server.base_url();
    let crawler = Crawler::new(&base).expect("new");
    let visited = crawler.crawl(10).expect("crawl ok");
    assert!(visited.contains(&format!("{base}/")));
    assert!(visited.contains(&format!("{base}/a.html")));
    assert!(visited.contains(&format!("{base}/b.html")));
    assert!(visited.contains(&format!("{base}/c.html")));
    // External links are not followed.
    assert!(!visited
        .iter()
        .any(|u| u.contains("other.example.com")));
}

#[test]
fn crawler_crawl_respects_robots_disallow() {
    let server = make_mock_server();
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/robots.txt");
        then.status(200)
            .body("User-agent: *\nDisallow: /private/\nAllow: /\n");
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/");
        then.status(200)
            .body(r#"<html><body><a href="/private/secret.html">x</a><a href="/pub.html">y</a></body></html>"#);
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/pub.html");
        then.status(200).body("<html><body>pub</body></html>");
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/private/secret.html");
        then.status(200).body("<html><body>secret</body></html>");
    });

    let base = server.base_url();
    let crawler = Crawler::new(&base).expect("new");
    let visited = crawler.crawl(10).expect("crawl ok");
    let secret_url = format!("{base}/private/secret.html");
    assert!(
        !visited.contains(&secret_url),
        "robots-disallowed URL must not appear in visited: {visited:?}"
    );
}

#[test]
fn robots_allows_fails_open_on_missing_robots_txt() {
    let server = make_mock_server();
    // No /robots.txt mock — every request to it returns 404 (default).
    let crawler = Crawler::new(&server.base_url()).expect("new");
    assert!(
        crawler.robots_allows(&server.url("/anything")),
        "robots_allows must return true when robots.txt is unreachable"
    );
}

// ---- Insta snapshots (3) ------------------------------------------------

#[test]
fn snapshot_document_debug_and_display() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    insta::assert_snapshot!("document_debug", format!("{doc:?}"));
    insta::assert_snapshot!("document_display", format!("{doc}"));
}

#[test]
fn snapshot_element_html() {
    let doc = Document::from_html(SAMPLE_HTML).expect("parse");
    let p = doc
        .select("p[data-id]")
        .expect("select")
        .into_iter()
        .next()
        .expect("p[data-id]");
    insta::assert_snapshot!("element_tagged_p_html", p.html());
    insta::assert_snapshot!("element_tagged_p_inner_html", p.inner_html());
    insta::assert_snapshot!("element_tagged_p_text", p.text());
    let attrs: Vec<(String, String)> = p
        .attrs()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    insta::assert_snapshot!("element_tagged_p_attrs", format!("{attrs:?}"));
}

#[test]
fn snapshot_scrape_error_variants() {
    let e1 = ScrapeError::EmptyInput;
    let e2 = ScrapeError::Selector("unexpected token".to_string());
    let e3 = ScrapeError::Http("connection refused".to_string());
    let e4 = ScrapeError::Url("invalid scheme".to_string());
    let e5 = ScrapeError::Panic;
    insta::assert_snapshot!(
        "scrape_error_variants",
        format!("{e1}\n{e2}\n{e3}\n{e4}\n{e5}")
    );
}
