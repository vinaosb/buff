# Buff for Visual Studio Code

Language support for the [Buff](https://github.com/buff-lang/buff) programming
language — the high-level language that transpiles to Rust. The extension
bundles three already-shipped components into a single install:

- **Syntax highlighting** — a TextMate grammar derived from the
  `tree-sitter-buff` highlight queries (T115). VSCode's native TextMate engine
  renders it; no experimental APIs required.
- **Language intelligence** — diagnostics, hover, completion, goto-definition,
  document symbols, and formatting via [`buff-lsp`](../../crates/buff-lsp)
  (T117) over stdio.
- **CLI commands** — `Buff: Run`, `Buff: Build`, `Buff: Check` drive the
  `buff` CLI and stream the output into a VSCode terminal or output channel.

## Features

| Feature                | Source         | Trigger                                  |
|------------------------|----------------|------------------------------------------|
| `.buff` file icon      | extension      | open any `.buff` file                    |
| Syntax highlighting    | TextMate       | automatic on `.buff` files               |
| Diagnostics            | buff-lsp       | automatic, debounced 300 ms              |
| Hover (type/symbol)    | buff-lsp       | hover an identifier                      |
| Completion             | buff-lsp       | type `.` or `Ctrl+Space`                 |
| Goto definition        | buff-lsp       | `F12` / `Ctrl+Click` on an identifier    |
| Document symbols       | buff-lsp       | `Ctrl+Shift+O`                           |
| Format document        | buff-lsp       | `Shift+Alt+F`                            |
| Format on save         | buff-lsp       | set `buff.formatOnSave: true`            |
| Run current file       | `buff` CLI     | `Buff: Run` command or `F5`              |
| Build current file     | `buff` CLI     | `Buff: Build` command                    |
| Check current file     | `buff` CLI     | `Buff: Check` command or `Shift+F6`      |
| Restart server         | extension      | `Buff: Restart Language Server`          |

Snippets included: `func`, `async func`, `let`, `let mut`, `if`, `if else`,
`elif`, `for`, `match`, `=>` (match arm), `return`, `print`, `enum`, `trait`,
`import`, `spawn`.

## Installation

The extension does **not** bundle the language server or the CLI — you build
both from this repo (they are tiny Rust crates, build in seconds):

```bash
# 1. Build the language server and CLI (release mode).
cargo build --release -p buff-lsp -p buff-lang-cli

# 2. Confirm the binaries exist.
ls target/release/buff-lsp   # or buff-lsp.exe on Windows
ls target/release/buff       # or buff.exe on Windows
```

### Install the extension

```bash
cd editors/vscode
npm install
npm run compile
npx @vscode/vsce package      # produces buff-vscode-1.2.0.vsix
```

Then in VSCode:

- **GUI**: *View → Command Palette → Extensions: Install from VSIX…* and pick
  the produced `.vsix`.
- **CLI**: `code --install-extension buff-vscode-1.2.0.vsix`

### Configure binary paths (only if VSCode can't find them)

If you opened the repo root as your workspace folder, the extension discovers
both binaries automatically under `target/release/`. If you opened a sub-folder
or installed Buff system-wide, you can leave the paths empty and the extension
will fall back to whatever is on `PATH`. Otherwise:

```jsonc
{
    "buff.serverPath": "/absolute/path/to/buff-lsp",
    "buff.binaryPath": "/absolute/path/to/buff"
}
```

### Format on save

```jsonc
{
    "buff.formatOnSave": true
}
```

This mirrors `editor.formatOnSave` for the `[buff]` language so the LSP
formatter (which routes through `buff fmt`) runs automatically.

## Commands

| Command                       | Keybinding       |
|-------------------------------|------------------|
| `Buff: Run Current File`      | `F5`             |
| `Buff: Build Current File`    | —                |
| `Buff: Check Current File`    | `Shift+F6`      |
| `Buff: Restart Language Server` | —              |

## Configuration reference

| Setting              | Type      | Default | Description                                                |
|----------------------|-----------|---------|------------------------------------------------------------|
| `buff.serverPath`    | `string`  | `""`    | Absolute path to the `buff-lsp` binary.                   |
| `buff.binaryPath`    | `string`  | `""`    | Absolute path to the `buff` CLI binary.                   |
| `buff.formatOnSave`  | `boolean` | `false` | Mirror to `editor.formatOnSave` for Buff documents.       |
| `buff.trace.server`  | `enum`    | `"off"` | LSP trace verbosity (`off` / `messages` / `verbose`).     |

## How highlighting was derived

The TextMate grammar in `syntaxes/buff.tmLanguage.json` is a hand-derived
mapping of `tree-sitter-buff/queries/highlights.scm` captures onto TextMate
scope names. The two grammars share the same keyword set and literal patterns;
they are kept in sync manually. See `DECISIONS.md` notes in
`.sisyphus/notepads/buff-post-v10-tooling/` for the rationale.

## Troubleshooting

- **No highlighting** — confirm the file's language mode is `Buff` (lower-right
  corner of the status bar). If not, run `Change Language Mode` and pick Buff.
- **No diagnostics / hover** — open the `Buff Language Server` output channel.
  If it says `language server not found`, set `buff.serverPath` or run
  `cargo build --release -p buff-lsp` from the repo root.
- **`Buff: Run` says CLI not found** — set `buff.binaryPath` or run
  `cargo build --release -p buff-lang-cli` from the repo root.
- **Stale diagnostics after a big edit** — run `Buff: Restart Language Server`.

## Manual QA checklist

The extension ships without `@vscode/test-electron` end-to-end tests (see
`issues.md` in `.sisyphus/notepads/buff-post-v10-tooling/` for the rationale).
Verify each item below after a manual install:

1. `cargo build --release -p buff-lsp -p buff-lang-cli` exits 0 in the repo.
2. `cd editors/vscode && npm install && npm run compile && npx @vscode/vsce package` exits 0.
3. `code --install-extension buff-vscode-1.2.0.vsix` succeeds.
4. Open `examples/ola.buff` from this repo:
   - Status bar shows language **Buff**.
   - `func`, `print`, `//` comments, `"Olá, Buff!"` are highlighted in distinct colors.
5. Open the **Buff Language Server** output channel:
   - It logs a successful `initialize` response on file open.
6. Open `examples/fibonacci.buff`:
   - Hover over `fib` inside `func fib(...)`: shows the function signature / return type.
   - `Ctrl+Click` on `fib(n)` call site in `main`: jumps to the declaration.
   - `Ctrl+Shift+O` lists `fib` and `main` in the document outline.
   - Type `print(`: completion offers `print` from the prelude.
7. Introduce a deliberate error (e.g. delete the closing quote of a string):
   - Diagnostics appear in the Problems panel within ~300 ms.
8. `F5` with `ola.buff` active: a `Buff` terminal opens and prints `Olá, Buff!`.
9. `Shift+F6` with `fibonacci.buff` active: the **Buff** output channel shows
   `buff check` output (clean if no issues).
10. `Shift+Alt+F`: formats the active document via `buff fmt` (round-trips a
    already-formatted file byte-identically).
11. Set `"buff.formatOnSave": true` in settings, edit a Buff file, save: format
    runs automatically.
12. Run `Buff: Restart Language Server`: the output channel shows a fresh
    `initialize` and no errors.

## License

MIT — same as the rest of the Buff toolchain. See [LICENSE](./LICENSE).
