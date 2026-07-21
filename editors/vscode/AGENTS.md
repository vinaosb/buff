# editors/vscode

Official VSCode extension for Buff. T118 (v1.2 "Use Buff"). Bundles `buff-lsp`,
TextMate grammar, snippets, and CLI commands. Ships as `buff-vscode-1.2.0.vsix`.
VSCode minimum ^1.80.0. License: MIT.

## STRUCTURE

- `package.json` — manifest: languages, grammars, commands, snippets, keybindings, config.
- `language-configuration.json` — bracket matching, comment toggles, auto-indent.
- `src/extension.ts` — TypeScript entry: activation, LSP client, commands, binary resolution.
- `out/extension.js` — compiled JS. NEVER edit. `npm run compile`.
- `syntaxes/buff.tmLanguage.json` — TextMate for `.buff` (derived from tree-sitter queries).
- `syntaxes/buffhtml.tmLanguage.json` — TextMate for `.buffhtml`.
- `snippets/buff.json` — snippets: func, let, if, for, match, enum, trait, import, spawn.
- `buff-vscode-1.2.0.vsix` — built artifact. `code --install-extension`.
- `README.md` — user docs, install, config, QA checklist.

## WHERE TO LOOK

| Task | File |
|---|---|
| Add command | `src/extension.ts` + `package.json` contributes.commands |
| Change highlighting | `syntaxes/buff.tmLanguage.json` |
| Add snippet | `snippets/buff.json` |
| Change LSP client | `src/extension.ts` (startLanguageServer) |
| Add keybinding | `package.json` contributes.keybindings |
| Add config option | `package.json` contributes.configuration |

## CONVENTIONS

- **TypeScript compiled to JS.** Source in `src/`, output in `out/`. Run `npm
  run compile` after src changes. NEVER edit `out/` directly.
- **LSP binary is `buff-lsp`** (`crates/buff-lsp`). stdio transport. No flags.
  Resolution: `buff.serverPath` config > `target/release/buff-lsp` in workspace >
  bare name on PATH.
- **CLI binary is `buff`** (`crates/buff-lang-cli`). Same resolution via
  `buff.binaryPath` config.
- **VSIX is distribution artifact.** Build via `npx @vscode/vsce package`. Install
  via `code --install-extension`. Filename version must match package.json.
- **TextMate grammar is a FALLBACK.** tree-sitter-buff (separate) is better.
- **Commands gated on Buff docs.** `buff.run`/`build`/`check` when
  `resourceLangId == buff`. `buff.restartServer` always.
- **`buff.run` opens a terminal.** `buff.build`/`check` stream to output channel.
- **Format on save is opt-in.** `buff.formatOnSave: true` mirrors to
  `editor.formatOnSave` for `[buff]`. Done by buff-lsp via `buff fmt`.
- **LSP startup is best-effort.** Extension works without `buff-lsp`.
- **Default Buff settings:** insertSpaces true, tabSize 4, detectIndentation false.

## KEYBINDINGS

`F5` = Run, `Shift+F6` = Check. Build and restart have no keybinding.

## COMMANDS

`npm install`, `npm run compile`, `npm run watch`, `npx @vscode/vsce package`.

## NOTES

TextMate grammar is a hand-derived mapping of tree-sitter-buff highlight queries
onto TextMate scopes (kept in sync manually). Binary resolution uses `where`
(Windows) / `which` (Unix). `buffhtml` is registered but minimal (grammar only,
no LSP). `node_modules/` is committed.
