// T44 example: three-locale translation (en, pt-BR, es).
//
// Demonstrates the full I18n lifecycle: construct catalog, add Fluent
// resources for three locales, switch via `load`, translate a static
// key + a parameterized key. Mirrors the buff-image examples pattern
// (one .rs file per concept, paired with a `.buff` forward-decl).

use buff_i18n::I18n;
use std::collections::BTreeMap;

const EN_FTL: &str = "\
hello = Hello, world!
greet = Hello, { $name }!
";

const PT_BR_FTL: &str = "\
hello = Olá, mundo!
greet = Olá, { $name }!
";

const ES_FTL: &str = "\
hello = ¡Hola, mundo!
greet = ¡Hola, { $name }!
";

fn main() {
    let i18n = I18n::with_fallback("en", "en").expect("construct I18n(en,en)");

    i18n.add_resource("en", EN_FTL).expect("add en");
    i18n.add_resource("pt-BR", PT_BR_FTL).expect("add pt-BR");
    i18n.add_resource("es", ES_FTL).expect("add es");

    println!("available locales: {:?}", i18n.available_locales());

    let mut args = BTreeMap::new();
    args.insert("name".to_string(), "Alice".to_string());

    for locale in &["en", "pt-BR", "es"] {
        i18n.load(locale)
            .unwrap_or_else(|e| panic!("load {locale}: {e:?}"));
        println!("[{}] hello = {}", locale, i18n.translate("hello"));
        println!(
            "[{}] greet = {}",
            locale,
            i18n.translate_with_args("greet", &args)
        );
    }

    println!("warnings: {:?}", i18n.warnings());
}
