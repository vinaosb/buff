# Changelog

All notable changes to the Buff transpiler are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.2.0] - 2026-07-19

### Added

- **buff-lsp**: Language Server Protocol server (`crates/buff-lsp`) with diagnostics, hover, completion, single-file goto-definition, document symbols, and formatting (via `buff fmt`). Communicates over stdio. Includes a typecheck-only analysis mode that runs the `TypeInferencer` without Rust codegen.
- **VSCode extension**: Editor support in [`editors/vscode/`](editors/vscode/) with TextMate syntax highlighting derived from the tree-sitter-buff grammar, automatic `buff-lsp` integration, `buff.run`/`buff.build`/`buff.check` commands, 16 code snippets, and format-on-save. Packaged as `buff-vscode-1.2.0.vsix`.

## [1.1.0] - 2026-07-19

### Added

- **Playground**: Wasm-based transpile-only playground (`playground/`) that runs the lexer, parser, and Rust codegen entirely in the browser. Includes URL-fragment sharing (`#s=<base64>`) so snippets link directly to an editor state. No runtime or GPU code is bundled, keeping the payload to ~2.3 MB.
- **tree-sitter grammar**: Full [tree-sitter-buff](tree-sitter-buff/) grammar with an external C scanner for the offside-rule INDENT/DEDENT/NEWLINE tokens. Ships highlight, fold, indent, and local queries. All 55 corpus tests pass, and every shipped `.buff` example parses without error nodes.
- **Website**: Static landing page (`website/`) with a "Rust performance with Go productivity" hero, six side-by-side Rust-vs-Buff code examples, and "Try this" links that load directly into the playground.

## [1.0.0] - 2026-07-16

- Heterogeneous CPU/GPU computing (Rayon + wgpu runtime), WGSL shader codegen, CLI tooling (`buff run` / `buff build` / `buff check` / `buff fmt` / `buff new` / `buff init`), lossless AST with trivia preservation, recursion detection, and pipeline caching.

## [0.5.0]

- Full type system, modules (`import`/`export`), async functions, closures, pattern matching, collections, error handling with `Result`/`?`, and the `buff fmt` formatter.

## [0.1.0]

- End-to-end transpilation proof of concept: hand-rolled lexer with offside rule, recursive-descent + Pratt parser, type inference, Rust codegen via `syn`/`quote`, and the `ola.buff` hello-world runs to native.
