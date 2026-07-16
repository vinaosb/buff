# Testing Conventions — buff-lang-ast

## Snapshot Tests

Use `insta::assert_snapshot!` for any human-readable output (Display, debug formatting).

### Pattern

```rust
#[test]
fn test_something() {
    let node = build_some_ast_node();
    insta::assert_snapshot!(format!("{}", node));
}
```

### Running

```bash
cargo test -p buff-lang-ast
cargo insta review   # review pending snapshots
```

## Property Tests

Use `proptest` for invariant testing.

### Pattern

```rust
proptest! {
    #[test]
    fn test_roundtrip(input in "[a-z]+") {
        let tokens = lex(&input);
        let output = unlex(tokens);
        prop_assert_eq!(input, output);
    }
}
```

## Conventions

- One test file per module: `tests/{module}_tests.rs`
- Snapshot files: committed alongside test files in `tests/snapshots/`
- NEVER commit `*.snap.new` files
- Each test must have a descriptive name: `test_binary_op_precedence` not `test1`
