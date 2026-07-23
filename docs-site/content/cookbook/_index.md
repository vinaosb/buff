+++
title = "Cookbook"
weight = 40
sort_by = "weight"
+++

# Cookbook

> Copy-pasteable Buff recipes for common tasks. Each entry follows the
> three-part **Problem → Solution → Explanation** shape: the problem is
> one sentence, the solution is a complete Buff code block, and the
> explanation is 2–3 paragraphs covering the approach, alternatives, and
> trade-offs.

## How to read a recipe

```markdown
## Recipe title

**Problem**: One-sentence description of what you want to do.

**Solution**:
```buff
<working Buff code>
```

**Explanation**: 2–3 paragraphs. The first always walks line-by-line
through the solution. The second compares alternatives (`Option` vs
`Result`, eager vs lazy, etc). The third — when present — points at
the underlying Rust lowering or a framework crate (`buff-web`,
`buff-dataframe`, …) that powers the recipe.
```

Recipes are self-contained: copy the `buff` block into a `.buff` file
and `buff run` it. Where a recipe touches a stdlib type that ships as
part of the **experimental** v1.13 frameworks wave (`DataFrame`,
`HttpClient`, `Database`, `Web`), the generated Rust is correct but
the end-to-end linker step requires the Cargo-project pipeline (v1.3
deferred). The syntax is verified by `buff check` and the docs are
smoke-tested by `crates/buff-lang-cli/tests/cookbook_tests.rs`.

## Categories

| Page | Recipes | Covers |
|---|---|---|
| [HTTP](./http/) | 6 | `HttpClient`, headers, POST JSON, retry/backoff |
| [Files](./files/) | 5 | `Path`, `Dir`, `Tempfile`, CSV round-trip, walk |
| [JSON](./json/) | 5 | `Toml`, `Yaml`, `Csv`, nested values, schema validation |
| [Database](./database/) | 5 | `Database.connect`, query, insert, transaction, pool |
| [Parallel](./parallel/) | 5 | `par_map`, `par_filter`, `par_reduce`, `Channel`, race |
| [Async](./async/) | 5 | `spawn`, `sleep`, timeout, `select`, gather |
| [Errors](./errors/) | 6 | `Result`, `Option`, `?`, custom errors, retry combinators |
| [Testing](./testing/) | 5 | `@test`, `assert_eq`, mock, property test, snapshot |
| [Strings](./strings/) | 5 | `Regex`, `Strings.split/join`, format, interpolate |
| [DataFrame](./dataframe/) | 6 | load CSV, filter, groupby, join, export, schema |

55 recipes total. Cross-references between recipes use relative
Markdown links — e.g. *see [Files → Write text](./files/#write-text)*.

## Conventions used in every recipe

- **4 spaces** per indentation level (Buff's offside rule rejects tabs).
- **Named arguments** for any call with more than one boolean or
  optional parameter: `cache.set(k, v, ttl: Duration.seconds(30))`.
- **No `await` keyword** — async propagates automatically.
- **No `unwrap` / `expect` / `panic!`** in non-test code. Fallible ops
  return `Result<T, E>`; the `?` operator propagates.
- Constructors are `Type.new(...)` / `Type.from(...)` only (no
  `Type.create()` / `Type.build()`).

## What's missing?

If a recipe you need isn't here, file an issue in the [buff] repo —
the cookbook grows every milestone. PRs welcome.

[buff]: https://github.com/buff-lang/buff
