// T46 example: tokenize + sentence-segment a paragraph.

use buff_nlp::Text;

fn main() {
    let paragraph = "Hello world! This is a test. Tokenization is fun; segmentation too.";

    println!("Tokens:");
    for (i, token) in Text::tokenize(paragraph).iter().enumerate() {
        println!("  {}: {token}", i + 1);
    }

    println!();
    println!("Sentences:");
    for (i, sent) in Text::sentences(paragraph).iter().enumerate() {
        println!("  {}: {sent}", i + 1);
    }
}
