//! T117 — regression guard for the v1.25 playground example snippets.
//!
//! Each example shipped in `playground/app.js::EXAMPLES` is mirrored here as
//! a string literal and driven through the SAME `transpile` entry point the
//! browser uses. The wire JSON MUST come back with `ok: true` AND a non-empty
//! `rust` field — anything else means a v1.25 feature regressed in the wasm
//! transpile path (or the playground snippet drifted from real syntax).
//!
//! Why mirror instead of parsing app.js:
//!   • Keep the test pure-Rust (no JS parser dep, no node toolchain).
//!   • Drift between the snippet and this file is the failure signal we want
//!     — a CI red forces the next editor of `app.js` to also touch this test,
//!     confirming the new snippet lowers cleanly through wasm-bindgen.
//!
//! Run: `cargo test -p buff-playground-wasm --test playground_examples_v125`

use buff_playground_wasm::transpile;

/// Decode the wire JSON. Panics on malformed output — the wire contract
/// guarantees valid JSON (the entry point never throws).
fn parse_wire(s: &str) -> serde_json::Value {
    serde_json::from_str(s).expect("wire output must be valid JSON")
}

/// Assert that `src` transpiles cleanly: `ok == true` AND `rust` is a non-
/// empty string. On failure, dump the actual error message so the CI log
/// shows what went wrong (not just "ok was false").
fn assert_transpiles_ok(label: &str, src: &str) {
    let out = transpile(src);
    let v = parse_wire(&out);
    if v["ok"] != true {
        let err = v["error"].as_str().unwrap_or("<non-string error>");
        let line = &v["line"];
        let col = &v["col"];
        panic!(
            "[{label}] expected ok:true, got error: {err} (line={line}, col={col})\n\
             --- source ---\n{src}"
        );
    }
    let rust = v["rust"].as_str().expect("rust field is a string");
    assert!(
        !rust.trim().is_empty(),
        "[{label}] transpile returned ok:true but empty Rust output\n--- source ---\n{src}"
    );
}

// === v1.25 LANGUAGE SURFACE =================================================

#[test]
fn generics_lowers_to_rust_with_type_params() {
    // Brace-form struct (the documented & tested form per T13 evidence
    // doc — `struct Pair<T, U> { x: T, y: U }` is the ✅ acceptance shape).
    // The layout-sensitive `struct Pair<T, U>:` form has a parser gap when
    // combined with generics (no end-to-end test exists); we use the brace
    // form in the playground for reliable transpilation.
    let src = "struct Pair<T, U> { x: T, y: U }\n\nfunc id<T>(x: T) -> T:\n    return x\n\nfunc main():\n    let p = Pair.new(x: 42, y: \"hello\")\n    let n = id(42)\n    let s = id(\"hello\")\n    print(p.x)\n    print(n)\n    print(s)\n";
    assert_transpiles_ok("generics", src);

    // The Rust output MUST preserve generic syntax (`<T, U>` on the struct
    // and `<T>` on the function) — that's the whole point of the example.
    let v = parse_wire(&transpile(src));
    let rust = v["rust"].as_str().expect("rust field");
    assert!(
        rust.contains("struct Pair<T, U>"),
        "expected `struct Pair<T, U>` in Rust output, got:\n{rust}"
    );
    assert!(
        rust.contains("fn id<T>"),
        "expected `fn id<T>` in Rust output, got:\n{rust}"
    );
}

#[test]
fn range_exclusive_and_inclusive_lowers_ok() {
    let src = "func main():\n    for i in 0..5:\n        print(i)\n    for j in 0..=5:\n        print(j)\n    if (0..10).contains(5):\n        print(\"yes\")\n";
    assert_transpiles_ok("range", src);
}

#[test]
fn pattern_matching_option_and_result_arms_lower_ok() {
    let src = "func lookup(id: Int) -> Result<Int, Error>:\n    if id == 1:\n        return Ok(111)\n    return Error(\"not found\")\n\nfunc main():\n    let mut drawer = [11, 22, 33]\n    let taken = drawer.pop()\n    match taken { Some(x) => print(x), None => print(0) }\n    let found = lookup(1)\n    match found { Ok(v) => print(v), Err(_) => print(0) }\n";
    assert_transpiles_ok("pattern_matching", src);
}

#[test]
fn raw_strings_r_quote_and_r_hash_lowers_ok() {
    // r"..." form (backslashes literal).
    let src1 = "func main():\n    let re = r\"\\d+\\.\\d+\"\n    print(re)\n";
    assert_transpiles_ok("raw_string_simple", src1);

    // r#"..."# form (embedded quotes allowed).
    let src2 = "func main():\n    let json = r#\"{\"x\": 1}\"#\n    print(json)\n";
    assert_transpiles_ok("raw_string_hash", src2);
}

#[test]
fn defer_statement_lowers_with_lifo_emission() {
    // `defer EXPR` schedules EXPR at function exit (T100). The codegen
    // collects deferred expressions and emits them in reverse order at
    // every exit point. We only assert the transpile succeeds; the
    // codegen-rust suite (defer_codegen.rs) checks the LIFO ordering.
    let src = "func process(label: String):\n    print(\"opening \" + label)\n    defer print(\"closing \" + label)\n    print(\"working with \" + label)\n\nfunc main():\n    process(\"file-a\")\n";
    assert_transpiles_ok("defer", src);
}

// === v1.25 STDLIB PRELUDE TYPES =============================================
//
// Http / Json are namespace-only prelude types that lower to calls into
// reqwest::blocking / serde_json (registered in codegen `extern_crates`).
// The single-file rustc pipeline cannot LINK these — but the playground is
// transpile-only, so all we need is for `generate_rust` to succeed.

#[test]
fn http_prelude_get_and_post_lower_ok() {
    // Use raw strings (T93) for any payload containing `{` — Buff's regular
    // string literals treat `{` as the start of interpolation. Raw strings
    // are the canonical way to embed JSON / regex / shell globs. The `r#"..."#`
    // form (vs plain `r"..."`) is needed when the payload itself contains `"`.
    let src = "func main():\n    let body = Http.get(\"https://example.com\")\n    print(body)\n    let json = r#\"{\"hi\": 1}\"#\n    let reply = Http.post(\"https://httpbin.org/post\", json)\n    print(reply)\n";
    assert_transpiles_ok("http_client", src);
}

#[test]
fn json_prelude_parse_and_stringify_lower_ok() {
    // Raw string for the JSON payload — same reason as http_client above.
    // `r#\"...\"#` form because the JSON itself contains `\"`.
    let src = "func main():\n    let payload = r#\"{\"name\":\"Buff\",\"version\":1.25}\"#\n    let data = Json.parse(payload)\n    print(data)\n    let back = Json.stringify(data)\n    print(back)\n";
    assert_transpiles_ok("json", src);
}

// === REGRESSION: existing v0.1/v0.5 examples must still work ================

#[test]
fn fibonacci_still_transpiles() {
    let src = "func fib(n: Int) -> Int:\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n\nfunc main():\n    print(fib(10))\n";
    assert_transpiles_ok("fibonacci", src);
}

#[test]
fn error_demo_still_returns_ok_false() {
    let src = "func ( broken\n";
    let out = transpile(src);
    let v = parse_wire(&out);
    assert_eq!(v["ok"], false, "broken input must surface as ok:false");
    let err = v["error"].as_str().expect("error is a string");
    assert!(
        err.starts_with("parse error:"),
        "expected `parse error:` prefix, got: {err}"
    );
}
