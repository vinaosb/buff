# buff-pipeline — snapshot tests

This directory is the conventional home for `insta` external snapshot
files (`.snap`). The `buff-pipeline` test suite currently uses
**inline snapshots** exclusively (`insta::assert_snapshot!(value, @"expected")`)
so there are no `.snap` files on disk — the expected output is checked
in directly inside `tests/unit_tests.rs`.

## Why inline?

1. **Self-contained**: the test + expected value live in the same source
   file. No separate `.snap` file to keep in sync.
2. **Reviewable**: `git diff` on `tests/unit_tests.rs` shows both the
   test logic and the snapshot change in one hunk.
3. **Five snapshots**: the T14 spec mandates 5+ snapshots; the inline
   form satisfies this with zero on-disk footprint.

## Migrating to external snapshots

If a snapshot grows large (e.g. multi-line CSV output >50 lines) or is
shared across multiple tests, promote it to an external `.snap` file:

```bash
# 1. Replace the inline `@"expected"` with an empty `@""` or just remove
#    the second argument.
# 2. Run `cargo insta review` to accept the new external snapshot.
```

The resulting `.snap` files land here.
