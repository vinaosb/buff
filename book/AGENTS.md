# book/

"The Buff Book" — official mdBook guide to the Buff language. Structured in the
spirit of *The Rust Programming Language* ("TRPL"): a 9-chapter tutorial from
install through heterogeneous CPU/GPU computing, web APIs, `.buffhtml` UI apps,
and migration from Rust/Python/Go. T55 (Wave 3 of the v1.26 launch plan).

## STRUCTURE

```
book/
├── book.toml                # mdBook config: title, authors, html output, search, fold
├── README.md                # Human-facing intro: what this book is, how to read it
├── .gitignore               # Ignores mdBook's rendered/ output
└── src/
    ├── SUMMARY.md           # mdBook table of contents (entry point — read first)
    ├── chapter-0.md         # Introduction
    ├── chapter-1.md         # Getting Started
    ├── chapter-2.md         # Build a CLI
    ├── chapter-3.md         # Build an API Server
    ├── chapter-4.md         # GPU Compute
    ├── chapter-5.md         # Build a UI App
    ├── chapter-6.md         # Language Reference
    ├── chapter-7.md         # Stdlib Reference
    ├── chapter-8.md         # Error Code Handbook
    ├── chapter-9.md         # Migration Guides (Rust / Python / Go)
    └── chapter-10.md        # Real-World Examples (T21; catalogs 19 v1.26 use cases)
```

## WHERE TO LOOK

| Task | Location |
|---|---|
| Find the TOC / entry point | `src/SUMMARY.md` |
| Add a new chapter | Add `chapter-N.md` → register link in `SUMMARY.md` |
| Find a use-case example referenced in chapter-10 | `../examples/use-cases/<name>.buff` (cross-referenced) |
| Render HTML locally | `mdbook build book/` → serves `book/book/` (mdBook default output dir) |
| Configure mdBook | `book.toml` |

## CONVENTIONS (this dir only)

- **SUMMARY.md is the canonical TOC.** Every chapter must be linked there or mdBook ignores it.
- **Code blocks use `` ```buff `` fenced tag**, not `` ```rust ``. Each code block traces to (or models) a runnable example in `../examples/`.
- **Chapter numbering is sequential and stable.** Chapter-10 (Real-World Examples, T21) was the most recent addition; chapter-11+ will be appended.
- **book.toml `site-url = "/buff/"`** is configured for GitHub Pages subpath hosting (matches pages.yml workflow).
- **edit-url-template points at `main` branch** — keep in sync if the default branch changes again (was `v1x-frameworks`, then `main`, with `legacy-master` as historical marker).

## NOTES

- **No build step required for reading on GitHub.** Every `chapter-N.md` renders as Markdown directly.
- **mdBook is OPTIONAL.** Install mdBook (`cargo install mdbook`) only if you want the rendered HTML site with search + fold.
- **Chapter-10 (T21) catalogs the 19 v1.26 use-cases** — keeps `examples/use-cases/` and the book in sync. Each entry references the example file by name.
- **`book/book/` (output dir)** is gitignored — don't commit mdBook's rendered HTML.
- **This dir is NOT in `docs/`.** `docs/` holds operational runbooks + the ErrorCode HTML catalog; `book/` holds the tutorial narrative. Two distinct audiences.
