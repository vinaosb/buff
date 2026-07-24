# Change Log

All notable changes to the **Buff** VSCode extension will be documented in this
file. The format is based on [Keep a Changelog](https://keepachangelog.com/)
and this project adheres to [Semantic Versioning](https://semver.org/).

## [1.3.0] — 2026-07-24

### Added
- v1.3 update of the Buff VSCode extension (T118, Buff v1.25 Wave 2b).
  Surfaces the four LSP capabilities that `buff-lsp` began advertising in T46
  (Wave 2a) and brings the TextMate grammar + snippets up to date with the
  Buff language surface as of v1.22.
- **LSP wiring (T46)** — `codeAction`, `codeLens`, `inlayHint`, and
  `semanticTokens` are now consumed by the client. The four handlers are
  auto-registered by `vscode-languageclient` 9.x based on the server's
  `initialize` response (no new npm dependencies, no per-capability client
  code). Lightbulb quick-fixes, CodeLens above top-level functions, inlay
  type hints at `let` bindings, and LSP-driven semantic highlighting now
  light up automatically when `buff-lsp` is on PATH.
- **Configuration toggles** — `buff.inlayHints.enabled` (default `true`) and
  `buff.codeLens.enabled` (default `true`) let users opt out of inlay hints
  or CodeLens in Buff files without affecting other languages. Both mirror
  into the per-language VSCode settings (`editor.inlayHints.enabled` and
  `editor.codeLens` for `[buff]`), the same pattern `buff.formatOnSave` uses.
- **Grammar updates** (`syntaxes/buff.tmLanguage.json`):
  - T93 raw strings `r"..."` and `r#"..."#` (Rust-style hash-delimited) now
    highlight as `string.quoted.other.buff`.
  - T21 triple-quoted strings `"""..."""` now highlight as
    `string.quoted.triple.buff`.
  - T13/T24 generic type lists after a known collection type
    (`Vector<Int>`, `Map<K, V>`, `Channel<T>`, …) get a `meta.generic.buff`
    block so the angle brackets are styled as
    `punctuation.definition.generic.{begin,end}.buff` instead of comparison
    operators.
  - T84 range operators `..` and `..=` are now matched longest-first so
    `0..=5` tokenises as ONE operator (was previously `..` followed by `=`).
    The duplicate `..` alternation at the end of the operator pattern was
    removed.
  - `defer` keyword (commit 2b12f75) was already in the keyword set; this
    release just documents it.
- **Snippets** (`snippets/buff.json`) — added: `func<` (generic function),
  `struct<` (generic struct), `let:` (typed let), `let vector` (Vector<T>),
  `match option`, `match result`, `or pattern` (T13 or-patterns), `range`,
  `range=`, `for range`, `for range=` (T84 ranges), `defer`. Total snippets
  went from 16 → 28.
- **Language configuration** — added auto-closing pairs for raw strings
  (`r"` → `"` and `r#"` → `"#`) so the closing delimiter is auto-inserted.

## [1.2.0] — 2026-07-19

### Added
- Initial release of the Buff VSCode extension (T118, v1.2 *Use Buff*).
- File association `.buff` → Buff language id `buff`.
- TextMate grammar (`syntaxes/buff.tmLanguage.json`) hand-derived from the
  `tree-sitter-buff` highlight queries (T115). Same keyword set and literal
  patterns as the tree-sitter grammar.
- Language-configuration.json reflecting Buff reality: `//` line comments,
  `/* */` block comments, `() [] {}` brackets, 4-space indentation, offside-rule
  indent patterns, no-tab policy (enforced via `configurationDefaults`).
- LSP client (`vscode-languageclient` 9.x) launching `buff-lsp` (T117) over
  stdio. Wires diagnostics / hover / completion / goto-def / document symbols /
  formatting automatically.
- Commands: `Buff: Run` (`F5`), `Buff: Build`, `Buff: Check` (`Shift+F6`),
  `Buff: Restart Language Server`.
- Snippets for `func`, `async func`, `let`, `let mut`, `if`, `if else`,
  `elif`, `for`, `match`, match arm (`=>`), `return`, `print`, `enum`,
  `trait`, `import`, `spawn`.
- Configuration: `buff.serverPath`, `buff.binaryPath`, `buff.formatOnSave`,
  `buff.trace.server`.
- Binary path resolution: config → `target/release/<bin>` in workspace folders
  → `<bin>` on `PATH`.
- Format-on-save bridge: `buff.formatOnSave: true` mirrors to
  `editor.formatOnSave` for the `[buff]` language so the LSP formatter runs
  automatically.
- `.vscodeignore`, `tsconfig.json`, `language-configuration.json`,
  `snippets/buff.json`, `README.md`, `LICENSE`, `CHANGELOG.md`.

