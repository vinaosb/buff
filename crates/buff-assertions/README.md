# buff-assertions

Fluent test assertions for the Buff language — `assertThat(value).isEqualTo(expected)` style.

## Quick start

```rust
use buff_assertions::assertThat;

assertThat(42).isEqualTo(42);
assertThat(10).isGreaterThan(5).isLessThan(20);
assertThat("hello".to_string()).startsWith("hel").endsWith("lo");
assertThat(Some(99)).isSome().isEqualTo(99);
assertThat(vec![1, 2, 3]).hasSize(3).containsItem(&2);
```

## API

| Method | Type | Description |
|---|---|---|
| `assertThat(value)` | any | Entry point — wraps a value in `AssertThat<T>` |
| `.isEqualTo(expected)` | `T: PartialEq` | Assert equality |
| `.isNotEqualTo(unexpected)` | `T: PartialEq` | Assert inequality |
| `.isGreaterThan(other)` | `T: PartialOrd` | Assert `> ` |
| `.isGreaterThanOrEqualTo(other)` | `T: PartialOrd` | Assert `>=` |
| `.isLessThan(other)` | `T: PartialOrd` | Assert `<` |
| `.isLessThanOrEqualTo(other)` | `T: PartialOrd` | Assert `<=` |
| `.isSome()` | `Option<T>` | Unwrap `Some`, return `AssertThat<T>` |
| `.isNone()` | `Option<T>` | Assert `None` |
| `.startsWith(prefix)` | `String` | Assert string prefix |
| `.endsWith(suffix)` | `String` | Assert string suffix |
| `.contains(substr)` | `String` | Assert substring |
| `.containsItem(item)` | `Vec<T>` | Assert vector contains item |
| `.hasSize(n)` | `Vec<T>` | Assert vector length |
| `.isEmpty()` | `Vec<T>` | Assert empty vector |

All methods are chainable. Assertions panic with descriptive messages on failure.

## License

MIT OR Apache-2.0 (matches the workspace).
