# Snapshot Tests

This directory documents the snapshot testing convention for Deox.

## Workflow

1. Write a test using `insta::assert_snapshot!(actual_output)`
2. Run `cargo test` — first run creates a `.pending-snap` file
3. Run `cargo insta review` to accept/reject snapshots interactively
   - OR `cargo insta accept` to accept all pending
4. Accepted `.snap` files are committed to git (per-crate `tests/snapshots/` or `snapshots/`)

## Per-crate snapshot location

Insta writes snapshots next to the test file by default:
- `crates/deox-ast/tests/snapshots/` — AST display snapshots
- `crates/deox-lexer/tests/snapshots/` — token stream snapshots (T6)
- `crates/deox-parser/tests/snapshots/` — AST parse snapshots (T7-T9)
- `crates/deox-codegen-rust/tests/snapshots/` — generated Rust source snapshots (T11-T13)

## CI behavior

CI runs `cargo test --workspace` — pending snapshots cause test failure. Always accept snapshots before committing.

## Files

- `*.snap` — accepted snapshot (committed)
- `*.snap.new` — pending snapshot (gitignored, do NOT commit)
- `.pending-snap` — old format pending marker (gitignored)
