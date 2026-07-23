+++
title = "Frameworks"
weight = 30
sort_by = "weight"
+++

Buff's "frameworks" are the `buff-*` crates that ship alongside the
compiler. They are *not* part of the language proper — they're pre-built
modules you can opt into via `import` or the prelude extension mechanism.

## Pages

- [Overview](./overview/) — full catalog of every `buff-*` crate.

## Maturity tiers

Crates fall into three maturity tiers (see the overview page for which
crate is in which tier):

| Tier | Meaning |
|---|---|
| **Core** | Part of the language's prelude; always available |
| **Standard library** | Stable, in-tree, behind `import` |
| **Ecosystem** | Optional, may depend on external crates or hardware |

The v1.x roadmap is steadily moving crates up the tiers as they stabilize.
See [`buff-v1x-frameworks.md`][plan] in `.sisyphus/plans/` for the full
schedule.

[plan]: https://github.com/buff-lang/buff/blob/master/.sisyphus/plans/buff-v1x-frameworks.md

## Using a framework

Most frameworks are imported by name:

```buff
import buff_http_client

func main():
    let resp = buff_http_client.get("https://buff-lang.org")
    print(resp.status)
```

A small set (HTTP, Filesystem, DateTime, Regex, Crypto, …) is in the
**prelude** — no `import` needed:

```buff
func main():
    print(DateTime.now())
    let re = Regex.new(r"\d+")
    let n = re.find("abc123")
```

See the [types reference](../language/types/) for the prelude types and
[`buff-lang-types/src/prelude_types.rs`][prelude-types] in the repo for
the full extensible registry.

[prelude-types]: https://github.com/buff-lang/buff/blob/master/crates/buff-lang-types/src/prelude_types.rs
