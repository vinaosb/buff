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
    Block, Decl, EnumDecl, ExportDecl, ExtendBlock, FuncDecl, Ident, Stmt, StructDecl, TraitDecl,
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
        | Decl::ExternFuncDecl(_) => {}
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
    for g in &e.generics {
        warn_pascal("type parameter", g, out);
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
        Stmt::ForWhile { body, .. } | Stmt::ForLet { body, .. } => {
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
