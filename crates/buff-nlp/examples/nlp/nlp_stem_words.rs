// T46 example: stem a list of words in multiple languages.

use buff_nlp::{StemAlgorithm, Text};

fn main() {
    let english_words = ["running", "jumping", "happily", "cats", "dogs"];
    for word in english_words {
        match Text::stem(word, StemAlgorithm::English) {
            Ok(stem) => println!("{word} -> {stem}"),
            Err(err) => println!("{word} -> ERROR: {err}"),
        }
    }

    println!();
    let portuguese_words = ["correndo", "pulando", "felizmente", "gatos", "cachorros"];
    for word in portuguese_words {
        match Text::stem(word, StemAlgorithm::Portuguese) {
            Ok(stem) => println!("{word} -> {stem}"),
            Err(err) => println!("{word} -> ERROR: {err}"),
        }
    }
}
