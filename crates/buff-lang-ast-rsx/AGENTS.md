# buff-lang-ast-rsx

Pure-data AST node definitions for `.buffhtml` Single-File Components. NO parsing, NO codegen. Sibling crate to `buff-lang-ast` (decision record `rsx-syntax-feasibility.md` §4: "MINOR" blast-radius choice).

## STRUCTURE

```
src/
└── lib.rs    # 451 lines — all node types + convenience constructors + is_component_tag() heuristic
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new template node (e.g. `{#portal}`) | `lib.rs` — add variant to `RsxNode` enum |
| Add a new attribute kind (e.g. `ref: value`) | `lib.rs` — add variant to `RsxAttributeKind` |
| Consume the AST | `buff-lang-buffhtml-parser` (produces) + `buff-lang-codegen-buffhtml` (consumes) |

## KEY TYPES

| Type | Role |
|---|---|
| `RsxTemplateFile` | Top-level: optional `ScriptBlock` + `root: Vec<RsxNode>` + span |
| `RsxNode` | 12-variant enum: Element, Fragment, Text, Interp, If, Each, Slot, Comment, Script, RawHtml, Await |
| `RsxAttributeKind` | 7-variant enum: Literal, Expression, Event, NamedProp, Boolean, Spread, Bind |
| `RsxElement` | `is_component: bool` (first-char-uppercase heuristic), attrs, children |
| `RsxIf` / `RsxEach` / `RsxAwait` | Block directives with branches/else/key/binding fields |
| `ScriptBlock` | `lang`, optional `props` type-name (T134), raw `source`, span |

## CONVENTIONS (this crate only)

- **`pub use buff_lang_error::Span`** at crate root. Do NOT re-define Span.
- **Derive `Debug, Clone, PartialEq`** on every struct/enum. No `Eq`/`Hash` needed (not used in maps).
- **Convenience constructors** (`new()`, `named()`, `with_props()`) on all node types. Used by parser and tests.
- **Pure-data, no logic** — the only non-trivial function is `is_component_tag(tag) -> bool` (ASCII uppercase first-char check).
- **Expression fields are raw `String`** — `RsxInterp.expr`, `RsxIfBranch.cond`, `RsxEach.iterable` etc. are stored as source text. The codegen parses them into Rust expressions at emit time. This avoids coupling the AST to `syn` types.

## WHY A SIBLING CRATE (not inside buff-lang-ast)

`buff-lang-ast` is depended on by lexer, parser, types, and codegen-rust. Adding RSX nodes there forces all four to recompile on every RSX change. The sibling crate keeps the blast radius to `buffhtml-parser` and `codegen-buffhtml` only.
