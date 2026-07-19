# Change Log

All notable changes to the **Buff** VSCode extension will be documented in this
file. The format is based on [Keep a Changelog](https://keepachangelog.com/)
and this project adheres to [Semantic Versioning](https://semver.org/).

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
