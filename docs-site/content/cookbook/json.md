+++
title = "JSON"
weight = 43
+++

# JSON / structured-data recipes

Recipes for parsing and producing structured data. Buff's prelude
ships four parallel-format modules: `Toml`, `Yaml`, `Csv`, plus the
`HttpClient`-aware `Response.json()` accessor for JSON. Each
namespace-only module mirrors the same `parse(text) -> Map` /
`stringify(value) -> String` shape.

## Parse TOML into a Map

**Problem**: Read a TOML config file and look up a value.

**Solution**:

```buff
func load_config(path: String) -> Result<Map<String, String>, Error>:
    let text = File.read(path)?
    return Ok(Toml.parse(text))

func main():
    match load_config("buff.toml") {
        Ok(cfg) =>
            match cfg.get("name") {
                Some(n) => print("project: " + n),
                None    => print("no name set"),
            }
        Err(e) => print("error: " + e.string()),
    }
```

**Explanation**:

`Toml.parse(text)` is the prelude surface for the `toml` Rust crate
(T124e). It lowers to `toml::from_str::<HashMap<String,
toml::Value>>(s).unwrap_or_default()` — never panics, returns an empty
`Map` on parse failure. The Buff surface type is `Map<String, String>`
(heterogeneous toml values are stringified on entry); a future
`TomlValue` typed accessor is on the v1.18+ roadmap.

`Map.get(key)` returns `Option<String>` — `None` for missing keys, so
you always match both arms. The pattern is identical for `Yaml.parse`
and `Csv.parse` (modulo return type — CSV returns
`Vector<Vector<String>>`).

## Serialize a Map back to TOML

**Problem**: Convert a `Map` to TOML text for writing to disk.

**Solution**:

```buff
func main():
    let cfg = {
        "name":    "my_app",
        "version": "0.1.0",
    }
    let text = Toml.stringify(cfg)
    File.write("buff.toml", text)?
    print("wrote " + text.len().string() + " bytes")
```

**Explanation**:

`Toml.stringify(value)` is the inverse of `Toml.parse` — it lowers to
`toml::to_string(&v).unwrap_or_default()` and never panics. The
output is canonical TOML: one section per top-level Map key, sorted
lexicographically (matching Rust's `BTreeMap` deterministic order).

For YAML output, swap `Toml` for `Yaml` (`serde_yml` under the hood);
for CSV, use `Csv.stringify(rows)` where `rows: Vector<Vector<String>>`.
The three modules share the same `parse`/`stringify` shape — muscle
memory transfers across formats.

## Work with nested objects

**Problem**: Reach into a nested JSON-like structure without panicking
on a missing key.

**Solution**:

```buff
func first_email(user: Map<String, String>) -> Option<String>:
    return user.get("email")

func main():
    let user = {
        "name":  "Ada",
        "email": "ada@example.com",
    }
    match first_email(user) {
        Some(addr) => print("found: " + addr),
        None       => print("no email on record"),
    }
```

**Explanation**:

`Map.get(key)` returns `Option<String>`. To chain through nested
maps, use the `?.` null-conditional operator (parse-time desugar to
`and_then`):

```buff
let domain = user.get("email")?.split("@")?[1]
```

`?.` returns `None` immediately if the left side is `None` — no panic,
no `unwrap`. The whole chain has type `Option<String>`; the caller
matches `Some` / `None` as usual.

Deeply-nested heterogeneous data (think a Slack webhook payload) is
where the lack of a typed `JsonValue` shows — today you string-coerce
at the boundary. The planned `JsonValue` prelude type (v1.18+) will
surface typed accessors (`obj.get_array`, `obj.get_int`, `obj.get_bool`).

## Parse a JSON-lines file

**Problem**: Read a file of newline-delimited JSON objects.

**Solution**:

```buff
func load_events(path: String) -> Result<Vector<Map<String, String>>, Error>:
    let text = File.read(path)?
    var events: Vector<Map<String, String>> = []
    for line in text.split("\n"):
        if line.len() > 0:
            let parsed = Toml.parse(line)
            events.push(parsed)
    return Ok(events)

func main():
    match load_events("events.jsonl") {
        Ok(events) => print("loaded " + events.len().string() + " events"),
        Err(e)     => print("error: " + e.string()),
    }
```

**Explanation**:

JSON-lines (`.jsonl`) is one JSON object per line. `String.split(sep)`
returns a `Vector<String>` (UTF-8, no allocation beyond the splits).
Iterate, skip empties, parse each line. The `for` loop is the only
iteration construct — Buff has no `while` keyword by convention; for
unbounded iteration use recursion or `for` over a stream.

`buff-dataframe` (T7) ships `DataFrame.from_json(path)` for the same
pattern with column-kind inference — `Int`/`Float`/`Bool`/`String`
auto-detected per column at load time. Prefer the DataFrame form when
you need to query or aggregate the loaded data.

## Validate a value against a schema

**Problem**: Check that a parsed value matches a runtime schema before
trusting it.

**Solution**:

```buff
func is_valid_user(value: Map<String, String>) -> Bool:
    let has_name = value.contains("name")
    let has_email = value.contains("email")
    let email_ok = match value.get("email") {
        Some(addr) => addr.contains("@"),
        None       => false
    }
    return has_name and has_email and email_ok

func main():
    let user = {"name": "Ada", "email": "ada@example.com"}
    if is_valid_user(user):
        print("valid")
    else:
        print("invalid")
```

**Explanation**:

Ad-hoc validation in a function works for small schemas. For larger
ones, the `Validator` prelude type (T36, v1.13 frameworks wave 5)
wraps `buff-validate` with a declarative schema API:

```buff
let v = Validator.new()
    .field("name", required: true, min_len: 1)
    .field("email", required: true, regex: r".*@.*")
let ok = v.check(user)
```

`Validator` returns `Result<Void, Vector<String>>` — `Ok` on success,
or a list of field-level error messages on failure. The schema can be
serialised to disk and reloaded, which makes it the right shape for
config-file validation.
