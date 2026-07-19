# buff-lang-lexer

Tokenizes `.buff` source → `Vec<Token>`. **Hand-rolled byte-scanner** (NOT logos, despite Cargo.toml).

## STRUCTURE

```
src/
├── lib.rs            # 23 lines — exports tokenize, Token, TokenKind, LexerError
├── token.rs          # TokenKind enum + spanned Token struct
├── lexer.rs          # Hand-rolled byte-scanner: entry point tokenize()
├── indent.rs         # Offside-rule indentation tracker (emits Indent/Dedent)
├── string_interp.rs  # String-literal scanner with {expr} interpolation
└── error.rs          # LexerError wrapping buff_lang_error::LexError
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new token kind | `token.rs` (TokenKind) + `lexer.rs` (scan rule) + parser `crates/buff-lang-parser/src/stream.rs` |
| Change indentation rules | `indent.rs` (offside-rule algorithm) |
| Change string interpolation | `string_interp.rs` |
| Add a new lex error | `error.rs` + `crates/buff-lang-error/src/span.rs` (LexError variant) |

## CONVENTIONS (this crate only)

- **HAND-ROLLED, not logos.** `logos` is still in `Cargo.toml` `[workspace.dependencies]` but UNUSED here. Do not switch back without a plan to also fix the parser's chumsky issue (same root cause — see root AGENTS.md NOTES).
- **Offside rule** (Python/Haskell-style): indentation level defines blocks. `indent.rs` synthesizes synthetic `Indent` / `Dedent` tokens. Tabs are REJECTED — 4 spaces only.
- **String interpolation** is lexed here, not parsed: `"hello {name}!"` produces a sequence of tokens the parser assembles. See `string_interp.rs`.
- **Entry point**: `tokenize(source: &str) -> Result<Vec<Token>, LexerError>`.
- **Tests**: `tests/lexer_tests.rs` (insta snapshots of token streams) + `tests/proptest_template.rs` (proptest fuzzing — lexer must NEVER panic on arbitrary input). Snapshots in `tests/snapshots/`.
- **Span tracking**: every Token carries a `Span` (re-exported from `buff_lang_error`).
