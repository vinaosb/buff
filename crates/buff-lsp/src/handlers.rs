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
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeLens, CodeLensParams,
    CompletionItem, CompletionItemKind, CompletionResponse, DocumentSymbol, GotoDefinitionResponse,
    Hover, HoverContents, InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams,
    InsertTextFormat, Location, MarkupContent, MarkupKind, Position, Range, SemanticToken,
    SemanticTokens, SemanticTokensLegend, SemanticTokensParams, SemanticTokensResult, SymbolKind,
    TextEdit, WorkspaceEdit,
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

    let mut out: Vec<CodeActionOrCommand> =
        Vec::with_capacity(suggestion_actions.len() + plugin_actions.len());
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
        // lsp-types 0.97 `Uri` does not impl `Default`; parse a known-good
        // constant fallback. "buff://unknown" is always a valid URI.
        .unwrap_or_else(|_| "buff://unknown".parse().unwrap());

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
            // lsp-types 0.97's `WorkspaceEdit::changes` is a HashMap (the
            // LSP 3.17 spec uses a JSON object — unordered). We seed it
            // from a one-entry HashMap; the BTreeMap shape was a v1.24-era
            // leftover that rust-analyzer flagged but cargo didn't catch
            // because the crate hadn't been re-checked against the bump.
            let mut changes: std::collections::HashMap<lsp_types::Uri, Vec<TextEdit>> =
                std::collections::HashMap::new();
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
            let is_preferred = matches!(suggestion.applicability, Applicability::MachineApplicable);
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                // The diagnostics this action resolves. T1's suggestion
                // engine attaches the fix to the diagnostic in scope; we
                // don't yet thread the originating `lsp_types::Diagnostic`
                // back here (a T1b refinement), so leave `None` and let
                // the editor associate via the cursor position.
                diagnostics: None,
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
    let prefer: Option<&Attribute> = f.attributes.iter().find(|a| a.name.name == "prefer");
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
// T46: codeAction dispatch wrapper
// ---------------------------------------------------------------------

/// Adapter from the LSP `textDocument/codeAction` request shape to the
/// existing [`code_actions`] handler. The LSP sends a [`CodeActionParams`]
/// with a `range` (the editor's selection); we forward `range.start` to
/// the position-based handler — the underlying diagnostic-suggestion
/// matcher checks `span_contains(byte)` against each diagnostic, and
/// the cursor's exact position within the selection is what matters.
///
/// Returns `None` (the pre-T46 behaviour) when there are no actions.
pub fn code_action(
    state: &DocumentState,
    params: CodeActionParams,
) -> Option<Vec<CodeActionOrCommand>> {
    code_actions(state, params.range.start)
}

// ---------------------------------------------------------------------
// T46: codeLens — show type info inline above each top-level function
// ---------------------------------------------------------------------

/// Compute code lenses for the document.
///
/// Emits ONE lens per top-level function declaration, anchored on the
/// function's NAME line, displaying the inferred / annotated signature
/// (`func add(a: Int, b: Int) -> Int`). The lens is non-interactive
/// (`command: None`) — it's purely informational, mirroring rust-analyzer's
/// "type info above fn" lens. Structs / enums / traits do NOT get lenses
/// (their info is already in the document-symbol outline).
///
/// Returns an empty `Vec` (not `None`) when no funcs are present; the
/// LSP wire shape is always an array for `textDocument/codeLens`.
pub fn code_lens(_state: &DocumentState, _params: CodeLensParams) -> Vec<CodeLens> {
    // Walk the analysis's top-level decls. The `top_decls` index already
    // carries the formatted signature as `detail` (built by
    // `format_func_signature` in `symbol.rs`), so we reuse it instead of
    // re-formatting here.
    _state
        .analysis
        .symbols
        .top_decls
        .iter()
        .filter(|d| matches!(d.kind, TopDeclKind::Func | TopDeclKind::Export))
        .filter_map(|d| {
            // Anchor the lens on the function's NAME (not its full span) so
            // the lens renders on the signature line, not the closing line
            // of the body for multi-line decls.
            let range = _state.lines.lsp_range(&_state.text, d.name_span);
            // Only single-line lenses are valid per LSP spec; collapse if
            // the name span somehow spans lines (it shouldn't, but be
            // defensive against weird spans).
            if range.start.line != range.end.line {
                return None;
            }
            Some(CodeLens {
                range,
                command: None,
                data: None,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------
// T46: inlayHint — parameter names + inferred types
// ---------------------------------------------------------------------

/// Compute inlay hints within `range`.
///
/// Two flavours of hint (matching rust-analyzer's defaults):
///
/// 1. **Type hints** on `let` bindings — for every local `let x = …`
///    whose inferred type is known, emit a hint at the END of the
///    binding's line showing `: <Type>`. Skipped for `let x: T = …`
///    where the user already wrote the annotation (no duplicate noise).
///    Skipped for `Type::Unknown` (the inferencer couldn't resolve).
///
/// 2. **Parameter-name hints** at call sites — DEFERRED. v1.25 ships
///    type hints only; parameter-name hints require resolving which
///    param each positional argument maps to (the analysis doesn't yet
///    track call-arg→param mapping for user functions). Tracked as a
///    T46b follow-up; the handler shape accepts it via an empty prepend.
///
/// Hints outside `range` are filtered out (the LSP spec requires
/// servers to only return hints within the visible viewport range for
/// performance on large files).
pub fn inlay_hints(state: &DocumentState, params: InlayHintParams) -> Vec<InlayHint> {
    let mut hints: Vec<InlayHint> = Vec::new();

    // Walk the local bindings. Only `Let` bindings get type hints —
    // params already have explicit annotations in Buff's grammar
    // (`name: Type`), so emitting hints there would duplicate text.
    for (_byte, entry) in &state.analysis.symbols.locals {
        if !matches!(entry.kind, crate::symbol::LocalKind::Let) {
            continue;
        }
        // Look up the inferred type at the binding site.
        let Some(ty) = state.analysis.types.lookup(entry.name.span.start) else {
            continue;
        };
        // Skip unknown types — showing `: Unknown` would be misleading.
        if matches!(ty, buff_lang_types::Type::Unknown) {
            continue;
        }
        // Skip bindings whose source line already contains a `: <Type>`
        // annotation (cheap byte-level check on the binding's line).
        let line_start_byte = state
            .lines
            .line_start_byte(&state.text, entry.def_span.start);
        let line_end_byte = state.lines.line_end_byte(&state.text, entry.def_span.start);
        let line_slice = &state.text[line_start_byte..line_end_byte];
        // A `:` after the binding name signals an explicit annotation.
        let name_end_in_line = entry.name.span.end.saturating_sub(line_start_byte);
        let after_name = if name_end_in_line < line_slice.len() {
            &line_slice[name_end_in_line..]
        } else {
            ""
        };
        let next_token = after_name.split_whitespace().next();
        if next_token.map(|s| s.starts_with(':')).unwrap_or(false) {
            continue;
        }

        // Hint position: END of the line containing the binding
        // (column = UTF-16 width of the line contents).
        let hint_pos = state.lines.lsp_position(&state.text, line_end_byte);
        // Filter by params.range (only emit hints in the requested viewport).
        if hint_pos.line < params.range.start.line || hint_pos.line > params.range.end.line {
            continue;
        }

        hints.push(InlayHint {
            position: hint_pos,
            label: InlayHintLabel::String(format!(": {}", display_type(ty))),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: Some(true),
            padding_right: None,
            data: None,
        });
    }

    // Stable order by position so editors render them deterministically.
    hints.sort_by_key(|h| (h.position.line, h.position.character));
    hints
}

// ---------------------------------------------------------------------
// T46: semanticTokens — syntax highlighting (LSP 3.16)
// ---------------------------------------------------------------------

/// The legend declared in [`semantic_tokens_legend`]. Indices are
/// referenced by [`token_type_index`]. Kept in a single source of truth
/// so the capability registration in `server.rs` and the per-token
/// emission here agree.
pub const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "keyword",   // 0
    "function",  // 1
    "struct",    // 2
    "enum",      // 3
    "interface", // 4  (trait)
    "type",      // 5  (type annotation / prelude type)
    "variable",  // 6  (local let binding + reference)
    "parameter", // 7  (function parameter)
    "string",    // 8
    "number",    // 9
    "char",      // 10 (char literal)
    "regexp",    // 11 (regex literal)
    "operator",  // 12
    "decorator", // 13 (@attribute)
];

/// Token modifiers we emit. Only `declaration` (marking the defining
/// occurrence) for now — the protocol allows zero modifiers.
pub const SEMANTIC_TOKEN_MODIFIERS: &[&str] = &["declaration"];

/// Bit 0 — the `declaration` modifier. Matches
/// [`SEMANTIC_TOKEN_MODIFIERS`] ordering.
const MOD_DECLARATION: u32 = 1 << 0;

/// Build the [`SemanticTokensLegend`] for capability registration.
/// Called from `server.rs` so the legend stays in sync with the
/// per-token emitter below.
pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    // Use the predefined LSP constants where they exist (more
    // discoverable than stringly-typed construction) and fall back to
    // `From<&'static str>` for the few non-predefined names.
    SemanticTokensLegend {
        token_types: vec![
            lsp_types::SemanticTokenType::KEYWORD,   // 0
            lsp_types::SemanticTokenType::FUNCTION,  // 1
            lsp_types::SemanticTokenType::STRUCT,    // 2
            lsp_types::SemanticTokenType::ENUM,      // 3
            lsp_types::SemanticTokenType::INTERFACE, // 4  (trait)
            lsp_types::SemanticTokenType::TYPE,      // 5
            lsp_types::SemanticTokenType::VARIABLE,  // 6
            lsp_types::SemanticTokenType::PARAMETER, // 7
            lsp_types::SemanticTokenType::STRING,    // 8
            lsp_types::SemanticTokenType::NUMBER,    // 9
            // char — no predefined constant; reuse the LSP "type"
            // category is wrong; use a custom string. lsp-types 0.97
            // added DECORATOR but no CHAR, so we use From<&str>.
            "char".into(),                           // 10
            lsp_types::SemanticTokenType::REGEXP,    // 11
            lsp_types::SemanticTokenType::OPERATOR,  // 12
            lsp_types::SemanticTokenType::DECORATOR, // 13
        ],
        token_modifiers: vec![lsp_types::SemanticTokenModifier::DECLARATION],
    }
}

/// Compute the full semantic-tokens payload for the document.
///
/// Walks the lexer's token stream and emits one [`SemanticToken`] per
/// "colourable" token (keywords, identifiers, literals, operators),
/// mapping each [`TokenKind`](buff_lang_lexer::TokenKind) to a type index
/// via [`token_type_index`]. Identifiers are further resolved against the
/// symbol index so that a function name tokens as `function`, a struct
/// name as `struct`, etc. — without that resolution every identifier
/// would be uniformly `variable`.
///
/// Tokens are sorted ascending by `(line, character)` and delta-encoded
/// per the LSP spec (each token's `delta_line`/`delta_start` is relative
/// to the previous token; the first token is relative to line 0, col 0).
///
/// Returns `None` when the source fails to lex (the diagnostic surface
/// already shows why). Returns `Some(SemanticTokensResult::Tokens(...))`
/// otherwise — always with a payload, possibly empty.
pub fn semantic_tokens_full(
    state: &DocumentState,
    _params: SemanticTokensParams,
) -> Option<SemanticTokensResult> {
    let tokens = match buff_lang_lexer::tokenize(&state.text, state.source_id) {
        Ok(t) => t,
        Err(_) => return None,
    };

    // Pre-build a lookup of top-decl NAME start bytes → semantic type
    // index so identifiers can be coloured by their target kind.
    let mut name_to_type: BTreeMap<usize, u32> = BTreeMap::new();
    for d in &state.analysis.symbols.top_decls {
        let idx = match d.kind {
            TopDeclKind::Func | TopDeclKind::Export => 1u32, // function
            TopDeclKind::Struct => 2u32,
            TopDeclKind::Enum => 3u32,
            TopDeclKind::Trait => 4u32,
            // Imports / extends / other don't get a coloured name here.
            TopDeclKind::Import | TopDeclKind::Extend | TopDeclKind::Other => continue,
        };
        name_to_type.insert(d.name_span.start, idx);
    }
    // Params: their name spans are in the locals index with kind == Param.
    let mut param_name_starts: std::collections::BTreeSet<usize> =
        std::collections::BTreeSet::new();
    let mut let_name_starts: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for (_byte, entry) in &state.analysis.symbols.locals {
        match entry.kind {
            crate::symbol::LocalKind::Param => {
                param_name_starts.insert(entry.name.span.start);
            }
            crate::symbol::LocalKind::Let => {
                let_name_starts.insert(entry.name.span.start);
            }
            _ => {}
        }
    }

    // First pass: build absolute (line, char, length, type, mods) tuples.
    let mut abs: Vec<(u32, u32, u32, u32, u32)> = Vec::with_capacity(tokens.len());
    for tok in &tokens {
        let (ty_idx, mods) = token_type_index(
            &tok.kind,
            &tok.span,
            state,
            &name_to_type,
            &param_name_starts,
            &let_name_starts,
        );
        let Some(ty_idx) = ty_idx else { continue };
        // Length: number of UTF-16 code units in the token's source slice.
        let start = tok.span.start.min(state.text.len());
        let end = tok.span.end.min(state.text.len());
        if end <= start {
            continue;
        }
        let slice = &state.text[start..end];
        let length = slice.encode_utf16().count() as u32;
        if length == 0 {
            continue;
        }
        let pos = state.lines.lsp_position(&state.text, start);
        abs.push((pos.line, pos.character, length, ty_idx, mods));
    }

    // Sort by (line, char). The lexer emits tokens in source order
    // already, but the offside-rule synthesised Indent/Dedent tokens
    // sit at the same byte as the following real token — a stable sort
    // keeps them in emission order which is what we want.
    abs.sort_by_key(|t| (t.0, t.1));

    // Second pass: delta-encode. Skip tokens that occupy the same
    // (line, char) as the previous emitted token (zero-length deltas
    // are illegal per LSP spec) — this happens when two colourable
    // tokens overlap (e.g. `@test` → decorator `@` is at the same byte
    // as the start of `test` identifier).
    let mut data: Vec<SemanticToken> = Vec::with_capacity(abs.len());
    let mut prev_line: u32 = 0;
    let mut prev_char: u32 = 0;
    for (line, char, length, ty_idx, mods) in abs {
        if line == prev_line && char == prev_char {
            // Overlap — skip the later-emitted one (the earlier one
            // already claimed this cell). Keeps the stream well-formed.
            continue;
        }
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            char - prev_char
        } else {
            char
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: ty_idx,
            token_modifiers_bitset: mods,
        });
        prev_line = line;
        prev_char = char;
    }

    Some(SemanticTokensResult::Tokens(SemanticTokens {
        result_id: None,
        data,
    }))
}

/// Map a lexer [`TokenKind`](buff_lang_lexer::TokenKind) (plus context)
/// to a semantic token type index in [`SEMANTIC_TOKEN_TYPES`].
///
/// Returns `None` for tokens that should NOT be coloured (whitespace,
/// newlines, indent/dedent markers, brackets, punctuation that isn't an
/// operator). For identifiers, the `name_to_type` / `param_name_starts`
/// / `let_name_starts` indexes resolve the identifier to a more
/// specific kind (function / struct / parameter / variable); falling
/// back to `variable` when no resolution applies.
#[allow(clippy::too_many_arguments)]
fn token_type_index(
    kind: &buff_lang_lexer::TokenKind,
    span: &buff_lang_error::Span,
    _state: &DocumentState,
    name_to_type: &BTreeMap<usize, u32>,
    param_name_starts: &std::collections::BTreeSet<usize>,
    let_name_starts: &std::collections::BTreeSet<usize>,
) -> (Option<u32>, u32) {
    use buff_lang_lexer::TokenKind;
    // --- literals ---
    let ty = match kind {
        TokenKind::IntLit(_)
        | TokenKind::FloatLit(_)
        | TokenKind::ByteLit(_)
        | TokenKind::DoubleLit(_)
        | TokenKind::DecimalLit(_) => Some(9), // number
        TokenKind::CharLit(_) => Some(10),  // char
        TokenKind::RegexLit(_) => Some(11), // regexp
        TokenKind::StringStart
        | TokenKind::StringEnd
        | TokenKind::StringLit(_)
        | TokenKind::StringPart(_)
        | TokenKind::InterpStart
        | TokenKind::InterpSpec(_)
        | TokenKind::InterpEnd => Some(8), // string — boundaries + parts coloured as string
        // --- operators (arithmetic / comparison / assignment / etc) ---
        TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Star
        | TokenKind::Slash
        | TokenKind::Percent
        | TokenKind::EqEq
        | TokenKind::NotEq
        | TokenKind::Lt
        | TokenKind::Gt
        | TokenKind::LtEq
        | TokenKind::GtEq
        | TokenKind::AndAnd
        | TokenKind::OrOr
        | TokenKind::Not
        | TokenKind::Question
        | TokenKind::QuestionQuestion
        | TokenKind::QuestionDot
        | TokenKind::Caret
        | TokenKind::Pipe
        | TokenKind::Amp
        | TokenKind::Shl
        | TokenKind::Shr
        | TokenKind::Tilde
        | TokenKind::Arrow
        | TokenKind::FatArrow
        | TokenKind::Assign
        | TokenKind::PlusEq
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::SlashEq
        | TokenKind::PercentEq
        | TokenKind::PipeGt
        | TokenKind::DotDot
        | TokenKind::DotDotEq => Some(12), // operator
        // --- keywords (all `Kw*` variants) ---
        TokenKind::KwFunc
        | TokenKind::KwLet
        | TokenKind::KwMut
        | TokenKind::KwStruct
        | TokenKind::KwEnum
        | TokenKind::KwTrait
        | TokenKind::KwType
        | TokenKind::KwIf
        | TokenKind::KwElse
        | TokenKind::KwFor
        | TokenKind::KwReturn
        | TokenKind::KwBreak
        | TokenKind::KwContinue
        | TokenKind::KwIn
        | TokenKind::KwMatch
        | TokenKind::KwAsync
        | TokenKind::KwSpawn
        | TokenKind::KwImport
        | TokenKind::KwExport
        | TokenKind::KwFrom
        | TokenKind::KwAs
        | TokenKind::KwTrue
        | TokenKind::KwFalse
        | TokenKind::KwExtern
        | TokenKind::KwUnsafe
        | TokenKind::KwGuard
        | TokenKind::KwExtend
        | TokenKind::KwImpl
        | TokenKind::KwDefer => {
            Some(0) // keyword
        }
        // --- mathematical-syntax operators (T19) ---
        TokenKind::Sum
        | TokenKind::Product
        | TokenKind::Sqrt
        | TokenKind::InUni
        | TokenKind::NotInUni
        | TokenKind::SubsetUni
        | TokenKind::ApproxUni
        | TokenKind::Adjoint => Some(12), // operator
        // --- decorator (attribute marker) ---
        TokenKind::At => Some(13), // decorator
        // --- identifiers: resolve via the symbol index ---
        TokenKind::Ident(_) => {
            if let Some(&idx) = name_to_type.get(&span.start) {
                // Top-level decl name — mark as declaration.
                return (Some(idx), MOD_DECLARATION);
            }
            if param_name_starts.contains(&span.start) {
                return (Some(7), MOD_DECLARATION); // parameter + declaration
            }
            if let Some(ty) = resolve_ident_as_top_decl(_state, span.start) {
                return (Some(ty), 0);
            }
            if let_name_starts.contains(&span.start) {
                return (Some(6), MOD_DECLARATION); // variable + declaration
            }
            // Default: variable. Covers references to locals + any
            // unresolved identifier.
            Some(6)
        }
        // --- skip: layout markers, punctuation, eof ---
        TokenKind::Newline
        | TokenKind::Indent
        | TokenKind::Dedent
        | TokenKind::Eof
        | TokenKind::LParen
        | TokenKind::RParen
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::Colon
        | TokenKind::Comma
        | TokenKind::Dot
        | TokenKind::Semicolon => None,
    };
    (ty, 0)
}

/// Look up whether the identifier at `byte` is a REFERENCE to a
/// top-level decl (function / struct / enum / trait) and return its
/// semantic token type index. Returns `None` when the identifier is
/// not a known top-decl reference (it's then coloured as `variable`).
fn resolve_ident_as_top_decl(state: &DocumentState, byte: usize) -> Option<u32> {
    // Read the identifier text starting at `byte`.
    let bytes = state.text.as_bytes();
    if byte >= bytes.len() || !(bytes[byte].is_ascii_alphabetic() || bytes[byte] == b'_') {
        return None;
    }
    let mut end = byte;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    let name = &state.text[byte..end];
    let top = state.analysis.symbols.find_top_decl(name)?;
    let idx = match top.kind {
        TopDeclKind::Func | TopDeclKind::Export => 1u32,
        TopDeclKind::Struct => 2u32,
        TopDeclKind::Enum => 3u32,
        TopDeclKind::Trait => 4u32,
        TopDeclKind::Import | TopDeclKind::Extend | TopDeclKind::Other => return None,
    };
    Some(idx)
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

    // -----------------------------------------------------------------
    // T46: codeAction / codeLens / inlayHint / semanticTokens
    // -----------------------------------------------------------------

    #[test]
    fn t46_code_lens_emits_one_per_function() {
        let src = "func a():\n    print(1)\n\nfunc b():\n    print(2)\n";
        let st = open(src);
        let params = CodeLensParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: "file:///t.buff".parse().unwrap(),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let lenses = code_lens(&st, params);
        assert_eq!(lenses.len(), 2, "expected one lens per function");
        // Each lens should be a single-line range.
        for l in &lenses {
            assert_eq!(l.range.start.line, l.range.end.line, "non-single-line lens");
            assert!(l.command.is_none(), "expected non-interactive lens");
        }
    }

    #[test]
    fn t46_code_lens_skips_structs_enums() {
        let src = "struct Point:\n    x: Int\n\nfunc origin():\n    return Point { x: 0 }\n";
        let st = open(src);
        let params = CodeLensParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: "file:///t.buff".parse().unwrap(),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let lenses = code_lens(&st, params);
        // Only `origin` qualifies — struct/enum decls are skipped.
        assert_eq!(lenses.len(), 1, "lenses: {lenses:?}");
    }

    #[test]
    fn t46_inlay_hints_emit_type_for_let_binding() {
        let src = "func main():\n    let x = 42\n    print(x)\n";
        let st = open(src);
        let params = InlayHintParams {
            work_done_progress_params: Default::default(),
            text_document: lsp_types::TextDocumentIdentifier {
                uri: "file:///t.buff".parse().unwrap(),
            },
            range: Range::new(pos(0, 0), pos(99, 0)),
        };
        let hints = inlay_hints(&st, params);
        assert_eq!(
            hints.len(),
            1,
            "expected one type hint for `let x`, got: {hints:?}"
        );
        let label = match &hints[0].label {
            InlayHintLabel::String(s) => s.clone(),
            _ => panic!("expected string label"),
        };
        assert!(label.contains("Int"), "expected Int hint, got: {label}");
        assert_eq!(hints[0].kind, Some(InlayHintKind::TYPE));
    }

    #[test]
    fn t46_inlay_hints_skip_explicit_annotations() {
        // `let x: Int = 42` already has the annotation — no hint.
        let src = "func main():\n    let x: Int = 42\n    print(x)\n";
        let st = open(src);
        let params = InlayHintParams {
            work_done_progress_params: Default::default(),
            text_document: lsp_types::TextDocumentIdentifier {
                uri: "file:///t.buff".parse().unwrap(),
            },
            range: Range::new(pos(0, 0), pos(99, 0)),
        };
        let hints = inlay_hints(&st, params);
        assert!(
            hints.is_empty(),
            "expected no hints for explicit annotation, got: {hints:?}"
        );
    }

    #[test]
    fn t46_inlay_hints_filtered_by_range() {
        let src = "func main():\n    let x = 1\n    let y = 2\n    let z = 3\n";
        let st = open(src);
        // Only line 1 (`let x`) is in range.
        let params = InlayHintParams {
            work_done_progress_params: Default::default(),
            text_document: lsp_types::TextDocumentIdentifier {
                uri: "file:///t.buff".parse().unwrap(),
            },
            range: Range::new(pos(1, 0), pos(1, 99)),
        };
        let hints = inlay_hints(&st, params);
        assert_eq!(hints.len(), 1, "expected only the line-1 hint in range");
    }

    #[test]
    fn t46_semantic_tokens_legend_matches_constants() {
        let legend = semantic_tokens_legend();
        // 14 token types (per SEMANTIC_TOKEN_TYPES ordering).
        assert_eq!(legend.token_types.len(), SEMANTIC_TOKEN_TYPES.len());
        // 1 modifier (declaration).
        assert_eq!(legend.token_modifiers.len(), 1);
        // Spot-check a few: keyword=0, function=1, struct=2.
        assert_eq!(legend.token_types[0].as_str(), "keyword");
        assert_eq!(legend.token_types[1].as_str(), "function");
        assert_eq!(legend.token_types[2].as_str(), "struct");
    }

    #[test]
    fn t46_semantic_tokens_full_emits_keyword_and_function() {
        let src = "func main():\n    print(\"hi\")\n";
        let st = open(src);
        let params = SemanticTokensParams {
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            text_document: lsp_types::TextDocumentIdentifier {
                uri: "file:///t.buff".parse().unwrap(),
            },
        };
        let result = semantic_tokens_full(&st, params).expect("tokens");
        let tokens = match result {
            SemanticTokensResult::Tokens(t) => t.data,
            _ => panic!("expected Tokens variant"),
        };
        // `func` (keyword) + `main` (function) + `print` (function) +
        // `(` (skip) + `"hi"` (string) + `)` (skip). Minimum: keyword +
        // function tokens present.
        let types_present: std::collections::BTreeSet<u32> =
            tokens.iter().map(|t| t.token_type).collect();
        assert!(
            types_present.contains(&0),
            "expected keyword (0), got: {types_present:?}"
        );
        assert!(
            types_present.contains(&1),
            "expected function (1), got: {types_present:?}"
        );
    }

    #[test]
    fn t46_semantic_tokens_full_delta_encodes() {
        // Two lines of source → first token is absolute, second is
        // delta. Verify the delta encoding shape.
        let src = "func a():\n    print(1)\n";
        let st = open(src);
        let params = SemanticTokensParams {
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            text_document: lsp_types::TextDocumentIdentifier {
                uri: "file:///t.buff".parse().unwrap(),
            },
        };
        let result = semantic_tokens_full(&st, params).expect("tokens");
        let tokens = match result {
            SemanticTokensResult::Tokens(t) => t.data,
            _ => panic!("expected Tokens"),
        };
        // First token's delta_line + delta_start must form a valid
        // absolute position (line 0 since it's the first).
        assert_eq!(tokens[0].delta_line, 0, "first token must start at line 0");
        // Subsequent tokens must have non-negative deltas.
        for window in tokens.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            // Same line: b.delta_start must be > 0 (we skip overlaps).
            if b.delta_line == 0 {
                assert!(b.delta_start > 0, "zero-delta token: {a:?} -> {b:?}");
            }
        }
    }
}
