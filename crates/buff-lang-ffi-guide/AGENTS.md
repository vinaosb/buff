# buff-lang-ffi-guide

Documentation-only crate. No code. Defines FFI safety rules and conventions for all Buff extern wrapper crates.

## STRUCTURE

```
buff-lang-ffi-guide/
├── Cargo.toml    # empty [dependencies], version 1.0.0, edition 2021
├── GUIDE.md      # main document: 6 hard rules + 4 reference examples
├── README.md     # short overview pointing to GUIDE.md
├── AGENTS.md     # this file
└── src/
    └── lib.rs    # empty crate root with module doc pointing to GUIDE.md
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Read the FFI rules | `GUIDE.md` (the entire crate IS the guide) |
| Check a rule before writing a wrapper | `GUIDE.md` sections R1-R6 |
| Find safe-pattern examples | `GUIDE.md` sections E1-E3 |
| Find anti-pattern examples | `GUIDE.md` section E4 |

## CONVENTIONS (this crate only)

- **No `[features]`, `[lints]`, or `[profile.*]` sections** in Cargo.toml (workspace convention).
- **No dependencies**. The guide references external crates by name but does not depend on them.
- **No `unsafe` code**. This crate has no code at all. It is purely documentation.
- **GUIDE.md is the authoritative source**. The `lib.rs` doc comment is a summary only.
- **Changes here affect all wrapper crates**. Wave 4 wrappers (T17-T21) and any future framework wrappers must comply. Treat edits to GUIDE.md as a semver-breaking change to the convention contract.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-codegen-rust` | Generates `unsafe { ... }` wrappers at call sites automatically. This guide documents that behavior and constrains what wrapper authors may do on top of it. |
| `buff-lang-types` | Defines `BuffError` and the prelude type registry. Wrapper error types lower through this system. |
| `buff-lang-ast` | Defines `ExternFuncDecl` and `Decl::ExternFuncDecl`. The parser accepts `extern "C" from "crate" func name(...)`. |
| Wave 4 wrappers (T17-T21) | Primary consumers of this guide. Each wrapper crate must follow all six rules. |
