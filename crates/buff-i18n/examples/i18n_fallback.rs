// T44 example: locale fallback when a key is missing in the current
// locale. Demonstrates the with_fallback constructor + the warning
// surface for missing keys.

use buff_i18n::I18n;

const EN_FTL: &str = "\
greeting = Hello!\n\
only-in-english = This key exists only in English.
";

const PT_BR_FTL: &str = "\
greeting = Olá!
";

fn main() {
    let i18n = I18n::with_fallback("pt-BR", "en").expect("construct pt-BR + en fallback");
    i18n.add_resource("en", EN_FTL).expect("add en");
    i18n.add_resource("pt-BR", PT_BR_FTL).expect("add pt-BR");
    i18n.load("pt-BR").expect("load pt-BR");

    println!("[pt-BR] greeting       = {}", i18n.translate("greeting"));
    println!(
        "[pt-BR] only-in-english = {}  (fell back to en)",
        i18n.translate("only-in-english")
    );
    println!(
        "[pt-BR] missing-key     = {}  (returned key, recorded warning)",
        i18n.translate("missing-key")
    );

    println!("warnings: {:?}", i18n.warnings());
}
