//! Top-level parser entry point.
//!
//! [`parse`] consumes a slice of lexer-produced tokens and emits a list of
//! top-level declarations ([`Decl`]). Statement-level parsing (`let`, `if`,
//! `func`, `return`, `for`, …) lives in [`crate::stmt`]; this module only
//! dispatches on top-level keywords.
//!
//! [`parse`] is the public boundary that downstream code calls. It hides
//! [`TokenStream`] construction and any pre-filtering of layout tokens.
//!
//! # Error-recovery entry point (T36)
//!
//! [`parse`] is **fail-fast**: it returns the first [`ParseError`] it
//! encounters and discards any partial output. For compiler-internal use
//! (e.g. surfacing *all* errors in one pass), [`parse_recovering`] is the
//! recovering variant: it accumulates every recoverable error into a
//! `Vec<ParseError>` and continues parsing after each error by skipping
//! forward to the next sync point (see
//! [`TokenStream::sync_to_recovery_point`]). Both entry points share the
//! same per-iteration dispatch logic via the internal [`parse_one_decl`]
//! helper, so they agree on what counts as a top-level declaration.

use buff_lang_ast::Decl;
use buff_lang_error::{Diagnostic, ParseError, SourceId};
use buff_lang_lexer::TokenKind;

use crate::options::Edition;
use crate::stmt::{
    parse_attributes, parse_enum_decl, parse_export_decl, parse_extend_decl, parse_extern_crate_decl,
    parse_extern_func_decl_with_abi, parse_func_decl, parse_import_decl, parse_struct_decl,
    parse_trait_decl,
};
use crate::stream::TokenStream;

/// Parse one top-level declaration (after parsing any leading `@attributes`)
/// from `stream`.
///
/// This is the shared per-iteration body used by both [`parse`] (fail-fast)
/// and [`parse_recovering`] (accumulating). Keeping the dispatch in one place
/// guarantees both entry points agree on what constitutes a top-level
/// declaration — they differ only in *how* they react to a [`ParseError`].
///
/// # Returns
///
/// - `Ok(Some(decl))` — one declaration parsed successfully.
/// - `Ok(None)` — end of input (no more declarations to parse).
/// - `Err(e)` — a syntax error. The cursor position after an error is
///   unspecified (it sits wherever the failed parse left it); callers that
///   want to continue should call [`TokenStream::sync_to_recovery_point`].
fn parse_one_decl(stream: &mut TokenStream) -> Result<Option<Decl>, ParseError> {
    if stream.is_at_end() {
        return Ok(None);
    }
    // T35: parse any leading `@name` attributes before the declaration.
    // When attributes are present, only a `func` declaration is valid
    // (the only attribute-attachable decl kind in v0.5). The attributes
    // are threaded into `parse_func_decl` so they land on the FuncDecl.
    let attributes = parse_attributes(stream)?;
    let saw_attributes = !attributes.is_empty();
    match stream.peek_kind() {
        Some(TokenKind::KwFunc) => {
            let f = parse_func_decl(stream, attributes)?;
            Ok(Some(Decl::FuncDecl(f)))
        }
        // T31: `async func name(...) { ... }` — the async modifier on a
        // function declaration. The `async` keyword (`TokenKind::KwAsync`)
        // precedes `func`; `parse_func_decl` consumes the leading `async`
        // (if present) and sets `is_async` accordingly. The dispatcher
        // routes both `async func` and `func` to `parse_func_decl` so
        // the modifier is handled in one place.
        Some(TokenKind::KwAsync)
            if matches!(stream.peek_second_kind(), Some(TokenKind::KwFunc)) =>
        {
            let f = parse_func_decl(stream, attributes)?;
            Ok(Some(Decl::FuncDecl(f)))
        }
        // T27: top-level enum declarations. Functions and enums are the
        // two top-level forms supported at this stage; struct/trait/module
        // parsing arrives in later waves.
        Some(TokenKind::KwEnum) => {
            if saw_attributes {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                return Err(ParseError::new(Diagnostic::error(
                    "attributes are not yet supported on `enum` declarations (only `func`)",
                    span,
                )));
            }
            let e = parse_enum_decl(stream)?;
            Ok(Some(Decl::EnumDecl(e)))
        }
        // T13: top-level struct declarations. Supports both layout-sensitive
        // (`struct Pair<T, U>:` + indented fields) and brace-delimited
        // (`struct Point { x: Float, y: Float }`) forms. The lexer already
        // tokenises `struct` as `TokenKind::KwStruct`; the parser dispatches
        // to `parse_struct_decl`. Generic params `<T, U>` follow the name.
        Some(TokenKind::KwStruct) => {
            if saw_attributes {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                return Err(ParseError::new(Diagnostic::error(
                    "attributes are not yet supported on `struct` declarations (only `func`)",
                    span,
                )));
            }
            let s = parse_struct_decl(stream)?;
            Ok(Some(Decl::StructDecl(s)))
        }
        // T29: top-level import / export declarations.
        Some(TokenKind::KwImport) => {
            if saw_attributes {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                return Err(ParseError::new(Diagnostic::error(
                    "attributes are not supported on `import` declarations",
                    span,
                )));
            }
            let imp = parse_import_decl(stream)?;
            Ok(Some(Decl::ImportDecl(imp)))
        }
        Some(TokenKind::KwExport) => {
            if saw_attributes {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                return Err(ParseError::new(Diagnostic::error(
                    "attributes are not supported on `export` declarations (put the attribute on the inner declaration instead, e.g. `@test\\nfunc ...` without `export`)",
                    span,
                )));
            }
            let exp = parse_export_decl(stream)?;
            Ok(Some(exp))
        }
        // T32/T119: top-level FFI declarations. Three shapes share the
        // `extern` keyword:
        //   1. `extern crate "name"` — record a dependency on an external
        //      Rust crate (→ [`Decl::ExternCrateDecl`]). (T32)
        //   2. `extern func name(...) -> Ret` — a foreign-function
        //      declaration with NO body (→ [`Decl::FuncDecl`] with
        //      `is_extern = true`; `parse_func_decl` consumes the leading
        //      `extern` and skips body parsing). (T32)
        //   3. `extern "ABI" [from "crate"] func name(...) -> Ret` — the
        //      rich-ABI form added in T119. The dispatcher disambiguates
        //      by peeking at the second token: a `StringStart` means the
        //      user wrote the new ABI-string form, route to
        //      [`parse_extern_func_decl_with_abi`]. (T119)
        Some(TokenKind::KwExtern) => {
            if saw_attributes {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                return Err(ParseError::new(Diagnostic::error(
                    "attributes are not supported on `extern` declarations",
                    span,
                )));
            }
            match stream.peek_second_kind() {
                Some(TokenKind::Ident(s)) if s == "crate" => {
                    let d = parse_extern_crate_decl(stream)?;
                    Ok(Some(d))
                }
                // T119: `extern "ABI" [from "crate"] func ...` — the new
                // ABI-string form. The ABI literal is the second token
                // (a `StringStart` from the lexer's interpolation
                // machinery — every `"..."` is tokenised as
                // `StringStart, StringPart, StringEnd`).
                Some(TokenKind::StringStart) => {
                    let d = parse_extern_func_decl_with_abi(stream)?;
                    Ok(Some(d))
                }
                Some(TokenKind::KwFunc) => {
                    let f = parse_func_decl(stream, attributes)?;
                    Ok(Some(Decl::FuncDecl(f)))
                }
                _ => Err(ParseError::new(Diagnostic::error(
                    "expected `extern crate \"<name>\"`, `extern \"ABI\" func ...`, or `extern func ...` after `extern`",
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                ))),
            }
        }
        // T75: `extend TYPE { fn ...; fn ...; ... }` — extension-method
        // block. Adds methods to an existing type (primitive or
        // user-defined). Each method is parsed via the shared
        // `parse_func_decl`; the resulting `ExtendBlock` lowers to a Rust
        // extension trait + blanket-free impl at codegen time.
        Some(TokenKind::KwExtend) => {
            if saw_attributes {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                return Err(ParseError::new(Diagnostic::error(
                    "attributes are not supported on `extend` declarations",
                    span,
                )));
            }
            let e = parse_extend_decl(stream)?;
            Ok(Some(Decl::ExtendBlock(e)))
        }
        // T93: `trait Name [: Super] { fn ...; fn ... { } }` — trait
        // declaration with default methods and inheritance. The body
        // contains a mix of REQUIRED methods (`fn ... ;`, bodyless) and
        // DEFAULT methods (`fn ... { body }`, with body). Supertraits
        // follow `:` after the name. Codegen lowers to a Rust trait.
        Some(TokenKind::KwTrait) => {
            if saw_attributes {
                let span = stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span());
                return Err(ParseError::new(Diagnostic::error(
                    "attributes are not supported on `trait` declarations",
                    span,
                )));
            }
            let t = parse_trait_decl(stream)?;
            Ok(Some(Decl::TraitDecl(t)))
        }
        // T35: attributes were present but the next token is not a
        // recognised attribute-attachable declaration. This is a parse
        // error (e.g. `@test let x = 1` or `@test` at EOF).
        _ if saw_attributes => {
            let span = stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span());
            let found = stream
                .peek_kind()
                .map(|k| k.to_string())
                .unwrap_or_else(|| "end of input".into());
            Err(ParseError::new(Diagnostic::error(
                format!("attributes must precede a `func` declaration, found `{found}`"),
                span,
            )))
        }
        other => {
            let span = stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span());
            Err(ParseError::new(Diagnostic::error(
                format!(
                    "only function declarations are allowed at top level, found `{}`",
                    other
                        .map(|k| k.to_string())
                        .unwrap_or_else(|| "end of input".into())
                ),
                span,
            )))
        }
    }
}

/// Parse a slice of tokens into zero or more top-level [`Decl`]s.
///
/// # Dispatch table
///
/// | Token   | Result                                                |
/// |---------|-------------------------------------------------------|
/// | `func`  | [`Decl::FuncDecl`] (T8)                               |
/// | `async func` | [`Decl::FuncDecl`] with `is_async = true` (T31) |
/// | `enum`  | [`Decl::EnumDecl`] (T27)                              |
/// | `import`| [`Decl::ImportDecl`] (T29 — ES6 `from "..."` + legacy)|
/// | `export`| [`Decl::ExportDecl`] / [`Decl::ReexportDecl`] (T29)  |
/// | `export async func` | [`Decl::ExportDecl`] wrapping an async fn (T31) |
/// | `extern crate "name"` | [`Decl::ExternCrateDecl`] (T32 — FFI)        |
/// | `extern func ...`     | [`Decl::FuncDecl`] with `is_extern = true` (T32 — FFI) |
/// | `@name ... func`      | [`Decl::FuncDecl`] with `attributes` populated (T35 — `buff test`) |
/// | `extend TYPE { fn ... }` | [`Decl::ExtendBlock`] (T75 — extension methods) |
/// | `trait Name [: Super] { fn ...; fn ... { } }` | [`Decl::TraitDecl`] (T93 — traits with defaults + inheritance) |
///
/// Any other token at top level is an error — statements such as
/// `let`/`return`/`if` belong inside a function body, not at module scope.
///
/// # Errors
///
/// Returns [`ParseError`] on **the first** syntax error encountered
/// (fail-fast). Any declarations parsed before the error are discarded. For
/// a recovering variant that collects multiple errors in one pass, see
/// [`parse_recovering`].
pub fn parse(
    tokens: &[buff_lang_lexer::Token],
    source_id: SourceId,
) -> Result<Vec<Decl>, ParseError> {
    parse_with_edition(tokens, source_id, Edition::default())
}

/// Edition-aware variant of [`parse`] (T57). Accepts an explicit [`Edition`]
/// which selects whether the parser accepts scientific-edition syntax
/// extensions (implicit multiplication, Unicode operators, matrix literals,
/// adjoint). Default-edition callers should use [`parse`] instead — this
/// function exists for the build pipeline to plumb the `edition` field from
/// `buff.toml` into the parser without making every existing call site
/// aware of editions.
pub fn parse_with_edition(
    tokens: &[buff_lang_lexer::Token],
    source_id: SourceId,
    edition: Edition,
) -> Result<Vec<Decl>, ParseError> {
    let mut stream = TokenStream::with_edition(tokens, source_id, edition);
    let mut decls = Vec::new();
    while let Some(d) = parse_one_decl(&mut stream)? {
        decls.push(d);
    }
    Ok(decls)
}

/// Recovering variant of [`parse`]: parse the same top-level declarations,
/// but on a [`ParseError`] **record the error and continue** rather than
/// failing fast.
///
/// After each error the parser skips tokens forward to the next sync point
/// (a token that could begin a fresh top-level declaration — `func`, `async`,
/// `enum`, `import`, `export`, `extern`, `extend`, `trait`, or `@`) via
/// [`TokenStream::sync_to_recovery_point`], then resumes parsing.
///
/// # Returns
///
/// `(decls, errors)` — the declarations that parsed successfully, plus every
/// error collected along the way. Both vectors may be non-empty
/// simultaneously (a partial program with several errors can still produce
/// some valid declarations).
///
/// # When to use this vs [`parse`]
///
/// - [`parse`] is for the **production CLI pipeline**: a single error stops
///   compilation, exactly one diagnostic is shown to the user.
/// - `parse_recovering` is for **IDE / `buff check` style tooling** that
///   wants to surface every error in the file in a single pass. It is also
///   useful for tests that exercise the multi-error path.
///
/// # Determinism
///
/// Errors appear in source order (left-to-right, top-to-bottom), matching
/// the order in which the parser encounters them as it advances the cursor.
pub fn parse_recovering(
    tokens: &[buff_lang_lexer::Token],
    source_id: SourceId,
) -> (Vec<Decl>, Vec<ParseError>) {
    let mut stream = TokenStream::new(tokens, source_id);
    let mut decls = Vec::new();
    let mut errors = Vec::new();
    loop {
        match parse_one_decl(&mut stream) {
            Ok(Some(d)) => decls.push(d),
            Ok(None) => break,
            Err(e) => {
                errors.push(e);
                let pos_before = stream.save();
                stream.sync_to_recovery_point();
                // Infinite-loop guard: if sync did not advance the cursor
                // AND we're not at EOF, force-advance one token so the next
                // iteration sees fresh input. This handles the rare case
                // where sync stops on a token that parse_one_decl keeps
                // rejecting (e.g. a stray `let` at top level — `let` is not
                // in the sync set, but the guard is cheap insurance against
                // any future change to the sync set).
                if pos_before == stream.save() && !stream.is_at_end() {
                    stream.advance();
                }
            }
        }
    }
    (decls, errors)
}

/// Parse a single top-level expression from a token slice. Convenience
/// wrapper for tests and embedding tools that want to evaluate an
/// expression without setting up a [`TokenStream`] themselves.
///
/// # Errors
///
/// Returns [`ParseError`] if the tokens do not form exactly one expression,
/// or if there are leftover tokens after the expression.
pub fn parse_expression(
    tokens: &[buff_lang_lexer::Token],
    source_id: SourceId,
) -> Result<buff_lang_ast::Expr, ParseError> {
    parse_expression_with_edition(tokens, source_id, Edition::default())
}

/// Edition-aware variant of [`parse_expression`] (T57). Mirrors
/// [`parse_with_edition`]: the parser accepts scientific-edition extensions
/// iff `edition` is [`Edition::Scientific`]. Convenience entry point for
/// REPL / Jupyter / test harnesses that want to evaluate a single
/// expression under the scientific edition.
pub fn parse_expression_with_edition(
    tokens: &[buff_lang_lexer::Token],
    source_id: SourceId,
    edition: Edition,
) -> Result<buff_lang_ast::Expr, ParseError> {
    let mut stream = TokenStream::with_edition(tokens, source_id, edition);
    let expr = crate::expr::parse_expression(&mut stream)?;
    if !stream.is_at_end() {
        return Err(stream.unexpected("extra tokens after expression"));
    }
    Ok(expr)
}
