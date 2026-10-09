//! FFI declarations: `extern crate` bindings and `extern func` with ABI
//! strings. ITER-52 pure move from stmt_decl.rs.

use buff_lang_ast::Decl;
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::TokenKind;

use crate::stream::TokenStream;

use super::func::*;
use super::shared::*;

/// Parse an `extern crate "name"` declaration (T32 — FFI basics).
///
/// Records a dependency on an external Rust crate. The Buff form uses a
/// STRING literal for the crate name (`extern crate "serde"`) — unlike
/// Rust's `extern crate serde;`, which uses a bare path — so the crate
/// name may be any crates.io identifier (including names with `-`, which
/// are not valid Buff identifiers). The codegen emits a `use <name>;`
/// item and records the name in its extern-crate dep set.
///
/// The leading `extern` keyword is consumed here; this function expects
/// the NEXT significant token to be the bare identifier `crate`. The
/// crate name string follows immediately.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing `crate` identifier after `extern`,
/// - missing crate-name string after `crate`,
/// - interpolation inside the crate-name string.
pub fn parse_extern_crate_decl(stream: &mut TokenStream<'_>) -> Result<Decl, ParseError> {
    let source_id = stream.source_id();
    let extern_tok = stream.expect(TokenKind::KwExtern)?;
    let start = extern_tok.span.start;

    // Expect the bare identifier `crate` (NOT a keyword in Buff — it is
    // parsed as a regular Ident). Reject anything else with a helpful msg.
    let crate_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected `crate` after `extern`, found end of input",
            stream.eof_span(),
        ))
    })?;
    match &crate_tok.kind {
        TokenKind::Ident(s) if s == "crate" => {}
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!(
                    "expected `crate` after `extern`, found `{other}` \
                     (Buff supports `extern crate \"<name>\"` and `extern func ...`)"
                ),
                crate_tok.span,
            )));
        }
    }

    // Consume the crate-name string literal (`"serde"`). Reuse the same
    // StringStart/StringPart/StringEnd machinery the lexer uses for every
    // string — interpolation is rejected (crate names must be plain).
    let (name, name_end) = expect_crate_name_string(stream)?;
    let span = Span::new(start, name_end, source_id);
    Ok(Decl::ExternCrateDecl(buff_lang_ast::ExternCrateDecl {
        name,
        span,
    }))
}

/// Parse an `extern "ABI" [from "crate"] func name(params) -> Ret`
/// declaration (T119 — minimal extern/bindgen). The leading `extern` is
/// consumed here; the cursor MUST be positioned at the `StringStart` of
/// the ABI literal when this function is called.
///
/// Returns a [`Decl::ExternFuncDecl`] carrying:
/// - the ABI string (validated to `"C"` in v1.3 — other ABIs are a
///   parse error per the T119 spec),
/// - the optional source crate from the `from "..."` annotation,
/// - the function name + parameter list + return type.
///
/// # Generics rejection
///
/// If the function name is followed by `<` (the start of a generic
/// parameter list), the parser returns a clear error: generics are NOT
/// supported on extern functions in v1.3 (per the T119 spec: "A
/// generic/trait-heavy Rust API is REJECTED with a clear error").
///
/// # Body
///
/// Extern functions have NO body — after the signature we expect either
/// EOF, a Newline, or the start of the next top-level decl. We do NOT
/// consume a trailing block.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing ABI string after `extern`,
/// - ABI string is not `"C"` (v1.3 accept-list),
/// - missing `func` keyword after ABI / `from "..."`,
/// - missing function name,
/// - missing parameter list `(`...`)`,
/// - generic `<...>` after the function name (rejected),
/// - interpolation in either string literal.
pub fn parse_extern_func_decl_with_abi(stream: &mut TokenStream<'_>) -> Result<Decl, ParseError> {
    let source_id = stream.source_id();
    let extern_tok = stream.expect(TokenKind::KwExtern)?;
    let start = extern_tok.span.start;

    // 1. Consume the ABI string literal (`"C"`). Reuse the helper that
    //    rejects interpolation — ABIs are plain strings.
    let (abi, abi_end) = expect_abi_string(stream)?;
    // Validate against the v1.3 accept-list. Only `"C"` is supported
    // (per the T119 spec: "use `"C"` ABI for stability/cross-language
    // compatibility"). Other ABIs (`"system"`, `"stdcall"`, `"fastcall"`)
    // are a parse error — surface a clear message naming the unsupported
    // ABI.
    if abi != "C" {
        // Re-compute the span of the ABI literal for the diagnostic. We
        // don't carry it back from `expect_abi_string` (it returns end
        // only); approximate with a span ending at `abi_end` and starting
        // at `abi_end - abi.len() - 2` (the two quotes). For the rare
        // user-written ABI this is good enough — they'll see "at column
        // N" pointing at the literal.
        let approx_start = abi_end.saturating_sub(abi.len() + 2);
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "unsupported ABI `{:?}` in `extern \"ABI\" func ...`: only `\"C\"` is supported in v1.3 \
                 (the T119 spec mandates the C ABI for cross-language stability; other ABIs are deferred)",
                abi
            ),
            Span::new(approx_start, abi_end, source_id),
        )));
    }

    // 2. Optionally consume `from "crate-name"`. The `from` keyword is a
    //    reserved Buff keyword (`TokenKind::KwFrom`), so we can peek for
    //    it directly. When present, the next token must be a string
    //    literal naming the source crate.
    let mut crate_name: Option<String> = None;
    if matches!(stream.peek_kind(), Some(TokenKind::KwFrom)) {
        stream.advance(); // consume `from`
        let (name, _name_end) = expect_crate_name_string(stream)?;
        crate_name = Some(name);
    }

    // 3. Consume the `func` keyword.
    let func_tok = stream.expect(TokenKind::KwFunc)?;
    let _ = func_tok; // span tracking not needed for v1.3

    // 4. Function name.
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected function name after `func`",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // 5. Generic-parameter rejection. If the next token is `<`, the user
    //    is trying to declare a generic extern fn (`extern "C" func
    //    parse<T>(...) -> T`). Buff does NOT support generics on
    //    functions in v1.3 (let alone on externs), so this is a clear
    //    parse error per the T119 spec.
    if matches!(stream.peek_kind(), Some(TokenKind::Lt)) {
        let lt_span = stream
            .peek()
            .map(|t| t.span)
            .unwrap_or_else(|| stream.eof_span());
        return Err(ParseError::new(Diagnostic::error(
            "generics are not supported on `extern` functions in v1.3 \
             (the T119 spec rejects generic/trait-heavy Rust APIs; declare a separate \
             concrete wrapper per type you need, e.g. `extern \"C\" from \"serde_json\" \
             func parse_int(s: String) -> Int`)",
            lt_span,
        )));
    }

    // 6. Parameter list `( ... )`.
    stream.expect(TokenKind::LParen)?;
    let params = parse_params(stream)?;
    let rparen = stream.expect(TokenKind::RParen)?;

    // 7. Optional return type: `-> Type`.
    let mut end = rparen.span.end;
    let return_type = if matches!(stream.peek_kind(), Some(TokenKind::Arrow)) {
        stream.advance(); // consume `->`
        let ty = parse_type_ref(stream)?;
        end = type_end(&ty);
        Some(ty)
    } else {
        None
    };

    let span = Span::new(start, end, source_id);
    Ok(Decl::ExternFuncDecl(buff_lang_ast::ExternFuncDecl {
        abi,
        crate_name,
        name,
        params,
        return_type,
        span,
    }))
}

/// Expect a plain string literal naming an ABI (the `"C"` part of
/// `extern "C"`). Returns the ABI string + the end offset of the closing
/// `"`. Mirrors [`expect_crate_name_string`] but with ABI-specific error
/// messages.
fn expect_abi_string(stream: &mut TokenStream<'_>) -> Result<(String, usize), ParseError> {
    let start_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected ABI string after `extern`, found end of input \
             (Buff supports `extern \"C\" func ...` in v1.3)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(start_tok.kind, TokenKind::StringStart) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "expected ABI string after `extern` (e.g. `extern \"C\" func ...`), found `{}`",
                start_tok.kind
            ),
            start_tok.span,
        )));
    }
    let part_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected ABI string content, found end of input",
            stream.eof_span(),
        ))
    })?;
    let abi = match part_tok.kind {
        TokenKind::StringPart(s) => s,
        TokenKind::InterpStart => {
            return Err(ParseError::new(Diagnostic::error(
                "ABI string cannot contain interpolation",
                part_tok.span,
            )));
        }
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!("expected ABI string content, found `{other}`"),
                part_tok.span,
            )));
        }
    };
    let end_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "unterminated ABI string (missing closing quote)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(end_tok.kind, TokenKind::StringEnd) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "ABI string cannot contain interpolation; expected end of string, found `{}`",
                end_tok.kind
            ),
            end_tok.span,
        )));
    }
    Ok((abi, end_tok.span.end))
}

/// Expect a plain string literal naming a crate (the `"serde"` part of
/// `extern crate "serde"`). Returns the crate name + the end offset of
/// the closing `"`.
///
/// Mirrors [`expect_path_string`] but with crate-specific error messages.
/// The Buff lexer tokenizes every `"..."` as `StringStart, StringPart,
/// StringEnd`; we consume that three-token sequence and reject any
/// interpolated form.
///
/// # Errors
///
/// Returns [`ParseError`] if the next token is not a `StringStart`, if
/// the `StringPart` is missing, if interpolation appears, or if the
/// closing `StringEnd` is missing.
fn expect_crate_name_string(stream: &mut TokenStream<'_>) -> Result<(String, usize), ParseError> {
    let start_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected crate-name string after `extern crate`, found end of input",
            stream.eof_span(),
        ))
    })?;
    if !matches!(start_tok.kind, TokenKind::StringStart) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "expected crate-name string after `extern crate`, found `{}`",
                start_tok.kind
            ),
            start_tok.span,
        )));
    }

    let part_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected crate-name string content, found end of input",
            stream.eof_span(),
        ))
    })?;
    let name = match part_tok.kind {
        TokenKind::StringPart(s) => s,
        TokenKind::InterpStart => {
            return Err(ParseError::new(Diagnostic::error(
                "crate-name string cannot contain interpolation",
                part_tok.span,
            )));
        }
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!("expected crate-name string content, found `{other}`"),
                part_tok.span,
            )));
        }
    };

    let end_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "unterminated crate-name string (missing closing quote)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(end_tok.kind, TokenKind::StringEnd) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "crate-name string cannot contain interpolation; expected end of string, found `{}`",
                end_tok.kind
            ),
            end_tok.span,
        )));
    }

    Ok((name, end_tok.span.end))
}
