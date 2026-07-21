//! 3-mode lexer for `.buffhtml` source.
//!
//! Modes (per decision record §3):
//! - `TEXT`: HTML text + tag scanning (`<...>`)
//! - `BUFF_CODE`: inside `{ expr }` — captures the raw expression source for
//!   the codegen to splice verbatim into the `rsx!{}` body.
//! - `BUFF_DIRECTIVE`: inside `{# ... }`, `{: ... }`, `{/ ... }`, `{@ ... }`
//!   control-flow markers (`{#each}`, `{:else if c}`, `{/if}`, etc.) plus
//!   Buff directive comments `{# comment #}`.
//!
//! The lexer is a single byte scanner. Brace-matching for `{...}` regions
//! respects nested `{...}` and `"..."` string literals so a `"}"` inside a
//! string does not terminate the interpolation.
//!
//! Output: a flat [`Vec<BuffHtmlToken>`] consumed by [`crate::parser`].

use buff_lang_error::{SourceId, Span};

use crate::error::BuffHtmlParseError;

/// Token kind produced by the `.buffhtml` lexer.
#[derive(Debug, Clone, PartialEq)]
pub enum BuffHtmlTokenKind {
    /// Raw HTML text run (possibly empty if zero bytes between tags).
    /// Whitespace-only runs ARE preserved — the parser decides whether to
    /// trim them (mirrors Svelte's behavior of collapsing whitespace).
    Text(String),
    /// `<tagname` open-tag start (just the leading wedge + name; the parser
    /// reads attributes / `>` / `/>` from subsequent tokens).
    OpenTagStart(String),
    /// `</tagname>` close tag (complete, including trailing `>`).
    CloseTag(String),
    /// `>` end of an open tag.
    TagEnd,
    /// `/>` self-close of an open tag.
    TagSelfClose,
    /// `<>` fragment open.
    FragmentOpen,
    /// `</>` fragment close.
    FragmentClose,
    /// Attribute / prop name. The lexer does NOT classify the attribute kind
    /// (literal / expression / event / named-prop); the parser does that by
    /// looking at the next token (`=`, `:`, or `{`).
    AttrName(String),
    /// `=` (attribute value separator).
    AttrEq,
    /// `:` (named-prop separator — `name: value`).
    AttrColon,
    /// `"..."` or `'...'` literal attribute value (quotes stripped).
    AttrStrLit(String),
    /// `{expr}` interpolation. `expr` is the raw source between the braces
    /// (trimmed of one leading / trailing space for ergonomics).
    Interp(String),
    /// `{#each iterable as binding}` or `{#each iterable as binding, index}`.
    EachOpen {
        iterable: String,
        binding: String,
        index: Option<String>,
    },
    /// `{/each}`.
    EachClose,
    /// `{#if cond}`.
    IfOpen(String),
    /// `{:else if cond}`.
    ElseIf(String),
    /// `{:else}`.
    Else,
    /// `{/if}`.
    IfClose,
    /// `<!-- ... -->` HTML comment (text trimmed).
    HtmlComment(String),
    /// `{# this is a Buff directive comment #}` (text trimmed).
    /// Disambiguation: a `{#` followed by `each` / `if` is a control-flow
    /// directive (EachOpen / IfOpen); a `{#` followed by anything else (or
    /// terminated by `#}`) is a BuffComment.
    BuffComment(String),
    /// `<slot` opening — emitted so the parser can build [`RsxSlot`] and
    /// reject `name=` attributes (deferred per decision record §6).
    SlotOpen,
    /// `<script ...>` opening. The lexer then emits [`Self::ScriptText`] for
    /// the raw body and [`Self::ScriptClose`] for `</script>`.
    ScriptOpen {
        lang: String,
    },
    ScriptText(String),
    ScriptClose,
    /// Synthetic end-of-input marker.
    Eof,
}

/// A single lexer-produced token with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct BuffHtmlToken {
    pub kind: BuffHtmlTokenKind,
    pub span: Span,
}

impl BuffHtmlToken {
    pub fn new(kind: BuffHtmlTokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Convenience: just the kind.
    pub fn kind(&self) -> &BuffHtmlTokenKind {
        &self.kind
    }
}

/// Tokenize a `.buffhtml` source string.
///
/// `source_id` is attached to every emitted span (re-used from
/// `buff_lang_error` so downstream diagnostics share the source map).
pub fn tokenize(
    source: &str,
    source_id: SourceId,
) -> Result<Vec<BuffHtmlToken>, BuffHtmlParseError> {
    let mut lx = LexerState::new(source, source_id);
    lx.scan_all()?;
    lx.push_text_if_any();
    lx.tokens.push(BuffHtmlToken::new(
        BuffHtmlTokenKind::Eof,
        Span::new(lx.bytes.len().saturating_sub(1), lx.bytes.len(), source_id),
    ));
    Ok(lx.tokens)
}

struct LexerState<'src> {
    src: &'src str,
    bytes: &'src [u8],
    pos: usize,
    /// Start of an in-progress TEXT run. `None` when not accumulating text.
    text_start: Option<usize>,
    source_id: SourceId,
    tokens: Vec<BuffHtmlToken>,
}

impl<'src> LexerState<'src> {
    fn new(src: &'src str, source_id: SourceId) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            text_start: None,
            source_id,
            tokens: Vec::new(),
        }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(start, end, self.source_id)
    }

    /// Flush any pending TEXT run into a `Text` token.
    fn push_text_if_any(&mut self) {
        if let Some(start) = self.text_start.take() {
            if start < self.pos {
                let text = self.src[start..self.pos].to_string();
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::Text(text),
                    self.span(start, self.pos),
                ));
            }
        }
    }

    fn begin_text_if_none(&mut self) {
        if self.text_start.is_none() {
            self.text_start = Some(self.pos);
        }
    }

    /// Top-level scan loop. Advances `self.pos` from 0 to `bytes.len()`.
    fn scan_all(&mut self) -> Result<(), BuffHtmlParseError> {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            match b {
                b'<' => {
                    self.push_text_if_any();
                    self.scan_tag()?;
                }
                b'{' => {
                    self.push_text_if_any();
                    self.scan_brace()?;
                }
                // TEXT mode — accumulate.
                _ => {
                    self.begin_text_if_none();
                    self.pos += 1;
                }
            }
        }
        Ok(())
    }

    /// Scan a `<...>` region. dispatches on the byte after `<`:
    /// - `!` → HTML comment `<!-- ... -->`
    /// - `/` → close tag `</tagname>` or fragment close `</>`
    /// - alpha → open tag `<tagname ...>`, fragment `<>`, slot, or script
    fn scan_tag(&mut self) -> Result<(), BuffHtmlParseError> {
        debug_assert_eq!(self.bytes[self.pos], b'<');
        let start = self.pos;
        self.pos += 1; // consume `<`

        if self.pos >= self.bytes.len() {
            // Stray `<` at EOF — emit as text.
            self.text_start = Some(start);
            self.pos = start + 1;
            self.begin_text_if_none();
            return Ok(());
        }

        match self.bytes[self.pos] {
            b'!' => self.scan_html_comment(start),
            b'/' => self.scan_close_tag(start),
            b'>' => {
                // `<>` fragment open.
                self.pos += 1;
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::FragmentOpen,
                    self.span(start, self.pos),
                ));
                Ok(())
            }
            first if first.is_ascii_alphabetic() => self.scan_open_tag(start),
            _ => {
                // Non-tag `<` (e.g. `< 5` in math expression) — treat as text.
                self.text_start = Some(start);
                self.pos = start + 1;
                self.begin_text_if_none();
                Ok(())
            }
        }
    }

    /// `<!-- ... -->` HTML comment.
    fn scan_html_comment(&mut self, start: usize) -> Result<(), BuffHtmlParseError> {
        // Current pos points at `!`. Require `<!--`.
        if !self.consume_literal(b"!--") {
            // Not a comment — emit the `<` as text and let the outer loop
            // re-dispatch on `!`.
            self.text_start = Some(start);
            self.pos = start + 1;
            self.begin_text_if_none();
            return Ok(());
        }
        let body_start = self.pos;
        // Find the next `-->`.
        while self.pos + 2 < self.bytes.len()
            && !(self.bytes[self.pos] == b'-'
                && self.bytes[self.pos + 1] == b'-'
                && self.bytes[self.pos + 2] == b'>')
        {
            self.pos += 1;
        }
        let body_end = self.pos;
        if self.pos + 2 >= self.bytes.len() {
            return Err(BuffHtmlParseError::lex(
                "unterminated HTML comment (missing `-->`)",
                self.span(start, self.bytes.len()),
            ));
        }
        let body = self.src[body_start..body_end].trim().to_string();
        self.pos += 3; // consume `-->`
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::HtmlComment(body),
            self.span(start, self.pos),
        ));
        Ok(())
    }

    /// `</tagname>` or `</>`.
    fn scan_close_tag(&mut self, start: usize) -> Result<(), BuffHtmlParseError> {
        // Current pos points at `/`.
        self.pos += 1; // consume `/`
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'>' {
            self.pos += 1;
            self.tokens.push(BuffHtmlToken::new(
                BuffHtmlTokenKind::FragmentClose,
                self.span(start, self.pos),
            ));
            return Ok(());
        }
        let name = self.read_tag_name().ok_or_else(|| {
            BuffHtmlParseError::lex("expected tag name after `</`", self.span(start, self.pos))
        })?;
        self.skip_ws();
        if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'>' {
            return Err(BuffHtmlParseError::lex(
                "expected `>` to close tag",
                self.span(start, self.pos),
            ));
        }
        self.pos += 1; // consume `>`
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::CloseTag(name),
            self.span(start, self.pos),
        ));
        Ok(())
    }

    /// `<tagname ...>`, `<tagname ... />`, `<>` (fragment), `<slot .../>`,
    /// `<script ...> ... </script>`.
    fn scan_open_tag(&mut self, start: usize) -> Result<(), BuffHtmlParseError> {
        let name = self.read_tag_name().ok_or_else(|| {
            BuffHtmlParseError::lex("expected tag name after `<`", self.span(start, self.pos))
        })?;
        match name.as_str() {
            "slot" => {
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::SlotOpen,
                    self.span(start, start + name.len() + 1),
                ));
                // Still need to consume attributes and `/>` or `>`.
                self.scan_attributes()?;
                self.expect_tag_end(start, /* allow_self_close */ true)?;
                Ok(())
            }
            "script" => {
                // Parse attributes locally (we only care about `lang="..."`).
                let lang = self.scan_script_attrs()?;
                // Consume the trailing `>` directly (do NOT emit TagEnd —
                // ScriptOpen's span already encompasses it).
                self.skip_ws();
                if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'>' {
                    return Err(BuffHtmlParseError::lex(
                        "expected `>` to end `<script ...>`",
                        self.span(self.pos, self.pos),
                    ));
                }
                self.pos += 1; // consume `>`
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::ScriptOpen { lang },
                    self.span(start, self.pos),
                ));
                // Body: raw text until `</script>`.
                self.scan_script_body(start)?;
                Ok(())
            }
            _ => {
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::OpenTagStart(name),
                    self.span(start, self.pos),
                ));
                self.scan_attributes()?;
                self.expect_tag_end(start, /* allow_self_close */ true)?;
                Ok(())
            }
        }
    }

    /// Scan attributes for a `<script>` block WITHOUT emitting attribute
    /// tokens into the output stream — only `lang` is captured.
    fn scan_script_attrs(&mut self) -> Result<String, BuffHtmlParseError> {
        let mut lang = String::from("buff");
        loop {
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err(BuffHtmlParseError::lex(
                    "unterminated `<script>` tag",
                    self.span(self.pos, self.pos),
                ));
            }
            match self.bytes[self.pos] {
                b'>' => return Ok(lang),
                b'/' => {
                    if self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'>' {
                        return Err(BuffHtmlParseError::lex(
                            "`<script />` self-close is not allowed — must have body",
                            self.span(self.pos, self.pos + 2),
                        ));
                    }
                    return Err(BuffHtmlParseError::lex(
                        "unexpected `/` in <script>",
                        self.span(self.pos, self.pos + 1),
                    ));
                }
                _ => {
                    // name = "value"
                    let name = self.read_attr_name().ok_or_else(|| {
                        BuffHtmlParseError::lex(
                            "expected attribute name in <script>",
                            self.span(self.pos, self.pos),
                        )
                    })?;
                    self.skip_ws_inline();
                    if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                        self.pos += 1;
                        self.skip_ws_inline();
                        if self.pos < self.bytes.len()
                            && (self.bytes[self.pos] == b'"' || self.bytes[self.pos] == b'\'')
                        {
                            let quote = self.bytes[self.pos];
                            self.pos += 1;
                            let val_start = self.pos;
                            while self.pos < self.bytes.len() && self.bytes[self.pos] != quote {
                                self.pos += 1;
                            }
                            let val = self.src[val_start..self.pos].to_string();
                            if self.pos < self.bytes.len() {
                                self.pos += 1; // consume closing quote
                            }
                            if name == "lang" {
                                lang = val;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Scan the raw body of a `<script>...</script>` block up to the literal
    /// `</script>` close.
    fn scan_script_body(&mut self, script_open_start: usize) -> Result<(), BuffHtmlParseError> {
        let body_start = self.pos;
        let mut close = None;
        while self.pos + 8 < self.bytes.len() {
            if &self.bytes[self.pos..self.pos + 9] == b"</script>" {
                close = Some(self.pos);
                break;
            }
            self.pos += 1;
        }
        let body_end = match close {
            Some(e) => e,
            None => {
                return Err(BuffHtmlParseError::lex(
                    "unterminated `<script>` block (missing `</script>`)",
                    self.span(script_open_start, self.bytes.len()),
                ));
            }
        };
        let body = self.src[body_start..body_end].to_string();
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::ScriptText(body),
            self.span(body_start, body_end),
        ));
        self.pos += 9; // consume `</script>`
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::ScriptClose,
            self.span(body_end, self.pos),
        ));
        Ok(())
    }

    /// Scan attributes / props / event handlers after an open-tag name.
    /// Stops at `>` or `/>`.
    fn scan_attributes(&mut self) -> Result<(), BuffHtmlParseError> {
        loop {
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err(BuffHtmlParseError::lex(
                    "unterminated tag (missing `>`)",
                    self.span(self.pos, self.pos),
                ));
            }
            match self.bytes[self.pos] {
                b'>' => return Ok(()),
                b'/' => {
                    // Expect `/>`.
                    if self.pos + 1 < self.bytes.len() && self.bytes[self.pos + 1] == b'>' {
                        return Ok(()); // TagSelfClose emitted by expect_tag_end
                    }
                    return Err(BuffHtmlParseError::lex(
                        "unexpected `/` in tag (did you mean `/>`?)",
                        self.span(self.pos, self.pos + 1),
                    ));
                }
                _ => self.scan_one_attribute()?,
            }
        }
    }

    /// Scan a single attribute: `name`, `name="lit"`, `name={expr}`,
    /// `on:event_mod={expr}`, `name: value` (named prop).
    fn scan_one_attribute(&mut self) -> Result<(), BuffHtmlParseError> {
        let attr_start = self.pos;
        let name = self.read_attr_name().ok_or_else(|| {
            BuffHtmlParseError::lex("expected attribute name", self.span(attr_start, self.pos))
        })?;
        // Split `on:event` at the `:` — emit as AttrName("on:event") so the
        // parser can interpret it as an event handler. (The lexer keeps the
        // name together; the parser does the splitting.)
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::AttrName(name),
            self.span(attr_start, self.pos),
        ));
        self.skip_ws_inline();
        if self.pos >= self.bytes.len() {
            return Err(BuffHtmlParseError::lex(
                "unexpected EOF inside tag",
                self.span(attr_start, self.pos),
            ));
        }
        match self.bytes[self.pos] {
            b'=' => {
                self.pos += 1;
                self.skip_ws_inline();
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::AttrEq,
                    self.span(attr_start, self.pos),
                ));
                self.scan_attr_value()?;
                Ok(())
            }
            b':' => {
                self.pos += 1;
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::AttrColon,
                    self.span(attr_start, self.pos),
                ));
                // Named-prop value: read until `>` or whitespace or `{`.
                self.skip_ws_inline();
                self.scan_named_prop_value()?;
                Ok(())
            }
            // Bare boolean attribute.
            _ => Ok(()),
        }
    }

    /// `"..."` or `'...'` or `{expr}` (expression attribute value).
    fn scan_attr_value(&mut self) -> Result<(), BuffHtmlParseError> {
        if self.pos >= self.bytes.len() {
            return Err(BuffHtmlParseError::lex(
                "expected attribute value",
                self.span(self.pos, self.pos),
            ));
        }
        match self.bytes[self.pos] {
            b'"' | b'\'' => self.scan_quoted_string(),
            b'{' => self.scan_brace(),
            _ => Err(BuffHtmlParseError::lex(
                "attribute value must be `\"...\"`, `'...'`, or `{expr}`",
                self.span(self.pos, self.pos + 1),
            )),
        }
    }

    /// Named-prop value: a bare word, a `"..."` literal, or `{expr}`.
    fn scan_named_prop_value(&mut self) -> Result<(), BuffHtmlParseError> {
        if self.pos >= self.bytes.len() {
            return Err(BuffHtmlParseError::lex(
                "expected named-prop value after `:`",
                self.span(self.pos, self.pos),
            ));
        }
        match self.bytes[self.pos] {
            b'"' | b'\'' => self.scan_quoted_string(),
            b'{' => self.scan_brace(),
            _ => {
                // Bare word.
                let start = self.pos;
                while self.pos < self.bytes.len() {
                    let b = self.bytes[self.pos];
                    if b.is_ascii_whitespace() || b == b'>' || b == b'/' || b == b'{' {
                        break;
                    }
                    self.pos += 1;
                }
                let word = self.src[start..self.pos].to_string();
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::AttrStrLit(word),
                    self.span(start, self.pos),
                ));
                Ok(())
            }
        }
    }

    /// `"..."` or `'...'` (quotes stripped).
    fn scan_quoted_string(&mut self) -> Result<(), BuffHtmlParseError> {
        let quote = self.bytes[self.pos];
        let start = self.pos;
        self.pos += 1;
        let body_start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != quote {
            // Allow interpolated `{expr}` inside the string — we capture it
            // as part of the literal body (codegen will splice it back).
            // No escape processing today — T133 floor: literal verbatim.
            self.pos += 1;
        }
        let body_end = self.pos;
        if self.pos >= self.bytes.len() {
            return Err(BuffHtmlParseError::lex(
                "unterminated string literal",
                self.span(start, self.pos),
            ));
        }
        let body = self.src[body_start..body_end].to_string();
        self.pos += 1; // consume closing quote
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::AttrStrLit(body),
            self.span(start, self.pos),
        ));
        Ok(())
    }

    /// Consume the trailing `>` or `/>` of a tag and emit TagEnd / TagSelfClose.
    fn expect_tag_end(
        &mut self,
        start: usize,
        allow_self_close: bool,
    ) -> Result<(), BuffHtmlParseError> {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return Err(BuffHtmlParseError::lex(
                "unterminated tag (missing `>`)",
                self.span(start, self.pos),
            ));
        }
        match self.bytes[self.pos] {
            b'>' => {
                self.pos += 1;
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::TagEnd,
                    self.span(start, self.pos),
                ));
                Ok(())
            }
            b'/' if allow_self_close => {
                if self.pos + 1 >= self.bytes.len() || self.bytes[self.pos + 1] != b'>' {
                    return Err(BuffHtmlParseError::lex(
                        "expected `/>` to self-close tag",
                        self.span(self.pos, self.pos + 1),
                    ));
                }
                self.pos += 2;
                self.tokens.push(BuffHtmlToken::new(
                    BuffHtmlTokenKind::TagSelfClose,
                    self.span(start, self.pos),
                ));
                Ok(())
            }
            _ => Err(BuffHtmlParseError::lex(
                "expected `>` or `/>` to end tag",
                self.span(self.pos, self.pos + 1),
            )),
        }
    }

    /// `{...}` — interpolation or directive.
    fn scan_brace(&mut self) -> Result<(), BuffHtmlParseError> {
        debug_assert_eq!(self.bytes[self.pos], b'{');
        let brace_start = self.pos;
        // Peek at next byte to dispatch.
        if self.pos + 1 >= self.bytes.len() {
            return Err(BuffHtmlParseError::lex(
                "unterminated `{` (missing `}`)",
                self.span(brace_start, self.bytes.len()),
            ));
        }
        let next = self.bytes[self.pos + 1];
        match next {
            b'#' => {
                // `{#...}` — directive. Could be EachOpen, IfOpen, or BuffComment.
                // Peek the keyword: skip `#`, skip spaces.
                let keyword_start = self.pos + 2;
                let mut p = keyword_start;
                while p < self.bytes.len() && self.bytes[p].is_ascii_whitespace() {
                    p += 1;
                }
                let kw_start = p;
                while p < self.bytes.len()
                    && (self.bytes[p].is_ascii_alphabetic() || self.bytes[p] == b'_')
                {
                    p += 1;
                }
                let kw = &self.src[kw_start..p];
                match kw {
                    "each" => self.scan_each_open(brace_start),
                    "if" => self.scan_if_open(brace_start),
                    _ => {
                        // BuffComment `{# ... #}`.
                        self.scan_buff_comment(brace_start)
                    }
                }
            }
            b':' => self.scan_else_directive(brace_start),
            b'/' => self.scan_block_close(brace_start),
            b'@' => Err(BuffHtmlParseError::lex(
                "`{@html}` is deferred to T134+ (decision record §6)",
                self.span(brace_start, brace_start + 1),
            )),
            _ => self.scan_interp(brace_start),
        }
    }

    /// `{expr}` interpolation.
    fn scan_interp(&mut self, brace_start: usize) -> Result<(), BuffHtmlParseError> {
        self.pos += 1; // consume `{`
        let body_start = self.pos;
        let close = find_matching_brace(self.bytes, self.pos).ok_or_else(|| {
            BuffHtmlParseError::lex(
                "unterminated `{expr}` (missing `}`)",
                self.span(brace_start, self.bytes.len()),
            )
        })?;
        // Move pos forward through body; we'll slice the raw expr from src.
        self.pos = close;
        let raw_body = &self.src[body_start..close];
        let trimmed = raw_body.trim();
        self.pos += 1; // consume `}`
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::Interp(trimmed.to_string()),
            self.span(brace_start, self.pos),
        ));
        Ok(())
    }

    /// `{#each iterable as binding}` or `{#each iterable as binding, index}`.
    fn scan_each_open(&mut self, brace_start: usize) -> Result<(), BuffHtmlParseError> {
        // Move past `{#each` keyword.
        let close = find_matching_brace(self.bytes, brace_start + 1).ok_or_else(|| {
            BuffHtmlParseError::lex(
                "unterminated `{#each ...` (missing `}`)",
                self.span(brace_start, self.bytes.len()),
            )
        })?;
        let body = self.src[brace_start + 1..close].trim();
        // body starts with `#each`.
        let rest = body.strip_prefix("#each").map(str::trim).ok_or_else(|| {
            BuffHtmlParseError::lex(
                "malformed `{#each}` directive",
                self.span(brace_start, close + 1),
            )
        })?;
        // rest = "iterable as binding" or "iterable as binding, index".
        // Reject keyed form `iterable as binding (key)` (T134+).
        if rest.contains('(') {
            return Err(BuffHtmlParseError::lex(
                "keyed each (`(key)`) is deferred to T134+ (decision record §6)",
                self.span(brace_start, close + 1),
            ));
        }
        let (iterable, binding_part) = match rest.split_once(" as ") {
            None => {
                return Err(BuffHtmlParseError::lex(
                    "`{#each}` requires `as` binding (`{#each items as item}`)",
                    self.span(brace_start, close + 1),
                ));
            }
            Some(pair) => pair,
        };
        let iterable = iterable.trim().to_string();
        let (binding, index) = parse_binding_and_index(binding_part.trim());
        self.pos = close + 1; // consume through `}`
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::EachOpen {
                iterable,
                binding,
                index,
            },
            self.span(brace_start, self.pos),
        ));
        Ok(())
    }

    /// `{#if cond}`.
    fn scan_if_open(&mut self, brace_start: usize) -> Result<(), BuffHtmlParseError> {
        let close = find_matching_brace(self.bytes, brace_start + 1).ok_or_else(|| {
            BuffHtmlParseError::lex(
                "unterminated `{#if ...` (missing `}`)",
                self.span(brace_start, self.bytes.len()),
            )
        })?;
        let body = self.src[brace_start + 1..close].trim();
        let cond = body
            .strip_prefix("#if")
            .map(str::trim)
            .ok_or_else(|| {
                BuffHtmlParseError::lex(
                    "malformed `{#if}` directive",
                    self.span(brace_start, close + 1),
                )
            })?
            .to_string();
        self.pos = close + 1;
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::IfOpen(cond),
            self.span(brace_start, self.pos),
        ));
        Ok(())
    }

    /// `{:else if cond}` or `{:else}`.
    fn scan_else_directive(&mut self, brace_start: usize) -> Result<(), BuffHtmlParseError> {
        let close = find_matching_brace(self.bytes, brace_start + 1).ok_or_else(|| {
            BuffHtmlParseError::lex(
                "unterminated `{:else ...` (missing `}`)",
                self.span(brace_start, self.bytes.len()),
            )
        })?;
        let body = self.src[brace_start + 1..close].trim();
        let rest = body.strip_prefix(':').map(str::trim).ok_or_else(|| {
            BuffHtmlParseError::lex(
                "malformed `{:...}` directive",
                self.span(brace_start, close + 1),
            )
        })?;
        if rest == "else" {
            self.pos = close + 1;
            self.tokens.push(BuffHtmlToken::new(
                BuffHtmlTokenKind::Else,
                self.span(brace_start, self.pos),
            ));
            return Ok(());
        }
        let cond = rest
            .strip_prefix("else if")
            .map(str::trim)
            .ok_or_else(|| {
                BuffHtmlParseError::lex(
                    "expected `{:else}` or `{:else if cond}`",
                    self.span(brace_start, close + 1),
                )
            })?
            .to_string();
        self.pos = close + 1;
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::ElseIf(cond),
            self.span(brace_start, self.pos),
        ));
        Ok(())
    }

    /// `{/each}` or `{/if}`.
    fn scan_block_close(&mut self, brace_start: usize) -> Result<(), BuffHtmlParseError> {
        let close = find_matching_brace(self.bytes, brace_start + 1).ok_or_else(|| {
            BuffHtmlParseError::lex(
                "unterminated `{/...}` (missing `}`)",
                self.span(brace_start, self.bytes.len()),
            )
        })?;
        let body = self.src[brace_start + 1..close].trim();
        let kw = body.strip_prefix('/').map(str::trim).ok_or_else(|| {
            BuffHtmlParseError::lex(
                "malformed `{/...}` directive",
                self.span(brace_start, close + 1),
            )
        })?;
        self.pos = close + 1;
        let kind = match kw {
            "each" => BuffHtmlTokenKind::EachClose,
            "if" => BuffHtmlTokenKind::IfClose,
            other => {
                return Err(BuffHtmlParseError::lex(
                    format!("unknown block close `{{/{other}}}`"),
                    self.span(brace_start, self.pos),
                ));
            }
        };
        self.tokens
            .push(BuffHtmlToken::new(kind, self.span(brace_start, self.pos)));
        Ok(())
    }

    /// `{# comment #}` Buff directive comment (any `{#...}` that is not
    /// `{#each` / `{#if`).
    fn scan_buff_comment(&mut self, brace_start: usize) -> Result<(), BuffHtmlParseError> {
        // Find `#}` terminator.
        let mut p = brace_start + 1;
        while p + 1 < self.bytes.len() && !(self.bytes[p] == b'#' && self.bytes[p + 1] == b'}') {
            p += 1;
        }
        if p + 1 >= self.bytes.len() {
            return Err(BuffHtmlParseError::lex(
                "unterminated Buff comment (missing `#}`)",
                self.span(brace_start, self.bytes.len()),
            ));
        }
        let body = self.src[brace_start + 1..p]
            .trim_start_matches('#')
            .trim()
            .to_string();
        self.pos = p + 2; // consume `#}`
        self.tokens.push(BuffHtmlToken::new(
            BuffHtmlTokenKind::BuffComment(body),
            self.span(brace_start, self.pos),
        ));
        Ok(())
    }

    // ----- small byte-scanner helpers ------------------------------------

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn skip_ws_inline(&mut self) {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b' ' || b == b'\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Read an ASCII tag name (`[A-Za-z][A-Za-z0-9_-]*`). Returns `None` if
    /// the next byte is not a valid name start.
    fn read_tag_name(&mut self) -> Option<String> {
        if self.pos >= self.bytes.len() || !self.bytes[self.pos].is_ascii_alphabetic() {
            return None;
        }
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
                self.pos += 1;
            } else {
                break;
            }
        }
        Some(self.src[start..self.pos].to_string())
    }

    /// Read an attribute name. Attribute names may include `:` (for `on:event`
    /// and `bind:value`) and `-` (for `aria-label`). A trailing `:` is NOT
    /// consumed — it is left as the start of the next token so the named-prop
    /// form `name: value` is correctly detected.
    fn read_attr_name(&mut self) -> Option<String> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let b = self.bytes[self.pos];
        if !b.is_ascii_alphabetic() && b != b'_' && b != b':' && b != b'@' {
            return None;
        }
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_alphanumeric()
                || b == b'_'
                || b == b'-'
                || b == b':'
                || b == b'.'
                || b == b'@'
            {
                self.pos += 1;
            } else {
                break;
            }
        }
        // Back up if we ended on a `:` — the colon is the named-prop
        // separator and belongs to the next token.
        let end = self.pos;
        if end > start && self.src.as_bytes().get(end - 1) == Some(&b':') {
            self.pos -= 1;
            return Some(self.src[start..end - 1].to_string());
        }
        Some(self.src[start..end].to_string())
    }

    /// Try to consume the given 3-byte literal at the cursor. On success,
    /// advance and return `true`; otherwise return `false` without advancing.
    fn consume_literal(&mut self, lit: &[u8]) -> bool {
        if self.pos + lit.len() <= self.bytes.len()
            && &self.bytes[self.pos..self.pos + lit.len()] == lit
        {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }
}

/// Parse `binding` or `binding, index` from the post-`as` part of an each directive.
fn parse_binding_and_index(s: &str) -> (String, Option<String>) {
    if let Some((b, i)) = s.split_once(',') {
        (b.trim().to_string(), Some(i.trim().to_string()))
    } else {
        (s.trim().to_string(), None)
    }
}

/// Find the matching `}` for the `{` at `start - 1`. Respects nested braces
/// and `"..."` strings so `{"}"} ` works.
fn find_matching_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            b'"' | b'\'' => {
                // Skip string literal.
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1; // consume closing quote
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(src: &str) -> Vec<BuffHtmlTokenKind> {
        tokenize(src, SourceId(0))
            .expect("tokenize failed")
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn empty_input_yields_eof() {
        let k = t("");
        assert_eq!(k.len(), 1);
        assert!(matches!(k[0], BuffHtmlTokenKind::Eof));
    }

    #[test]
    fn plain_text() {
        let k = t("hello world");
        assert!(matches!(&k[0], BuffHtmlTokenKind::Text(s) if s == "hello world"));
    }

    #[test]
    fn element_open_and_close() {
        let k = t("<div></div>");
        assert!(matches!(&k[0], BuffHtmlTokenKind::OpenTagStart(n) if n == "div"));
        assert!(matches!(k[1], BuffHtmlTokenKind::TagEnd));
        assert!(matches!(&k[2], BuffHtmlTokenKind::CloseTag(n) if n == "div"));
    }

    #[test]
    fn self_closing_element() {
        let k = t("<br />");
        assert!(matches!(&k[0], BuffHtmlTokenKind::OpenTagStart(n) if n == "br"));
        assert!(matches!(k[1], BuffHtmlTokenKind::TagSelfClose));
    }

    #[test]
    fn fragment_open_close() {
        let k = t("<></>");
        assert!(matches!(k[0], BuffHtmlTokenKind::FragmentOpen));
        assert!(matches!(k[1], BuffHtmlTokenKind::FragmentClose));
    }

    #[test]
    fn interpolation() {
        let k = t("{count}");
        assert!(matches!(&k[0], BuffHtmlTokenKind::Interp(e) if e == "count"));
    }

    #[test]
    fn interpolation_with_expr() {
        let k = t("{a + b}");
        assert!(matches!(&k[0], BuffHtmlTokenKind::Interp(e) if e == "a + b"));
    }

    #[test]
    fn interpolation_with_nested_braces_in_string() {
        // `{"}"} ` should lex as one interpolation.
        let k = t("{\"}\"}");
        assert!(matches!(&k[0], BuffHtmlTokenKind::Interp(_)));
    }

    #[test]
    fn each_open() {
        let k = t("{#each items as item}");
        match &k[0] {
            BuffHtmlTokenKind::EachOpen {
                iterable,
                binding,
                index,
            } => {
                assert_eq!(iterable, "items");
                assert_eq!(binding, "item");
                assert_eq!(*index, None);
            }
            other => panic!("expected EachOpen, got {other:?}"),
        }
    }

    #[test]
    fn each_open_with_index() {
        let k = t("{#each items as item, i}");
        match &k[0] {
            BuffHtmlTokenKind::EachOpen {
                iterable,
                binding,
                index,
            } => {
                assert_eq!(iterable, "items");
                assert_eq!(binding, "item");
                assert_eq!(*index, Some("i".to_string()));
            }
            other => panic!("expected EachOpen, got {other:?}"),
        }
    }

    #[test]
    fn each_open_rejects_keyed_form() {
        let r = tokenize("{#each xs as x (x.id)}", SourceId(0));
        assert!(r.is_err());
    }

    #[test]
    fn if_else_directives() {
        let k = t("{#if c}{:else if d}{:else}{/if}");
        assert!(matches!(&k[0], BuffHtmlTokenKind::IfOpen(c) if c == "c"));
        assert!(matches!(&k[1], BuffHtmlTokenKind::ElseIf(c) if c == "d"));
        assert!(matches!(k[2], BuffHtmlTokenKind::Else));
        assert!(matches!(k[3], BuffHtmlTokenKind::IfClose));
    }

    #[test]
    fn each_close() {
        let k = t("{/each}");
        assert!(matches!(k[0], BuffHtmlTokenKind::EachClose));
    }

    #[test]
    fn html_comment() {
        let k = t("<!-- hi -->");
        assert!(matches!(&k[0], BuffHtmlTokenKind::HtmlComment(s) if s == "hi"));
    }

    #[test]
    fn buff_comment() {
        let k = t("{# this is a comment #}");
        assert!(matches!(&k[0], BuffHtmlTokenKind::BuffComment(s) if s == "this is a comment"));
    }

    #[test]
    fn slot_open_emitted() {
        let k = t("<slot />");
        assert!(matches!(k[0], BuffHtmlTokenKind::SlotOpen));
        assert!(matches!(k[1], BuffHtmlTokenKind::TagSelfClose));
    }

    #[test]
    fn script_block_captures_body() {
        let k = t("<script lang=\"buff\">hello</script>");
        assert!(matches!(&k[0], BuffHtmlTokenKind::ScriptOpen { lang } if lang == "buff"));
        assert!(matches!(&k[1], BuffHtmlTokenKind::ScriptText(s) if s == "hello"));
        assert!(matches!(k[2], BuffHtmlTokenKind::ScriptClose));
    }

    #[test]
    fn on_event_attribute_emits_as_attrname() {
        let k = t("<button on:click={h}>x</button>");
        // The lexer emits `on:click` as a single AttrName token (kept together).
        assert!(matches!(
            &k[1],
            BuffHtmlTokenKind::AttrName(n) if n == "on:click"
        ));
    }

    #[test]
    fn named_prop_form() {
        let k = t("<Greeting name: \"Alice\" />");
        // OpenTagStart, AttrName, AttrColon, AttrStrLit, TagSelfClose, Eof
        assert!(matches!(&k[0], BuffHtmlTokenKind::OpenTagStart(n) if n == "Greeting"));
        assert!(matches!(&k[1], BuffHtmlTokenKind::AttrName(n) if n == "name"));
        assert!(matches!(k[2], BuffHtmlTokenKind::AttrColon));
        assert!(matches!(&k[3], BuffHtmlTokenKind::AttrStrLit(v) if v == "Alice"));
    }

    #[test]
    fn html_at_html_deferred_errors() {
        let r = tokenize("{@html x}", SourceId(0));
        assert!(r.is_err());
    }

    #[test]
    fn unterminated_interp_errors() {
        let r = tokenize("{ unclosed", SourceId(0));
        assert!(r.is_err());
    }
}
