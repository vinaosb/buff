# Modules: `import` and `export`

Buff uses ES6-style module syntax for multi-file programs. Three forms of import:

```buff
import { greet, farewell } from "./hello.buff"   // named
import greet from "./hello.buff"                  // default
import * from "./utils.buff"                      // wildcard
```

And three forms of export:

```buff
export func greet(): ...                           // wrap a func decl
export enum Color { Red, Green, Blue }             // wrap an enum decl
export { helper }                                  // re-export a local symbol
export * from "./other.buff"                       // re-export all from another file
```

Module visibility: a symbol is **module-private** by default; only `export`ed symbols can be imported by other files. Circular imports (`A` imports `B`, `B` imports `A`) are detected and rejected at graph-build time.

## Current v0.5 limitation

End-to-end multi-file linking is a codegen gap — the CLI compiles one file at a time. The parser, however, fully supports the syntax above (verified by `crates/buff-lang-parser/tests/module_system.rs`). This exercise focuses on getting the SYNTAX right; the path you write doesn't need to exist.

## Your task

1. Add `import { greet } from "./greet.buff"` at the very top of the file (line 1, before any other declaration).
2. Make `main` exported: `export func main():` instead of `func main():`.

**Hint:** the import line starts with `import`, ends with a quoted path string. The export keyword replaces `func` — it goes BEFORE the `func` keyword on the same line.
