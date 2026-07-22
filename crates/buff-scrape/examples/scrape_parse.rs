// T43 example: parse an HTML string + query headings + links.
//
// Demonstrates the Document/Element parsing path (no network — the
// HTML is inline). Mirrors the `image_load_filter.rs` smoke test
// pattern: build a tiny fixture inline so the example is hermetic.

use buff_scrape::Document;

const HTML: &str = r#"<html>
  <head><title>Example</title></head>
  <body>
    <h1>Hello</h1>
    <ul>
      <li><a href="https://example.com/a">A</a></li>
      <li><a href="https://example.com/b">B</a></li>
    </ul>
  </body>
</html>"#;

fn main() {
    let doc = Document::from_html(HTML).expect("parse");
    println!("title: {:?}", doc.title());

    let heading = doc
        .select("h1")
        .expect("select h1")
        .into_iter()
        .next()
        .expect("h1");
    println!("heading text: {}", heading.text());

    let links: Vec<String> = doc
        .select("a")
        .expect("select a")
        .into_iter()
        .filter_map(|a| a.attr("href"))
        .collect();
    println!("links: {links:?}");

    let body_text = doc.text();
    println!("document text: {body_text}");
}
