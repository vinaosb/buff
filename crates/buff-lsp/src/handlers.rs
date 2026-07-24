//! LSP request handlers — pure functions on [`DocumentState`].
//!
//! Every handler takes a `&DocumentState` (plus request params) and returns
//! the matching LSP response type. There is no I/O here — the [`server`]
//! module is responsible for transport. This split keeps the handlers
//! trivially unit-testable: drive `analyze::analyze` →
//! [`DocumentState::new`] → call a handler → assert on the response.

use buff_lang_ast::{Attribute, Decl, FuncDecl};
use buff_lang_error::{Applicability, Diagnostic, Severity};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CompletionItem, CompletionItemKind,
    CompletionResponse, DocumentSymbol, GotoDefinitionResponse, Hover, HoverContents,
    InsertTextFormat, Location, MarkupContent, MarkupKind, Position, Range, SymbolKind, TextEdit,
    WorkspaceEdit,
};
use std::collections::BTreeMap;

use crate::analysis::DocumentAnalysis;
use crate::state::DocumentState;
use crate::symbol::{CompletionItemKind as BuffCompletionKind, TopDeclKind};

// ---------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------

/// Convert the analysis's diagnostics into the LSP wire format.
pub fn diagnostics(state: &DocumentState) -> Vec<lsp_types::Diagnostic> {
    state
        .analysis
        .diagnostics
        .iter()
        .map(|d| diagnostic_to_lsp(d, &state.text, &state.lines))
        .collect()
}

fn diagnostic_to_lsp(
    d: &Diagnostic,
    src: &str,
    lines: &crate::position::LineIndex,
) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic::new(
        lines.lsp_range(src, d.span),
        Some(severity_to_lsp(d.severity)),
        None,
        Some("buff".to_string()),
        d.message.clone(),
        None,
        None,
    )
}

fn severity_to_lsp(s: Severity) -> lsp_types::DiagnosticSeverity {
    match s {
        Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
        Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
        Severity::Info => lsp_types::DiagnosticSeverity::INFORMATION,
    }
}

// ---------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------

/// Compute hover info at `position`. Returns `None` when no symbol or type
/// can be reported.
pub fn hover(state: &DocumentState, position: Position) -> Option<Hover> {
    let byte = state.lines.byte_offset(&state.text, position);
    let analysis = &state.analysis;

    // Try the type index first. The cursor usually sits ON an identifier;
    // walk back up to 4 bytes to find its start.
    let mut ty = analysis.types.lookup_covering(byte);
    if ty.is_none() {
        for back in 0..=4.min(byte) {
            if let Some(t) = analysis.types.lookup(byte - back) {
                ty = Some(t);
                break;
            }
        }
    }

    // Symbol name + kind for context.
    let name_and_kind = symbol_at(state, byte);

    let mut lines: Vec<String> = Vec::new();
    if let Some((name, kind)) = name_and_kind.as_ref() {
        lines.push(format!("**{name}** _{kind}_"));
    }
    if let Some(t) = ty {
        lines.push(format!("type: `{}`", display_type(t)));
    }
    if let Some(top) = top_decl_containing(state, byte) {
        if !lines.iter().any(|l| l.contains(&top.name)) {
            lines.push(format!("declared in `{}`", top.name));
        }
    }

    // T45: --explain dispatch info in hover. When the cursor is inside a
    // function, surface what the heterogeneous runtime WOULD do: the
    // `@prefer(...)` hint (if any), the CPU/GPU routing bands, and the
    // GPU-dispatch overhead threshold. Mirrors the `BUFF_EXPLAIN_DISPATCH=1`
    // runtime diagnostic that `buff run --explain` prints at execution time,
    // but computed statically from the AST so the user sees it on hover
    // WITHOUT running the program. Constants are inlined (NOT imported from
    // `buff-lang-runtime`) so the LSP stays decoupled from the heavy
    // wgpu/rayon/tokio runtime crate — see T45 spec ("quick" task; do not
    // add deps").
    if let Some(explain) = dispatch_explain_for(state, byte) {
        lines.push(explain);
    }

    // T72: LSP plugin dispatch. Calls into the global plugin registry
    // (env-var-loaded via BUFF_PLUGIN_DIR / BUFF_PLUGIN_PATH). Empty
    // registry → Ok(None) → no-op. When a plugin returns hover
    // content, it's APPENDED to the built-in lines (so plugin docs
    // augment, never replace, the built-in type/symbol info).
    if let Some(plugin_hover) = plugin_hover_for(state, position) {
        lines.push(plugin_hover);
    }

    if lines.is_empty() {
        None
    } else {
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: lines.join("\n\n"),
            }),
            range: Some(Range::new(position, position)),
        })
    }
}

/// T72: Call the global plugin registry's `hover` dispatch and
/// return the content if any plugin provides hover info. Empty
/// registry → `None` (pure no-op).
///
/// Uses a synthetic URI derived from `source_id` because the
/// hover handler's signature takes `&DocumentState` (no direct
/// URI). The synthetic form `buff://source-{id}` is stable per
/// document (source_id is the canonical per-document identifier
/// in buff-lsp) so a plugin that wants to discriminate per file
/// can match on the trailing id.
fn plugin_hover_for(state: &DocumentState, position: Position) -> Option<String> {
    let uri = format!("buff://source-{}", state.source_id.0);
    let cursor = buff_plugins::PluginPosition::new(position.line, position.character);
    match buff_plugins::dispatch_global_lsp_hover(&uri, cursor) {
        Ok(Some(hover)) => Some(hover.content),
        _ => None,
    }
}

// ---------------------------------------------------------------------
// Code actions (T72 plugin hook + T1 suggestion → CodeAction conversion)
// ---------------------------------------------------------------------

/// Compute code actions at `position`. Returns the list of actions from
/// two sources (merged, suggestion-derived first):
///
/// 1. **Diagnostic fix suggestions** (T1, v1.25 Wave 0) — every
///    [`buff_lang_error::CodeSuggestion`] attached to a diagnostic whose
///    primary span contains `position` becomes a `quickfix` CodeAction
///    with a `WorkspaceEdit` that applies the replacement. Actions with
///    [`Applicability::MachineApplicable`] are marked `is_preferred` so
///    VSCode / Neovim auto-highlight them.
/// 2. **LSP plugins** (T72) — the global plugin registry's
///    `dispatch_global_lsp_code_actions` hook. Empty registry → no
///    actions from this source.
///
/// Returns `None` when both sources are empty (the pre-T1 / pre-T72
/// behavior — the LSP server reports "no code actions" to the editor).
///
/// **Light touch**: this adds the suggestion → CodeAction CONVERSION only.
/// No new LSP capabilities are registered in `server.rs`; the server's
/// existing code-action dispatch (when wired) picks this up automatically.
pub fn code_actions(state: &DocumentState, position: Position) -> Option<Vec<CodeActionOrCommand>> {
    let suggestion_actions = suggestions_to_code_actions(state, position);
    let plugin_actions = plugin_code_actions(state, position);

    if suggestion_actions.is_empty() && plugin_actions.is_empty() {
        return None;
    }

    let mut out: Vec<CodeActionOrCommand> = Vec::with_capacity(
        suggestion_actions.len() + plugin_actions.len(),
    );
    // Suggestion-derived quickfixes first (most actionable), then plugin
    // actions. Editors typically show them in declaration order.
    out.extend(suggestion_actions);
    out.extend(plugin_actions);
    Some(out)
}

/// Convert [`Diagnostic::suggestions`] on diagnostics whose primary span
/// contains `position` into LSP [`CodeAction`]s (T1, v1.25 Wave 0).
///
/// Each suggestion becomes a `quickfix` CodeAction with:
///
/// - `title` — the suggestion's label, or `"Apply suggestion: <replacement>"`
///   when no label is attached.
/// - `kind` — [`CodeActionKind::QUICK_FIX`].
/// - `is_preferred` — `Some(true)` when [`Applicability::MachineApplicable`]
///   (so editors auto-highlight the safe fix).
/// - `edit` — a [`WorkspaceEdit`] with one [`TextEdit`] replacing the
///   suggestion's byte span with `replacement`. The edit is keyed by the
///   synthesized `buff://source-{id}` URI (same form the plugin hook
///   uses); the LSP server substitutes the real document URI when it
///   forwards the response.
///
/// Returns an empty `Vec` when no diagnostic at `position` carries
/// suggestions (the common case — most diagnostics have no fix yet).
fn suggestions_to_code_actions(
    state: &DocumentState,
    position: Position,
) -> Vec<CodeActionOrCommand> {
    let byte = state.lines.byte_offset(&state.text, position);
    let uri: lsp_types::Uri = format!("buff://source-{}", state.source_id.0)
        .parse()
        .unwrap_or_else(|_| "buff://unknown".parse().unwrap_or_default());

    let mut out: Vec<CodeActionOrCommand> = Vec::new();
    for diag in &state.analysis.diagnostics {
        // Only surface suggestions for diagnostics whose primary span
        // contains the cursor. (Suggestions on the same diagnostic but
        // at a different span still qualify — the diagnostic IS the
        // anchor; the suggestion's own span is in the TextEdit.)
        if !span_contains(diag.span, byte) {
            continue;
        }
        for suggestion in &diag.suggestions {
            let range = state.lines.lsp_range(&state.text, suggestion.span);
            let edit = TextEdit {
                range,
                new_text: suggestion.replacement.clone(),
            };
            let mut changes: BTreeMap<lsp_types::Uri, Vec<TextEdit>> = BTreeMap::new();
            changes.insert(uri.clone(), vec![edit]);
            let workspace_edit = WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            };
            let title = suggestion
                .label
                .clone()
                .unwrap_or_else(|| format!("Apply suggestion: `{}`", suggestion.replacement));
            let is_preferred = matches!(
                suggestion.applicability,
                Applicability::MachineApplicable
            );
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICK_FIX),
                command: None,
                is_preferred: if is_preferred { Some(true) } else { None },
                disabled: None,
                data: Some(serde_json::Value::String(
                    suggestion.applicability.to_string(),
                )),
                edit: Some(workspace_edit),
            }));
        }
    }
    out
}

/// T72: Call the global plugin registry's code-action dispatch and
/// convert the results to LSP wire types. Returns the actions or an
/// empty `Vec` when no plugins are registered / no plugin fires.
fn plugin_code_actions(state: &DocumentState, position: Position) -> Vec<CodeActionOrCommand> {
    let uri = format!("buff://source-{}", state.source_id.0);
    let cursor = buff_plugins::PluginPosition::new(position.line, position.character);
    let plugin_actions = buff_plugins::dispatch_global_lsp_code_actions(&uri, cursor);
    plugin_actions
        .into_iter()
        .map(|a| {
            CodeActionOrCommand::CodeAction(CodeAction {
                title: a.title,
                kind: a.kind.map(CodeActionKind::from),
                ..Default::default()
            })
        })
        .collect()
}

/// `true` when `byte` lies within `[span.start, span.end)` (inclusive of
/// start, exclusive of end — the standard half-open range).
fn span_contains(span: buff_lang_error::Span, byte: usize) -> bool {
    span.start <= byte && byte < span.end
}

/// Render a [`buff_lang_types::Type`] for hover. Defaults collapse to their
/// bare name (`Int<64>` → `Int`) so the hover shows the user-friendly form
/// matching the language's annotation surface.
fn display_type(ty: &buff_lang_types::Type) -> String {
    use buff_lang_types::{FloatWidth, IntWidth, Type};
    match ty {
        Type::Int {
            width: IntWidth::W64,
        } => "Int".to_string(),
        Type::Float {
            width: FloatWidth::W32,
        } => "Float".to_string(),
        other => other.to_string(),
    }
}

/// Find the smallest enclosing identifier at `byte`. Returns `(name, kind)`
/// if a known symbol exists there.
fn symbol_at(state: &DocumentState, byte: usize) -> Option<(String, String)> {
    let analysis = &state.analysis;
    for back in 0..=32.min(byte) {
        let b = byte - back;
        if let Some(local) = analysis.symbols.find_local_at(b) {
            return Some((local.name.name.clone(), format!("{:?}", local.kind)));
        }
        if let Some(top) = analysis
            .symbols
            .top_decls
            .iter()
            .find(|d| d.name_span.start == b)
        {
            return Some((top.name.clone(), format!("{:?}", top.kind)));
        }
    }
    None
}

/// Find the top-level decl whose `def_span` contains `byte`.
fn top_decl_containing(state: &DocumentState, byte: usize) -> Option<crate::symbol::TopDeclEntry> {
    state
        .analysis
        .symbols
        .top_decls
        .iter()
        .find(|d| d.span.start <= byte && byte < d.span.end)
        .cloned()
}

/// T45: Build the `--explain` dispatch info string for the function whose
/// span contains `byte`. Returns `None` when the cursor is not inside a
/// function (so non-function hovers are unchanged) or when the function
/// has no `@prefer(...)` hint AND nothing noteworthy to say (kept minimal
/// to avoid hover spam on every plain function — see T45 spec).
///
/// When the cursor IS inside a function, the returned string is a single
/// markdown block summarising:
///
/// 1. The `@prefer(gpu)` / `@prefer(npu)` / no-hint disposition.
/// 2. The runtime's CPU/GPU routing bands (the same thresholds
///    `buff-lang-runtime::threshold::decide` uses, inlined here as
///    constants — see file-level doc on why we don't import the runtime
///    crate).
/// 3. The lowered GPU threshold when a `@prefer(gpu)` hint is present
///    (mirrors `PREFER_GPU_MIN_ELEMENTS` from the runtime's `hints` module).
///
/// **Why static-only?** The LSP cannot run the user's program, so the
/// actual element count at the dispatch site is unknown at hover time.
/// We surface the DECISION RULE — the user can then reason "my array is
/// 10k elements → CPU parallel" without executing. For the live decision
/// on a real run, `buff run --explain` prints it.
fn dispatch_explain_for(state: &DocumentState, byte: usize) -> Option<String> {
    let func = enclosing_func(state, byte)?;
    let prefer = prefer_hint(&func);

    // Header line — always present when we have a function.
    let header = match &prefer {
        Some(h) => format!("⚙️ **Dispatch** — hint: `{h}`"),
        None => "⚙️ **Dispatch** — no hint (runtime decides by element count)".to_string(),
    };

    // Threshold table. These are the SAME constants the runtime uses
    // (`SINGLE_THREAD_MAX = 999`, `CPU_PARALLEL_MAX = 50_000`,
    //  `PREFER_GPU_MIN_ELEMENTS = 1024`) — duplicated here to keep the LSP
    // decoupled from `buff-lang-runtime` (which pulls wgpu+rayon+tokio).
    // If the runtime constants ever change, this table needs the same bump.
    let bands = "| elements | backend |\n|---|---|\n\
        | < 1000 | single-thread CPU |\n\
        | 1000–50 000 | parallel CPU (rayon) |\n\
        | > 50 000 | GPU (wgpu), when available + fits VRAM |";

    let note = match &prefer {
        Some(h) if h.contains("gpu") => {
            "\n\nWith `@prefer(gpu)`, the GPU band opens at **≥ 1024 elements** \
             (overrides cost model when a GPU is present)."
        }
        Some(_) => {
            "\n\n`@prefer(npu)` is reserved — currently routes through the \
             unhinted cost model."
        }
        None => "",
    };

    Some(format!("{header}\n\n```\n{bands}\n```{note}"))
}

/// Find the [`FuncDecl`] whose `span` contains `byte`, or `None`.
///
/// Walks the parsed top-level decls (kept in [`DocumentAnalysis::decls`])
/// and returns the first [`Decl::FuncDecl`] whose span covers the cursor.
/// Returns the raw AST node (not the [`TopDeclEntry`](crate::symbol::TopDeclEntry)
/// summary) so we can read `@prefer(...)` attributes.
fn enclosing_func<'a>(state: &'a DocumentState, byte: usize) -> Option<&'a FuncDecl> {
    for decl in &state.analysis.decls {
        if let Decl::FuncDecl(f) = decl {
            if f.span.start <= byte && byte < f.span.end {
                return Some(f);
            }
        }
        // `export func …` wraps a FuncDecl — its outer span also covers
        // the body, so check the inner func too.
        if let Decl::ExportDecl(exp) = decl {
            if let Decl::FuncDecl(f) = exp.inner.as_ref() {
                if exp.span.start <= byte && byte < exp.span.end {
                    return Some(f);
                }
            }
        }
    }
    None
}

/// Extract the `@prefer(...)` hint from a function's attributes. Returns
/// the rendered hint string (e.g. `"@prefer(gpu)"`) or `None` when the
/// function has no `@prefer` attribute.
///
/// Mirrors the parsing the runtime's `prefer_from_name_args` would do —
/// kept local so the LSP doesn't depend on the runtime crate.
fn prefer_hint(f: &FuncDecl) -> Option<String> {
    let prefer: Option<&Attribute> = f
        .attributes
        .iter()
        .find(|a| a.name.name == "prefer");
    let attr = prefer?;
    if attr.args.is_empty() {
        return Some("@prefer".to_string());
    }
    Some(format!(
        "@prefer({})",
        attr.args.iter().cloned().collect::<Vec<_>>().join(", ")
    ))
}

// ---------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------

/// Compute completion candidates for `position`. Returns `None` when no
/// candidates apply (rare — there's always SOMETHING in scope).
pub fn completion(state: &DocumentState, _position: Position) -> Option<CompletionResponse> {
    let cands = state.analysis.symbols.completions();
    if cands.is_empty() {
        return None;
    }
    let items: Vec<CompletionItem> = cands
        .into_iter()
        .map(|c| CompletionItem {
            label: c.label,
            kind: Some(completion_kind(c.kind)),
            detail: c.detail,
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            ..Default::default()
        })
        .collect();
    Some(CompletionResponse::Array(items))
}

fn completion_kind(k: BuffCompletionKind) -> CompletionItemKind {
    match k {
        BuffCompletionKind::Function => CompletionItemKind::FUNCTION,
        BuffCompletionKind::Struct => CompletionItemKind::STRUCT,
        BuffCompletionKind::Enum => CompletionItemKind::ENUM,
        BuffCompletionKind::Variable => CompletionItemKind::VARIABLE,
        BuffCompletionKind::Field => CompletionItemKind::FIELD,
    }
}

// ---------------------------------------------------------------------
// Goto definition (single-file)
// ---------------------------------------------------------------------

/// Resolve a goto-definition request within the same file.
///
/// Returns `None` if no in-file definition is found. Cross-file goto-def
/// is a v2.0 feature.
pub fn goto_definition(
    state: &DocumentState,
    uri: &lsp_types::Uri,
    position: Position,
) -> Option<GotoDefinitionResponse> {
    let byte = state.lines.byte_offset(&state.text, position);
    let analysis = &state.analysis;

    // Primary approach: read the identifier that CONTAINS the cursor byte.
    // This handles the common case (cursor sits on an identifier token).
    // Falls back to walking back a few bytes for cursors just past an
    // identifier end (e.g. immediately after the last char).
    let name = read_ident_at_cursor(state, byte).or_else(|| {
        for back in 1..=4.min(byte) {
            if let Some(n) = read_ident_at_cursor(state, byte - back) {
                return Some(n);
            }
        }
        None
    })?;

    // Try local bindings first (params / lets) — they shadow top-level
    // decls in lexical order.
    if let Some(local) = analysis.symbols.find_local_named(&name) {
        return Some(GotoDefinitionResponse::Scalar(Location {
            uri: uri.clone(),
            range: state.lines.lsp_range(&state.text, local.def_span),
        }));
    }

    // Then top-level decls.
    if let Some(top) = analysis.symbols.find_top_decl(&name) {
        return Some(GotoDefinitionResponse::Scalar(Location {
            uri: uri.clone(),
            range: state.lines.lsp_range(&state.text, top.name_span),
        }));
    }

    None
}

/// Read an identifier from `src` starting at `byte`.
fn read_ident(src: &str, byte: usize) -> Option<String> {
    let bytes = src.as_bytes();
    if byte >= bytes.len() {
        return None;
    }
    if !bytes[byte].is_ascii_alphabetic() && bytes[byte] != b'_' {
        return None;
    }
    let mut end = byte;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    Some(src[byte..end].to_string())
}

/// Read whatever identifier touches `byte` (cursor sits inside or just
/// after). Falls back to scanning the surrounding 32 bytes.
fn read_ident_at_cursor(state: &DocumentState, byte: usize) -> Option<String> {
    let bytes = state.text.as_bytes();
    let mut start = byte.min(bytes.len().saturating_sub(1));
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    if start < bytes.len() && (bytes[start].is_ascii_alphabetic() || bytes[start] == b'_') {
        return read_ident(&state.text, start);
    }
    None
}

// ---------------------------------------------------------------------
// Document symbols
// ---------------------------------------------------------------------

/// Build the document-symbol outline (functions, structs, enums).
pub fn document_symbols(state: &DocumentState) -> Vec<DocumentSymbol> {
    state
        .analysis
        .symbols
        .top_decls
        .iter()
        .filter_map(|d| {
            let kind = top_decl_symbol_kind(d.kind)?;
            let range = state.lines.lsp_range(&state.text, d.span);
            let selection = state.lines.lsp_range(&state.text, d.name_span);
            // The `deprecated` field on `DocumentSymbol` is marked deprecated
            // by lsp-types ("Use tags instead"). We always set it to `None`
            // (no deprecated symbols surfaced by v1.2 LSP). Narrow allow so
            // the lint stays on for the rest of the crate.
            #[allow(deprecated)]
            let symbol = DocumentSymbol {
                name: d.name.clone(),
                detail: Some(d.detail.clone()),
                kind,
                tags: None,
                deprecated: None,
                range,
                selection_range: selection,
                children: None,
            };
            Some(symbol)
        })
        .collect()
}

fn top_decl_symbol_kind(kind: TopDeclKind) -> Option<SymbolKind> {
    match kind {
        TopDeclKind::Func | TopDeclKind::Export => Some(SymbolKind::FUNCTION),
        TopDeclKind::Struct => Some(SymbolKind::STRUCT),
        TopDeclKind::Enum => Some(SymbolKind::ENUM),
        TopDeclKind::Trait => Some(SymbolKind::INTERFACE),
        TopDeclKind::Extend => Some(SymbolKind::OBJECT),
        // Imports aren't outline-worthy in v1.2 (they clutter the view).
        TopDeclKind::Import | TopDeclKind::Other => None,
    }
}

// ---------------------------------------------------------------------
// Formatting (textDocument/formatting)
// ---------------------------------------------------------------------

/// Format the entire document via `buff fmt`. Returns one `TextEdit`
/// spanning the whole document with the canonical-formatted source, or
/// `None` if the file already parses identically (no edits needed).
///
/// v1.2 routes through `buff_lang_cli::fmt::format_source` so the LSP's
/// formatter is byte-identical to the CLI's. NO reimplementation.
pub fn formatting(state: &DocumentState) -> Option<Vec<TextEdit>> {
    let canonical = match buff_lang_cli::fmt::format_source(&state.text) {
        Ok(s) => s,
        // Parse / lex errors mean we can't safely reformat. Skip — the
        // diagnostics will already have surfaced the underlying problem.
        Err(_) => return None,
    };
    if canonical == state.text {
        return None;
    }
    let last_line = state.lines.line_count().saturating_sub(1) as u32;
    let last_col = state
        .text
        .lines()
        .last()
        .map(|l| l.chars().count() as u32)
        .unwrap_or(0);
    let full_range = Range::new(Position::new(0, 0), Position::new(last_line, last_col));
    Some(vec![TextEdit {
        range: full_range,
        new_text: canonical,
    }])
}

// ---------------------------------------------------------------------
// Misc helpers
// ---------------------------------------------------------------------

/// Re-export of [`DocumentAnalysis`] for callers that want the raw type.
pub fn analysis_of(state: &DocumentState) -> &DocumentAnalysis {
    &state.analysis
}

/// Convert `(line, character)` to a [`Position`] (test ergonomics).
pub fn pos(line: u32, character: u32) -> Position {
    Position::new(line, character)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::SourceId;

    fn open(src: &str) -> DocumentState {
        DocumentState::new(src.to_string(), SourceId(0), None)
    }

    #[test]
    fn diagnostics_clean_for_clean_source() {
        let st = open("func main():\n    print(\"hi\")\n");
        assert!(diagnostics(&st).is_empty());
    }

    #[test]
    fn diagnostics_type_mismatch_emitted() {
        let st = open("func main():\n    let x: Int = \"hi\"\n");
        let diags = diagnostics(&st);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Some(lsp_types::DiagnosticSeverity::ERROR))
            .collect();
        assert!(!errors.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn hover_let_binding_shows_int() {
        let st = open("func main():\n    let x = 42\n    print(x)\n");
        // Position of the `x` in `let x = 42` (line 1, col 8).
        let h = hover(&st, pos(1, 8)).expect("hover at x binding");
        let s = match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(s.contains("Int"), "expected type Int, got: {s}");
    }

    #[test]
    fn hover_reference_shows_int() {
        let st = open("func main():\n    let x = 42\n    print(x)\n");
        // Find the byte offset of the second `x` (inside print(x)).
        let byte = st.text.rfind("x").unwrap();
        let p = st.lines.lsp_position(&st.text, byte);
        let h = hover(&st, p).expect("hover at x reference");
        let s = match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(s.contains("Int"), "expected type Int, got: {s}");
    }

    #[test]
    fn completion_offers_params_and_func() {
        // Typed params required by Buff grammar (`name: Type`, not bare `name`).
        let st = open("func add(a: Int, b: Int) -> Int:\n    return a + b\n");
        let c = completion(&st, pos(1, 100)).expect("completion");
        let labels: Vec<String> = match c {
            CompletionResponse::Array(items) => items.into_iter().map(|i| i.label).collect(),
            _ => panic!("expected array"),
        };
        assert!(labels.contains(&"a".to_string()), "labels: {labels:?}");
        assert!(labels.contains(&"b".to_string()), "labels: {labels:?}");
        assert!(labels.contains(&"add".to_string()), "labels: {labels:?}");
    }

    #[test]
    fn goto_def_local_navigates_to_let() {
        let st = open("func main():\n    let x = 42\n    print(x)\n");
        let uri: lsp_types::Uri = "file:///test.buff".parse().unwrap();
        // Position of `x` inside `print(x)`.
        let byte = st.text.rfind("x").unwrap();
        let p = st.lines.lsp_position(&st.text, byte);
        let resp = goto_definition(&st, &uri, p).expect("goto-def");
        match resp {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.range.start.line, 1, "expected line 1 for let x");
            }
            _ => panic!("expected scalar, got {resp:?}"),
        }
    }

    #[test]
    fn goto_def_top_func_navigates_to_name() {
        let src = "func add(a: Int, b: Int) -> Int:\n    return a + b\n\nfunc main():\n    return add(1, 2)\n";
        let st = open(src);
        let uri: lsp_types::Uri = "file:///test.buff".parse().unwrap();
        // Position of `add` inside the call (second occurrence).
        let byte = src.rfind("add").unwrap();
        let p = st.lines.lsp_position(&st.text, byte);
        let resp = goto_definition(&st, &uri, p).expect("goto-def");
        match resp {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.range.start.line, 0, "expected line 0 for func add");
            }
            _ => panic!("expected scalar, got {resp:?}"),
        }
    }

    #[test]
    fn document_symbols_lists_funcs_structs_enums() {
        let src = "func a():\n    print(1)\n";
        let st = open(src);
        let syms = document_symbols(&st);
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "a");
    }

    #[test]
    fn formatting_canonicalizes_unformatted_source() {
        // Trailing whitespace + missing trailing newline.
        let src = "func main():   \n    print(\"hi\")   \n";
        let st = open(src);
        let edits = formatting(&st).expect("expected formatting edits");
        assert_eq!(edits.len(), 1);
        // The canonical form should NOT contain trailing whitespace.
        assert!(!edits[0].new_text.contains("   \n"));
        assert!(edits[0].new_text.ends_with('\n'));
    }

    #[test]
    fn formatting_noop_when_already_canonical() {
        // Run format_source once to get a canonical fixture.
        let raw = "func main():\n    print(\"hi\")\n";
        let canonical = buff_lang_cli::fmt::format_source(raw).unwrap();
        let st = open(&canonical);
        assert!(formatting(&st).is_none(), "expected no edits");
    }

    // -----------------------------------------------------------------
    // T45: --explain dispatch info in hover
    // -----------------------------------------------------------------

    #[test]
    fn t45_hover_inside_function_shows_dispatch_info() {
        // Plain function — no @prefer hint. Cursor in the body.
        let st = open("func main():\n    let x = 42\n    print(x)\n");
        let h = hover(&st, pos(1, 8)).expect("hover inside function body");
        let s = match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(
            s.contains("Dispatch"),
            "expected Dispatch section, got: {s}"
        );
        // Bands table should be present.
        assert!(s.contains("CPU") && s.contains("GPU"), "missing bands: {s}");
    }

    #[test]
    fn t45_hover_on_prefer_gpu_function_mentions_lowered_threshold() {
        // Function with @prefer(gpu) hint.
        let src = "@prefer(gpu)\nfunc kernel(data: Vector<Float>):\n    return data.map({ x => x * 2.0 })\n";
        let st = open(src);
        // Cursor on the function name line (line 1, the `f` of `func`).
        let h = hover(&st, pos(1, 4)).expect("hover on @prefer(gpu) function");
        let s = match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(s.contains("@prefer(gpu)"), "missing hint: {s}");
        assert!(
            s.contains("1024"),
            "expected lowered GPU threshold (1024) mentioned, got: {s}"
        );
    }

    #[test]
    fn t45_hover_on_prefer_npu_function_mentions_reserved_routing() {
        let src = "@prefer(npu)\nfunc infer(x: Tensor<Float>):\n    return x\n";
        let st = open(src);
        let h = hover(&st, pos(1, 4)).expect("hover on @prefer(npu) function");
        let s = match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(s.contains("@prefer(npu)"), "missing hint: {s}");
        assert!(
            s.contains("reserved"),
            "expected NPU-reserved note, got: {s}"
        );
    }
}
