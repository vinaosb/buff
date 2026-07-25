## Summary

- Fixed buff-lang-cli: 45+ pre-existing compilation errors resolved
- Fixed framework crates: buff-nlp, buff-observe, buff-xml API drift
- Workspace clippy: allow result_large_err in 9 crates (981 warnings)
- New CI/CD: 5 split jobs with caching + buff validation + Docker build
- Branch protection: main requires PR + 1 review + 5 checks + linear history
- Dockerfile.dev for local compilation (bypasses Windows MSVC blocker)

## Verified

```
buff run examples/ola.buff          -> Ola, Buff!  OK
buff run examples/fibonacci.buff    -> 55          OK
buff run examples/prelude_demo.buff -> 3           OK
cargo clippy (37 crates, lib)       -> PASS        OK
```
