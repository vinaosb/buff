# tree-sitter-buff

[![npm](https://img.shields.io/npm/v/tree-sitter-buff)](https://www.npmjs.com/package/tree-sitter-buff)

Tree-sitter grammar for the [Buff](https://github.com/buff-lang/buff) programming language.

Buff is a layout-sensitive language that transpiles to Rust — indentation defines blocks (no braces for control flow), braces `{}` are reserved for data structures. This grammar provides syntax highlighting, code folding, indentation, and structural navigation in any editor that supports tree-sitter (Neovim, Helix, Zed, VS Code, GitHub, etc).

> **Note:** This is a **derived approximation** for editor tooling. The authoritative parser is the hand-rolled Rust parser in `crates/buff-lang-parser/`. If they diverge, the Rust parser wins.

## Installation

```bash
npm install tree-sitter-buff
```

## Usage

```javascript
import Parser from "tree-sitter";
import Buff from "tree-sitter-buff";

const parser = new Parser();
parser.setLanguage(Buff);

const source = `
func greet(name: String) -> String
    return "Hello, " + name
`;

const tree = parser.parse(source);
console.log(tree.rootNode.toString());
```

## Development

```bash
# Regenerate parser.c from grammar.js
npm run generate

# Run corpus tests
npm test

# Parse a .buff file and print the syntax tree
tree-sitter parse examples/hello.buff
```

## License

`MIT OR Apache-2.0` — same as the Buff project.

## Links

- [Buff language](https://github.com/buff-lang/buff) — main repository
- [Tree-sitter](https://tree-sitter.github.io/) — parser framework
