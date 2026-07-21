# buff-mock

> Mocking framework for the Buff language — `Mock<Trait>` generic with `expect` / `verify` / `spy`.

Foundational testing library consumed by T22 (API compatibility spike) and T23 (flagship tests). Provides both a pure-Rust runtime API and a codegen-time helper for future `@mock`-attribute integration.

## Quick start

```rust
use buff_mock::{ArgumentValue, Mock, ReturnValue};

trait Greeter {
    fn greet(&self, name: String) -> String;
}

// Either hand-write this impl, OR generate it via `lower_mock_for_trait`.
impl Greeter for Mock<dyn Greeter> {
    fn greet(&self, name: String) -> String {
        self.record_call("greet", vec![ArgumentValue::String(name)]);
        match self.lookup_return("greet", &[]) {
            Some(ReturnValue::String(s)) => s,
            _ => String::new(),
        }
    }
}

#[test]
fn greet_returns_programmed_value() {
    let mock = Mock::<dyn Greeter>::new();
    mock.expect("greet").returning(ReturnValue::String("hello".into()));

    assert_eq!(mock.greet("alice".into()), "hello");
    mock.verify().unwrap();
}
```

## Spy pattern

```rust
let spy = mock.spy("greet");
let _ = mock.greet("alice".into());
let _ = mock.greet("bob".into());

assert_eq!(spy.call_count(), 2);
assert_eq!(spy.args()[0], vec![ArgumentValue::String("alice".into())]);
```

## Expect / verify / spy

| API | Purpose |
|---|---|
| `Mock::<dyn Trait>::new()` | Construct an empty mock |
| `mock.expect("method").returning(value)` | Program a return value |
| `mock.expect("method").times(n)` | Assert exact call count |
| `mock.expect("method").at_least(n)` / `.at_most(n)` | Range constraints |
| `mock.expect("method").never()` | Assert NOT called |
| `mock.expect("method").with_args(args).returning(v)` | Argument matching |
| `mock.verify()` | Assert every expectation was satisfied |
| `mock.spy("method")` | Get a `SpyHandle` for observation |
| `spy.calls()` / `spy.args()` / `spy.call_count()` | Inspect the call log |

## Codegen helper

`lower_mock_for_trait(trait_decl: &TraitDecl) -> MockResult<syn::Item>` emits an `impl Trait for Mock<Trait>` block as a `syn::Item`. Used by the future `@mock`-attribute integration in `buff-lang-codegen-rust`:

```rust,ignore
use buff_mock::lower_mock_for_trait;
use buff_lang_ast::{TraitDecl, ...};

let trait_decl: TraitDecl = ...;
let item = lower_mock_for_trait(&trait_decl)?;

let file = syn::File { items: vec![item], ..Default::default() };
let rust_source = prettyplease::unparse(&file);
// rust_source now contains:
//   impl Greeter for buff_mock::Mock<Greeter> {
//       fn greet(&self, name: String) -> String {
//           self.record_call("greet", vec![buff_mock::ArgumentValue::String(name)]);
//           match self.lookup_return("greet", &[]) {
//               Some(buff_mock::ReturnValue::String(s)) => s,
//               _ => String::new(),
//           }
//       }
//   }
```

The lowered impl delegates every required method to `Mock<T>`'s `record_call` + `lookup_return`, falling back to a type-appropriate default (`""`, `0`, `0.0`, `false`, `()`) when no expectation matches.

## Why no procedural macros?

The T3 macro spike ([`.sisyphus/decisions/macro-system-v1x.md`](../.sisyphus/decisions/macro-system-v1x.md)) deferred the macro system post-v1.17 and recommended runtime workarounds. `buff-mock` follows that recommendation:

1. **Runtime API** — pure-Rust library usable directly from any test.
2. **Codegen helper** — `lower_mock_for_trait` emits the trait impl as `syn::Item`, ready for `buff-lang-codegen-rust` to push into the generated source when a `@mock`-attributed `let` binding is seen.

Zero parser/AST/codegen-rust ripple — the MVP is self-contained.

## Examples

Run the three example programs:

```bash
cargo run --example hello_mock -p buff-mock
cargo run --example verify_interaction -p buff-mock
cargo run --example spy_on_calls -p buff-mock
```

## License

MIT OR Apache-2.0 (matches the workspace).
