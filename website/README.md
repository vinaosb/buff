# Buff Website

Static marketing landing page for the [Buff language](../README.md).
Showcases side-by-side Rust-vs-Buff comparisons with "Try this" links to
the playground.

> Implemented as task [T116](../.sisyphus/plans/buff-post-v10-tooling.md)
> of the post-v1.0 tooling roadmap.

## Structure

```
website/
├── index.html              # Landing page (hero, examples, quick start)
├── styles.css              # "Leatherbound" design system (matches playground)
├── app.js                  # Wire "Try this" links with base64 fragments
├── package.json            # Playwright dev dependency + npm scripts
├── playwright.config.cjs   # Auto-starts static server on port 8093
├── tests/
│   └── website.spec.cjs   # Playwright E2E tests
├── .gitignore
└── README.md               # This file
```

## Local development

Any static file server works. The Playwright config uses `python -m http.server`.

```bash
cd website

# Install test dependencies (one-time).
npm install

# Serve locally for manual browsing.
npx serve . -l 8093
# Or: python -m http.server 8093 --bind 127.0.0.1

# Open http://127.0.0.1:8093/
```

## Test

```bash
cd website
npx playwright test
```

The config auto-starts a static server on port 8093 and runs two test scenarios:
1. Landing page loads with hero pitch and at least 5 side-by-side examples.
2. "Try this" links encode Buff source into the playground URL fragment.

## Deploy

This is a **static site** with no build step. Deploy by copying the `website/`
directory to any static host.

> Deployment is a **user action**. The T116 deliverable is local only.

### GitHub Pages

```bash
# From repo root, copy website/ contents to a gh-pages branch or docs folder.
# GitHub Pages serves the files as-is; no build needed.
```

### Netlify / Vercel / Cloudflare Pages

- **Publish directory:** `website/`
- **Build command:** (none required)
- **Framework preset:** Static site

## Design notes

The website reuses the "Leatherbound" design system from the playground
(`playground/styles.css`). Both pages share the same color palette, fonts
(Space Mono, IBM Plex Mono, IBM Plex Sans), and texture so they feel like
one site.

The "Try this" links use the same UTF-8-safe base64 encoding as the
playground's `encodeBase64()` function, so clicking a link opens the
playground with the Buff source pre-loaded in the editor.
