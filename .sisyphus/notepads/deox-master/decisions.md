# Deox Decisions

## Project Setup
- 9 crates workspace: deox-ast, deox-lexer, deox-parser, deox-types, deox-codegen-rust, deox-codegen-wgsl, deox-runtime, deox-cli, deox-error
- Stack: logos (lexer) + chumsky (parser) + syn/quote/prettyplease (codegen)
- Testing: insta (snapshots) + proptest
- 25 keywords, 13 types
