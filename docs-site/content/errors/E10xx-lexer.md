+++
title = "Lexer errors (E10xx)"
weight = 51
+++

# Lexer errors (`E10xx`)

The lexer (`buff-lang-lexer`) turns `.buff` source bytes into tokens. When
it hits a byte sequence it cannot start a token with, or a literal that
does not close, it emits an `E10xx` error and aborts before parsing.

All lexer errors carry a span pointing at the offending byte range, so
the diagnostic renders with a caret underline.

## Codes

| Code   | Variant                  | Trigger                                              |
|--------|--------------------------|------------------------------------------------------|
| `E1001`| `UnexpectedChar`         | a byte that cannot start any token (e.g. stray `@`)  |
| `E1002`| `UnterminatedString`     | `"...` or `'...` with no closing quote               |
| `E1003`| `InvalidNumber`          | overflow, malformed radix, bad decimal               |
| `E1004`| `MixedTabsSpaces`        | indentation mixes tabs and spaces                    |
| `E1005`| `InconsistentIndent`     | dedent lands on an unknown scope level               |
| `E1006`| `UnterminatedBlockComment`| `/* ...` with no `*/`                               |
| `E1007`| `UnterminatedRegex`      | `/...` with no closing `/`                           |
| `E1008`| `EmptyRegex`             | `//` (empty regex body)                              |
| `E1009`| `UnterminatedCharLiteral`| `'...` with no closing `'`                           |
| `E1010`| `EmptyCharLiteral`       | `''` (empty char body)                               |
| `E1011`| `InvalidCharEscape`      | unknown `\x` escape inside a string/char             |
| `E1012`| `InvalidUnicodeEscape`   | `\u{...}` with bad hex / out of range                |
| `E1013`| `UnexpectedBraceInString`| bare `}` inside `"..."`                             |
| `E1014`| `UnterminatedInterpolation`| `"{expr` with no closing `}`                       |

## Common fixes

**Tabs (E1004).** Buff mandates 4-space indentation. Configure your editor
to insert spaces when you press Tab. `buff check` also runs a source-level
tab scan that reports every tab-indented line at once (T63), so you see
all of them in one pass rather than one-at-a-time through the lexer.

**Unterminated strings (E1002).** String literals cannot span lines. Use
interpolation `"{expr}"` or escape `\n` for newlines, or split the literal
across several literals joined by `+`.

**Unexpected characters (E1001).** Code pasted from a blog post or PDF
often contains non-ASCII lookalikes (curly quotes, non-breaking spaces,
CJK punctuation). Retype the offending line by hand.

## Example

```text
[Error] error[E1002]: unterminated string literal
  |
1 | print("hello)
  |           ^^^
```

The caret covers the unclosed literal so you can see exactly where the
closing `"` is missing.
