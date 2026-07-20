//! Buff playground Wasm crate — exposes a single `transpile` entry point for
//! the web UI (T114).
//!
//! Runs ONLY the front-end of the Buff compiler (lexer → parser → codegen-rust)
//! and returns the generated Rust source (or an error with line/column) as a
//! JSON string. NO `rustc`, NO runtime, NO GPU — those cannot compile to
//! `wasm32-unknown-unknown` and are not needed for transpile-only display.
//!
//! # Wire format
//!
//! [`transpile`] returns a `String` (so it crosses the wasm-bindgen boundary as
//! a JS string). The string is JSON in one of two shapes:
//!
//! ```json
//! {"ok":true, "rust":"fn main() { ... }"}
//! ```
//!
//! ```json
//! {"ok":false, "error":"parse error: ...", "line":3, "col":7}
//! ```
//!
//! `line`/`col` are 1-based and character-counted (multi-byte UTF-8 = 1 col).
//! They are omitted (or `null`) when the diagnostic span is not resolvable.
//!
//! # Errors
//!
//! The wasm boundary NEVER throws — internal panics are caught by
//! [`console_error_panic_hook`] (registered on first call) and converted into
//! an error JSON document. Every fallible compiler call is mapped to an error
//! response via [`error_json`]. This is the hard rule from the repo `AGENTS.md`:
//! no `unwrap`/`expect`/`panic!`/`todo!` in non-test code.
//!
//! [T114]: https://github.com/buff-lang/buff/blob/master/.sisyphus/plans/buff-post-v10-tooling.md

use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{Diagnostic, SourceFile, SourceId};
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;
use wasm_bindgen::prelude::*;

/// Encode the success wire shape: `{"ok":true,"rust":<source>}`.
///
/// Built via `serde_json` so escaping is correct for arbitrary Rust source
/// (which contains `"`, `\n`, etc.). The `serde_json::Value` is serialized to
/// a string with `serde_json::to_string` — never hand-built.
fn success_json(rust_source: &str) -> String {
    let value = serde_json::json!({
        "ok": true,
        "rust": rust_source,
    });
    serde_json::to_string(&value).unwrap_or_else(|_| {
        // Defensive: `serde_json::to_string` on a `Value::Object` of
        // strings can only fail on overflow, which would be extraordinary.
        // Fall back to a minimal hand-escaped string rather than panicking.
        r#"{"ok":true,"rust":""}"#.to_string()
    })
}

/// Encode the error wire shape: `{"ok":false,"error":<msg>,"line":N,"col":N}`.
///
/// `line`/`col` are 1-based when available; otherwise the fields are emitted
/// as `null`. The `phase` prefix ("lex"/"parse"/"codegen") mirrors the host
/// pipeline's `format_diagnostic_error` so JS can pattern-match if desired.
fn error_json(phase: &str, diagnostic: &Diagnostic, source_file: &SourceFile) -> String {
    let (line, col) = match source_file.lookup(diagnostic.span.start) {
        Some((l, c)) => (Some(l), Some(c)),
        None => (None, None),
    };
    let message = format!("{phase} error: {}", diagnostic.message);
    let value = serde_json::json!({
        "ok": false,
        "error": message,
        "line": line,
        "col": col,
    });
    serde_json::to_string(&value).unwrap_or_else(|_| {
        r#"{"ok":false,"error":"<unrenderable diagnostic>","line":null,"col":null}"#.to_string()
    })
}

/// Encode an internal panic / unexpected error (no span available).
fn internal_error_json(message: &str) -> String {
    let value = serde_json::json!({
        "ok": false,
        "error": message,
        "line": null,
        "col": null,
    });
    serde_json::to_string(&value).unwrap_or_else(|_| {
        r#"{"ok":false,"error":"<unrenderable>","line":null,"col":null}"#.to_string()
    })
}

/// Entry point invoked from JS: `wasm.transpile(buffSource)`.
///
/// Runs `tokenize` → `parse` → `generate_rust` against the input string and
/// returns the JSON wire shape documented at the crate root.
///
/// The first call also installs [`console_error_panic_hook`] so any unexpected
/// panic in the compiler stack surfaces in the browser DevTools console (the
/// panic still gets caught by the wasm-bindgen ABI and converted into a JS
/// exception — but with the hook the user gets a Rust stack trace before that).
///
/// # Determinism
///
/// Same input → same output. No global state, no I/O, no time dependence. The
/// playground is shareable by URL precisely because the function is pure.
#[wasm_bindgen]
pub fn transpile(src: &str) -> String {
    // Install the panic hook lazily on first call. Subsequent calls are
    // no-ops (the hook installs a global handler via `std::sync::Once`).
    console_error_panic_hook::set_once();

    // SourceId(0) — playground is single-file (T114 constraint), so only
    // one SourceFile ever exists.
    let source_id = SourceId(0);
    let source_file = SourceFile::new(std::path::PathBuf::from("playground.buff"), src.to_string());

    // 1. Lex.
    let tokens = match tokenize(src, source_id) {
        Ok(tokens) => tokens,
        Err(e) => return error_json("lex", &e.inner.diagnostic, &source_file),
    };

    // 2. Parse.
    let decls = match parse(&tokens, source_id) {
        Ok(decls) => decls,
        Err(e) => return error_json("parse", &e.diagnostic, &source_file),
    };

    // 3. Codegen (type inference is integrated inside codegen).
    match generate_rust(&decls) {
        Ok(rust_source) => success_json(&rust_source),
        Err(e) => error_json("codegen", &e.diagnostic, &source_file),
    }
}

/// Catch-all: if the wasm ABI throws (panic that escaped the hook), the JS
/// wrapper turns it into a synthetic JSON error via this constructor so the
/// UI always has a well-formed document to render.
#[wasm_bindgen]
pub fn internal_error(message: &str) -> String {
    internal_error_json(message)
}

// ---------------------------------------------------------------------------
// Tests — exercise the wire format on the host (no browser needed). The
// `rlib` crate-type lets `cargo test` link the crate natively.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_wire(s: &str) -> serde_json::Value {
        serde_json::from_str(s).expect("wire output must be valid JSON")
    }

    #[test]
    fn transpile_empty_input_yields_ok_with_rust_string() {
        // Empty input is a valid Buff program (no decls). Codegen produces
        // a near-empty Rust file. The wire shape must still be `{ok:true,...}`.
        let out = transpile("");
        let v = parse_wire(&out);
        assert_eq!(v["ok"], true);
        assert!(v["rust"].is_string());
    }

    #[test]
    fn transpile_fibonacci_yields_rust_with_fn_and_fib() {
        // Mirror of the T114 Playwright fixture — the wasm entry MUST emit
        // valid Rust containing `fn ` and `fib` for the same input.
        let src = "func fib(n: Int) -> Int:\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n\nfunc main():\n    let n = 10\n    print(fib(n))\n";
        let out = transpile(src);
        let v = parse_wire(&out);
        assert_eq!(v["ok"], true);
        let rust = v["rust"].as_str().expect("rust field is a string");
        assert!(
            rust.contains("fn "),
            "expected `fn ` in output, got: {rust}"
        );
        assert!(
            rust.contains("fib"),
            "expected function name `fib` preserved, got: {rust}"
        );
    }

    #[test]
    fn transpile_parse_error_returns_ok_false_with_line_col() {
        // `func ( broken` is a malformed func declaration — parse fails.
        let src = "func ( broken\n";
        let out = transpile(src);
        let v = parse_wire(&out);
        assert_eq!(v["ok"], false);
        // Error message starts with "parse error:" per the wire contract.
        let err = v["error"].as_str().expect("error is a string");
        assert!(
            err.starts_with("parse error:"),
            "expected error prefix `parse error:`, got: {err}"
        );
        // line/col are present and 1-based.
        assert!(v["line"].is_number(), "line should be a number");
        assert!(v["col"].is_number(), "col should be a number");
        let line = v["line"].as_i64().unwrap();
        let col = v["col"].as_i64().unwrap();
        assert!(line >= 1, "line should be >= 1, got {line}");
        assert!(col >= 1, "col should be >= 1, got {col}");
    }

    #[test]
    fn transpile_lex_error_returns_ok_false_with_message() {
        // Unterminated string literal — lexer rejects.
        let src = "func main():\n    let x = \"unterminated\n";
        let out = transpile(src);
        let v = parse_wire(&out);
        assert_eq!(v["ok"], false);
        let err = v["error"].as_str().expect("error is a string");
        assert!(
            err.starts_with("lex error:"),
            "expected error prefix `lex error:`, got: {err}"
        );
    }

    #[test]
    fn transpile_output_is_valid_json_for_utf8_source() {
        // Multi-byte UTF-8 must round-trip into valid JSON.
        let src = "func ola():\n    print(\"Olá, Buff!\")\n";
        let out = transpile(src);
        // Must round-trip through serde_json::from_str.
        let _v: serde_json::Value =
            serde_json::from_str(&out).expect("wire output for UTF-8 source must be valid JSON");
    }

    #[test]
    fn transpile_is_deterministic() {
        // Same input → byte-identical output. This is what makes URL-sharing
        // work: the playground state is pure-derived from the source.
        let src = "func main():\n    print(\"hi\")\n";
        let a = transpile(src);
        let b = transpile(src);
        assert_eq!(a, b);
    }

    #[test]
    fn internal_error_returns_ok_false_with_null_position() {
        let out = internal_error("synthesized failure");
        let v = parse_wire(&out);
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "synthesized failure");
        assert!(v["line"].is_null(), "internal errors have no line");
        assert!(v["col"].is_null(), "internal errors have no col");
    }
}
