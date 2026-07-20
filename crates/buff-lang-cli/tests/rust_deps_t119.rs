//! T119 end-to-end CLI test — `[rust-deps]` auto-population.
//!
//! Verifies the full pipeline: a `.buff` source with `extern "C" from
//! "serde_json" func ...` declarations → `collect_rust_deps` →
//! `render_rust_deps_toml` produces a well-formed `[rust-deps]` TOML
//! block suitable for appending to `buff.toml`.

use std::collections::BTreeSet;

use buff_lang_cli::config::render_rust_deps_toml;
use buff_lang_codegen_rust::collect_rust_deps;
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

#[test]
fn end_to_end_rust_deps_toml_from_extern_decls() {
    // A Buff program declares multiple extern fns backed by 3 different
    // Rust crates. The pipeline must collect all 3 crates and render them
    // as a deterministic `[rust-deps]` block.
    let src = concat!(
        "extern \"C\" from \"serde_json\" func parse_str(s: String) -> String\n",
        "extern \"C\" from \"reqwest\" func fetch(url: String) -> String\n",
        "extern \"C\" from \"tokio\" func sleep(ms: Int) -> Unit\n",
        "func main():\n",
        "    print(\"externs ready\")\n",
    );
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize must succeed");
    let decls = parse(&tokens, sid).expect("parse must succeed");

    // collect_rust_deps returns a BTreeSet — sorted, deduped.
    let deps: BTreeSet<String> = collect_rust_deps(&decls);
    assert_eq!(
        deps.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        vec!["reqwest", "serde_json", "tokio"],
        "expected 3 unique crates in alphabetical order"
    );

    // render_rust_deps_toml produces a TOML block ready for buff.toml.
    let toml = render_rust_deps_toml(&deps);
    assert!(toml.starts_with("[rust-deps]\n"));
    assert!(toml.contains("reqwest = \"*\""));
    assert!(toml.contains("serde_json = \"*\""));
    assert!(toml.contains("tokio = \"*\""));
    // Order is alphabetical (BTreeSet iteration).
    let reqwest_pos = toml.find("reqwest").expect("reqwest present");
    let serde_pos = toml.find("serde_json").expect("serde_json present");
    let tokio_pos = toml.find("tokio").expect("tokio present");
    assert!(reqwest_pos < serde_pos);
    assert!(serde_pos < tokio_pos);
}

#[test]
fn end_to_end_no_externs_yields_empty_rust_deps() {
    // A program with no extern declarations produces no `[rust-deps]` block.
    let src = "func main():\n    print(\"hello\")\n";
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize");
    let decls = parse(&tokens, sid).expect("parse");
    let deps = collect_rust_deps(&decls);
    assert!(deps.is_empty());
    assert_eq!(render_rust_deps_toml(&deps), "");
}

#[test]
fn end_to_end_legacy_extern_crate_also_counted() {
    // The v0.5 `extern crate "serde"` form contributes to rust-deps
    // alongside the new `extern "C" from "..."` form.
    let src = concat!(
        "extern crate \"serde\"\n",
        "extern \"C\" from \"serde_json\" func parse(s: String) -> String\n",
    );
    let sid = SourceId(0);
    let tokens = tokenize(src, sid).expect("tokenize");
    let decls = parse(&tokens, sid).expect("parse");
    let deps = collect_rust_deps(&decls);
    assert_eq!(deps.len(), 2);
    assert!(deps.contains("serde"));
    assert!(deps.contains("serde_json"));
}
