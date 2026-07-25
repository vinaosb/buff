# docs/

Generated error catalog + handwritten technical guides. Two kinds of content:
handwritten `.md` guides at the top level and machine-generated `.html` error
pages under `errors/`. NOTE: The official guide ("The Buff Book") lives in
`../book/`, not here.

## STRUCTURE

```
docs/
    binary-size.md          # T60: --minimal profile, size-vs-speed tuning
    compile-speed.md        # T55: compile-time optimization program
    component-model.md      # T134: .buffhtml component lifecycle + typed props
    docker.md               # Official images (buff:builder, buff:slim)
    editions.md             # Language edition mechanism (buff.toml)
    extern-guide.md         # T119: calling Rust crates via extern "C"
    stability-tiers.md      # Tier 1/2/3 API stability quick-reference
    RELEASE.md              # Release runbook (SBOM, checksums, multi-arch buildx)
    errors/
        index.html          # Error catalog landing page
        styles.css          # Shared error page stylesheet
        E1xxx.html          # One page per ErrorCode (see prefix scheme below)
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| FFI / extern "C" rules | `extern-guide.md` (companion: `crates/buff-lang-ffi-guide/GUIDE.md`) |
| Component model / .buffhtml | `component-model.md` |
| Binary size tuning | `binary-size.md` |
| Compile speed knobs | `compile-speed.md` |
| Edition migration | `editions.md` |
| Stability promise | `stability-tiers.md` + `../.sisyphus/decisions/stability-promise.md` |
| Release procedure | `RELEASE.md` |
| Individual error details | `errors/E<N>.html` |
| Error catalog overview | `errors/index.html` |
| The official guide | `../book/src/SUMMARY.md` (mdBook, separate dir) |

## CONVENTIONS (this dir only)

- **ErrorCode prefix scheme** (see root AGENTS.md CONVENTIONS for stability rules):
  - E10xx = lex, E11xx = parse, E12xx = type, E13xx = codegen, E14xx = runtime, E15xx = LSP
- **ErrorCodes are STABLE FOREVER** (root AGENTS.md, anti-patterns section). Never renumber,
  reuse, silently remove, or back-fill. This dir's HTML pages reflect that contract.
- **Generated HTML must not be hand-edited.** Error pages are produced from
  `crates/buff-lang-error/src/code.rs` ErrorCode definitions. Edit the source,
  regenerate the HTML.
- **New ErrorCode**: add variant in `buff-lang-error`, regenerate error pages.
  See root AGENTS.md WHERE TO LOOK table, "Add an error variant" row.
- **No `index.md`** at the top level. The website (`../website/`) and the mdBook
  guide (`../book/`) are the user-facing docs; this dir is in-tree reference
  material (operational runbooks + the error catalog).

## NOTES

- `extern-guide.md` and `crates/buff-lang-ffi-guide/GUIDE.md` are companions:
  the guide here is the user-facing how-to; the crate guide has 6 hard rules
  for anyone writing an extern wrapper crate.
- `stability-tiers.md` references `docs.buff-lang.org/stability/` which renders
  `stability-promise.md` from `.sisyphus/decisions/`.
- Error pages all share `errors/styles.css`. They link to each other via relative
  paths, so the directory structure must stay flat (no subdirectories in errors/).
