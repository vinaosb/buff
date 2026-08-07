//! Naming-convention linter for Buff (T55).
//!
//! Enforces the naming rules from `.sisyphus/plans/buff-conventions.md` §1:
//!
//! | Element                 | Convention     | Example                       |
//! |-------------------------|----------------|-------------------------------|
//! | Functions               | `snake_case`   | `func calculate_total()`      |
//! | Variables (`let`)       | `snake_case`   | `let item_count = 42`         |
//! | Types (Struct/Enum)     | `PascalCase`   | `struct HttpRequest`          |
//! | Enum variants           | `PascalCase`   | `Red`, `Green`, `Ok`, `Err`   |
//! | Traits                  | `PascalCase`   | `trait Iterable`              |
//! | Struct fields           | `snake_case`   | `name`, `item_count`          |
//!
//! Constants (`SCREAMING_SNAKE_CASE`) are NOT linted — Buff has no syntactic
//! `const` keyword, so `let MAX = 3` is indistinguishable from a regular
//! `let`. The conventions table lists the rule for future reference.
//!
//! # Determinism
//!
//! The walker visits decls + their bodies in source order (no HashMap), so
//! the same AST produces byte-identical diagnostics every run.
//!
//! # Output
//!
//! Every diagnostic is a [`Severity::Warning`](buff_lang_error::Severity).
//! Warnings do NOT fail the `buff check` exit code by default; the
//! `--deny-warnings` / `-D` flag (see [`crate::check`]) promotes them to
//! exit-non-zero.

use buff_lang_ast::{
    Block, Decl, EnumDecl, ExportDecl, Expr, ExtendBlock, FuncDecl, Ident, Stmt, StructDecl,
    TraitDecl,
};
use buff_lang_error::Diagnostic;

// ---------------------------------------------------------------------------
// Pure predicates (public for unit testing).
// ---------------------------------------------------------------------------

/// Returns `true` when `s` is a valid Buff `snake_case` identifier.
///
/// A snake_case identifier consists of lowercase ASCII letters, ASCII digits,
/// and underscores, with at least one alphanumeric character. There is no
/// restriction on leading, trailing, or consecutive underscores (the leading
/// underscore is a Rust idiom for intentionally-unused bindings, and Buff
/// keeps the convention).
///
/// # Examples
///
/// ```
/// # use buff_lang_cli::naming_lint::is_snake_case;
/// assert!( is_snake_case("foo"));
/// assert!( is_snake_case("foo_bar"));
/// assert!( is_snake_case("item_count_42"));
/// assert!( is_snake_case("_unused"));
/// assert!( is_snake_case("x"));
/// assert!( is_snake_case("a1_b2"));
/// assert!(!is_snake_case(""));
/// assert!(!is_snake_case("fooBar"));   // uppercase letter
/// assert!(!is_snake_case("Foo"));      // starts uppercase
/// assert!(!is_snake_case("foo-bar"));  // hyphen
/// assert!(!is_snake_case("_"));        // no alphanumeric
/// ```
pub fn is_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let has_alnum = s.chars().any(|c| c.is_ascii_alphanumeric());
    if !has_alnum {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Returns `true` when `s` is a valid Buff `PascalCase` identifier.
///
/// A PascalCase identifier begins with an uppercase ASCII letter and contains
/// only ASCII alphanumeric characters afterwards (no underscores, no hyphens).
/// Single-letter names are accepted (covers enum variants like `T` and short
/// type aliases).
///
/// # Examples
///
/// ```
/// # use buff_lang_cli::naming_lint::is_pascal_case;
/// assert!( is_pascal_case("Foo"));
/// assert!( is_pascal_case("HttpRequest"));
/// assert!( is_pascal_case("Color"));
/// assert!( is_pascal_case("Ok"));
/// assert!( is_pascal_case("T"));
/// assert!( is_pascal_case("Vector2"));
/// assert!(!is_pascal_case(""));
/// assert!(!is_pascal_case("foo"));          // starts lowercase
/// assert!(!is_pascal_case("Foo_Bar"));      // underscore
/// assert!(!is_pascal_case("Foo-Bar"));      // hyphen
/// assert!(!is_pascal_case("1Foo"));         // starts digit
/// assert!(!is_pascal_case("Foo Bar"));      // space
/// ```
pub fn is_pascal_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().expect("non-empty checked above");
    if !first.is_ascii_uppercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric())
}

// ---------------------------------------------------------------------------
// Linter entry point.
// ---------------------------------------------------------------------------

/// Walk a top-level declaration list in source order and emit naming-lint
/// warning diagnostics for any identifier that violates the conventions.
///
/// The walk is purely deterministic: same AST → byte-identical diagnostics.
/// All emitted diagnostics are
/// [`Severity::Warning`](buff_lang_error::Severity::Warning).
///
/// Recurses into function bodies (for `let`-binding names), `extend` blocks
/// (for method names), trait default-method bodies, and the inner decl of
/// `export` wrappers. Struct field names, enum variant names, and trait
/// method names are checked at their declaration sites.
pub fn lint_naming(decls: &[Decl]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for d in decls {
        lint_decl(d, &mut out);
    }
    out
}

fn lint_decl(decl: &Decl, out: &mut Vec<Diagnostic>) {
    match decl {
        Decl::FuncDecl(f) => lint_func(f, out),
        Decl::StructDecl(s) => lint_struct(s, out),
        Decl::EnumDecl(e) => lint_enum(e, out),
        Decl::TraitDecl(t) => lint_trait(t, out),
        Decl::ExtendBlock(b) => lint_extend(b, out),
        Decl::ExportDecl(ExportDecl { inner, .. }) => lint_decl(inner, out),
        // Import / Module / Reexport / ExternCrate: no user-named identifiers
        // to lint at the convention-level (module names follow Buff source
        // paths and the convention says "snake_case" for filenames — that's
        // a file-system lint, out of scope for T55).
        Decl::ImportDecl(_)
        | Decl::ModuleDecl(_)
        | Decl::ReexportDecl(_)
        | Decl::ExternCrateDecl(_)
        | Decl::ExternFuncDecl(_)
        | Decl::ImplBlock(_) => {}
    }
}

fn lint_func(f: &FuncDecl, out: &mut Vec<Diagnostic>) {
    warn_snake("function", &f.name, out);
    // Parameters are bindings → snake_case.
    for p in &f.params {
        warn_snake("parameter", &p.name, out);
    }
    // Walk body for nested let-bindings.
    lint_block(&f.body, out);
}

fn lint_struct(s: &StructDecl, out: &mut Vec<Diagnostic>) {
    warn_pascal("struct", &s.name, out);
    for (field_name, _) in &s.fields {
        warn_snake("struct field", field_name, out);
    }
}

fn lint_enum(e: &EnumDecl, out: &mut Vec<Diagnostic>) {
    warn_pascal("enum", &e.name, out);
    // Generic type params are conventionally PascalCase (single letter OK).
    for tp in &e.type_params {
        warn_pascal("type parameter", &tp.name, out);
    }
    for v in &e.variants {
        warn_pascal("enum variant", &v.name, out);
    }
}

fn lint_trait(t: &TraitDecl, out: &mut Vec<Diagnostic>) {
    warn_pascal("trait", &t.name, out);
    // Required method signatures: snake_case names.
    for sig in &t.required {
        warn_snake("trait method", &sig.name, out);
    }
    // Default methods: full FuncDecl walk (name + body).
    for d in &t.defaults {
        lint_func(d, out);
    }
}

fn lint_extend(b: &ExtendBlock, out: &mut Vec<Diagnostic>) {
    for m in &b.methods {
        lint_func(m, out);
    }
}

fn lint_block(block: &Block, out: &mut Vec<Diagnostic>) {
    for stmt in &block.stmts {
        lint_stmt(stmt, out);
    }
}

fn lint_stmt(stmt: &Stmt, out: &mut Vec<Diagnostic>) {
    match stmt {
        Stmt::LetDecl { name, .. } => {
            warn_snake("variable", name, out);
        }
        Stmt::ForIn { var, body, .. } => {
            warn_snake("loop variable", var, out);
            lint_block(body, out);
        }
        Stmt::ForWhile { body, .. } | Stmt::While { body, .. } | Stmt::ForLet { body, .. } => {
            lint_block(body, out);
        }
        Stmt::Guard { else_block, .. } => {
            lint_block(else_block, out);
        }
        // LetPattern / Assignment / ExprStmt / Return / Break / Continue /
        // Defer — either no identifier to lint, or pattern bindings that v0.5
        // leaves to the destructuring pass. Skip cleanly.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Internal: per-identifier warning helpers.
// ---------------------------------------------------------------------------

/// Push a Warning diagnostic when `name` is not `snake_case`.
fn warn_snake(kind: &str, name: &Ident, out: &mut Vec<Diagnostic>) {
    if !is_snake_case(&name.name) {
        out.push(Diagnostic::warning(
            format!("{kind} `{}` should be snake_case", name.name),
            name.span,
        ));
    }
}

/// Push a Warning diagnostic when `name` is not `PascalCase`.
fn warn_pascal(kind: &str, name: &Ident, out: &mut Vec<Diagnostic>) {
    if !is_pascal_case(&name.name) {
        out.push(Diagnostic::warning(
            format!("{kind} `{}` should be PascalCase", name.name),
            name.span,
        ));
    }
}

// ---------------------------------------------------------------------------
// T63 — Common-mistake linter patterns.
// ---------------------------------------------------------------------------

/// Walk `decls` looking for common beginner mistakes and emit warning
/// diagnostics with `help:` suggestion notes.
///
/// Covered patterns (T63 spec):
///
/// - **PascalCase prelude call** — `Print(...)` / `Abs(...)` / etc. The
///   user wrote a builtin with the wrong case. Emits a warning naming the
///   prelude fn + a `help: function names are lowercase, did you mean
///   \`print\`?` note.
/// - **Typo of a prelude fn** — `prin(...)` / `abs(...)`-ish misspelling
///   of a builtin. Emits a `help: did you mean \`print\`?` note when the
///   callee is within Levenshtein distance 2 of a prelude name.
///
/// Both checks look only at [`Expr::FuncCall`] whose callee is a bare
/// [`Expr::Ident`] (the shape of a free-fn / prelude call). Method calls
/// and indirect calls are skipped. Recurses into `let`-binding RHS,
/// expression statements, returns, and call arguments.
///
/// All emitted diagnostics are
/// [`Severity::Warning`](buff_lang_error::Severity) — they do not fail
/// `buff check` unless `--deny-warnings` is passed.
pub fn lint_common_mistakes(decls: &[Decl]) -> Vec<Diagnostic> {
    let candidates = prelude_candidate_names();
    // Collect user-defined function names so the lint does NOT flag
    // calls to them as "unknown function" false positives.
    let defined_funcs: Vec<String> = decls
        .iter()
        .filter_map(|d| match d {
            Decl::FuncDecl(f) => Some(f.name.name.clone()),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for d in decls {
        lint_mistakes_decl(d, &candidates, &defined_funcs, &mut out);
    }
    out
}

/// Scan raw `src` for leading tab characters and emit one warning per
/// tab-indented line.
///
/// The lexer already rejects tabs as [`E1004`](buff_lang_error::ErrorCode::MixedTabsSpaces),
/// but that error fires at lex time and aborts parsing — so the user sees
/// only the *first* tab. This source-level scan complements it: even when
/// the lexer has already rejected the file, `buff check` can report every
/// tab-indented line at once with the clearer "Buff uses 4 spaces, not
/// tabs" message.
///
/// Returns one [`Diagnostic::warning`] per offending line, anchored at the
/// byte offset of the tab on that line (span covers just the leading tab
/// run so the caret points at the whitespace).
pub fn lint_tab_indentation(src: &str) -> Vec<Diagnostic> {
    let source_id = buff_lang_error::SourceId(0);
    let mut out = Vec::new();
    for (line_idx, line) in src.lines().enumerate() {
        // Count leading tabs.
        let leading_tabs = line.bytes().take_while(|&b| b == b'\t').count();
        if leading_tabs == 0 {
            continue;
        }
        // Byte offset of the first tab = sum of lengths of prior lines
        // (including their `\n`) — `lines()` strips the `\n`, so re-add 1.
        let line_start = src
            .lines()
            .take(line_idx)
            .map(|l| l.len() + 1)
            .sum::<usize>();
        let span = buff_lang_error::Span::new(line_start, line_start + leading_tabs, source_id);
        out.push(Diagnostic::warning("Buff uses 4 spaces, not tabs", span));
    }
    out
}

/// Collect the source names of every prelude free fn + prelude type, for
/// use as the suggestion candidate set. The prelude is small (~60 names)
/// so the `Vec` is rebuilt per `lint_common_mistakes` call (an analysis
/// pass, not a hot path).
fn prelude_candidate_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::with_capacity(64);
    for &pf in buff_lang_types::PreludeFn::ALL {
        names.push(pf.name());
    }
    for &pt in buff_lang_types::prelude_types::PreludeType::ALL {
        names.push(pt.name());
    }
    names
}

fn lint_mistakes_decl(
    decl: &Decl,
    candidates: &[&str],
    defined_funcs: &[String],
    out: &mut Vec<Diagnostic>,
) {
    match decl {
        Decl::FuncDecl(f) => {
            for stmt in &f.body.stmts {
                lint_mistakes_stmt(stmt, candidates, defined_funcs, out);
            }
        }
        Decl::TraitDecl(t) => {
            for d in &t.defaults {
                for stmt in &d.body.stmts {
                    lint_mistakes_stmt(stmt, candidates, defined_funcs, out);
                }
            }
        }
        Decl::ExtendBlock(b) => {
            for m in &b.methods {
                for stmt in &m.body.stmts {
                    lint_mistakes_stmt(stmt, candidates, defined_funcs, out);
                }
            }
        }
        Decl::ExportDecl(inner) => lint_mistakes_decl(&inner.inner, candidates, defined_funcs, out),
        _ => {}
    }
}

fn lint_mistakes_stmt(
    stmt: &Stmt,
    candidates: &[&str],
    defined_funcs: &[String],
    out: &mut Vec<Diagnostic>,
) {
    match stmt {
        Stmt::LetDecl { value, .. } => lint_mistakes_expr(value, candidates, defined_funcs, out),
        Stmt::ExprStmt(e, _) | Stmt::Return(Some(e), _) => {
            lint_mistakes_expr(e, candidates, defined_funcs, out);
        }
        Stmt::ForIn { body, .. }
        | Stmt::ForWhile { body, .. }
        | Stmt::While { body, .. }
        | Stmt::ForLet { body, .. } => {
            for s in &body.stmts {
                lint_mistakes_stmt(s, candidates, defined_funcs, out);
            }
        }
        _ => {}
    }
}

fn lint_mistakes_expr(
    expr: &Expr,
    candidates: &[&str],
    defined_funcs: &[String],
    out: &mut Vec<Diagnostic>,
) {
    if let Expr::FuncCall { callee, args, span } = expr {
        if let Expr::Ident(ident, _) = callee.as_ref() {
            let name = &ident.name;
            // Skip if it IS a valid prelude name already.
            if buff_lang_types::is_prelude(name) {
                // Recurse into args and stop here.
                for a in args {
                    lint_mistakes_expr(a, candidates, defined_funcs, out);
                }
                return;
            }
            // Skip if it IS a user-defined function.
            if defined_funcs.iter().any(|f| f == name) {
                for a in args {
                    lint_mistakes_expr(a, candidates, defined_funcs, out);
                }
                return;
            }
            // Check 1: PascalCase variant of a lowercase prelude fn.
            // e.g. `Print` -> `print`. Only suggest when the lowercase
            // form is a real prelude fn.
            let lower = name.to_ascii_lowercase();
            if &lower != name && buff_lang_types::is_prelude(&lower) {
                out.push(
                    Diagnostic::warning(
                        format!("function names are lowercase, not `{name}`"),
                        *span,
                    )
                    .with_note(format!("help: did you mean `{lower}`?")),
                );
                for a in args {
                    lint_mistakes_expr(a, candidates, defined_funcs, out);
                }
                return;
            }
            // Check 2: generic typo near a prelude name.
            if let Some(msg) = buff_lang_error::suggest_with_message(name, candidates) {
                out.push(
                    Diagnostic::warning(format!("unknown function `{name}`"), *span)
                        .with_note(format!("help: {msg}")),
                );
            }
        }
        // Recurse into callee + args for nested calls.
        for a in args {
            lint_mistakes_expr(a, candidates, defined_funcs, out);
        }
        lint_mistakes_expr(callee, candidates, defined_funcs, out);
        return;
    }
    // Recurse into other expression shapes that may contain calls.
    match expr {
        Expr::BinaryOp { lhs, rhs, .. } => {
            lint_mistakes_expr(lhs, candidates, defined_funcs, out);
            lint_mistakes_expr(rhs, candidates, defined_funcs, out);
        }
        Expr::UnaryOp { operand, .. } => {
            lint_mistakes_expr(operand, candidates, defined_funcs, out)
        }
        Expr::MethodCall { receiver, args, .. } => {
            lint_mistakes_expr(receiver, candidates, defined_funcs, out);
            for a in args {
                lint_mistakes_expr(a, candidates, defined_funcs, out);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_snake_case edge cases (the QA acceptance matrix) --

    #[test]
    fn snake_case_basic_word() {
        assert!(is_snake_case("foo"));
    }

    #[test]
    fn snake_case_two_words() {
        assert!(is_snake_case("foo_bar"));
    }

    #[test]
    fn snake_case_with_digits() {
        assert!(is_snake_case("item_count_42"));
    }

    #[test]
    fn snake_case_single_char() {
        assert!(is_snake_case("x"));
        assert!(is_snake_case("a"));
    }

    #[test]
    fn snake_case_leading_underscore() {
        assert!(is_snake_case("_unused"));
    }

    #[test]
    fn snake_case_rejects_empty() {
        assert!(!is_snake_case(""));
    }

    #[test]
    fn snake_case_rejects_camel() {
        assert!(!is_snake_case("fooBar"));
    }

    #[test]
    fn snake_case_rejects_pascal() {
        assert!(!is_snake_case("Foo"));
    }

    #[test]
    fn snake_case_rejects_all_caps() {
        // All-caps isn't snake_case (it's SCREAMING_SNAKE_CASE).
        assert!(!is_snake_case("FOO_BAR"));
    }

    #[test]
    fn snake_case_rejects_hyphen() {
        assert!(!is_snake_case("foo-bar"));
    }

    #[test]
    fn snake_case_rejects_pure_underscore() {
        assert!(!is_snake_case("_"));
        assert!(!is_snake_case("___"));
    }

    #[test]
    fn snake_case_lowercase_of_constants() {
        // Sanity: lowercase variant of MAX_SIZE is OK.
        assert!(is_snake_case("max_size"));
    }

    // -- is_pascal_case edge cases --

    #[test]
    fn pascal_case_basic() {
        assert!(is_pascal_case("Foo"));
    }

    #[test]
    fn pascal_case_multi_word() {
        assert!(is_pascal_case("HttpRequest"));
    }

    #[test]
    fn pascal_case_single_char() {
        assert!(is_pascal_case("T"));
        assert!(is_pascal_case("X"));
    }

    #[test]
    fn pascal_case_with_digit() {
        assert!(is_pascal_case("Vector2"));
    }

    #[test]
    fn pascal_case_rejects_empty() {
        assert!(!is_pascal_case(""));
    }

    #[test]
    fn pascal_case_rejects_lowercase_start() {
        assert!(!is_pascal_case("foo"));
        assert!(!is_pascal_case("fFoo"));
    }

    #[test]
    fn pascal_case_rejects_underscore() {
        assert!(!is_pascal_case("Foo_Bar"));
    }

    #[test]
    fn pascal_case_rejects_leading_digit() {
        assert!(!is_pascal_case("1Foo"));
    }

    #[test]
    fn pascal_case_rejects_space() {
        assert!(!is_pascal_case("Foo Bar"));
    }

    #[test]
    fn pascal_case_rejects_hyphen() {
        assert!(!is_pascal_case("Foo-Bar"));
    }
}
