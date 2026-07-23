+++
title = "Strings"
weight = 49
+++

# String recipes

Recipes for text. Buff's `String` type is owned UTF-8 (lowering to
Rust's `String`). The prelude provides the `Strings` namespace
(T124f) for functional-style combinators and the `Regex` type
(T124d) for compiled regular expressions.

## Split and join

**Problem**: Tokenise a string on a separator, then re-join the
tokens.

**Solution**:

```buff
func main():
    let csv = "a,b,c,d"
    let fields = Strings.split(csv, ",")
    print(fields)
    let rebuilt = Strings.join(fields, "-")
    print(rebuilt)
```

**Explanation**:

`Strings.split(text, sep)` returns a `Vector<String>` — every
occurrence of `sep` in `text` is a split point. `Strings.join(vec,
sep)` is the inverse — it concatenates the elements with `sep`
between each pair. Both live in the `Strings` namespace so they read
left-to-right in a pipeline:

```buff
let normalised = input
    |> Strings.split(_:, ",")
    |> Strings.map(_:, { s => Strings.trim(s) })
    |> Strings.join(_:, "|")
```

Some of these methods exist as instance methods on `String` too
(`text.split(sep)` works); the namespace form enables functional call
chains where the receiver isn't always a `String` (e.g. piping a
`Vector` through `Strings.join`).

## Match a regular expression

**Problem**: Test whether a string matches a regex and extract the
first capture group.

**Solution**:

```buff
func extract_domain(email: String) -> Option<String>:
    let re = Regex.compile(r"^[\w.]+@([\w.]+)$")
    match re.captures(email) {
        Some(groups) => return groups.get("1"),
        None         => return None
    }

func main():
    match extract_domain("ada@example.com") {
        Some(d) => print("domain: " + d),
        None    => print("not an email"),
    }
```

**Explanation**:

`Regex.compile(pattern)` returns a compiled `Regex` value (T124d) —
wraps `regex::Regex`. `regex.captures(text)` returns `Option<Map<
String, String>>` — `None` when no match, otherwise a map of named
and numbered groups (the numbered groups use string keys `"1"`,
`"2"`, etc.).

For a yes/no match without captures, `regex.match(text)` returns
`Option<Match>` (a zero-length match means `Some`, no match means
`None`). `regex.find(text)` returns `Option<String>` — the first
matching substring. `regex.replace(text, repl)` substitutes every
match with `repl`.

## Format a string

**Problem**: Compose a string from multiple values.

**Solution**:

```buff
func greet(name: String, count: Int) -> String:
    return "hello, " + name + " — you have " + count.string() + " messages"

func main():
    print(greet("Ada", 5))
```

**Explanation**:

Buff doesn't have Rust's `format!` macro exposed at the language
level (it would be a raw-string-codegen smell). The canonical shape
is string concatenation with explicit conversions: `.string()` on any
value serialises it to its display form (lowers to `to_string()`),
`+` concatenates.

For multi-arg prints, `print(a, b, c)` takes any number of arguments
and prints each separated by a space (lowers to `println!("{} {} {}",
a, b, c)`). The same shape works inside `Log.info(msg, field1: val1,
field2: val2)` for structured logging — fields are key/value pairs,
not just a formatted blob.

## Trim whitespace and case-fold

**Problem**: Normalise user input — strip surrounding whitespace and
convert to lowercase.

**Solution**:

```buff
func normalise(input: String) -> String:
    return Strings.lowercase(Strings.trim(input))

func main():
    let raw = "  Hello, World!  "
    print(normalise(raw))
```

**Explanation**:

`Strings.trim(text)` strips leading and trailing whitespace (lowers
to `str::trim`). `Strings.uppercase(text)` and `Strings.lowercase(text)`
case-fold the whole string. They're in the `Strings` namespace so you
can compose them as a pipeline (`text |> Strings.trim |> Strings.lowercase`
— note the `_:` placeholder for the pipeline arg).

For Unicode-aware case folding, the `Strings` module uses Rust's
`unicode_tolower`/`unicode_toupper` (correct for the basic multilingual
plane; full case-folding for combining characters is a v1.18+
enhancement). ASCII-only case folding is not exposed separately — use
`for c in s: if c.is_ascii(): ...` if you need ASCII semantics.

## Interpolate a value into a template

**Problem**: Fill in placeholders in a string template.

**Solution**:

```buff
func render(template: String, vars: Map<String, String>) -> String:
    var out: String = template
    for (key, value) in vars:
        let placeholder = "{{" + key + "}}"
        out = Strings.replace(out, placeholder, value)
    return out

func main():
    let t = "Hello, {{name}}! You are {{age}} years old."
    let vars = {"name": "Ada", "age": "36"}
    print(render(t, vars))
```

**Explanation**:

Buff has no first-class string interpolation syntax (`f"..."` /
`$"..."`) — the design bet is that explicit substitution reads more
clearly than interpolated syntax once you have more than two vars.
The recipe above walks the `vars` map and replaces each `{{key}}`
placeholder with its value. `Strings.replace(haystack, needle,
replacement)` substitutes every occurrence (lowers to
`str::replace`).

For HTML templating, prefer `buff-template` (T19) — it ships a
proper template engine with escaping, conditionals, and loops. The
recipe above is the right shape for config-file substitution,
URL templating, and other small text-replacement jobs where pulling
in a full template engine would be overkill.

## Count substring occurrences

**Problem**: Count how many times a substring appears in a string.

**Solution**:

```buff
func count_substring(haystack: String, needle: String) -> Int:
    if needle.len() == 0:
        return 0
    let parts = Strings.split(haystack, needle)
    return parts.len() - 1

func main():
    print(count_substring("the quick brown fox", "the"))
    print(count_substring("mississippi", "ss"))
```

**Explanation**:

`Strings.split(haystack, needle)` returns `N + 1` parts when `needle`
appears `N` times. Subtracting 1 gives the count. The empty-needle
guard avoids the infinite split (every character would be a split
point) — return `0` for that edge case.

For more sophisticated counting (regex, word boundaries), use
`Regex.compile(pattern)` then iterate `regex.find_all(text)` (planned
for v1.18+; today, `regex.captures(text)` returns only the first
match — wrap it in a loop with `regex.find(text[pos..])` to walk all
matches manually).
