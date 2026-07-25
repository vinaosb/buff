Three codegen fixes enabling Buff self-hosting:

1. Enum PartialEq derive - enums now derive Clone+PartialEq+Debug (was Clone+Debug only)
2. Match arm trailing semicolons - unconditional strip (was conditional on return position)
3. Function tail expression - last ExprStmt semicolon stripped for non-void functions

First crate file ported: crates/buff-lang-error/selfhost/span.buff mirrors span.rs.

Verified working:
- buff run ola.buff -> Ola, Buff!
- buff run fibonacci.buff -> 55
- buff run selfhost_match.buff -> 1 2 3 (enum + match + implicit return)
- buff run span.buff -> 10 20 1 0 0 0 (struct + nested fields)
