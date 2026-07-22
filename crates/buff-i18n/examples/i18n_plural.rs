// T44 example: pluralization via Fluent { $count -> [one] ... } select.
//
// Demonstrates that add_resource + translate_with_args honor Fluent's
// plural select syntax. The same code path powers gender / ICU select
// rules (Fluent resolves them all via the same mechanism).

use buff_i18n::I18n;
use std::collections::BTreeMap;

const EN_FTL: &str = "\
emails = { $count ->\n\
    [one] You have one email.\n\
    *[other] You have { $count } emails.\n\
}
";

const PT_BR_FTL: &str = "\
emails = { $count ->\n\
    [one] Você tem um e-mail.\n\
    *[other] Você tem { $count } e-mails.\n\
}
";

fn main() {
    let i18n = I18n::with_fallback("en", "en").expect("construct");
    i18n.add_resource("en", EN_FTL).expect("add en");
    i18n.add_resource("pt-BR", PT_BR_FTL).expect("add pt-BR");

    for count in &["1", "5"] {
        let mut args = BTreeMap::new();
        args.insert("count".to_string(), count.to_string());
        println!(
            "[en  count={count}] {}",
            i18n.translate_with_args("emails", &args)
        );
    }

    i18n.load("pt-BR").expect("load pt-BR");
    for count in &["1", "5"] {
        let mut args = BTreeMap::new();
        args.insert("count".to_string(), count.to_string());
        println!(
            "[pt-BR count={count}] {}",
            i18n.translate_with_args("emails", &args)
        );
    }
}
