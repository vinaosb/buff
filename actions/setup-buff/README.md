# Setup Buff GitHub Action

A GitHub Action to install the [Buff programming language](https://github.com/buff-lang/buff) in your CI workflow.

## Usage

```yaml
- uses: buff-lang/setup-buff@v1
  with:
    buff-version: "1.0.0"
- run: buff --version
```

## Inputs

| Input | Description | Required | Default |
|---|---|---|---|
| `buff-version` | Version of Buff to install (e.g. `"1.0.0"` or `"latest"`) | No | `"latest"` |
| `buffup-version` | Version of the buffup installer to use | No | `"latest"` |

## Caching

The action caches `$HOME/.buff/versions/` to speed up subsequent runs. The cache key is based on the runner OS, architecture, and the requested Buff version.

## Full Example

```yaml
name: CI
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: buff-lang/setup-buff@v1
        with:
          buff-version: "1.0.0"
      - run: buff --version
      - run: buff check src/main.buff
```

## Publishing to Marketplace

> Publishing to GitHub Actions Marketplace is a manual **user action** (out of scope of this action's code). To publish, create a release tag (e.g. `v1`) and push it to the repository. Then use the GitHub UI to publish the action to the Marketplace.

## License

MIT OR Apache-2.0 — same as the Buff project.
