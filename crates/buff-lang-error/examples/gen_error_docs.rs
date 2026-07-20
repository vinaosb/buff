//! T124 — static error-catalog site generator.
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo run -p buff-lang-error --example gen_error_docs
//! ```
//!
//! Writes the static site to `<workspace>/docs/errors/` — one `index.html`
//! listing every [`ErrorCode`] grouped by phase, plus one `<CODE>.html` page
//! per code with the title + full explanation (verbatim from the compiler).
//!
//! # Why this is a committed generator
//!
//! The committed deliverable is the **HTML files** themselves (plain static
//! HTML/CSS, no build step — same as `playground/` and `website/`). This
//! generator exists so that when `code.rs` changes, a maintainer can re-run
//! it to refresh the site without hand-editing 35+ HTML files. The generator
//! is the source of truth; the HTML files are its deterministic output.
//!
//! # House style
//!
//! Visual design reuses the "Leatherbound" warm-brutalist palette from
//! `playground/styles.css` and `website/styles.css`: dark amber-toned
//! background, IBM Plex Mono / Space Mono fonts, crimson accents for errors.
//! Like the playground/website, no CDN, no framework, no build step.

use buff_lang_error::ErrorCode;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // buff-lang-error lives at <workspace>/crates/buff-lang-error, so the
    // workspace root is two `parent()` calls up.
    let workspace_root = manifest_dir
        .parent()
        .expect("manifest dir has no parent")
        .parent()
        .expect("manifest parent has no parent");
    let docs_dir = workspace_root.join("docs").join("errors");

    fs::create_dir_all(&docs_dir).expect("could not create docs/errors/ directory");

    let styles_css = build_styles_css();
    fs::write(docs_dir.join("styles.css"), &styles_css).expect("writing styles.css");

    let index_html = build_index_html();
    fs::write(docs_dir.join("index.html"), index_html).expect("writing index.html");

    for &code in ErrorCode::all() {
        let html = build_code_page(code);
        let file_name = format!("{}.html", code.code_str());
        fs::write(docs_dir.join(&file_name), html)
            .unwrap_or_else(|e| panic!("writing {}: {e}", file_name));
    }

    eprintln!(
        "wrote {} code pages + index.html + styles.css to {}",
        ErrorCode::all().len(),
        docs_dir.display()
    );
}

// ---------------------------------------------------------------------------
// Phase grouping — derived from the E1xxx prefix (E10xx=Lexing, etc.).
// ---------------------------------------------------------------------------

fn phase_of(code: ErrorCode) -> &'static str {
    match code.code_str().chars().nth(2).unwrap_or('0') {
        '0' => "Lexing",
        '1' => "Parsing",
        '2' => "Type-checking",
        '3' => "Code generation",
        _ => "Other",
    }
}

fn phase_blurb(phase: &str) -> &'static str {
    match phase {
        "Lexing" => {
            "Errors from the byte-scanner that turns `.buff` source text into tokens. \
                     Source crate: `buff-lang-lexer`."
        }
        "Parsing" => {
            "Errors from the recursive-descent + Pratt parser that turns tokens into an AST. \
                      Source crate: `buff-lang-parser`."
        }
        "Type-checking" => {
            "Errors from type inference, exhaustiveness, and module resolution. \
                            Source crate: `buff-lang-types`."
        }
        "Code generation" => {
            "Errors (and one warning) emitted while lowering the AST to Rust. \
                              Source crate: `buff-lang-codegen-rust`."
        }
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// HTML builders
// ---------------------------------------------------------------------------

fn build_styles_css() -> String {
    // Leatherbound design tokens — verbatim from playground/website.
    r#"/* Buff error catalog — Leatherbound design system (matches playground/website). */
:root {
    --color-bg-deep:    #14110d;
    --color-bg:         #1a1612;
    --color-surface:    #221d17;
    --color-surface-2:  #2a231d;
    --color-border:     #3b3128;
    --color-border-strong: #574a3c;

    --color-text:       #f5e6d3;
    --color-text-dim:   #c8b8a3;
    --color-text-mute:  #8a7c6a;

    --color-accent:     #e8a04e;
    --color-accent-hot: #f4b860;
    --color-accent-dim: #b27a36;

    --color-error:      #e35454;
    --color-error-bg:   #2a1715;
    --color-warning:    #d8b545;
    --color-warning-bg: #2a2415;
    --color-info:       #7b8fd4;

    --font-display: "Space Mono", "IBM Plex Mono", ui-monospace, monospace;
    --font-code:    "IBM Plex Mono", ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace;
    --font-body:    "IBM Plex Sans", system-ui, sans-serif;

    --text-xs:    0.75rem;
    --text-sm:    0.875rem;
    --text-base:  1rem;
    --text-lg:    1.25rem;
    --text-xl:    1.5rem;
    --text-2xl:   2rem;
    --text-3xl:   3rem;

    --space-1:  4px;
    --space-2:  8px;
    --space-3:  12px;
    --space-4:  16px;
    --space-5:  20px;
    --space-6:  24px;
    --space-8:  32px;
    --space-10: 40px;
    --space-12: 48px;
    --space-16: 64px;
    --space-20: 80px;
    --space-24: 96px;

    --radius-sm: 3px;
    --radius-md: 6px;
    --radius-lg: 10px;

    --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.4);
    --shadow-md: 0 4px 12px rgba(0, 0, 0, 0.5);

    --transition-fast: 120ms cubic-bezier(0.4, 0, 0.2, 1);

    --max-w: 1100px;
}

*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
html { scroll-behavior: smooth; }
body {
    background-color: var(--color-bg-deep);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-base);
    line-height: 1.6;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
    background-image:
        radial-gradient(ellipse 80% 50% at 50% -10%, rgba(232, 160, 78, 0.07), transparent 60%);
    background-attachment: fixed;
    min-height: 100vh;
}
a { color: var(--color-accent); text-decoration: none; }
a:hover { color: var(--color-accent-hot); text-decoration: underline; }
code, pre { font-family: var(--font-code); }

/* Header */
.site-header {
    position: sticky;
    top: 0;
    z-index: 50;
    background: linear-gradient(180deg, rgba(232, 160, 78, 0.04) 0%, transparent 100%), var(--color-bg);
    border-bottom: 1px solid var(--color-border);
}
.site-header::after {
    content: "";
    position: absolute;
    left: 0; right: 0; bottom: -1px;
    height: 2px;
    background: linear-gradient(90deg, transparent 0%, var(--color-accent-dim) 12%, var(--color-accent) 50%, var(--color-accent-dim) 88%, transparent 100%);
    opacity: 0.7;
}
.header-inner {
    max-width: var(--max-w);
    margin: 0 auto;
    padding: var(--space-5) var(--space-8);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-6);
    flex-wrap: wrap;
}
.brand {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    color: var(--color-accent);
}
.brand-word {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: var(--text-xl);
    letter-spacing: 0.08em;
    color: var(--color-text);
}
.brand-tag {
    font-family: var(--font-code);
    font-size: var(--text-sm);
    color: var(--color-text-mute);
    letter-spacing: 0.06em;
}
.nav-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-5);
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-family: var(--font-code);
    font-size: var(--text-sm);
    font-weight: 500;
    letter-spacing: 0.04em;
    transition: background var(--transition-fast), border-color var(--transition-fast);
}
.nav-btn:hover {
    background: var(--color-surface-2);
    border-color: var(--color-border-strong);
    color: var(--color-text);
    text-decoration: none;
}

/* Main container */
main {
    max-width: var(--max-w);
    margin: 0 auto;
    padding: var(--space-12) var(--space-8) var(--space-20);
}

/* Page heading */
.page-title {
    font-family: var(--font-display);
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin-bottom: var(--space-3);
}
.page-subtitle {
    color: var(--color-text-dim);
    font-size: var(--text-lg);
    margin-bottom: var(--space-10);
    max-width: 70ch;
}

/* Phase section on the index */
.phase-section {
    margin-bottom: var(--space-12);
}
.phase-heading {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-accent);
    border-bottom: 1px solid var(--color-border);
    padding-bottom: var(--space-2);
    margin-bottom: var(--space-3);
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
}
.phase-range {
    font-family: var(--font-code);
    font-size: var(--text-sm);
    color: var(--color-text-mute);
    font-weight: 400;
    letter-spacing: 0.04em;
}
.phase-blurb {
    color: var(--color-text-dim);
    font-size: var(--text-sm);
    margin-bottom: var(--space-5);
    max-width: 75ch;
    line-height: 1.55;
}
.phase-blurb code {
    color: var(--color-accent);
    background: var(--color-surface);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    font-size: 0.85em;
}

/* Code list (table-like) */
.code-list {
    list-style: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    overflow: hidden;
    background: var(--color-bg);
}
.code-list li {
    border-bottom: 1px solid var(--color-border);
}
.code-list li:last-child { border-bottom: none; }
.code-row {
    display: grid;
    grid-template-columns: 100px 1fr;
    gap: var(--space-5);
    padding: var(--space-4) var(--space-5);
    color: var(--color-text);
    align-items: baseline;
    transition: background var(--transition-fast);
}
.code-row:hover {
    background: var(--color-surface);
    color: var(--color-text);
    text-decoration: none;
}
.code-tag {
    font-family: var(--font-code);
    font-weight: 700;
    color: var(--color-error);
    font-size: var(--text-sm);
    letter-spacing: 0.04em;
}
.code-title {
    color: var(--color-text);
    font-size: var(--text-base);
}

/* Per-code page */
.code-page-header {
    margin-bottom: var(--space-8);
    display: flex;
    align-items: baseline;
    gap: var(--space-5);
    flex-wrap: wrap;
}
.code-page-tag {
    font-family: var(--font-code);
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-error);
    background: var(--color-error-bg);
    border: 1px solid #5a2a28;
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-sm);
    letter-spacing: 0.04em;
}
.code-page-tag.warning {
    color: var(--color-warning);
    background: var(--color-warning-bg);
    border-color: #5a4a28;
}
.code-page-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
}
.back-link {
    display: inline-block;
    margin-bottom: var(--space-8);
    font-family: var(--font-code);
    font-size: var(--text-sm);
    color: var(--color-text-dim);
}
.explanation {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-6) var(--space-8);
    color: var(--color-text);
    font-size: var(--text-base);
    line-height: 1.75;
    max-width: 75ch;
}
.explanation p { margin-bottom: var(--space-4); }
.explanation p:last-child { margin-bottom: 0; }
.explanation code {
    color: var(--color-accent);
    background: var(--color-surface);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    font-size: 0.88em;
}

/* Metadata block at bottom of each code page */
.meta {
    margin-top: var(--space-10);
    padding-top: var(--space-5);
    border-top: 1px solid var(--color-border);
    color: var(--color-text-mute);
    font-size: var(--text-sm);
    font-family: var(--font-code);
    line-height: 1.7;
}
.meta strong { color: var(--color-text-dim); font-weight: 600; }

/* Footer */
.site-footer {
    border-top: 1px solid var(--color-border);
    background: var(--color-bg);
}
.footer-inner {
    max-width: var(--max-w);
    margin: 0 auto;
    padding: var(--space-6) var(--space-8);
    display: flex;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
    color: var(--color-text-mute);
    font-size: var(--text-sm);
    font-family: var(--font-code);
}
.footer-link { color: var(--color-accent); }
.footer-link:hover { color: var(--color-accent-hot); }

@media (max-width: 600px) {
    .code-row { grid-template-columns: 1fr; gap: var(--space-2); }
    .header-inner { padding: var(--space-4); }
    main { padding: var(--space-8) var(--space-4) var(--space-16); }
}
"#.to_string()
}

fn build_index_html() -> String {
    let mut phases: Vec<(&'static str, Vec<ErrorCode>)> = Vec::new();
    for &code in ErrorCode::all() {
        let phase = phase_of(code);
        if let Some((_, entries)) = phases.iter_mut().find(|(p, _)| *p == phase) {
            entries.push(code);
        } else {
            phases.push((phase, vec![code]));
        }
    }

    let mut sections = String::new();
    for (phase, entries) in &phases {
        let first = entries.first().expect("phase non-empty").code_str();
        let last = entries.last().expect("phase non-empty").code_str();
        sections.push_str(&format!(
            r#"    <section class="phase-section">
        <h2 class="phase-heading">{phase} <span class="phase-range">{first}&ndash;{last}</span></h2>
        <p class="phase-blurb">{blurb}</p>
        <ul class="code-list">
"#,
            phase = phase,
            first = first,
            last = last,
            blurb = render_inline_code(html_escape(phase_blurb(phase))),
        ));
        for &code in entries {
            sections.push_str(&format!(
                "            <li><a class=\"code-row\" href=\"{code}.html\">\n                <span class=\"code-tag\">{code}</span>\n                <span class=\"code-title\">{title}</span>\n            </a></li>\n",
                code = code.code_str(),
                title = html_escape(code.title()),
            ));
        }
        sections.push_str("        </ul>\n    </section>\n\n");
    }

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Buff error codes</title>
    <meta name="description" content="Every Buff compiler error code with a stable identifier, a plain-English explanation, and a fix recipe." />
    <meta name="color-scheme" content="dark" />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600;700&family=IBM+Plex+Sans:wght@400;500;600;700&family=Space+Mono:wght@400;700&display=swap" rel="stylesheet" />
    <link rel="stylesheet" href="./styles.css" />
</head>
<body>

<header class="site-header" role="banner">
    <div class="header-inner">
        <a class="brand" href="./index.html" aria-label="Buff error codes">
            <span class="brand-word">BUFF</span>
            <span class="brand-tag">error code index</span>
        </a>
        <a class="nav-btn" href="https://github.com/buff-lang/buff/blob/master/crates/buff-lang-error/src/code.rs" target="_blank" rel="noopener noreferrer">source &#8599;</a>
    </div>
</header>

<main>
    <h1 class="page-title">Error code index</h1>
    <p class="page-subtitle">
        Every diagnostic the Buff compiler can emit carries a stable <code>E1xxx</code>
        code. This index lists every code, grouped by compiler phase. Click a code
        for its full explanation, an example, and a fix recipe. Codes are stable
        across releases &mdash; see the
        <a href="https://github.com/buff-lang/buff/blob/master/.sisyphus/plans/buff-conventions.md">conventions doc</a>
        for the full policy.
    </p>

{sections}</main>

<footer class="site-footer">
    <div class="footer-inner">
        <span>Buff language. Open source under MIT + Apache-2.0.</span>
        <a class="footer-link" href="https://github.com/buff-lang/buff" target="_blank" rel="noopener noreferrer">GitHub &#8599;</a>
    </div>
</footer>

</body>
</html>
"#,
        sections = sections,
    )
}

fn build_code_page(code: ErrorCode) -> String {
    let code_str = code.code_str();
    let title = code.title();
    let explanation_raw = code.explanation();

    // Lightly paragraph-break the explanation: split on ". " followed by a
    // capital letter (sentence boundary heuristic) every ~2 sentences so the
    // page reads as a few short paragraphs rather than one wall of text.
    let explanation_html = paragraphize(explanation_raw);

    // Warnings (Severity::Warning) get a yellow tag; everything else is red.
    let is_warning = code == ErrorCode::AsyncBlockDeadlock;
    let tag_class = if is_warning { " warning" } else { "" };
    let severity_label = if is_warning { "Warning" } else { "Error" };
    let phase = phase_of(code);

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{code_str} &mdash; {title}</title>
    <meta name="description" content="{code_str}: {title} &mdash; explanation, example, and fix." />
    <meta name="color-scheme" content="dark" />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600;700&family=IBM+Plex+Sans:wght@400;500;600;700&family=Space+Mono:wght@400;700&display=swap" rel="stylesheet" />
    <link rel="stylesheet" href="./styles.css" />
</head>
<body>

<header class="site-header" role="banner">
    <div class="header-inner">
        <a class="brand" href="./index.html" aria-label="Buff error codes">
            <span class="brand-word">BUFF</span>
            <span class="brand-tag">error code index</span>
        </a>
        <a class="nav-btn" href="./index.html">&larr; all codes</a>
    </div>
</header>

<main>
    <a class="back-link" href="./index.html">&larr; back to index</a>

    <header class="code-page-header">
        <span class="code-page-tag{tag_class}">{code_str}</span>
        <h1 class="code-page-title">{title}</h1>
    </header>

    <section class="explanation">
{explanation_html}
    </section>

    <p class="meta">
        <strong>Severity:</strong> {severity_label}<br />
        <strong>Phase:</strong> {phase}<br />
        <strong>Stability:</strong> this code is stable across releases &mdash; it will never be renumbered or reused. See
        <a href="https://github.com/buff-lang/buff/blob/master/.sisyphus/plans/buff-conventions.md">&sect;19 of the conventions doc</a>
        for the full policy.
    </p>
</main>

<footer class="site-footer">
    <div class="footer-inner">
        <span>Buff language. Open source under MIT + Apache-2.0.</span>
        <a class="footer-link" href="https://github.com/buff-lang/buff" target="_blank" rel="noopener noreferrer">GitHub &#8599;</a>
    </div>
</footer>

</body>
</html>
"#,
        code_str = code_str,
        title = html_escape(title),
        tag_class = tag_class,
        severity_label = severity_label,
        phase = phase,
        explanation_html = explanation_html,
    )
}

/// HTML-escape the four characters that matter in inline text: `&`, `<`, `>`,
/// `"`. (We never emit unescaped `'`.)
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Wrap the prose explanation in `<p>` tags. Splits the text into ~2-sentence
/// paragraphs at ". " boundaries that are followed by a capital letter, so
/// the page reads as a few short paragraphs rather than one wall of text.
fn paragraphize(raw: &str) -> String {
    // Walk the string collecting sentences. A sentence ends at ". " followed
    // by an uppercase letter or end of string. Pair sentences 2-at-a-time.
    let mut sentences: Vec<String> = Vec::new();
    let bytes = raw.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            // Lookahead: ". " + ASCII uppercase OR end-of-string.
            let next_is_upper_or_end = i + 1 >= bytes.len()
                || (i + 2 <= bytes.len()
                    && bytes[i + 1] == b' '
                    && bytes.get(i + 2).is_some_and(|c| c.is_ascii_uppercase()));
            if next_is_upper_or_end {
                let end = if i + 1 >= bytes.len() { i + 1 } else { i + 2 };
                sentences.push(raw[start..end].trim().to_string());
                start = end;
            }
        }
        i += 1;
    }
    if start < bytes.len() {
        sentences.push(raw[start..].trim().to_string());
    }

    // Group into paragraphs of 2 sentences each. If odd count, the last
    // paragraph is a single sentence.
    let mut out = String::new();
    let mut idx = 0;
    while idx < sentences.len() {
        let end = (idx + 2).min(sentences.len());
        let body: Vec<String> = sentences[idx..end]
            .iter()
            .map(|s| render_inline_code(html_escape(s)))
            .collect();
        out.push_str(&format!("        <p>{}</p>\n", body.join(" ")));
        idx = end;
    }
    out
}

/// Convert `` `...` `` backticks into `<code>...</code>` so the prose renders
/// inline code styling on the page. Runs AFTER [`html_escape`] so backticks
/// survive escaping (they're not in the escape set). Every backtick toggles
/// code mode; there is no brace-depth tracking (the prose may legitimately
/// contain `{` / `}` as literal characters inside a code span, e.g. `"{"
/// opening ...`).
fn render_inline_code(s: String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_code = false;
    for c in s.chars() {
        if c == '`' {
            if in_code {
                out.push_str("</code>");
                in_code = false;
            } else {
                out.push_str("<code>");
                in_code = true;
            }
        } else {
            out.push(c);
        }
    }
    // If we somehow ended mid-code (unbalanced backticks), close it so the
    // page still renders.
    if in_code {
        out.push_str("</code>");
    }
    out
}
