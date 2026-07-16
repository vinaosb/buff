# Buff Issues

## T6: Lexer Implementation
- **Identifiers are ASCII-only in v0.1** (documented limitation). Non-ASCII identifiers like 
a�ve error out. Future enhancement: support XID_continue/XID_start via unicode-xid crate.
- **String escapes are not interpreted** at lex time � \n in source becomes the 2 chars \ and 
 in StringPart. Parser/codegen must interpret. Could be revisited if parser wants pre-processed strings.
- **Nested string-with-interp inside interp**: e.g. "{ "{x}" }" is not fully tested. ind_matching_brace skips string bodies as opaque bytes, so an unescaped " inside an inner interp could confuse it. Probably fine for v0.1 (uncommon pattern) but should be hardened in v0.5.
- **Multi-line interpolation expressions** (e.g. "x = { let y = 1; y + 1 }") � newlines inside {} are silently dropped (no Newline token emitted, no indent tracking). Parser will see the inner expression as a single line. Acceptable for v0.1.
- **Scientific notation** (1.5e10) not supported � would tokenize as Float(1.5) + Ident(e10). Add if needed.
- **Integer suffixes** (42i32, 42u8) not supported � would tokenize as Int(42) + Ident(i32). Add if needed.
- **logos dependency is unused** at runtime � kept per task spec ("add logos.workspace = true") and for future optimization. Could remove if it becomes a maintenance burden.
