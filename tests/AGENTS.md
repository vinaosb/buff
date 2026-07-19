# tests/

Workspace-level test inputs and snapshot conventions. Per-crate integration tests live in `crates/*/tests/`.

## STRUCTURE

```
tests/
├── fixtures/
│   ├── valid/              # Well-formed .buff programs (must lex+parse+codegen)
│   │   ├── .gitkeep
│   │   ├── ola.buff
│   │   └── arithmetic.buff
│   └── invalid/            # Malformed inputs (must produce clean errors, not panic)
│       ├── .gitkeep
│       ├── bad_indent.buff        # Mixed tabs/spaces
│       └── missing_semicolon.buff
└── snapshots/
    ├── README.md           # Insta workflow + per-crate snapshot locations (read this)
    └── .gitkeep            # Workspace-root snapshots land here (rare)
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| Add a golden valid program | `fixtures/valid/<name>.buff` + a test in the relevant `crates/*/tests/` |
| Add a malformed input test | `fixtures/invalid/<name>.buff` + assertion that lex/parse fails cleanly |
| Find snapshot workflow | `snapshots/README.md` |
| Find per-crate snapshots | `crates/buff-lang-{ast,lexer,parser,codegen-rust}/tests/snapshots/` |

## CONVENTIONS (this dir only)

- **`fixtures/valid/`**: programs that MUST succeed end-to-end. Add a corresponding integration test in the relevant crate.
- **`fixtures/invalid/`**: programs that MUST fail with a clean `Result::Err`. NEVER allow a panic — if a fixture panics the lexer/parser, that's a bug, not a test failure.
- **Insta snapshots are per-crate**, NOT here. This dir's `snapshots/` is reserved for workspace-level snapshots (rarely used).
- **Insta workflow**: `cargo insta review` (interactive) or `cargo insta accept` (all). Pending `.snap.new` / `.pending-snap` files are gitignored — NEVER commit them.
- **CI gate**: `cargo test --workspace` fails on any pending snapshot. Always accept before commit.
- **`*.buff` naming**: snake_case (e.g. `bad_indent.buff`, `missing_semicolon.buff`).
