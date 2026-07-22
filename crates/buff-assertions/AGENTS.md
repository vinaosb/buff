# buff-assertions

Fluent test assertions for the Buff language — `assertThat(value).isEqualTo(expected)` style.

## STRUCTURE

```
src/
├── lib.rs          # AssertThat<T> struct + assertThat() entry point + all fluent methods
├── error.rs        # AssertionError struct
```

Total: ~200 src LOC + 100 LOC tests + 50 LOC Cargo.toml. **~20 public functions** (under the 30-budget).

## PUBLIC API

```text
// Entry point:
pub fn assertThat<T>(actual: T) -> AssertThat<T>

// AssertThat<T> methods:
AssertThat::new, isEqualTo, isNotEqualTo, isGreaterThan,
isGreaterThanOrEqualTo, isLessThan, isLessThanOrEqualTo,
isInstanceOf, isNull, isNotNull

// AssertThat<Option<T>> methods:
isSome, isNone

// AssertThat<String> methods:
startsWith, endsWith, contains, matches

// AssertThat<Vec<T>> methods:
containsItem, hasSize, isEmpty
```

## CONVENTIONS

- **No `unwrap`/`expect`/`panic!`/`todo!`** in non-test code (project hard rule). Assertions use `panic!` intentionally — that's the assertion mechanism.
- **No external dependencies** — pure Rust stdlib only.
- **Fluent API** — every method returns `Self` (consumes `self`, returns `Self`) for chaining.
- **Descriptive failure messages** — every assertion panic includes the actual and expected values.

## DEPENDENCIES

None. Pure Rust stdlib.

## TESTS

Integration tests (`tests/assertions_tests.rs`, 20+ tests) + inline `#[cfg(test)]` unit tests in `src/lib.rs` (20+ tests):

- **Equality** — isEqualTo passes/fails, isNotEqualTo passes/fails
- **Numeric** — isGreaterThan, isLessThan, isGreaterThanOrEqualTo, isLessThanOrEqualTo
- **String** — startsWith, endsWith, contains
- **Option** — isSome, isNone
- **Vector** — containsItem, hasSize, isEmpty
- **Fluent chains** — multi-method chains

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T38 (line 3337).
- Sibling: `crates/buff-mock/` (same test-framework pattern).
- Hamcrest/Chai/testify/claim/AssertJ/FluentAssertions (cross-language prevalence: 6/6).
