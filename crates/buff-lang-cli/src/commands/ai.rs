//! `buff ai` — AI assistant integration (T65).
//!
//! Two subcommands:
//!
//! - [`run_context`] — emit a Markdown "AI context pack" describing the
//!   Buff language surface + current project structure. Paste into
//!   Copilot / Claude / etc.
//! - [`run_verify`] — type-check AI-generated `.buff` code via the T55
//!   standalone typecheck pipeline, with AI-friendly hints appended to
//!   diagnostics.
//!
//! # Offline-only
//!
//! This command does NOT call any AI APIs. The user runs `buff ai context`,
//! copies the output into their AI tool of choice, then runs `buff ai verify`
//! on the generated `.buff` file. This mirrors the existing Buff
//! "no network in the loop" stance.
//!
//! # Pipeline
//!
//! ```text
//!   buff ai context [--project <PATH>] [--output <FILE>]
//!        │
//!        ▼  build_context_pack(project_root)
//!   String (Markdown)
//!        │
//!        ├──▶ stdout (default)
//!        └──▶ write to <FILE> (--output)
//!
//!   buff ai verify <FILE>
//!        │
//!        ▼  run_check_file (T55 standalone typecheck)
//!   CheckReport { diagnostics, outcome }
//!        │
//!        ▼  append AI-friendly hints
//!   rendered diagnostics → stderr
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use buff_lang_error::Severity;

use crate::check::{run_check_file, CheckOutcome, CheckReport};
use crate::cli::AiCmd;

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch entry point consumed by `main.rs`.
///
/// Returns the [`CheckOutcome`] for `verify` (so the CLI binary can map to
/// an exit code, mirroring `buff check`); `context` always returns `Ok(())`
/// on success and propagates I/O errors via `Err`.
pub fn run(cmd: AiCmd) -> Result<CheckOutcome> {
    match cmd {
        AiCmd::Context { output, project } => {
            let pack = build_context_pack(&project);
            if let Some(out_path) = output {
                std::fs::write(&out_path, pack.as_bytes()).with_context(|| {
                    format!("failed to write context pack to `{}`", out_path.display())
                })?;
                eprintln!("wrote {} bytes to {}", pack.len(), out_path.display());
            } else {
                println!("{pack}");
            }
            Ok(CheckOutcome::Clean)
        }
        AiCmd::Verify { file } => run_verify(&file),
    }
}

// ---------------------------------------------------------------------------
// verify — T55 typecheck + AI hints
// ---------------------------------------------------------------------------

/// Type-check AI-generated `.buff` code, appending AI-friendly hints to
/// diagnostics before they hit stderr.
///
/// Wraps [`run_check_file`] (T55) so all existing lex/parse/typecheck
/// rules apply unchanged. The hints layer adds heuristic suggestions
/// for common AI mistakes (misspelled prelude names, missing `func
/// main` entry point) without changing the outcome.
pub fn run_verify(file: &Path) -> Result<CheckOutcome> {
    let report = run_check_file(file)?;
    let enhanced = enhance_report(report, file);
    render_verify_report(&enhanced, file);
    Ok(enhanced.outcome)
}

/// Append AI-friendly hints to a [`CheckReport`] based on its diagnostics.
///
/// Hints are surfaced as additional [`Severity::Info`] diagnostics so they
/// render in the same stream without affecting the outcome (Info is
/// neither Warning nor Error).
fn enhance_report(mut report: CheckReport, _file: &Path) -> CheckReport {
    let mut hints: Vec<buff_lang_error::Diagnostic> = Vec::new();
    for d in &report.diagnostics {
        if let Some(h) = hint_for_diagnostic(&d.message) {
            hints.push(buff_lang_error::Diagnostic::info(
                h,
                buff_lang_error::Span::dummy(),
            ));
        }
    }
    report.diagnostics.extend(hints);
    report
}

/// Heuristic: produce an AI-friendly hint for a given diagnostic message.
///
/// Returns `None` when no suggestion applies. Current heuristics:
///
/// - Mentions an unknown identifier that's a near-miss of a prelude fn
///   (e.g. `prnt` → `did you mean \`print\`?`).
/// - Mentions an unknown identifier that's a near-miss of a prelude type.
/// - Generic hint for parse errors (suggest running `buff fmt`).
fn hint_for_diagnostic(message: &str) -> Option<String> {
    use buff_lang_types::PreludeFn;
    let lower = message.to_lowercase();
    if lower.contains("parse") || lower.contains("unexpected token") {
        return Some(
            "AI hint: parse errors often come from indentation (Buff uses 4 spaces, no tabs) \
             or missing `:` after a header keyword. Run `buff fmt <file>` to canonicalise layout."
                .to_string(),
        );
    }
    for &pf in PreludeFn::ALL {
        let name = pf.name();
        if message.contains(name) {
            continue;
        }
        if let Some(typo) = extract_unknown_ident(message) {
            if levenshtein(typo, name) <= 2 && typo.len() >= 3 {
                return Some(format!(
                    "AI hint: unknown name `{typo}` — did you mean the prelude function `{name}`?"
                ));
            }
        }
    }
    use buff_lang_types::PreludeType;
    for &pt in PreludeType::ALL {
        let name = pt.name();
        if message.contains(name) {
            continue;
        }
        if let Some(typo) = extract_unknown_ident(message) {
            if levenshtein(typo, name) <= 2 && typo.len() >= 3 {
                return Some(format!(
                    "AI hint: unknown type `{typo}` — did you mean the prelude type `{name}`?"
                ));
            }
        }
    }
    None
}

/// Best-effort extraction of an unknown identifier from a diagnostic
/// message. Looks for backtick-quoted tokens first, then falls back to
/// `unknown ... 'name'` patterns. Returns `None` when no ident-shaped
/// token is found.
fn extract_unknown_ident(message: &str) -> Option<&str> {
    if let Some(start) = message.find('`') {
        if let Some(end_rel) = message[start + 1..].find('`') {
            let candidate = &message[start + 1..start + 1 + end_rel];
            if is_ident_shaped(candidate) {
                return Some(candidate);
            }
        }
    }
    if let Some(start) = message.find('\'') {
        if let Some(end_rel) = message[start + 1..].find('\'') {
            let candidate = &message[start + 1..start + 1 + end_rel];
            if is_ident_shaped(candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_ident_shaped(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !s.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Iterative Levenshtein distance (≤ `max` early-exit). Bounded so the
/// O(n*m) cost stays small for short identifier comparisons.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.is_empty() {
        return b_bytes.len();
    }
    if b_bytes.is_empty() {
        return a_bytes.len();
    }
    let mut prev: Vec<usize> = (0..=b_bytes.len()).collect();
    let mut curr: Vec<usize> = vec![0; b_bytes.len() + 1];
    for (i, &ac) in a_bytes.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &bc) in b_bytes.iter().enumerate() {
            let cost = if ac == bc { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_bytes.len()]
}

fn render_verify_report(report: &CheckReport, file: &Path) {
    let has_errors = report
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    let has_warnings = report
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Warning));
    if report.diagnostics.is_empty() {
        eprintln!("{}: AI verify — no issues found", file.display());
        return;
    }
    for d in &report.diagnostics {
        let label = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        eprintln!("{label}: {}", d.message);
    }
    let summary = match (has_errors, has_warnings) {
        (true, _) => "AI verify FAILED: errors found",
        (false, true) => "AI verify OK with warnings",
        (false, false) => "AI verify OK",
    };
    eprintln!("{summary}");
}

// ---------------------------------------------------------------------------
// context — Markdown pack generation
// ---------------------------------------------------------------------------

/// Build the full AI context pack as a Markdown string.
///
/// Sections:
/// 1. Header + offline notice
/// 2. Language syntax summary (keywords, operators, primitive types)
/// 3. Prelude free functions (from [`buff_lang_types::PreludeFn`])
/// 4. Prelude types (from [`buff_lang_types::PreludeType`])
/// 5. Per-Type method signatures (assoc_fn + instance_fn enumeration)
/// 6. Current project structure (when `.buff` files exist under `project_root`)
/// 7. Idioms + examples pointers
pub fn build_context_pack(project_root: &Path) -> String {
    let mut out = String::with_capacity(16 * 1024);
    out.push_str("# Buff AI Context Pack\n\n");
    out.push_str(
        "> Generated by `buff ai context`. Paste this entire file into your AI tool \
         (Copilot / Claude / etc.) to ground it in the Buff language + your project. \
         Offline — no AI APIs called.\n\n",
    );

    out.push_str(&section_language_syntax());
    out.push_str(&section_prelude_functions());
    out.push_str(&section_prelude_types());
    out.push_str(&section_type_methods());
    out.push_str(&section_project_structure(project_root));
    out.push_str(&section_idioms());

    out
}

fn section_language_syntax() -> String {
    let mut s = String::with_capacity(4 * 1024);
    s.push_str("## 1. Language Syntax Summary\n\n");
    s.push_str("Buff is a high-level language that transpiles to Rust. Key properties:\n\n");
    s.push_str("- **Layout-sensitive**: indentation defines blocks (4 spaces, no tabs).\n");
    s.push_str("- **No braces** for control flow — only indentation.\n");
    s.push_str(
        "- **Braces `{ }`** reserved for data: struct literals, maps, lambdas `{ x => ... }`.\n",
    );
    s.push_str("- **Statically typed** with aggressive inference — types rarely written.\n");
    s.push_str(
        "- **No references** (`&`), **no visible lifetimes** (`'a`), **no `await` keyword**.\n",
    );
    s.push_str("- **No `null`/`nil`** — absence is `Option<T>`.\n");
    s.push_str("- **No class hierarchies** — OOP via structs + traits + embedding.\n\n");

    s.push_str("### Reserved keywords (25)\n\n");
    s.push_str("```\n");
    s.push_str("func let mut struct enum trait type if else for return break continue in match\n");
    s.push_str("async spawn import export from as true false extern unsafe\n");
    s.push_str("```\n\n");

    s.push_str("### Primitive types\n\n");
    s.push_str("- `Int` (default Int<64>), `Float` (default Float<32>), `Double` (Float<64>)\n");
    s.push_str("- `Bool`, `String`, `Char`, `Byte`\n");
    s.push_str("- `Void` (unit / `()`)\n");
    s.push_str("- `Option<T>` (Some/None), `Result<T, E>` (Ok/Err)\n");
    s.push_str("- Collections: `Vector<T>` (or `Vec<T>`), `Map<K, V>`, `Matrix<T>`\n");
    s.push_str("- `Decimal` (fixed-point, financial)\n\n");

    s.push_str("### Function shape\n\n");
    s.push_str("```buff\n");
    s.push_str("func add(a: Int, b: Int) -> Int:\n");
    s.push_str("    return a + b\n");
    s.push_str("\n");
    s.push_str("func main():\n");
    s.push_str("    print(add(2, 3))\n");
    s.push_str("```\n\n");
    s.push_str("- `func` declares a function. The body is the indented block after `:`.\n");
    s.push_str("- Parameters are `name: Type`. Return type after `->` (omit for `Void`).\n");
    s.push_str("- `func main():` is the entry point for runnable programs.\n");
    s.push_str("- `async func` declares an async function (NO `await` keyword needed).\n");
    s.push_str("- `@test` attribute marks a test function (run via `buff test`).\n");
    s.push_str("- Named args at call sites: `fetch(url, cache: true)` (NOT positional).\n\n");
    s
}

fn section_prelude_functions() -> String {
    use buff_lang_types::{PreludeCategory, PreludeFn};
    let mut s = String::with_capacity(2 * 1024);
    s.push_str("## 2. Prelude Functions (implicit — no `import` needed)\n\n");
    s.push_str("These are always in scope. Grouped by category.\n\n");

    let cats = [
        (PreludeCategory::Math, "Math"),
        (PreludeCategory::Convert, "Type conversions"),
        (PreludeCategory::Io, "I/O"),
        (PreludeCategory::System, "System / environment"),
        (PreludeCategory::Test, "Testing"),
        (PreludeCategory::Collection, "Collections (reserved)"),
    ];
    for (cat, label) in cats {
        let names: Vec<&str> = PreludeFn::ALL
            .iter()
            .filter(|f| f.category() == cat)
            .map(|f| f.name())
            .collect();
        if names.is_empty() {
            continue;
        }
        s.push_str(&format!("### {label}\n\n"));
        for n in names {
            s.push_str(&format!("- `{n}`\n"));
        }
        s.push('\n');
    }
    s.push_str("Common signatures:\n\n");
    s.push_str("```buff\n");
    s.push_str("print(x)           // prints without newline\n");
    s.push_str("println(x)         // prints with newline\n");
    s.push_str("read_line() -> String\n");
    s.push_str("input() -> String  // reads stdin (T124g)\n");
    s.push_str("input(prompt: String) -> String\n");
    s.push_str("sleep(d: Duration) -> Void  // async-transparent\n");
    s.push_str("abs(x), min(a, b), max(a, b), pow(base, exp)\n");
    s.push_str("sqrt(x) -> Float, floor(x), ceil(x), round(x)\n");
    s.push_str("Int(x), Float(x), String(x), Bool(x)  // type conversions\n");
    s.push_str("args() -> Vector<String>, env(\"KEY\") -> Option<String>\n");
    s.push_str("exit(code: Int)\n");
    s.push_str("assert_eq(a, b)    // panics on mismatch (test helper)\n");
    s.push_str("```\n\n");
    s
}

fn section_prelude_types() -> String {
    use buff_lang_types::PreludeType;
    let mut s = String::with_capacity(4 * 1024);
    s.push_str("## 3. Prelude Types (implicit — no `import` needed)\n\n");
    s.push_str(
        "All types below are available without `import`. Namespace-only types \
         (Log, Toml, Math, ...) are never instantiated — they expose associated \
         functions only (e.g. `Log.info(\"msg\")`). Value types carry instance \
         methods (e.g. `dt.year()`).\n\n",
    );

    let mut value_types: Vec<&str> = Vec::new();
    let mut namespace_types: Vec<&str> = Vec::new();
    for &pt in PreludeType::ALL {
        if pt.is_namespace_only() {
            namespace_types.push(pt.name());
        } else {
            value_types.push(pt.name());
        }
    }
    s.push_str("### Value types (instances exist, have methods)\n\n");
    for n in &value_types {
        s.push_str(&format!("- `{n}`\n"));
    }
    s.push('\n');
    s.push_str("### Namespace-only types (associated fns only)\n\n");
    for n in &namespace_types {
        s.push_str(&format!("- `{n}`\n"));
    }
    s.push('\n');
    s
}

fn section_type_methods() -> String {
    use buff_lang_types::{PreludeAssocFn, PreludeInstanceFn, PreludeType};
    let mut s = String::with_capacity(8 * 1024);
    s.push_str("## 4. Per-Type Method Signatures\n\n");
    s.push_str(
        "Enumerates every associated function (`Type.method(args)`) and instance \
         method (`recv.method(args)`) registered on each prelude type. AI tools \
         should use these as the authoritative surface.\n\n",
    );

    for &pt in PreludeType::ALL {
        let type_name = pt.name();
        let assoc_fns: Vec<&str> = PreludeAssocFn::ALL
            .iter()
            .filter(|m| buff_lang_types::assoc_fn_return_type(pt, **m, &[]).is_some())
            .map(|m| m.name())
            .collect();
        let instance_fns: Vec<&str> = PreludeInstanceFn::ALL
            .iter()
            .filter(|m| {
                buff_lang_types::instance_fn_return_type(&pt.buff_type(), *m, &[]).is_some()
            })
            .map(|m| m.name())
            .collect();
        if assoc_fns.is_empty() && instance_fns.is_empty() {
            continue;
        }
        s.push_str(&format!("### `{type_name}`\n\n"));
        if !assoc_fns.is_empty() {
            s.push_str(&format!("- Associated functions: "));
            let joined = assoc_fns
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ");
            s.push_str(&joined);
            s.push_str(&format!("  (call as `{type_name}.method(args)`)\n"));
        }
        if !instance_fns.is_empty() {
            s.push_str(&format!("- Instance methods: "));
            let joined = instance_fns
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ");
            s.push_str(&joined);
            s.push_str(&format!(
                "  (call as `recv.method(args)` where `recv: {type_name}`)\n"
            ));
        }
        s.push('\n');
    }
    s
}

fn section_project_structure(project_root: &Path) -> String {
    let mut s = String::with_capacity(2 * 1024);
    s.push_str("## 5. Current Project Structure\n\n");

    let buff_files = collect_buff_files(project_root);
    if buff_files.is_empty() {
        s.push_str(&format!(
            "No `.buff` source files found under `{}`. \
             Run `buff ai context` from a project root, or pass `--project <PATH>`.\n\n",
            project_root.display()
        ));
        return s;
    }

    s.push_str(&format!(
        "Scanned `{}` — {} `.buff` file(s) found.\n\n",
        project_root.display(),
        buff_files.len()
    ));

    s.push_str("### Files\n\n");
    for f in &buff_files {
        let rel = f
            .strip_prefix(project_root)
            .unwrap_or(f)
            .display()
            .to_string();
        s.push_str(&format!("- `{rel}`\n"));
    }
    s.push('\n');

    s.push_str("### Symbols (functions, structs, enums discovered)\n\n");
    let mut total_symbols = 0usize;
    for f in &buff_files {
        let rel = f
            .strip_prefix(project_root)
            .unwrap_or(f)
            .display()
            .to_string();
        match extract_project_symbols(f) {
            Ok(syms) if !syms.is_empty() => {
                total_symbols += syms.len();
                s.push_str(&format!("**{rel}:**\n\n"));
                for sym in syms {
                    s.push_str(&format!("- {sym}\n"));
                }
                s.push('\n');
            }
            _ => {}
        }
    }
    if total_symbols == 0 {
        s.push_str("_(No callable symbols discovered — files may not yet parse cleanly.)_\n\n");
    }
    s
}

fn section_idioms() -> String {
    let mut s = String::with_capacity(2 * 1024);
    s.push_str("## 6. Idioms + Examples\n\n");
    s.push_str("### Naming conventions\n\n");
    s.push_str("- Functions / variables: `snake_case` (e.g. `parse_url`, `user_count`)\n");
    s.push_str("- Types (struct / enum): `PascalCase` (e.g. `HttpRequest`, `Color`)\n");
    s.push_str("- Enum variants: `PascalCase` (e.g. `Red`, `NotFound`)\n");
    s.push_str("- Constants: `UPPER_SNAKE_CASE`\n\n");
    s.push_str("### Constructors\n\n");
    s.push_str(
        "- Use `Type.new(...)` or `Type.from(...)` — NOT `Type.create()` / `Type.build()`.\n",
    );
    s.push_str("- Example: `Regex.compile(pattern)` returns a `Regex` value.\n\n");
    s.push_str("### Async\n\n");
    s.push_str("- `async func` declares; `spawn` runs in background; **no `await` keyword**.\n");
    s.push_str("- The compiler propagates `.await` automatically through the call graph.\n\n");
    s.push_str("### Error handling\n\n");
    s.push_str("- `Result<T, E>` return types; `?` propagates errors early.\n");
    s.push_str("- `Ok(value)` / `Err(error)` constructors; builtin `Error(\"msg\")`.\n");
    s.push_str("- `match` on `Ok` / `Err` arms.\n\n");
    s.push_str("### Examples\n\n");
    s.push_str(
        "See the `examples/` directory in the Buff repo for runnable reference code. \
         Highlights:\n",
    );
    s.push_str("- `examples/ola.buff` — hello world\n");
    s.push_str("- `examples/fibonacci.buff` — recursion + typed params\n");
    s.push_str("- `examples/closures.buff` — lambdas `{ x => ... }`\n");
    s.push_str("- `examples/collections.buff` — `Vector<T>`, `Map<K,V>`\n");
    s.push_str("- `examples/error_handling.buff` — `Result`, `?`, builtin `Error`\n");
    s.push_str("- `examples/pattern_matching.buff` — `match`, `Option<T>`, `Result<T,E>`\n\n");
    s.push_str("### Verify AI output\n\n");
    s.push_str(
        "After the AI generates `.buff` code, run `buff ai verify <file>` to \
         type-check it locally. The verify command catches lex/parse/type errors \
         and emits AI-friendly hints (e.g. misspelled prelude names).\n",
    );
    s
}

// ---------------------------------------------------------------------------
// Project structure discovery
// ---------------------------------------------------------------------------

/// Recursively collect `.buff` files under `root`, sorted for determinism.
///
/// Skips `target/`, `node_modules/`, `.git/`, and hidden directories so the
/// context pack doesn't drown in build artifacts.
fn collect_buff_files(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut visited: BTreeSet<PathBuf> = BTreeSet::new();
    while let Some(dir) = stack.pop() {
        if !visited.insert(dir.clone()) {
            continue;
        }
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if path.is_dir() {
                if name_str == "target"
                    || name_str == "node_modules"
                    || name_str == ".git"
                    || name_str.starts_with('.')
                {
                    continue;
                }
                stack.push(path);
            } else if path.is_file() && name_str.ends_with(".buff") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Parse a single `.buff` file and extract a list of human-readable
/// symbol signatures (functions, structs, enums).
///
/// Returns `Err` if the file fails to lex/parse (which is fine — the
/// context pack still lists the file in section 5 above).
fn extract_project_symbols(file: &Path) -> Result<Vec<String>> {
    use buff_lang_ast::Decl;
    use buff_lang_error::SourceId;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read `{}`", file.display()))?;
    let tokens = buff_lang_lexer::tokenize(&src, SourceId(0))
        .map_err(|e| anyhow::anyhow!("lex error: {}", e.inner.diagnostic.message))?;
    let decls = buff_lang_parser::parse(&tokens, SourceId(0))
        .map_err(|e| anyhow::anyhow!("parse error: {}", e.diagnostic.message))?;
    Ok(extract_decl_signatures(&decls))
}

/// Walk a flat `&[Decl]` and produce one signature string per public
/// declaration. Recurses into `ExportDecl` wrappers.
fn extract_decl_signatures(decls: &[buff_lang_ast::Decl]) -> Vec<String> {
    let mut out = Vec::new();
    for d in decls {
        walk_decl(d, &mut out);
    }
    out
}

fn walk_decl(decl: &buff_lang_ast::Decl, out: &mut Vec<String>) {
    use buff_lang_ast::{Decl, EnumDecl, ExportDecl, FuncDecl, StructDecl};
    match decl {
        Decl::FuncDecl(f) => out.push(format_func_signature(f, false)),
        Decl::ExportDecl(ExportDecl { inner, .. }) => walk_decl(inner.as_ref(), out),
        Decl::StructDecl(StructDecl {
            name,
            fields,
            traits,
            ..
        }) => {
            let trait_list = if traits.is_empty() {
                String::new()
            } else {
                let joined = traits
                    .iter()
                    .map(|t| t.name.clone())
                    .collect::<Vec<_>>()
                    .join(" + ");
                format!(": {joined}")
            };
            let fields_list = if fields.is_empty() {
                String::from("(no fields)")
            } else {
                fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n.name, t))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push(format!(
                "struct {}{trait_list} {{ {fields_list} }}",
                name.name
            ));
        }
        Decl::EnumDecl(EnumDecl { name, variants, .. }) => {
            let v = variants
                .iter()
                .map(|x| x.name.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!("enum {} {{ {v} }}", name.name));
        }
        _ => {}
    }
}

fn format_func_signature(f: &FuncDecl, exported: bool) -> String {
    let prefix = if exported { "export " } else { "" };
    let async_kw = if f.is_async { "async " } else { "" };
    let params = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name.name, p.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let ret = match &f.return_type {
        Some(rt) => format!(" -> {rt}"),
        None => String::new(),
    };
    format!("{prefix}{async_kw}func {}({params}){ret}", f.name.name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_identical_is_zero() {
        assert_eq!(levenshtein("print", "print"), 0);
    }

    #[test]
    fn levenshtein_near_miss_one_edit() {
        assert_eq!(levenshtein("prnt", "print"), 1);
        assert_eq!(levenshtein("printx", "print"), 1);
    }

    #[test]
    fn extract_unknown_ident_backticks() {
        assert_eq!(
            extract_unknown_ident("unknown ident `prnt` here"),
            Some("prnt")
        );
    }

    #[test]
    fn extract_unknown_ident_returns_none_for_empty() {
        assert_eq!(extract_unknown_ident("no idents here"), None);
    }

    #[test]
    fn hint_for_misspelled_print_suggests_print() {
        let h = hint_for_diagnostic("unknown function `prnt`");
        assert!(h.is_some(), "should suggest a hint");
        let h = h.expect("checked above");
        assert!(h.contains("print"), "hint should suggest `print`: {h}");
    }

    #[test]
    fn hint_for_parse_error_mentions_fmt() {
        let h = hint_for_diagnostic("parse error: unexpected token");
        assert!(h.is_some());
        assert!(h.expect("checked").contains("buff fmt"));
    }

    #[test]
    fn context_pack_contains_all_six_sections() {
        let pack = build_context_pack(Path::new("/nonexistent"));
        assert!(pack.contains("## 1. Language Syntax Summary"));
        assert!(pack.contains("## 2. Prelude Functions"));
        assert!(pack.contains("## 3. Prelude Types"));
        assert!(pack.contains("## 4. Per-Type Method Signatures"));
        assert!(pack.contains("## 5. Current Project Structure"));
        assert!(pack.contains("## 6. Idioms + Examples"));
    }

    #[test]
    fn collect_buff_files_skips_target_dir() {
        let dir = std::env::temp_dir().join("buff-ai-context-test");
        let _ = std::fs::create_dir_all(dir.join("target"));
        let _ = std::fs::create_dir_all(dir.join("src"));
        std::fs::write(
            dir.join("src/main.buff"),
            "func main():\n    print(\"hi\")\n",
        )
        .expect("write");
        std::fs::write(
            dir.join("target/junk.buff"),
            "func leftover():\n    print(\"no\")\n",
        )
        .expect("write");
        let found = collect_buff_files(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            !found.iter().any(|p| p.to_string_lossy().contains("target")),
            "should NOT recurse into target/: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|p| p.to_string_lossy().contains("main.buff")),
            "should find src/main.buff: {found:?}"
        );
    }

    #[test]
    fn extract_signatures_from_real_buff_src() {
        let dir = std::env::temp_dir().join("buff-ai-sig-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("sig.buff");
        std::fs::write(
            &file,
            "func add(a: Int, b: Int) -> Int:\n    return a + b\n\nfunc main():\n    print(add(1, 2))\n",
        )
        .expect("write");
        let sigs = extract_project_symbols(&file).expect("parse ok");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(sigs.iter().any(|s| s.contains("func add(")), "{sigs:?}");
        assert!(sigs.iter().any(|s| s.contains("func main(")), "{sigs:?}");
    }
}
