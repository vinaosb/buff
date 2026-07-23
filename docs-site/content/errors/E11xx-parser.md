+++
title = "Parser errors (E11xx)"
weight = 52
+++

# Parser errors (`E11xx`)

The parser (`buff-lang-parser`) is a hand-rolled recursive-descent + Pratt
parser. When the token stream does not match the grammar, it emits an
`E11xx` error. The parser has both a fail-fast `parse()` (production) and
an accumulating `parse_recovering()` (LSP / `buff check`) entry point; the
recovering variant collects multiple errors per pass.

## Codes

| Code   | Variant                  | Trigger                                            |
|--------|--------------------------|----------------------------------------------------|
| `E1101`| `ExpectedToken`          | expected a specific token, found another / EOF     |
| `E1102`| `UnexpectedToken`        | a token that cannot legally appear here            |
| `E1103`| `ExpectedLayoutNewline`  | missing newline after `:` that opens a block       |
| `E1104`| `ExpectedIndentedBlock`  | missing indented body after `:` + newline          |
| `E1105`| `FuncMustBeTopLevel`     | `func` nested inside another block                 |
| `E1106`| `ExpectedIdentifier`     | missing name after `import`/`export`/`let`/`func`  |
| `E1107`| `UnterminatedList`       | `(...)` / `{...}` / `<...>` with no closing delim   |
| `E1108`| `UnsupportedAbi`         | `extern "ABI"` with ABI other than `"C"`           |
| `E1109`| `ExternGenericsUnsupported`| generics on an `extern "C" func`                 |
| `E1110`| `MalformedComptime`      | `comptime` not followed by a block / in bad position|

## Layout-sensitive parsing

Buff is indentation-based (Python/Haskell-style offside rule). The two
most common parser errors are about layout:

- **E1103** — you wrote `func f(): x = 1` on one line. Buff requires a
  newline after the `:`. Press Enter, then indent the body.
- **E1104** — after the `:` and its newline, the body must be indented
  deeper than the header. An empty body or a body at the same indentation
  triggers this.

## Suggestions

For `ExpectedIdentifier` on a name that is *almost* a keyword or prelude
name, the parser attaches a `note: Did you mean \`X\`?` line (T36). This
is the sentence-case form; the type-checker and linter use the lowercase
`help: did you mean \`X\`?` form (T63).

## Example

```text
[Error] error[E1104]: expected indented block after `:`
  |
1 | func main():
2 | print("hi")
  | ^^^^
  |
  note: indent the body by 4 spaces (one level)
```
