//! Buff `.buffhtml` Parser crate — 3-mode lexer + recursive-descent parser.
//!
//! Implements the T133 floor grammar (decision record
//! `rsx-syntax-feasibility.md` §6 MUST-ship list):
//!
//! - HTML elements / tags / attributes
//! - `{expr}` interpolation
//! - `{#each}/{:else}/{/each}` loops
//! - `{#if}/{:else if}/{:else}/{/if}` conditionals
//! - Fragments `<> ... </>`
//! - Child component composition (capitalized tags)
//! - Default `<slot />`
//! - Comments (`<!-- -->` HTML and `{# ... #}` Buff directive)
//! - `<script lang="buff"> ... </script>` blocks
//! - `on:event_modifier={handler}` event directives
//! - Named props (`name: value`)
//!
//! Deferred per §6 (NOT implemented; emitted as a parse error with TODO):
//! named slots, keyed each, spread props, two-way binding, await, `{@html}`.
//!
//! # Layout
//!
//! - [`lexer`]: byte-scanner producing a flat [`lexer::BuffHtmlToken`] stream.
//!   The lexer has THREE modes per the decision record §3: `TEXT`,
//!   `BUFF_CODE` (inside `{...}`), and `BUFF_DIRECTIVE` (inside `{#...}`,
//!   `{:...}`, `{/...}`, `{@...}`).
//! - [`parser`]: recursive-descent over the token stream → [`RsxTemplateFile`].

pub mod error;
pub mod lexer;
pub mod parser;

pub use error::BuffHtmlParseError;
pub use lexer::{tokenize, BuffHtmlToken, BuffHtmlTokenKind};
pub use parser::parse;
