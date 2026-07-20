//! T124 — `ErrorCode` integration tests.
//!
//! Coverage:
//!
//! - Diagnostic render format WITH a code emits `error[E1xxx]:` immediately
//!   after the severity tag (and the same applies to `Display`).
//! - Diagnostic render format WITHOUT a code is unchanged (no `error[` tag).
//! - For every variant in `ErrorCode::all()`, the static catalog at
//!   `docs/errors/` has both a per-code page (`<CODE>.html`) and an entry in
//!   `index.html`. Enforces the no-drift rule between `code.rs` and the site
//!   (conventions §19).

use buff_lang_error::{Diagnostic, ErrorCode, SourceId, Span};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// 1. Render format WITH and WITHOUT an ErrorCode.
// ---------------------------------------------------------------------------

#[test]
fn diagnostic_render_with_code_emits_error_tag_in_header() {
    let src = "let x = @";
    // Span covers the `@` at byte offset 8.
    let diag = Diagnostic::error("unexpected character: '@'", Span::new(8, 9, SourceId(0)))
        .with_code(ErrorCode::UnexpectedChar);
    let rendered = diag.render(src);

    assert!(
        rendered.contains("[Error] error[E1001]: unexpected character: '@'"),
        "missing `[Error] error[E1001]:` header in:\n{rendered}"
    );
    // The caret block must still render alongside the code-tagged header.
    assert!(rendered.contains('^'), "missing caret line in:\n{rendered}");
}

#[test]
fn diagnostic_display_with_code_emits_error_tag() {
    // `Display` is the other render path (used by `BuffError` /
    // `format!("{diagnostic}")`); it must agree with `render()` on the
    // header shape.
    let diag = Diagnostic::error("invalid numeric literal", Span::new(0, 3, SourceId(0)))
        .with_code(ErrorCode::InvalidNumber);
    let s = format!("{diag}");
    assert!(
        s.contains("[Error] error[E1003]: invalid numeric literal"),
        "Display did not emit `error[E1003]:` header: {s:?}"
    );
    assert!(
        !s.contains('\n'),
        "Display should be a single line for a note-less diagnostic: {s:?}"
    );
}

#[test]
fn diagnostic_render_without_code_is_backward_compatible() {
    // A diagnostic with NO code must render byte-identically to the
    // pre-T124 format: `[Error] message`, with NO `error[E1xxx]:` tag.
    // This is the backward-compatibility guarantee: existing snapshots
    // that never attached a code must still pass.
    let src = "let x = 1";
    let diag = Diagnostic::error("some message", Span::new(0, 1, SourceId(0)));
    let rendered = diag.render(src);

    assert!(
        rendered.starts_with("[Error] some message\n"),
        "expected `[Error] some message` header (no code), got:\n{rendered}"
    );
    assert!(
        !rendered.contains("error["),
        "no-code diagnostic must NOT emit an `error[` tag: {rendered:?}"
    );
    assert!(
        !rendered.contains("E1"),
        "no-code diagnostic must NOT mention any E1xxx code: {rendered:?}"
    );
}

#[test]
fn diagnostic_with_code_and_note_renders_both() {
    // The code tag in the header must not eat the note list. Notes must
    // still appear below the caret block, same as the no-code path.
    let src = "pritn(\"hi\")";
    let diag = Diagnostic::error("unknown identifier `pritn`", Span::new(0, 5, SourceId(0)))
        .with_code(ErrorCode::UndefinedVariable)
        .with_note("Did you mean `print`?");
    let rendered = diag.render(src);
    assert!(
        rendered.contains("[Error] error[E1201]: unknown identifier `pritn`"),
        "missing header in:\n{rendered}"
    );
    assert!(
        rendered.contains("  note: Did you mean `print`?"),
        "missing note in:\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// 2. Static error-catalog site coverage.
//
// `code.rs` is the source of truth; `docs/errors/` is generated from it via
// `cargo run -p buff-lang-error --example gen_error_docs`. This test enforces
// conventions §19 rule 5: every code in `ErrorCode::all()` MUST have a
// committed per-code HTML page, and the index MUST exist and mention every
// code.
// ---------------------------------------------------------------------------

/// Resolve `<workspace>/docs/errors/` from this crate's `CARGO_MANIFEST_DIR`.
///
/// `buff-lang-error` lives at `<workspace>/crates/buff-lang-error`, so two
/// `parent()` calls reach the workspace root.
fn docs_errors_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR has no parent")
        .parent()
        .expect("CARGO_MANIFEST_DIR parent has no parent");
    workspace_root.join("docs").join("errors")
}

#[test]
fn error_catalog_index_html_exists_and_lists_every_code() {
    let dir = docs_errors_dir();
    let index = dir.join("index.html");
    assert!(
        index.is_file(),
        "docs/errors/index.html is missing (looked at {}). Run `cargo run -p buff-lang-error \
         --example gen_error_docs` to (re)generate the site.",
        index.display()
    );

    let body = std::fs::read_to_string(&index)
        .unwrap_or_else(|e| panic!("reading {}: {e}", index.display()));

    // Every code must be mentioned in the index (as a link target and a
    // visible tag).
    for &code in ErrorCode::all() {
        let code_str = code.code_str();
        assert!(
            body.contains(&format!("href=\"{code_str}.html\"")),
            "index.html does not link to {code_str}.html"
        );
        assert!(
            body.contains(code_str),
            "index.html does not mention the code string `{code_str}`"
        );
    }
}

#[test]
fn error_catalog_site_pages_exist_for_every_code() {
    let dir = docs_errors_dir();
    assert!(
        dir.is_dir(),
        "docs/errors/ directory is missing. Run `cargo run -p buff-lang-error --example \
         gen_error_docs` to (re)generate the site."
    );

    let mut missing: Vec<String> = Vec::new();
    let mut content_missing: Vec<String> = Vec::new();

    for &code in ErrorCode::all() {
        let code_str = code.code_str();
        let page = dir.join(format!("{code_str}.html"));
        if !page.is_file() {
            missing.push(format!("{} ({:?} -> {})", code_str, code, page.display()));
            continue;
        }

        // The page must contain the code, the title, AND a non-empty slice
        // of the explanation. The generator HTML-escapes `<`,`>`,`&`,`"` and
        // converts backtick runs to `<code>...</code>` tags. To avoid the
        // prefix straddling an unbalanced backtick run, we strip `<code>`
        // and `</code>` tags from the body and HTML-unescape entities, then
        // search for the raw explanation prefix (backticks intact).
        let body = std::fs::read_to_string(&page)
            .unwrap_or_else(|e| panic!("reading {}: {e}", page.display()));
        if !body.contains(code_str) {
            content_missing.push(format!("{code_str} (missing code string in page body)"));
        }
        let title = code.title();
        if !body.contains(title) {
            content_missing.push(format!("{code_str} (missing title `{title}` in page body)"));
        }
        let normalised = strip_code_tags(&body);
        let expl = code.explanation();
        // Strip backticks from the expected prefix because the generator
        // converts them to `<code>` tags (which `strip_code_tags` then
        // removed from the body). After both transforms the prose text
        // should match verbatim.
        let prefix_len = expl.len().min(40);
        let expected_prefix: String = expl[..prefix_len].chars().filter(|&c| c != '`').collect();
        if !normalised.contains(&expected_prefix) {
            content_missing.push(format!(
                "{code_str} (missing explanation prefix `{}` in page body)",
                expected_prefix
            ));
        }
        // Sanity check: the page must NOT be a stub. Any per-code page under
        // 800 bytes is missing its explanation paragraph.
        let file_len = std::fs::metadata(&page).map(|m| m.len()).unwrap_or(0);
        if file_len < 800 {
            content_missing.push(format!(
                "{code_str} (page is only {file_len} bytes - looks like a stub)"
            ));
        }
    }

    assert!(
        missing.is_empty(),
        "missing per-code HTML pages (run `cargo run -p buff-lang-error --example \
         gen_error_docs`):\n  - {}",
        missing.join("\n  - ")
    );
    assert!(
        content_missing.is_empty(),
        "per-code HTML pages exist but their content is missing expected text:\n  - {}",
        content_missing.join("\n  - ")
    );
}

/// Strip `<code>` and `</code>` tags and HTML-unescape the four entities the
/// generator emits (`&amp;` / `&lt;` / `&gt;` / `&quot;`). Used by the
/// explanation spot-check so the expected text can be compared verbatim
/// against the raw `code.explanation()` string (after also stripping
/// backticks, which the test does separately).
fn strip_code_tags(body: &str) -> String {
    body.replace("<code>", "")
        .replace("</code>", "")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}
