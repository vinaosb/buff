// T46 example: detect the natural language of a text sample.

use buff_nlp::Text;

fn main() {
    let samples = [
        ("English", "The quick brown fox jumps over the lazy dog."),
        ("Portuguese", "A rápida raposa marrom salta sobre o cão preguiçoso."),
        ("French", "Le rapide renard brun saute par-dessus le chien paresseux."),
        ("Spanish", "El rápido zorro marrón salta sobre el perro perezoso."),
        ("German", "Der schnelle braune Fuchs springt über den faulen Hund."),
    ];

    for (label, text) in samples {
        match Text::detect_language(text) {
            Some(lang) => println!("{label}: {} ({})", lang.name(), lang.code()),
            None => println!("{label}: (no language detected)"),
        }
    }
}
