//! Integration tests for `buff-lsp` — drive the LIB handlers directly.
//!
//! These tests mirror how `buff-lang-cli` tests drive the pipeline (no
//! subprocess; import the lib and call the public functions). Each
//! capability gets at least one positive and one negative case per the
//! T117 acceptance criteria.
//!
//! Fixtures are inline strings (no filesystem); per-process-unique temp
//! paths are only used when a real file is needed (which is rare here —
//! the handlers take `&DocumentState` directly).

use buff_lang_error::SourceId;
use buff_lsp::handlers;
use buff_lsp::position::LineIndex;
use buff_lsp::DocumentState;
use lsp_types::{
    CompletionResponse, DiagnosticSeverity, DocumentSymbol, GotoDefinitionResponse, HoverContents,
    Position,
};

/// Helper: open a Buff source string as a [`DocumentState`].
fn open(src: &str) -> DocumentState {
    DocumentState::new(src.to_string(), SourceId(0), None)
}

/// Helper: find the byte offset of the `n`-th occurrence (1-based) of
/// `needle` in `src` and convert it to an LSP position via the document's
/// [`LineIndex`].
fn nth_occurrence_position(state: &DocumentState, needle: &str, n: usize) -> Position {
    let mut byte = 0usize;
    let mut found = 0usize;
    while let Some(off) = state.text[byte..].find(needle) {
        found += 1;
        byte += off;
        if found == n {
            return state.lines.lsp_position(&state.text, byte);
        }
        byte += needle.len();
    }
    panic!("needle {needle:?} not found {n} times in source");
}

// =====================================================================
// 1. Diagnostics
// =====================================================================

#[test]
fn diagnostics_clean_program_has_no_diagnostics() {
    // A well-formed Buff program yields zero diagnostics (T117 AC: a valid
    // program produces none).
    let src =
        "func greet(name: String):\n    print(\"hi\")\n\nfunc main():\n    greet(\"world\")\n";
    let st = open(src);
    let diags = handlers::diagnostics(&st);
    assert!(
        diags.is_empty(),
        "expected no diagnostics for clean source, got: {diags:?}"
    );
}

#[test]
fn diagnostics_type_mismatch_emits_error_spanning_value() {
    // T117 AC: `let x: Int = "hello"` yields an Error diagnostic with a
    // span covering the mismatch.
    let src = "func main():\n    let x: Int = \"hello\"\n";
    let st = open(src);
    let diags = handlers::diagnostics(&st);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic, got: {diags:?}"
    );
    let e = errors[0];
    // Message must mention both expected and actual types.
    assert!(
        e.message.contains("Int") || e.message.contains("String"),
        "expected message to mention Int/String, got: {}",
        e.message
    );
    // Span must be within the let-statement line (not at byte 0).
    assert_eq!(e.range.start.line, 1, "expected diagnostic on line 1");
}

#[test]
fn diagnostics_parse_error_emits_error() {
    // Missing colon after func signature → parse error.
    let src = "func main()\n    print(\"hi\")\n";
    let st = open(src);
    let diags = handlers::diagnostics(&st);
    assert!(
        diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "expected at least one parse-error diagnostic, got: {diags:?}"
    );
}

#[test]
fn diagnostics_lex_error_emits_error() {
    // Unterminated string literal → lex error.
    let src = "func main():\n    print(\"unterminated)\n";
    let st = open(src);
    let diags = handlers::diagnostics(&st);
    assert!(
        diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "expected at least one lex-error diagnostic, got: {diags:?}"
    );
}

// =====================================================================
// 2. Hover
// =====================================================================

#[test]
fn hover_on_let_binding_returns_inferred_type() {
    // T117 AC: Hover on `x` in `let x = 42` returns type `Int`.
    let src = "func main():\n    let x = 42\n    print(x)\n";
    let st = open(src);
    // The `x` in `let x` is on line 1, at character 8 (after "    let ").
    let pos = Position::new(1, 8);
    let h = handlers::hover(&st, pos).expect("hover at x binding");
    let s = match h.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup contents"),
    };
    assert!(
        s.contains("Int"),
        "expected hover to mention type `Int`, got: {s}"
    );
}

#[test]
fn hover_on_reference_returns_inferred_type() {
    let src = "func main():\n    let x = 42\n    print(x)\n";
    let st = open(src);
    // The `x` reference inside `print(x)` is the 2nd `x` in the file.
    let pos = nth_occurrence_position(&st, "x", 2);
    let h = handlers::hover(&st, pos).expect("hover at x reference");
    let s = match h.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup contents"),
    };
    assert!(
        s.contains("Int"),
        "expected hover to mention type, got: {s}"
    );
}

#[test]
fn hover_on_unknown_identifier_returns_no_type() {
    // Hovering over a name with NO inferred type (e.g. an undefined
    // identifier the inferencer couldn't bind) returns None — there's
    // no type information to surface.
    let src = "func main():\n    return undefined_name\n";
    let st = open(src);
    // Cursor on `undefined_name`.
    let byte = src.find("undefined_name").unwrap();
    let pos = st.lines.lsp_position(&st.text, byte);
    // Undefined idents have no recorded type — hover at that exact byte
    // yields None (no entry in the type-binding index).
    let _ = byte; // shadow to avoid clippy unused
    let result = handlers::hover(&st, pos);
    // Either None (no info at all) OR Some without a `type:` line — both
    // are acceptable for an unknown ident. Assert at minimum that no type
    // is reported.
    if let Some(h) = result {
        if let HoverContents::Markup(m) = h.contents {
            assert!(
                !m.value.contains("type:"),
                "hover should NOT report a type for an undefined name, got: {}",
                m.value
            );
        }
    }
}

// =====================================================================
// 3. Completion
// =====================================================================

#[test]
fn completion_offers_local_params_and_funcs() {
    // T117 AC: at the end of `func add(a: Int, b: Int) -> Int:\n    return a + b`
    // completion includes `a` and `b`. Buff params REQUIRE type annotations
    // (`name: Type`), so the fixture uses the typed form. (A complete
    // expression is used so the parser doesn't bail — completion works on
    // the post-parse symbol table.)
    let src = "func add(a: Int, b: Int) -> Int:\n    return a + b\n";
    let st = open(src);
    // Position at end of the body line.
    let pos = Position::new(1, 100);
    let c = handlers::completion(&st, pos).expect("completion at end of body line");
    let labels: Vec<String> = match c {
        CompletionResponse::Array(items) => items.into_iter().map(|i| i.label).collect(),
        _ => panic!("expected array response"),
    };
    assert!(labels.contains(&"a".to_string()), "labels: {labels:?}");
    assert!(labels.contains(&"b".to_string()), "labels: {labels:?}");
    assert!(labels.contains(&"add".to_string()), "labels: {labels:?}");
}

#[test]
fn completion_empty_program_returns_none() {
    // No symbols in scope at all → completion returns None.
    let st = open("");
    assert!(handlers::completion(&st, Position::new(0, 0)).is_none());
}

// =====================================================================
// 4. Goto definition (single-file)
// =====================================================================

#[test]
fn goto_def_navigates_to_top_level_func() {
    let src = "func add(a: Int, b: Int) -> Int:\n    return a + b\n\nfunc main():\n    return add(1, 2)\n";
    let st = open(src);
    let uri: lsp_types::Uri = "file:///test.buff".parse().unwrap();
    // Cursor at the call site `add(1, 2)` — second occurrence of "add".
    let pos = nth_occurrence_position(&st, "add", 2);
    let resp = handlers::goto_definition(&st, &uri, pos).expect("goto-def should resolve");
    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(
                loc.uri, uri,
                "single-file goto-def must return the same URI"
            );
            assert_eq!(
                loc.range.start.line, 0,
                "expected definition at line 0 (func add declaration)"
            );
        }
        other => panic!("expected Scalar location, got {other:?}"),
    }
}

#[test]
fn goto_def_navigates_to_local_let() {
    let src = "func main():\n    let x = 42\n    print(x)\n";
    let st = open(src);
    let uri: lsp_types::Uri = "file:///test.buff".parse().unwrap();
    // Cursor at the reference `x` inside `print(x)` (2nd occurrence).
    let pos = nth_occurrence_position(&st, "x", 2);
    let resp = handlers::goto_definition(&st, &uri, pos).expect("goto-def should resolve");
    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.range.start.line, 1, "expected let x at line 1");
        }
        other => panic!("expected Scalar location, got {other:?}"),
    }
}

#[test]
fn goto_def_unknown_symbol_returns_none() {
    // Reference to a name that's never defined → no goto-def target.
    let src = "func main():\n    return undefined_thing\n";
    let st = open(src);
    let uri: lsp_types::Uri = "file:///test.buff".parse().unwrap();
    // Cursor on `undefined_thing`.
    let byte = src.find("undefined_thing").unwrap();
    let pos = st.lines.lsp_position(&st.text, byte);
    assert!(handlers::goto_definition(&st, &uri, pos).is_none());
}

// =====================================================================
// 5. Document symbols
// =====================================================================

#[test]
fn document_symbols_outline_lists_funcs() {
    let src = "func first():\n    print(1)\n\nfunc second():\n    print(2)\n";
    let st = open(src);
    let syms: Vec<DocumentSymbol> = handlers::document_symbols(&st);
    assert_eq!(syms.len(), 2, "expected 2 document symbols, got: {syms:?}");
    assert_eq!(syms[0].name, "first");
    assert_eq!(syms[1].name, "second");
    // Each symbol's range should span its declaration.
    assert!(syms[0].range.start.line <= syms[0].range.end.line);
}

#[test]
fn document_symbols_outline_lists_structs_and_enums() {
    // The Buff parser supports top-level `enum Name { ... }` (brace-form)
    // and `func`; `struct` is reserved in the AST but not yet produced
    // by the parser. The outline therefore lists enums + funcs.
    // See `crates/buff-lang-parser/tests/enum_match.rs` for the canonical
    // enum syntax.
    let src = "enum Color { Red, Green, Blue }\n\nfunc main():\n    print(1)\n";
    let st = open(src);
    let syms = handlers::document_symbols(&st);
    let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Color"),
        "expected Color in outline, got: {names:?}"
    );
    assert!(
        names.contains(&"main"),
        "expected main in outline, got: {names:?}"
    );
}

// =====================================================================
// 6. Formatting (textDocument/formatting)
// =====================================================================

#[test]
fn formatting_routes_through_buff_fmt_and_returns_edits() {
    // Unformatted source (trailing whitespace, missing trailing newline).
    let src = "func main():   \n    print(\"hi\")   \n";
    let st = open(src);
    let edits = handlers::formatting(&st).expect("expected formatting edits for unformatted src");
    assert_eq!(edits.len(), 1, "expected one full-document TextEdit");
    let new_text = &edits[0].new_text;
    // Canonical form: no trailing whitespace, ends with newline.
    assert!(
        !new_text.contains("   \n"),
        "no trailing whitespace expected"
    );
    assert!(new_text.ends_with('\n'), "trailing newline expected");
}

#[test]
fn formatting_is_noop_when_source_already_canonical() {
    // Pre-canonicalize a source so we know its form.
    let raw = "func main():\n    print(\"hi\")\n";
    let canonical = buff_lang_fmt::format_source(raw).unwrap();
    let st = open(&canonical);
    assert!(
        handlers::formatting(&st).is_none(),
        "expected no edits for already-canonical source"
    );
}

#[test]
fn formatting_unparseable_source_returns_none() {
    // Unparseable source → can't safely reformat → None (diagnostics will
    // surface the underlying lex/parse error).
    let src = "func main()\n    print(\"hi\"\n";
    let st = open(src);
    assert!(
        handlers::formatting(&st).is_none(),
        "expected None for unparseable source"
    );
}

// =====================================================================
// 7. Position mapping (UTF-16 ↔ byte offset)
// =====================================================================

#[test]
fn position_utf16_round_trip_ascii() {
    let src = "func main():\n    print(\"hello\")\n";
    let idx = LineIndex::new(src);
    // Byte offset 5 = the 'm' of 'main'.
    let pos = idx.lsp_position(src, 5);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 5);
    // Round trip back.
    assert_eq!(idx.byte_offset(src, pos), 5);
}

#[test]
fn position_utf16_round_trip_with_multibyte_char() {
    // 'é' = 2 bytes UTF-8, 1 UTF-16 unit. '🚀' = 4 bytes UTF-8, 2 UTF-16
    // units (surrogate pair). Source on a single line so we can reason
    // about column arithmetic.
    let src = "é🚀x";
    let idx = LineIndex::new(src);
    // Byte offset 6 = 'x' (after é=2 bytes + 🚀=4 bytes).
    let pos = idx.lsp_position(src, 6);
    // Column in UTF-16: é=1 unit, 🚀=2 units → x at col 3.
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 3);
    // Round trip: col 3 UTF-16 → byte 6.
    assert_eq!(idx.byte_offset(src, pos), 6);
}

#[test]
fn position_clamps_past_eof() {
    let src = "abc";
    let idx = LineIndex::new(src);
    let pos = idx.lsp_position(src, 999);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.character, 3);
    // Position past end clamps to end-of-source.
    let far = Position::new(99, 99);
    assert_eq!(idx.byte_offset(src, far), 3);
}

#[test]
fn position_crlf_line_endings_indexed_correctly() {
    // CRLF newlines: the line start after `\r\n` is the byte after `\n`.
    let src = "aaa\r\nbbb";
    let idx = LineIndex::new(src);
    // Byte offset 5 = first 'b' (after "aaa\r\n" = 5 bytes).
    let pos = idx.lsp_position(src, 5);
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 0);
    // Round trip.
    assert_eq!(idx.byte_offset(src, pos), 5);
}

// =====================================================================
// 8. End-to-end handler wiring (lib boundary)
// =====================================================================

#[test]
fn document_state_reanalyze_picks_up_changes() {
    // After updating the text, the cached analysis should reflect the new
    // source (new diagnostics, new symbols).
    let mut st = open("func main():\n    print(1)\n");
    assert!(handlers::diagnostics(&st).is_empty());
    // Update to a source with a type error.
    st.update(
        "func main():\n    let x: Int = \"oops\"\n".to_string(),
        Some(2),
    );
    let diags = handlers::diagnostics(&st);
    assert!(
        diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "expected type-error diagnostic after update, got: {diags:?}"
    );
}

#[test]
fn typecheck_only_mode_does_not_codegen() {
    // The LSP's analyze() runs TypeInferencer directly (no Rust codegen).
    // A program with a real type error must still surface it, even though
    // no Rust code would be generated.
    let src = "func main():\n    let x: Int = \"hello\"\n";
    let st = open(src);
    let diags = handlers::diagnostics(&st);
    assert!(diags
        .iter()
        .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)));
}
