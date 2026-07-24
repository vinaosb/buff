//! Span capture during codegen — builds the [`SourceMap`] from the Buff
//! AST + the formatted Rust source.
//!
//! `prettyplease` reformats the generated `syn::File`, so line numbers
//! computed during AST lowering would be wrong. Instead this module runs
//! a **post-format scan**: it walks the formatted Rust source line-by-line
//! looking for stable anchors (function names) and records the Rust line
//! where each anchor appears alongside its originating Buff span.
//!
//! # Anchors
//!
//! The stable anchors today are user-defined function names. Each
//! `func <name>(...)` declaration in Buff lowers to a Rust
//! `fn <name>(...)` item, and the function name survives `prettyplease`
//! verbatim (it's an identifier — `prettyplease` never rewrites those).
//! So scanning for `fn <name>` patterns in the formatted Rust source
//! reliably identifies the Rust line where each Buff function starts.
//!
//! Additional anchor kinds (struct names, let-binding names) can be
//! added later without changing the JSON schema — they'd just populate
//! more entries in the line mapping table.
//!
//! # Why not use syn spans?
//!
//! `syn` spans are opaque `proc_macro2::Span`s carrying no source-line
//! info (the codegen doesn't go through a `proc_macro` invocation). Even
//! if it did, `prettyplease` rewrites the tree and drops span info in
//! the process. The post-format line scan is the simplest viable
//! approach and matches the pattern T133 used for `.buffhtml` span
//! mapping (see `crates/buff-lang-codegen-buffhtml/src/span_map.rs`).

use std::path::Path;

use buff_lang_ast::Decl;
use buff_lang_error::{SourceFile, SourceId};

use crate::{BuffLocation, FunctionAnchor, SourceMap};

/// Build a [`SourceMap`] from the Buff AST + the formatted Rust source.
///
/// Walks every top-level `Decl::FuncDecl` in `decls`, computes its Buff
/// `(line, col)` via the canonical [`SourceFile::lookup`], then finds
/// the corresponding `fn <name>` line in the formatted Rust source.
///
/// For each function the scan records:
///
/// - A function-level [`FunctionAnchor`] (Buff function name + Buff
///   span + Rust line range). The Rust line range is computed by
///   scanning from the `fn <name>` line forward to the closing `}`
///   brace of the function body.
/// - A line-level [`BuffLocation`] at the function's Rust start line
///   (carrying the function name as its `name` field).
///
/// The `buff_source` is consumed only for line/col lookup; it is NOT
/// stored in the resulting [`SourceMap`] (the path is, via `buff_path`).
pub fn build_source_map(
    decls: &[Decl],
    rust_source: &str,
    buff_path: &Path,
    buff_source: &str,
) -> SourceMap {
    let source_id = SourceId(0);
    let source_file = SourceFile::new(buff_path.to_path_buf(), buff_source.to_string());
    let mut map = SourceMap::new().with_buff_file(buff_path.to_path_buf(), source_id);

    for decl in decls {
        if let Decl::FuncDecl(func) = decl {
            if func.is_extern {
                continue;
            }
            let name = &func.name.name;
            let span = func.span;
            let Some((buff_line, buff_col)) = source_file.lookup(span.start) else {
                continue;
            };
            let Some(rust_start_line) = find_fn_line(rust_source, name) else {
                continue;
            };
            let rust_end_line = find_fn_end_line(rust_source, rust_start_line);
            map.add_function(FunctionAnchor {
                name: name.clone(),
                buff_span: span,
                buff_line,
                buff_col,
                rust_start_line,
                rust_end_line,
                buff_location: Some(BuffLocation {
                    line: buff_line,
                    col: buff_col,
                    span,
                    name: Some(name.clone()),
                }),
            });
            map.add_line_mapping(
                rust_start_line,
                BuffLocation {
                    line: buff_line,
                    col: buff_col,
                    span,
                    name: Some(name.clone()),
                },
            );
        }
    }
    map
}

/// Find the 1-based Rust source line where `fn <name>` appears.
///
/// Scans line-by-line, matching the pattern `fn <name>(` or
/// `fn <name><` (generic) or `fn <name> {` (no params). Returns the
/// first match — Buff functions are unique within a compilation unit
/// (the parser rejects duplicate names), so this is unambiguous.
fn find_fn_line(rust_source: &str, name: &str) -> Option<usize> {
    for (idx, line) in rust_source.lines().enumerate() {
        if line_matches_fn_decl(line, name) {
            return Some(idx + 1);
        }
    }
    None
}

/// Returns `true` when `line` is a `fn <name>` declaration (not a call
/// site, not a reference inside another fn's body).
///
/// The pattern `fn <name>` followed by `(`, `<`, `{`, or whitespace is
/// specific enough to avoid false positives on call sites (which have
/// no `fn` keyword) and trait-impl signatures (which have a `fn`
/// keyword but are inside an `impl` block — those don't match Buff's
/// top-level fn shape today).
fn line_matches_fn_decl(line: &str, name: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("fn ") else {
        return false;
    };
    let Some(after_name) = rest.strip_prefix(name) else {
        return false;
    };
    matches!(
        after_name.chars().next(),
        Some('(') | Some('<') | Some('{') | Some(' ') | Some('\t')
    )
}

/// Find the 1-based Rust source line of the closing `}` of the function
/// that starts at `fn_start_line`.
///
/// Walks from `fn_start_line` tracking brace depth. The function body's
/// opening `{` may be on the same line as `fn <name>(...)` or on the
/// next line; both shapes are handled by counting every `{` and `}`
/// encountered. Returns `fn_start_line` itself if no balanced close
/// brace is found (defensive — the formatted source always has one).
fn find_fn_end_line(rust_source: &str, fn_start_line: usize) -> usize {
    let lines: Vec<&str> = rust_source.lines().collect();
    let mut depth: i64 = 0;
    let mut seen_open = false;
    for (idx, line) in lines
        .iter()
        .enumerate()
        .skip(fn_start_line.saturating_sub(1))
    {
        for ch in line.chars() {
            if ch == '{' {
                depth += 1;
                seen_open = true;
            } else if ch == '}' {
                depth -= 1;
            }
        }
        if seen_open && depth == 0 {
            return idx + 1;
        }
    }
    fn_start_line
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::{common::Ident, decl::FuncDecl};
    use buff_lang_error::{SourceId, Span};

    fn make_func(name: &str, span_start: usize, span_end: usize) -> FuncDecl {
        FuncDecl { name: Ident::new(name, Span::dummy()),
        params: Vec::new(),
        return_type: None,
        body: buff_lang_ast::common::Block::empty(Span::dummy()),
        is_async: false,
        is_unsafe: false,
        is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: Span::new(span_start, span_end, SourceId(0)), }
    }

    #[test]
    fn find_fn_line_finds_top_level_fn() {
        let src = "mod x {}\nfn helper() {\n    1 + 1\n}\nfn main() {\n    helper()\n}\n";
        assert_eq!(find_fn_line(src, "helper"), Some(2));
        assert_eq!(find_fn_line(src, "main"), Some(5));
        assert_eq!(find_fn_line(src, "missing"), None);
    }

    #[test]
    fn line_matches_fn_decl_distinguishes_decl_from_call() {
        assert!(line_matches_fn_decl("fn helper() {", "helper"));
        assert!(line_matches_fn_decl("fn helper {", "helper"));
        assert!(line_matches_fn_decl("fn helper<T>(", "helper"));
        assert!(!line_matches_fn_decl("    helper()", "helper"));
        assert!(!line_matches_fn_decl("let x = helper();", "helper"));
    }

    #[test]
    fn find_fn_end_line_handles_single_line_body() {
        let src = "fn helper() { 1 + 1 }\nfn main() {}\n";
        assert_eq!(find_fn_end_line(src, 1), 1);
    }

    #[test]
    fn find_fn_end_line_handles_multi_line_body() {
        let src = "fn helper() {\n    let x = 1;\n    x + 1\n}\nfn main() {}\n";
        assert_eq!(find_fn_end_line(src, 1), 4);
    }

    #[test]
    fn find_fn_end_line_handles_brace_on_next_line() {
        let src = "fn helper()\n{\n    1 + 1\n}\n";
        assert_eq!(find_fn_end_line(src, 1), 4);
    }

    #[test]
    fn build_source_map_captures_function_anchors() {
        let buff = "func helper():\n    return 1\n\nfunc main():\n    return helper()\n";
        let rust = "fn helper() {\n    1\n}\nfn main() {\n    helper()\n}\n";
        let helper_span_start = buff.find("func helper").unwrap_or(0);
        let helper_span_end = helper_span_start + "func helper():\n    return 1".len();
        let main_start = buff.find("func main").unwrap_or(0);
        let main_end = main_start + "func main():\n    return helper()".len();
        let decls = vec![
            Decl::FuncDecl(make_func_with_span(
                "helper",
                helper_span_start,
                helper_span_end,
            )),
            Decl::FuncDecl(make_func_with_span("main", main_start, main_end)),
        ];
        let map = build_source_map(&decls, rust, Path::new("test.buff"), buff);
        assert_eq!(map.functions.len(), 2);
        assert_eq!(map.functions[0].name, "helper");
        assert_eq!(map.functions[0].buff_line, 1);
        assert_eq!(map.functions[0].rust_start_line, 1);
        assert_eq!(map.functions[0].rust_end_line, 3);
        assert_eq!(map.functions[1].name, "main");
        assert_eq!(map.functions[1].buff_line, 4);
        assert_eq!(map.functions[1].rust_start_line, 4);
    }

    fn make_func_with_span(name: &str, start: usize, end: usize) -> FuncDecl {
        let mut f = make_func(name, start, end);
        f.span = Span::new(start, end, SourceId(0));
        f
    }

    #[test]
    fn build_source_map_skips_extern_funcs() {
        let buff = "extern func helper():\n    return 1\n";
        let rust = "extern \"C\" {\n    fn helper();\n}\n";
        let mut f = make_func("helper", 0, 10);
        f.is_extern = true;
        let decls = vec![Decl::FuncDecl(f)];
        let map = build_source_map(&decls, rust, Path::new("test.buff"), buff);
        assert_eq!(map.functions.len(), 0, "extern funcs should be skipped");
    }

    #[test]
    fn build_source_map_handles_missing_fn_in_rust_gracefully() {
        let buff = "func ghost():\n    return 1\n";
        let rust = "fn other() {}\n";
        let decls = vec![Decl::FuncDecl(make_func_with_span("ghost", 0, 10))];
        let map = build_source_map(&decls, rust, Path::new("test.buff"), buff);
        assert_eq!(map.functions.len(), 0, "no match in Rust → skip silently");
    }

    #[test]
    fn build_source_map_records_buff_path_and_source_id() {
        let buff = "func main():\n    print(1)\n";
        let rust = "fn main() {\n    println!(\"{}\", 1)\n}\n";
        let decls = vec![Decl::FuncDecl(make_func_with_span("main", 0, buff.len()))];
        let path = Path::new("examples/debug/panic_demo.buff");
        let map = build_source_map(&decls, rust, path, buff);
        assert_eq!(
            map.buff_file
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            Some("examples/debug/panic_demo.buff".to_string())
        );
        assert_eq!(map.source_id, SourceId(0));
    }
}
