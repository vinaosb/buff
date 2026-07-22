// T43 example: drill into a document tree via nested Element.select.
//
// Demonstrates that Element values returned from Document.select are
// first-class — you can call .select() / .attr() / .text() on them
// directly. The crawler/HTTP path is intentionally avoided here to
// keep the example hermetic.

use buff_scrape::Document;

const HTML: &str = r#"<html>
  <body>
    <section id="posts">
      <article class="post">
        <h2>First</h2>
        <a href="/p/1">read</a>
      </article>
      <article class="post">
        <h2>Second</h2>
        <a href="/p/2">read</a>
      </article>
    </section>
  </body>
</html>"#;

fn main() {
    let doc = Document::from_html(HTML).expect("parse");
    let posts = doc.select("article.post").expect("select articles");
    println!("{} posts:", posts.len());
    for (i, post) in posts.into_iter().enumerate() {
        let heading = post
            .select("h2")
            .expect("h2")
            .into_iter()
            .next()
            .map(|h| h.text())
            .unwrap_or_default();
        let link = post
            .select("a")
            .expect("a")
            .into_iter()
            .next()
            .and_then(|a| a.attr("href"))
            .unwrap_or_default();
        println!("  post {}: heading={heading:?} link={link:?}", i + 1);
    }
}
