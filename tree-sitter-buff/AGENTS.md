# tree-sitter-buff

Tree-sitter grammar for Buff. T115 (v1.1 "Try Buff"). Provides universal editor
highlighting (Neovim, Helix, Zed, GitHub code viewer, etc). This is a DERIVED
APPROXIMATION for editor tooling, NOT the authoritative parser. tree-sitter CLI
`^0.26.0`. Scope: `source.buff`. Bindings: C + Node only.

## STRUCTURE

- `grammar.js` — source of truth (all syntax rules).
- `tree-sitter.json` — scope, file-types, query paths, bindings.
- `package.json` — npm metadata, tree-sitter-cli ^0.26.0.
- `binding.gyp` — Node native addon build config.
- `queries/` — `highlights.scm`, `folds.scm`, `indents.scm`, `locals.scm`.
- `src/parser.c` — GENERATED from grammar.js. NEVER edit.
- `src/scanner.c` — HAND-WRITTEN C. Offside-rule indent tracking.
- `src/{grammar.json,node-types.json,tree_sitter/}` — GENERATED.
- `test/corpus/` — 5 expected-parse-tree files.

## WHERE TO LOOK

| Task | File |
|---|---|
| Add keyword/operator | `grammar.js` → then `tree-sitter generate` |
| Tune highlighting | `queries/highlights.scm` |
| Add fold region | `queries/folds.scm` |
| Change indent behavior | `queries/indents.scm` |
| Change local scope | `queries/locals.scm` |
| Fix indent tracking | `src/scanner.c` (hand-written C) |
| Add corpus test | `test/corpus/*.txt` |

## CONVENTIONS

- **Authoritative parser is `crates/buff-lang-parser`** (hand-rolled Rust). If
  they diverge, the Rust parser wins.
- **`src/parser.c` is GENERATED.** Never hand-edit. Run `tree-sitter generate`
  after grammar.js changes.
- **`src/scanner.c` is HAND-WRITTEN C.** tree-sitter's Lex DSL cannot express
  indentation-sensitive syntax. This external scanner emits NEWLINE/INDENT/DEDENT
  tokens based on leading whitespace changes. The same offside rule lives in
  buff-lang-lexer (Rust) and buff-lang-parser (Rust); this C shim is a different
  tradeoff for tree-sitter's C runtime consumer.
- **Precedence table** (`PREC` in grammar.js) must match the Rust parser's Pratt
  table. Divergence causes highlighting-vs-compiler confusion.
- **Ambiguity goes in `conflicts` array**, not grammar restructuring.
- **4-space indent, no tabs.** Enforced by scanner.c. Mirrors buff-lang-lexer.
- **`field()` annotations** in grammar.js give editors named child access. Keep
  consistent with AST field names in `crates/buff-lang-ast`.
- **License**: `MIT OR Apache-2.0`.

## COMMANDS

`tree-sitter generate` (rebuild parser.c), `tree-sitter test` (corpus),
`tree-sitter parse <file>` (print tree).

## NOTES

Grammar handles both layout blocks (`: INDENT ... DEDENT`) and brace blocks.
String interpolation is a child of string node. `type_identifier` is PascalCase,
disambiguating struct init `Foo { }` from block `if c { }`.
