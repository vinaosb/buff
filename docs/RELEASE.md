# Buff Release Process

> Step-by-step guide for cutting and publishing a Buff release.

---

## Pre-Release Checklist

Before tagging a release, verify ALL of the following:

- [ ] **F1-F4 Final Verification**: All 4 final-verification tasks APPROVE (plan compliance, code quality, manual QA, scope fidelity)
- [ ] **Tests green**: `cargo test --workspace` passes on CI's 3-OS matrix (ubuntu, windows, macos)
- [ ] **Clippy clean**: `cargo clippy --workspace --all-targets -- -D warnings` passes on all OSes
- [ ] **Format check**: `cargo fmt --check` passes
- [ ] **CHANGELOG updated**: All changes since last release documented in `CHANGELOG.md`
- [ ] **Version bumped**: All crate `version` fields in Cargo.toml match the release version
- [ ] **Baseline captured**: `buff bench` produces baseline JSON for comparison
- [ ] **Decision record current**: `.sisyphus/decisions/` reflects current state
- [ ] **Compatibility review**: No breaking changes (or edition-gated with migration path)
- [ ] **Security scan**: `cargo audit` passes (no known CVEs)
- [ ] **SBOM generated**: CycloneDX SBOM artifact produced

---

## Release Steps

### 1. Tag the Release

```bash
# Verify you're on the right branch + commit
git status
git log --oneline -5

# Create the release tag
git tag v1.X.0 -m "v1.X.0 — <codename>"

# Push the tag (triggers release CI)
git push origin v1.X.0
```

### 2. CI Builds Artifacts

The tag push triggers `.github/workflows/release.yml` which:
- Builds stripped release binaries for 5 targets (linux-x64/arm64,
  macOS-x64/arm64, Windows-x64; windows-arm64 lands when GitHub ships the runner)
- Generates a SHA256 checksum sidecar for every archive
- Publishes all artifacts to a GitHub Release (with auto-generated notes)

`security.yml` runs in parallel on the same tag push and attaches a CycloneDX
SBOM to the release.

Monitor at: `https://github.com/<org>/buff/actions`

### 3. Verify Artifacts

- [ ] All 6 binaries present on GitHub Releases page
- [ ] SBOM artifact attached
- [ ] `buff --version` reports correct version in each binary
- [ ] Spot-test: download linux-x64 binary, run `buff run examples/ola.buff` → "Olá, Buff!"

### 4. Update Installer Channels

#### Scoop (Windows)
```bash
# Update the scoop manifest in the scoop-bucket repo
# Bump version + update URL to new release artifact
```

#### Homebrew (macOS/Linux)
```bash
# Update the Homebrew tap formula
# Bump version + update sha256
```

#### cargo install
```bash
# Verify cargo install works
cargo install --path crates/buff-lang-cli --locked
buff --version
```

#### buffup (version manager)
```bash
# Verify buffup can install the new version
buffup install v1.X.0
buffup use v1.X.0
buff --version
```

### 5. Publish to Registry

If the buff-registry is running:
```bash
# Publish the standard library packages
buff publish stdlib/
```

### 6. Deploy Documentation

- [ ] Update `website/index.html` with new version number + features
- [ ] Update `playground/` with new wasm build
- [ ] Deploy docs site (if running mdbook/T55)
- [ ] Update README.md status table

### 7. Announce

- [ ] Write release announcement (blog post / GitHub Discussion)
- [ ] Post to social media (Twitter, Reddit r/rust, Hacker News)
- [ ] Update GitHub Releases page with release notes from CHANGELOG
- [ ] Notify Discord/community channels

---

## Rollback Procedure

If a critical issue is found after release:

1. **Immediate**: Mark the GitHub Release as a "pre-release" (hides it from latest)
2. **Communicate**: Post a known-issues note on the GitHub Release page
3. **Patch**: Cut a patch release (v1.X.1) with the fix
4. **Update channels**: Scoop/Homebrew/cargo point to the patch release
5. **Document**: Add a post-mortem to `.sisyphus/decisions/`

Do NOT delete tags (they are immutable history). A bad release stays tagged; the fix is a new tag.

---

## Post-Release Metrics

Track the following after release:

| Metric | Tool | Target |
|---|---|---|
| Download count | GitHub Releases API | Track trend |
| `buffup install` count | Registry stats endpoint | Track adoption |
| Issue reports | GitHub Issues | <5 critical issues in first week |
| CI pass rate | GitHub Actions | 100% on release tag |
| Crate compile time | `buff bench` baseline comparison | No regression from previous release |

---

## Sign-Off Matrix

| Role | Responsibility | Sign-Off |
|---|---|---|
| Release Engineer | Tag, push, verify artifacts | _____ |
| QA Lead | F1-F4 approved, tests green | _____ |
| Security Officer | `cargo audit` clean, SBOM reviewed | _____ |
| Docs Lead | CHANGELOG, README, website updated | _____ |
| Community Lead | Announcement ready, channels notified | _____ |

All 5 sign-offs required before public announcement.

---

## Version Numbering

- **MAJOR** (2.0.0): Reserved for first genuinely breaking change (not planned for v1.x)
- **MINOR** (1.25.0, 1.26.0, ...): New features, additions (backwards-compatible)
- **PATCH** (1.25.1, 1.25.2, ...): Bug fixes, security patches (backwards-compatible)

See [COMPATIBILITY.md](COMPATIBILITY.md) for the full compatibility promise.

---

## References

- [CHANGELOG.md](CHANGELOG.md)
- [COMPATIBILITY.md](COMPATIBILITY.md)
- [MEMORY_SAFETY.md](MEMORY_SAFETY.md)
- [STABILITY promise](.sisyphus/decisions/stability-promise.md)
- [CI workflow](.github/workflows/ci.yml)
- [Release workflow](.github/workflows/release.yml)
- [Security workflow](.github/workflows/security.yml)
