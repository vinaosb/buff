# Buff Documentation Site

A static documentation site for the Buff language, built with
[Zola](https://www.getzola.org/) (the Rust static site generator).

## Layout

```
docs-site/
├── config.toml           # Zola config (base_url, title, search, markdown)
├── README.md             # this file
├── content/              # Markdown source (10+ pages)
│   ├── _index.md         # landing page
│   ├── getting-started/  # install + first program + project layout
│   ├── language/         # syntax, types, async, errors, attributes
│   ├── frameworks/       # buff-* crate catalog
│   ├── cookbook/         # placeholder (T68)
│   └── migration/        # placeholder
└── templates/            # Tera templates (base + index + page + section)
```

## Build

Install Zola (any release ≥ 0.18):

```bash
# macOS:     brew install zola
# Arch:      pacman -S zola
# Other:     cargo install zola --locked
# Binaries:  https://github.com/getzola/zola/releases
```

Then, from this directory:

```bash
zola build    # → public/   (deployable static site)
zola serve    # → http://127.0.0.1:1111  (live reload)
```

The output directory is `public/`. Deploy it to any static host (GitHub Pages,
Netlify, Cloudflare Pages, S3, …).

## Content conventions

Every Markdown file starts with Zola frontmatter:

```toml
+++
title = "Page Title"
weight = N      # ordering inside the section sidebar
+++
```

`weight` is ascending — lower numbers appear first in navigation. The landing
page (`content/_index.md`) always has weight `0`.

## Why Zola?

- Single Rust binary, no Node.js / npm / JavaScript toolchain.
- Content is plain Markdown + TOML — matches the repo's "no build-step"
  philosophy for the `website/` and `playground/` assets.
- Built-in search index (elasticlunr) with no external JS dependencies.
- Templates are Tera (Jinja-like), all hand-written under `templates/`.

## Testing

The site is smoke-tested by `crates/buff-lang-cli/tests/docs_site.rs`, which
verifies:

1. `config.toml` exists and parses as TOML.
2. At least 10 Markdown files exist under `content/`.
3. Every Markdown file begins with a `+++` frontmatter block.

Run it with:

```bash
cargo test -p buff-lang-cli --test docs_site
```

## License

MIT OR Apache-2.0, matching the rest of the Buff workspace.
