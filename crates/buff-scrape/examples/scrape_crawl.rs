// T43 example: Crawler crawls a local httpmock server (hermetic —
// no real network). Demonstrates fetch + crawl + robots_allows.
//
// Run with: cargo run -p buff-scrape --example scrape_crawl

fn main() {
    let server = httpmock::MockServer::start();
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/");
        then.status(200).body(
            r#"<html><body>
                <h1>Home</h1>
                <a href="/page1">Page 1</a>
                <a href="/page2">Page 2</a>
               </body></html>"#,
        );
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/page1");
        then.status(200).body("<html><body><h1>Page 1</h1></body></html>");
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/page2");
        then.status(200).body("<html><body><h1>Page 2</h1></body></html>");
    });
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/robots.txt");
        then.status(200).body("User-agent: *\nDisallow: /page2\n");
    });

    let base = server.base_url();
    let crawler = buff_scrape::Crawler::new(&base).expect("crawler");
    println!("seed: {}", crawler.seed());

    let doc = crawler.fetch(&format!("{base}/")).expect("fetch");
    println!("home title: {:?}", doc.title());

    println!(
        "robots_allows(\"/page1\") = {}",
        crawler.robots_allows(&format!("{base}/page1"))
    );
    println!(
        "robots_allows(\"/page2\") = {} (disallowed)",
        crawler.robots_allows(&format!("{base}/page2"))
    );

    let visited = crawler.crawl(10).expect("crawl");
    println!("crawled {} pages:", visited.len());
    for url in &visited {
        println!("  {url}");
    }
}
