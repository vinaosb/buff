// T50 example: parse XML, query elements, serialize.
//
// Demonstrates the full XML pipeline: parse a string into an
// XmlDocument, query elements via XPath-like paths, read attributes
// and text content, and serialize back to XML.

use buff_xml::XmlDocument;

fn main() {
    let xml = r#"
<catalog>
  <book id="1">
    <title>Buff Guide</title>
    <author>Vinicius</author>
    <price currency="USD">29.99</price>
  </book>
  <book id="2">
    <title>Rust for Beginners</title>
    <author>Jane Doe</author>
    <price currency="EUR">34.99</price>
  </book>
</catalog>"#;

    let doc = XmlDocument::from_str(xml).expect("parse XML");
    println!("root: {}", doc.root().name());
    println!("children: {}", doc.root().children().len());

    let first_title = doc.find("catalog/book/title").expect("find title");
    println!("first title: {}", first_title.text().unwrap_or("(none)"));

    let first_price = doc.find("catalog/book/price").expect("find price");
    println!(
        "first price: {} {}",
        first_price.text().unwrap_or("(none)"),
        first_price.attr("currency").unwrap_or("(none)")
    );

    let serialized = doc.to_string().expect("serialize");
    println!("\n--- serialized ---\n{}", serialized);
}
