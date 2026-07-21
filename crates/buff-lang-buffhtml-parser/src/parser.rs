//! Recursive-descent parser over [`crate::lexer`] tokens → [`RsxTemplateFile`].
//!
//! Builds the template tree with span tracking on every node. The parser is
//! hand-rolled (no chumsky), matching the Buff-language parser conventions.
//!
//! # Structure
//!
//! - [`Parser::parse_template`] — entry point. Drains the token slice.
//! - [`Parser::parse_node`] — one element / fragment / text / interp /
//!   directive (`{#if}` / `{#each}` / comment).
//! - [`Parser::parse_element`] — open tag + attributes + children + close tag.
//! - [`Parser::parse_attributes`] — `name="lit"` / `name={expr}` /
//!   `on:event_mod={expr}` / `name: value` / boolean.
//! - [`Parser::parse_children_until`] — text / interp / nested elements.

use buff_lang_ast_rsx::{
    is_component_tag, RsxAttribute, RsxAttributeKind, RsxComment, RsxEach, RsxElement, RsxFragment,
    RsxIf, RsxIfBranch, RsxInterp, RsxNode, RsxSlot, RsxTemplateFile, ScriptBlock, Span,
};
use buff_lang_error::SourceId;

use crate::error::BuffHtmlParseError;
use crate::lexer::{BuffHtmlToken, BuffHtmlTokenKind};

/// Parse a `.buffhtml` source string into an [`RsxTemplateFile`].
pub fn parse(source: &str, source_id: SourceId) -> Result<RsxTemplateFile, BuffHtmlParseError> {
    let tokens = crate::lexer::tokenize(source, source_id)?;
    let mut p = Parser::new(tokens, source_id);
    p.parse_template()
}

struct Parser {
    tokens: Vec<BuffHtmlToken>,
    pos: usize,
    source_id: SourceId,
}

impl Parser {
    fn new(tokens: Vec<BuffHtmlToken>, source_id: SourceId) -> Self {
        Self {
            tokens,
            pos: 0,
            source_id,
        }
    }

    fn peek(&self) -> &BuffHtmlTokenKind {
        &self.tokens[self.pos].kind
    }

    fn span_here(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn advance(&mut self) -> BuffHtmlToken {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), BuffHtmlTokenKind::Eof)
    }

    fn err(&self, msg: impl Into<String>, span: Span) -> BuffHtmlParseError {
        BuffHtmlParseError::parse(msg, span)
    }

    // -------------------------------------------------------------------
    // Template top level.
    // -------------------------------------------------------------------

    fn parse_template(&mut self) -> Result<RsxTemplateFile, BuffHtmlParseError> {
        let start_span = self.span_here();
        let mut script: Option<ScriptBlock> = None;
        let mut root: Vec<RsxNode> = Vec::new();

        // Optional leading `<script lang="buff" props="..."> ... </script>` block.
        if let BuffHtmlTokenKind::ScriptOpen {
            ref lang,
            ref props,
        } = self.peek()
        {
            let lang_clone = lang.clone();
            let props_clone = props.clone();
            let open_span = self.span_here();
            self.advance(); // ScriptOpen
            let body = match self.peek() {
                BuffHtmlTokenKind::ScriptText(s) => {
                    let body = s.clone();
                    self.advance();
                    body
                }
                _ => String::new(),
            };
            // Expect ScriptClose.
            if !matches!(self.peek(), BuffHtmlTokenKind::ScriptClose) {
                return Err(self.err("expected `</script>` after script body", self.span_here()));
            }
            let end_span = self.span_here();
            self.advance();
            script = Some(match props_clone {
                Some(p) => ScriptBlock::with_props(
                    lang_clone,
                    p,
                    body,
                    Span::new(open_span.start, end_span.end, self.source_id),
                ),
                None => ScriptBlock::new(
                    lang_clone,
                    body,
                    Span::new(open_span.start, end_span.end, self.source_id),
                ),
            });
        }

        while !self.is_at_end() {
            let node = self.parse_node()?;
            root.push(node);
        }

        Ok(RsxTemplateFile {
            script,
            root,
            span: Span::new(start_span.start, self.span_here().end, self.source_id),
        })
    }

    // -------------------------------------------------------------------
    // One node.
    // -------------------------------------------------------------------

    fn parse_node(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        match self.peek().clone() {
            BuffHtmlTokenKind::Text(_) => self.parse_text_node(),
            BuffHtmlTokenKind::OpenTagStart(_) => self.parse_element(),
            BuffHtmlTokenKind::FragmentOpen => self.parse_fragment(),
            BuffHtmlTokenKind::SlotOpen => self.parse_slot(),
            BuffHtmlTokenKind::Interp(_) => self.parse_interp(),
            BuffHtmlTokenKind::IfOpen(_) => self.parse_if(),
            BuffHtmlTokenKind::EachOpen { .. } => self.parse_each(),
            BuffHtmlTokenKind::HtmlComment(_) => self.parse_html_comment(),
            BuffHtmlTokenKind::BuffComment(_) => self.parse_buff_comment(),
            BuffHtmlTokenKind::HtmlEscape(_) => self.parse_html_escape(),
            BuffHtmlTokenKind::AwaitOpen(_) => self.parse_await(),
            BuffHtmlTokenKind::ScriptOpen { .. } => {
                // Nested script not allowed (only top-level).
                Err(self.err(
                    "<script> blocks are only allowed at the top of a .buffhtml file",
                    self.span_here(),
                ))
            }
            other => Err(self.err(
                format!("unexpected token at node position: {other:?}"),
                self.span_here(),
            )),
        }
    }

    fn parse_text_node(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let tok = self.advance();
        let (text, span) = match tok.kind {
            BuffHtmlTokenKind::Text(s) => (s, tok.span),
            _ => unreachable!("parse_text_node called on non-text token"),
        };
        Ok(RsxNode::Text(buff_lang_ast_rsx::RsxText::new(text, span)))
    }

    fn parse_interp(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let tok = self.advance();
        let (expr, span) = match tok.kind {
            BuffHtmlTokenKind::Interp(s) => (s, tok.span),
            _ => unreachable!(),
        };
        Ok(RsxNode::Interp(RsxInterp::new(expr, span)))
    }

    fn parse_html_comment(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let tok = self.advance();
        let (text, span) = match tok.kind {
            BuffHtmlTokenKind::HtmlComment(s) => (s, tok.span),
            _ => unreachable!(),
        };
        Ok(RsxNode::Comment(RsxComment::new(text, span)))
    }

    fn parse_buff_comment(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let tok = self.advance();
        let (text, span) = match tok.kind {
            BuffHtmlTokenKind::BuffComment(s) => (s, tok.span),
            _ => unreachable!(),
        };
        Ok(RsxNode::Comment(RsxComment::new(text, span)))
    }

    /// `{@html raw_trusted_html}` raw HTML escape hatch (T133 stretch).
    fn parse_html_escape(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let tok = self.advance();
        let (expr, span) = match tok.kind {
            BuffHtmlTokenKind::HtmlEscape(s) => (s, tok.span),
            _ => unreachable!(),
        };
        Ok(RsxNode::RawHtml(buff_lang_ast_rsx::RsxRawHtml {
            expr,
            span,
        }))
    }

    /// `{#await fut} pending {:then b} ok {:catch b} err {/await}`
    /// (T133 stretch).
    ///
    /// Grammar:
    /// - `{#await fut_expr}` → AwaitOpen
    /// - optional pending body (any nodes)
    /// - `{:then binding}` (required) → AwaitThen
    /// - then body
    /// - optional `{:catch binding}` → AwaitCatch + catch body
    /// - `{/await}` → AwaitClose
    fn parse_await(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let start_span = self.span_here();
        let fut_expr = match self.advance().kind {
            BuffHtmlTokenKind::AwaitOpen(f) => f,
            _ => unreachable!(),
        };
        let fut_span = start_span;

        // Pending body: nodes until AwaitThen, AwaitCatch, or AwaitClose.
        let pending_body = self.parse_children_until_terminators(&[
            Terminator::AwaitThen,
            Terminator::AwaitCatch,
            Terminator::AwaitClose,
        ])?;
        let pending_opt = if pending_body.is_empty() {
            None
        } else {
            Some(pending_body)
        };

        // Required `{:then binding}`.
        let (then_binding, then_body) = match self.peek().clone() {
            BuffHtmlTokenKind::AwaitThen(b) => {
                let binding = b;
                self.advance();
                let body = self.parse_children_until_terminators(&[
                    Terminator::AwaitCatch,
                    Terminator::AwaitClose,
                ])?;
                (binding, body)
            }
            BuffHtmlTokenKind::AwaitCatch(_) => {
                return Err(self.err(
                    "`{#await}` requires a `{:then binding}` before `{:catch}`",
                    self.span_here(),
                ));
            }
            _ => {
                return Err(self.err(
                    "`{#await}` requires a `{:then binding}` block",
                    self.span_here(),
                ));
            }
        };

        // Optional `{:catch binding} body`.
        let mut catch_binding: Option<String> = None;
        let mut catch_body: Option<Vec<RsxNode>> = None;
        if let BuffHtmlTokenKind::AwaitCatch(b) = self.peek().clone() {
            catch_binding = Some(b);
            self.advance();
            let body = self.parse_children_until_terminators(&[Terminator::AwaitClose])?;
            catch_body = Some(body);
        }

        // Required `{/await}`.
        if !matches!(self.peek(), BuffHtmlTokenKind::AwaitClose) {
            return Err(self.err("expected `{/await}` to close await-block", self.span_here()));
        }
        self.advance();

        Ok(RsxNode::Await(buff_lang_ast_rsx::RsxAwait {
            fut_expr,
            fut_span,
            pending_body: pending_opt,
            then_binding,
            then_body,
            catch_binding,
            catch_body,
            span: Span::new(start_span.start, self.span_here().end, self.source_id),
        }))
    }

    fn parse_slot(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let start_span = self.span_here();
        self.advance(); // SlotOpen
                        // Walk attribute tokens. Recognize only `name="..."`. Other attrs → error.
        let mut name_value: Option<String> = None;
        loop {
            match self.peek().clone() {
                BuffHtmlTokenKind::TagSelfClose | BuffHtmlTokenKind::TagEnd => break,
                BuffHtmlTokenKind::AttrName(n) => {
                    if n != "name" {
                        return Err(self.err(
                            format!(
                                "unknown attribute `{n}` on `<slot>` (only `name=` is allowed)"
                            ),
                            self.span_here(),
                        ));
                    }
                    self.advance(); // AttrName("name")
                                    // Expect AttrEq + AttrStrLit.
                    if !matches!(self.peek(), BuffHtmlTokenKind::AttrEq) {
                        return Err(
                            self.err("expected `=` after `name` on `<slot>`", self.span_here())
                        );
                    }
                    self.advance(); // AttrEq
                    let val_tok = self.advance();
                    match val_tok.kind {
                        BuffHtmlTokenKind::AttrStrLit(s) => {
                            if s.is_empty() {
                                return Err(self.err(
                                    "`<slot name=\"...\">` requires a non-empty name",
                                    val_tok.span,
                                ));
                            }
                            if !s
                                .chars()
                                .next()
                                .map(|c| c.is_ascii_alphabetic())
                                .unwrap_or(false)
                            {
                                return Err(self.err(
                                    "slot name must start with an ASCII letter",
                                    val_tok.span,
                                ));
                            }
                            name_value = Some(s);
                        }
                        other => {
                            return Err(self.err(
                                format!(
                                    "`name=` on `<slot>` requires a string literal, got {other:?}"
                                ),
                                val_tok.span,
                            ));
                        }
                    }
                }
                other => {
                    return Err(self.err(
                        format!(
                            "unexpected token inside `<slot>`: {other:?} (only `name=\"...\"` is allowed)"
                        ),
                        self.span_here(),
                    ));
                }
            }
        }
        // Consume TagSelfClose or TagEnd. Slot does not have children —
        // `<slot>...</slot>` form is rejected (must be self-closing or empty).
        let end_tok = self.advance();
        let _ = end_tok;
        Ok(RsxNode::Slot(RsxSlot {
            name: name_value,
            span: Span::new(start_span.start, self.span_here().end, self.source_id),
        }))
    }

    fn parse_fragment(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let start_span = self.span_here();
        self.advance(); // FragmentOpen
        let children = self.parse_children_until_fragment_close()?;
        // Expect FragmentClose.
        let end_span = self.span_here();
        if !matches!(self.peek(), BuffHtmlTokenKind::FragmentClose) {
            return Err(self.err("expected `</>` to close fragment", self.span_here()));
        }
        self.advance();
        Ok(RsxNode::Fragment(RsxFragment::new(
            children,
            Span::new(start_span.start, end_span.end, self.source_id),
        )))
    }

    fn parse_element(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let start_span = self.span_here();
        let tag = match self.advance().kind {
            BuffHtmlTokenKind::OpenTagStart(n) => n,
            _ => unreachable!(),
        };
        let is_component = is_component_tag(&tag);
        let attributes = self.parse_attributes()?;
        // Either TagSelfClose (no children) or TagEnd (children follow).
        let mut self_closing = false;
        match self.peek() {
            BuffHtmlTokenKind::TagSelfClose => {
                self_closing = true;
                self.advance();
            }
            BuffHtmlTokenKind::TagEnd => {
                self.advance();
            }
            other => {
                return Err(self.err(
                    format!("expected `>` or `/>` after element attrs, got {other:?}"),
                    self.span_here(),
                ));
            }
        }
        let children = if self_closing {
            Vec::new()
        } else {
            self.parse_children_until_close(&tag)?
        };
        Ok(RsxNode::Element(RsxElement::new(
            tag,
            is_component,
            attributes,
            children,
            self_closing,
            Span::new(start_span.start, self.span_here().end, self.source_id),
        )))
    }

    // -------------------------------------------------------------------
    // Attributes.
    // -------------------------------------------------------------------

    fn parse_attributes(&mut self) -> Result<Vec<RsxAttribute>, BuffHtmlParseError> {
        let mut out: Vec<RsxAttribute> = Vec::new();
        loop {
            match self.peek() {
                BuffHtmlTokenKind::TagEnd | BuffHtmlTokenKind::TagSelfClose => return Ok(out),
                BuffHtmlTokenKind::AttrSpread(ident) => {
                    let span = self.span_here();
                    let ident = ident.clone();
                    self.advance();
                    out.push(RsxAttribute {
                        kind: RsxAttributeKind::Spread { ident },
                        span,
                    });
                }
                BuffHtmlTokenKind::AttrName(name) => {
                    let attr = self.parse_one_attribute(name.clone())?;
                    out.push(attr);
                }
                other => {
                    return Err(self.err(
                        format!(
                            "expected attribute name, `>`, `/>`, or `{{...spread}}`, got {other:?}"
                        ),
                        self.span_here(),
                    ));
                }
            }
        }
    }

    fn parse_one_attribute(&mut self, name: String) -> Result<RsxAttribute, BuffHtmlParseError> {
        let name_span = self.span_here();
        self.advance(); // AttrName
        match self.peek() {
            BuffHtmlTokenKind::AttrEq => self.parse_eq_attribute(name, name_span),
            BuffHtmlTokenKind::AttrColon => self.parse_named_prop(name, name_span),
            // Boolean attribute: bare `disabled`, `required`, etc.
            _ => Ok(RsxAttribute {
                kind: RsxAttributeKind::Boolean { name },
                span: name_span,
            }),
        }
    }

    /// `name="lit"` or `name={expr}` or `on:event_mod={handler}` or
    /// `bind:value={signal}` (T133 stretch two-way binding).
    fn parse_eq_attribute(
        &mut self,
        name: String,
        name_span: Span,
    ) -> Result<RsxAttribute, BuffHtmlParseError> {
        self.advance(); // AttrEq
        let val_tok = self.advance();
        let kind = match val_tok.kind {
            BuffHtmlTokenKind::AttrStrLit(s) => RsxAttributeKind::Literal { name, value: s },
            BuffHtmlTokenKind::Interp(expr) => {
                // `on:event_mod` → Event variant.
                if let Some((event, modifiers)) = parse_event_name(&name) {
                    RsxAttributeKind::Event {
                        event,
                        modifiers,
                        handler_expr: expr.clone(),
                        handler_span: val_tok.span,
                    }
                } else if let Some(prop) = name.strip_prefix("bind:") {
                    // `bind:value={sig}` two-way binding (T133 stretch).
                    if prop.is_empty() {
                        return Err(self.err(
                            "`bind:` requires a prop name (`bind:value={...}`)",
                            name_span,
                        ));
                    }
                    RsxAttributeKind::Bind {
                        prop: prop.to_string(),
                        signal: expr.clone(),
                    }
                } else {
                    RsxAttributeKind::Expression {
                        name,
                        expr: expr.clone(),
                        expr_span: val_tok.span,
                    }
                }
            }
            other => {
                return Err(self.err(
                    format!("attribute value must be `\"...\"` or `{{expr}}`, got {other:?}"),
                    val_tok.span,
                ));
            }
        };
        Ok(RsxAttribute {
            kind,
            span: Span::new(name_span.start, val_tok.span.end, self.source_id),
        })
    }

    /// `name: value` named prop.
    fn parse_named_prop(
        &mut self,
        name: String,
        name_span: Span,
    ) -> Result<RsxAttribute, BuffHtmlParseError> {
        self.advance(); // AttrColon
        let val_tok = self.advance();
        let (value, value_span): (String, Span) = match val_tok.kind {
            BuffHtmlTokenKind::AttrStrLit(s) => (s, val_tok.span),
            BuffHtmlTokenKind::Interp(s) => (s, val_tok.span),
            other => {
                return Err(self.err(
                    format!("named-prop value must be `\"...\"` or `{{expr}}`, got {other:?}"),
                    val_tok.span,
                ));
            }
        };
        Ok(RsxAttribute {
            kind: RsxAttributeKind::NamedProp {
                name,
                value,
                value_span,
            },
            span: Span::new(name_span.start, val_tok.span.end, self.source_id),
        })
    }

    // -------------------------------------------------------------------
    // Children sequences (until close tag, fragment close, or block close).
    // -------------------------------------------------------------------

    fn parse_children_until_close(
        &mut self,
        parent_tag: &str,
    ) -> Result<Vec<RsxNode>, BuffHtmlParseError> {
        let mut out: Vec<RsxNode> = Vec::new();
        loop {
            match self.peek().clone() {
                BuffHtmlTokenKind::CloseTag(name) => {
                    if name != parent_tag {
                        return Err(self.err(
                            format!("expected `</{parent_tag}>`, got `</{name}>`"),
                            self.span_here(),
                        ));
                    }
                    self.advance(); // consume close
                    return Ok(out);
                }
                BuffHtmlTokenKind::Eof => {
                    return Err(self.err(
                        format!(
                            "unexpected EOF inside `<{parent_tag}>` (missing `</{parent_tag}>`)"
                        ),
                        self.span_here(),
                    ));
                }
                BuffHtmlTokenKind::FragmentClose => {
                    return Err(self.err("unexpected `</>` (no matching `<>`)", self.span_here()));
                }
                BuffHtmlTokenKind::EachClose
                | BuffHtmlTokenKind::IfClose
                | BuffHtmlTokenKind::AwaitClose
                | BuffHtmlTokenKind::Else
                | BuffHtmlTokenKind::ElseIf(_)
                | BuffHtmlTokenKind::AwaitThen(_)
                | BuffHtmlTokenKind::AwaitCatch(_) => {
                    return Err(self.err(
                        format!("unexpected block terminator inside `<{parent_tag}>`"),
                        self.span_here(),
                    ));
                }
                _ => out.push(self.parse_node()?),
            }
        }
    }

    fn parse_children_until_fragment_close(&mut self) -> Result<Vec<RsxNode>, BuffHtmlParseError> {
        self.parse_children_until_terminators(&[Terminator::FragmentClose])
    }

    fn parse_children_until_terminators(
        &mut self,
        terms: &[Terminator],
    ) -> Result<Vec<RsxNode>, BuffHtmlParseError> {
        let mut out: Vec<RsxNode> = Vec::new();
        loop {
            let is_term = match self.peek() {
                BuffHtmlTokenKind::Eof => true,
                BuffHtmlTokenKind::FragmentClose if terms.contains(&Terminator::FragmentClose) => {
                    true
                }
                BuffHtmlTokenKind::Else if terms.contains(&Terminator::Else) => true,
                BuffHtmlTokenKind::ElseIf(_) if terms.contains(&Terminator::ElseIf) => true,
                BuffHtmlTokenKind::EachClose if terms.contains(&Terminator::EachClose) => true,
                BuffHtmlTokenKind::IfClose if terms.contains(&Terminator::IfClose) => true,
                BuffHtmlTokenKind::AwaitThen(_) if terms.contains(&Terminator::AwaitThen) => true,
                BuffHtmlTokenKind::AwaitCatch(_) if terms.contains(&Terminator::AwaitCatch) => true,
                BuffHtmlTokenKind::AwaitClose if terms.contains(&Terminator::AwaitClose) => true,
                _ => false,
            };
            if is_term {
                return Ok(out);
            }
            out.push(self.parse_node()?);
        }
    }

    // -------------------------------------------------------------------
    // `{#if}` / `{:else if}` / `{:else}` / `{/if}`.
    // -------------------------------------------------------------------

    fn parse_if(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let start_span = self.span_here();
        let first_cond = match self.advance().kind {
            BuffHtmlTokenKind::IfOpen(c) => c,
            _ => unreachable!(),
        };
        let first_cond_span = start_span;
        let mut branches: Vec<RsxIfBranch> = Vec::new();
        // Initial `{#if cond}` body — terminated by `{:else if}`, `{:else}`, or `{/if}`.
        let first_body = self.parse_children_until_terminators(&[
            Terminator::Else,
            Terminator::ElseIf,
            Terminator::IfClose,
        ])?;
        branches.push(RsxIfBranch {
            cond: first_cond,
            cond_span: first_cond_span,
            body: first_body,
        });

        let mut else_branch: Option<Vec<RsxNode>> = None;
        loop {
            match self.peek().clone() {
                BuffHtmlTokenKind::ElseIf(c) => {
                    let branch_span = self.span_here();
                    self.advance();
                    let body = self.parse_children_until_terminators(&[
                        Terminator::Else,
                        Terminator::ElseIf,
                        Terminator::IfClose,
                    ])?;
                    branches.push(RsxIfBranch {
                        cond: c,
                        cond_span: branch_span,
                        body,
                    });
                }
                BuffHtmlTokenKind::Else => {
                    self.advance();
                    let body = self.parse_children_until_terminators(&[Terminator::IfClose])?;
                    else_branch = Some(body);
                }
                BuffHtmlTokenKind::IfClose => {
                    self.advance();
                    break;
                }
                other => {
                    return Err(self.err(
                        format!(
                            "expected `{{:else}}`, `{{:else if}}`, or `{{/if}}`, got {other:?}"
                        ),
                        self.span_here(),
                    ));
                }
            }
        }

        Ok(RsxNode::If(RsxIf {
            branches,
            else_branch,
            span: Span::new(start_span.start, self.span_here().end, self.source_id),
        }))
    }

    // -------------------------------------------------------------------
    // `{#each}` / `{:else}` / `{/each}`.
    // -------------------------------------------------------------------

    fn parse_each(&mut self) -> Result<RsxNode, BuffHtmlParseError> {
        let start_span = self.span_here();
        let (iterable, iterable_span, binding, index, key) = match self.advance().kind {
            BuffHtmlTokenKind::EachOpen {
                iterable,
                binding,
                index,
                key,
            } => (iterable, start_span, binding, index, key),
            _ => unreachable!(),
        };
        let body =
            self.parse_children_until_terminators(&[Terminator::Else, Terminator::EachClose])?;
        let else_branch = match self.peek() {
            BuffHtmlTokenKind::Else => {
                self.advance();
                Some(self.parse_children_until_terminators(&[Terminator::EachClose])?)
            }
            _ => None,
        };
        if !matches!(self.peek(), BuffHtmlTokenKind::EachClose) {
            return Err(self.err("expected `{/each}` to close each-block", self.span_here()));
        }
        self.advance();
        Ok(RsxNode::Each(RsxEach {
            iterable,
            iterable_span,
            binding,
            index_binding: index,
            key,
            body,
            else_branch,
            span: Span::new(start_span.start, self.span_here().end, self.source_id),
        }))
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Terminator {
    FragmentClose,
    Else,
    ElseIf,
    EachClose,
    IfClose,
    AwaitThen,
    AwaitCatch,
    AwaitClose,
}

/// Parse `on:event_modifier_modifier` into `(event, [modifier, modifier])`.
/// Returns `None` if `name` is not an `on:` directive.
fn parse_event_name(name: &str) -> Option<(String, Vec<String>)> {
    let rest = name.strip_prefix("on:")?;
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.split('_');
    let event = parts.next()?.to_string();
    let modifiers: Vec<String> = parts.map(str::to_string).collect();
    Some((event, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> RsxTemplateFile {
        parse(src, SourceId(0)).expect("parse failed")
    }

    fn must_fail(src: &str) -> BuffHtmlParseError {
        parse(src, SourceId(0)).expect_err("expected parse error")
    }

    #[test]
    fn empty_template_yields_no_root() {
        let f = p("");
        assert!(f.script.is_none());
        assert!(f.root.is_empty());
    }

    #[test]
    fn text_only() {
        let f = p("hello");
        assert_eq!(f.root.len(), 1);
        match &f.root[0] {
            RsxNode::Text(t) => assert_eq!(t.text, "hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn div_with_interp_child() {
        let f = p("<div>{count}</div>");
        assert_eq!(f.root.len(), 1);
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            other => panic!("expected Element, got {other:?}"),
        };
        assert_eq!(e.tag, "div");
        assert!(!e.is_component);
        assert_eq!(e.children.len(), 1);
        match &e.children[0] {
            RsxNode::Interp(i) => assert_eq!(i.expr, "count"),
            other => panic!("expected Interp, got {other:?}"),
        }
    }

    #[test]
    fn self_closing_br() {
        let f = p("<br />");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        assert!(e.self_closing);
        assert!(e.children.is_empty());
    }

    #[test]
    fn fragment_two_children() {
        let f = p("<><h1>x</h1><p>y</p></>");
        match &f.root[0] {
            RsxNode::Fragment(fr) => assert_eq!(fr.children.len(), 2),
            _ => panic!(),
        }
    }

    #[test]
    fn component_tag_recognized() {
        let f = p("<Counter />");
        match &f.root[0] {
            RsxNode::Element(e) => {
                assert_eq!(e.tag, "Counter");
                assert!(e.is_component);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn literal_attr() {
        let f = p("<div class=\"card\" />");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        assert_eq!(e.attributes.len(), 1);
        match &e.attributes[0].kind {
            RsxAttributeKind::Literal { name, value } => {
                assert_eq!(name, "class");
                assert_eq!(value, "card");
            }
            other => panic!("expected Literal, got {other:?}"),
        }
    }

    #[test]
    fn expression_attr() {
        let f = p("<div class={some_var} />");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        match &e.attributes[0].kind {
            RsxAttributeKind::Expression { name, expr, .. } => {
                assert_eq!(name, "class");
                assert_eq!(expr, "some_var");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn on_event_handler() {
        let f = p("<button on:click={handle}>x</button>");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        match &e.attributes[0].kind {
            RsxAttributeKind::Event {
                event,
                modifiers,
                handler_expr,
                ..
            } => {
                assert_eq!(event, "click");
                assert!(modifiers.is_empty());
                assert_eq!(handler_expr, "handle");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn on_event_with_modifier() {
        let f = p("<form on:submit_prevent={h}></form>");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        match &e.attributes[0].kind {
            RsxAttributeKind::Event {
                event, modifiers, ..
            } => {
                assert_eq!(event, "submit");
                assert_eq!(modifiers, &vec!["prevent".to_string()]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn named_prop_form() {
        let f = p("<Greeting name: \"Alice\" age: 30 />");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        assert_eq!(e.attributes.len(), 2);
        match &e.attributes[0].kind {
            RsxAttributeKind::NamedProp { name, value, .. } => {
                assert_eq!(name, "name");
                assert_eq!(value, "Alice");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn boolean_attr() {
        let f = p("<input disabled />");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        match &e.attributes[0].kind {
            RsxAttributeKind::Boolean { name } => assert_eq!(name, "disabled"),
            _ => panic!(),
        }
    }

    #[test]
    fn each_block() {
        let f = p("<ul>{#each items as item}<li>{item}</li>{/each}</ul>");
        let ul = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        match &ul.children[0] {
            RsxNode::Each(each) => {
                assert_eq!(each.iterable, "items");
                assert_eq!(each.binding, "item");
                assert_eq!(each.body.len(), 1);
                assert!(each.else_branch.is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn each_block_with_else() {
        let f = p("{#each items as item}<li>{item}</li>{:else}<p>empty</p>{/each}");
        match &f.root[0] {
            RsxNode::Each(each) => {
                assert!(each.else_branch.is_some());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn if_block_with_else() {
        let f = p("{#if a}<x />{:else}<y />{/if}");
        match &f.root[0] {
            RsxNode::If(ifn) => {
                assert_eq!(ifn.branches.len(), 1);
                assert!(ifn.else_branch.is_some());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn if_block_with_else_if() {
        let f = p("{#if a}<x />{:else if b}<y />{:else}<z />{/if}");
        match &f.root[0] {
            RsxNode::If(ifn) => {
                assert_eq!(ifn.branches.len(), 2);
                assert_eq!(ifn.branches[1].cond, "b");
                assert!(ifn.else_branch.is_some());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn slot_default() {
        let f = p("<slot />");
        match &f.root[0] {
            RsxNode::Slot(s) => assert!(s.name.is_none()),
            other => panic!("expected Slot, got {other:?}"),
        }
    }

    #[test]
    fn slot_named_accepted() {
        // T133 stretch: named slots now work.
        let f = p("<slot name=\"header\" />");
        match &f.root[0] {
            RsxNode::Slot(s) => assert_eq!(s.name.as_deref(), Some("header")),
            other => panic!("expected Slot with name=header, got {other:?}"),
        }
    }

    #[test]
    fn slot_named_rejects_non_string_value() {
        let e = must_fail("<slot name={expr} />");
        assert!(format!("{e}").contains("string literal"));
    }

    #[test]
    fn slot_rejects_unknown_attribute() {
        let e = must_fail("<slot foo=\"bar\" />");
        assert!(format!("{e}").contains("unknown attribute"));
    }

    #[test]
    fn html_escape_node() {
        // T133 stretch: `{@html expr}` is now a parseable node.
        let f = p("{@html raw_html}");
        match &f.root[0] {
            RsxNode::RawHtml(r) => assert_eq!(r.expr, "raw_html"),
            other => panic!("expected RawHtml, got {other:?}"),
        }
    }

    #[test]
    fn await_block_basic() {
        // T133 stretch: minimal `{#await fut}{:then x}{body}{/await}`.
        let f = p("{#await fetchUser(id)}{:then user}<Profile user: {user} />{/await}");
        match &f.root[0] {
            RsxNode::Await(a) => {
                assert_eq!(a.fut_expr, "fetchUser(id)");
                assert_eq!(a.then_binding, "user");
                assert!(a.pending_body.is_none());
                assert!(a.catch_binding.is_none());
                assert_eq!(a.then_body.len(), 1);
            }
            other => panic!("expected Await, got {other:?}"),
        }
    }

    #[test]
    fn await_block_full_form() {
        let f = p("{#await fetchUser(id)}<Spinner />{:then user}<Profile user: {user} />{:catch err}<Error msg: {err.message} />{/await}");
        match &f.root[0] {
            RsxNode::Await(a) => {
                assert_eq!(a.fut_expr, "fetchUser(id)");
                assert!(a.pending_body.is_some());
                assert_eq!(a.then_binding, "user");
                assert_eq!(a.catch_binding.as_deref(), Some("err"));
                assert!(a.catch_body.is_some());
            }
            other => panic!("expected Await, got {other:?}"),
        }
    }

    #[test]
    fn await_block_requires_then() {
        let e = must_fail("{#await f()}{/await}");
        assert!(format!("{e}").contains("`{:then binding}`"));
    }

    #[test]
    fn keyed_each_basic() {
        // T133 stretch.
        let f = p("{#each items as item (item.id)}<li>{item}</li>{/each}");
        match &f.root[0] {
            RsxNode::Each(e) => {
                assert_eq!(e.iterable, "items");
                assert_eq!(e.binding, "item");
                assert_eq!(e.key.as_deref(), Some("item.id"));
            }
            other => panic!("expected Each, got {other:?}"),
        }
    }

    #[test]
    fn keyed_each_with_method_iterable() {
        // T133 stretch fix: parens in iterable expr now allowed.
        let f = p("{#each items.read() as item (item.id)}<li>{item}</li>{/each}");
        match &f.root[0] {
            RsxNode::Each(e) => {
                assert_eq!(e.iterable, "items.read()");
                assert_eq!(e.key.as_deref(), Some("item.id"));
            }
            other => panic!("expected Each, got {other:?}"),
        }
    }

    #[test]
    fn spread_props_basic() {
        // T133 stretch.
        let f = p("<Button {...rest} label: \"Override\" />");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        assert_eq!(e.attributes.len(), 2);
        match &e.attributes[0].kind {
            RsxAttributeKind::Spread { ident } => assert_eq!(ident, "rest"),
            other => panic!("expected Spread, got {other:?}"),
        }
    }

    #[test]
    fn bind_attribute_form() {
        // T133 stretch: `bind:value={sig}` two-way binding.
        let f = p("<input bind:value={name} />");
        let e = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        match &e.attributes[0].kind {
            RsxAttributeKind::Bind { prop, signal } => {
                assert_eq!(prop, "value");
                assert_eq!(signal, "name");
            }
            other => panic!("expected Bind, got {other:?}"),
        }
    }

    #[test]
    fn html_comment_emitted_as_comment_node() {
        let f = p("<!-- hi -->");
        match &f.root[0] {
            RsxNode::Comment(c) => assert_eq!(c.text, "hi"),
            _ => panic!(),
        }
    }

    #[test]
    fn buff_directive_comment() {
        let f = p("{# a comment #}");
        match &f.root[0] {
            RsxNode::Comment(c) => assert_eq!(c.text, "a comment"),
            _ => panic!(),
        }
    }

    #[test]
    fn script_block_at_top_level() {
        let f = p("<script lang=\"buff\">hello</script>\n<div />");
        let s = f.script.expect("script missing");
        assert_eq!(s.lang, "buff");
        assert_eq!(s.source, "hello");
        assert_eq!(f.root.len(), 2); // "\n" Text + <div />
    }

    #[test]
    fn nested_script_rejected() {
        let e = must_fail("<div><script lang=\"buff\">x</script></div>");
        assert!(format!("{e}").contains("only allowed at the top"));
    }

    #[test]
    fn script_then_counter_layout() {
        // From the decision record example.
        let src = "<script lang=\"buff\">\ncomponent Counter = fn(props: { initial: Int = 0 }) -> Element:\n    count = state(props.initial)\n</script>\n\n<div class=\"counter\">\n    <span>{count}</span>\n    <button on:click={increment}>+1</button>\n</div>";
        let f = p(src);
        assert!(f.script.is_some());
        assert_eq!(f.root.len(), 2); // "\n\n" text + <div>
        let div = match &f.root[1] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        assert_eq!(div.tag, "div");
    }

    #[test]
    fn unterminated_each_errors() {
        let e = must_fail("{#each items as item}<li>{item}</li>");
        assert!(format!("{e}").contains("{/each}"));
    }

    #[test]
    fn mismatched_close_tag_errors() {
        let e = must_fail("<div></span>");
        assert!(format!("{e}").contains("expected `</div>`"));
    }

    #[test]
    fn component_with_named_props_and_children() {
        let src = "<Layout><Header /><main>{children}</main><Footer /></Layout>";
        let f = p(src);
        let layout = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        assert_eq!(layout.tag, "Layout");
        assert!(layout.is_component);
        assert_eq!(layout.children.len(), 3);
    }

    #[test]
    fn counter_e2e_shape() {
        // Mirrors the T121b generated counter rsx!{} shape:
        let src = "<button on:click={increment}>Increment (count: {count})</button>";
        let f = p(src);
        let btn = match &f.root[0] {
            RsxNode::Element(e) => e,
            _ => panic!(),
        };
        assert_eq!(btn.tag, "button");
        assert_eq!(btn.attributes.len(), 1);
        // Children = "Increment (count: " + {count} + ")"
        assert_eq!(btn.children.len(), 3);
    }
}
