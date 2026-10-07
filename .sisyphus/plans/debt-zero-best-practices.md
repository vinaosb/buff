# Debt-Zero & Best Practices — Work Plan

## TL;DR (For humans)

> **What:** 35 todos in 7 sequential PR-waves that clear every FP-verified technical debt in the Buff repo and lock in best practices — plus the 4 adopted structure items. **Why:** the 4-agent audit (FP-debunked) left a precise debt ledger; the structure agents (explore/oracle/librarian) unanimously confirmed the repo's stage-per-crate architecture is already the industry-correct shape (rustc/swc/rust-analyzer pattern; Cargo Book recommends the current layout), so this plan does hygiene + truth + 4 surgical structure wins and explicitly rejects hexagonal/DDD/MVC/MVVM ceremony and any crate renames (~2,200-import blast radius, frozen self-host names).
> **Waves:** W1 hygiene (BOM×58, mojibake, dead pin) → W2 lint/CI (clippy `--all-targets` gate, self-host-check actually gates, golden-outputs actually gate) → W3 docs truth (keywords/while/const/counts/CI claims, both book+docs-site) → W4 code rules (3 AGENTS.md, 5 unwraps + 1 documented exception, buff-eval dedup, dep upgrades, DR) → W5 deferred debt (BUG-15 bounded, BUG-10 TDD, token.buff sync, audit.toml per-ID comments) → W6 structure (ARCHITECTURE.md + wgsl read-only trace) → W7 thiserror 1→2 (Display byte-identical, rollback = single-PR revert). F1-F4 verify everything with commands, zero human checks.
> **Reviewer mandate:** Metis pre-write ✅ (all blockers folded in). Momus + Oracle post-write — iterate until all agree. Execution: separate worker session only.

> Slug: `debt-zero-best-practices` · Status: **approved** (Momus ✅ + Oracle ✅ Round 2; Metis pre-write ✅) · Intent: CLEAR · review_required: true (user-mandated Metis + Momus + Oracle, iterate until all agree — CONVERGED)
> One request → ONE plan. Executor: read top-to-bottom; every todo carries References / Acceptance / QA / Commit. You have NO interview context — if something seems ambiguous, re-read; it is specified.

## Scope

**IN** — everything FP-verified as real debt, plus the 4 adopted structure items:
1. File hygiene: 58 tracked UTF-8-BOM files, EOL renormalization, mojibake comment repair, dead dep pin removal.
2. Lint/CI: unblock + widen clippy to `--all-targets`, make self-host-check actually gate, use-cases golden coverage, pre-commit hook truth.
3. Docs truth pass: every enumerated stale claim in book/, docs-site/, README, CONTRIBUTING, AGENTS files, BUGS-FOUND.md.
4. Code rules: 3 missing AGENTS.md, 2 actionable TODOs, 5 verified unwrap/expect conversions + 1 documented exception (web3), buff-eval↔pipeline dedup, documented-duplicates note, cheap dep upgrades, accepted-debt DR.
5. Deferred-debt attacks: BUG-15 (Windows panic path), BUG-10 codegen follow-up (smallest TDD fix), self-host token.buff sync, audit.toml per-ID comment test (single ignore source).
6. Structure: ARCHITECTURE.md + codegen-wgsl wiring trace (read-only).
7. thiserror 1→2 migration (byte-identical Display strings).

**OUT / Must-NOT-Have (guardrails — violations fail F4):**
- ❌ NO rewiring the GPU/WGSL path; the wgsl trace changes ZERO code and ZERO Cargo.toml deps.
- ❌ NO converting the 107 `TokenStream::expect(...)?` fallible-API sites or ANY `#[cfg(test)]` unwrap/expect/panic (legal per repo rule).
- ❌ NO chapter/authoring rewrites — W3 is minimal truth-editing (delete/correct false claims) only; ARCHITECTURE.md documents what IS, not aspirational design.
- ❌ NO error-message/Display rewording in W7 — snapshot strings must stay byte-identical.
- ❌ NO refactoring beyond each todo's named site (pattern_guards = one assertion; no sibling span rewrites).
- ❌ NO touching the `BackendChoice`/`cranelift` mirrors in buff-eval (explicitly out — only the 4 named mirrors are deduplicated).
- ❌ NO implementing registry-dependencies cargo wiring (T127 stays deferred; W4 only rewords the emitted comment).
- ❌ NO new inference framework for BUG-10 — smallest fix that turns the RED test GREEN.
- ❌ NO `[features]`/`[lints]`/`[profile.*]` Cargo.toml sections; no crate-level `deny/forbid` attributes; ErrorCodes E10xx–E13xx untouched.
- ❌ ethers-3 / oauth2-5 upgrades stay OUT (documented accepted debt, see todo 26).
- ❌ NO test deletion/skipping to make anything pass. Fix code, not tests.

## Verification strategy

- Agent-executed QA per todo (happy + failure paths below where meaningful). Zero human-intervention checks: every acceptance = exact command + expected output.
- Evidence convention: every todo leaves artifacts at `.sisyphus/evidence/task-{N}-{slug}.{ext}` (gitignored locally; NOT committed except where stated).
- Tooling bootstrap (once, before first use): `cargo insta --version` || `cargo install cargo-insta --locked` — OR rely on plain `cargo test` everywhere (insta fails on divergence by default; "zero `.snap.new` files after the run" is the stasis gate).
- CI tests are ADVISORY (`continue-on-error`) — the REAL gates for behavior-touching waves are local `cargo test --workspace` (recorded to evidence) + CI hard gates (fmt, clippy, deny, docker, self-host-check [post-todo-7], equivalence-check, buff-validation goldens [post-todo-8]).
- Local env quirk: prepend `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"` before cargo in fresh shells; PowerShell 5.1 note — capture native-command stderr with `*>` (not `2>`), which mangles into ErrorRecord formatting.

## Execution strategy

- 7 waves, strictly sequential: W1 hygiene → W2 lint/CI → W3 docs → W4 code rules → W5 deferred-debt → W6 structure → W7 thiserror. Each wave = one branch + one PR, merged before the next starts.
- Branch `debt-zero/w{N}-{slug}`; PR title `[debt-zero] W{N}: <summary>`. One commit PER TODO (message via `git commit --no-verify -F <tempfile>`; tempfiles under `C:\Users\vsbb1\AppData\Local\Temp\opencode\`).
- `--no-verify` for ALL worker commits (the pre-commit hook is for human contributors; it rewrites AGENTS.md metadata and would churn 8 PRs).
- Worker merges own PR after CI hard gates green (standing user workflow). If dependabot PRs land mid-wave, rebase.
- Worker updates `.sisyphus/boulder.json` (tracked session state) at each wave merge.
- TDD where behavior changes: BUG-10 (todo 28) RED→GREEN; all other code todos are behavior-preserving (tests-after/none + regression suites).

## Todos

### W1 — File hygiene (branch `debt-zero/w1-hygiene`)

- [ ] 1. Strip UTF-8 BOM from all 58 tracked files + renormalize EOL
  References: verified inventory = exactly 58 tracked BOM files incl. root `Cargo.toml` + 6 crate Cargo.tomls (7 total — the cause of actions/cache "Invalid TOML" warnings), `.githooks/pre-commit` (BOM before `#!/bin/bash` breaks Linux shebang; exec bit already `100755` — do NOT chmod), `.github/dependabot.yml`.
  Acceptance: PowerShell `git ls-files | % { $p=$_; if (Test-Path $p) { $b=[IO.File]::ReadAllBytes($p); if ($b.Length -ge 3 -and $b[0]-eq0xEF -and $b[1]-eq0xBB -and $b[2]-eq0xBF) { $p } } }` outputs ZERO files; then `git add --renormalize .` (fixes the 3 committed-CRLF + root mixed-EOL manifests); `cargo metadata --no-deps > $null` succeeds (all 74 manifests parse).
  QA: happy = acceptance command empty + renormalize diff shows only the 58+ EOL files; failure-path check = `bash -n .githooks/pre-commit` succeeds and first 3 bytes are `23 21 62` (`#!b`).
  Commit: `w1: strip 58 BOMs + renormalize EOL`.
- [ ] 2. Repair mojibake sequences in comments (double-encoded UTF-8)
  References: verified sites: `crates/buff-eval/src/lib.rs:119,135`; `crates/buff-lang-pipeline/src/lib.rs:275,304`; root `Cargo.toml:8,21,671`; `crates/buff-web3/src/lib.rs:250` — plus sweep. (Note: `.gitattributes` itself is CLEAN — no edit there.)
  Acceptance: `git grep -nE "â€|â†|Ã©|Ã¢"` over tracked text files = 0 hits after repair (run same grep BEFORE to capture full file list into `task-2-mojibake-inventory.txt`); each repair is comment-only (no code-line changes; `cargo check --workspace` green).
  QA: happy = grep 0 hits; failure-evidence = pre-grep inventory committed to evidence.
  Commit: `w1: repair mojibake comments`.
- [ ] 3. Delete dead `tokio-tungstenite = "0.24"` workspace pin + mirror plan into tracked home
  References: root `Cargo.toml:347`. Pre-check (MANIFEST-ONLY — a full `crates/` grep returns ~60 comment/snapshot/walker hits and must NOT be used): `git grep -n "tokio-tungstenite" -- "crates/*/Cargo.toml"` — expected result: ONLY the explanatory comment at `crates/buff-lang-cli/Cargo.toml:114` and ZERO dependency entries. Note: codegen's `extern_crates` records the STRING "tokio-tungstenite" for USER-project Cargo.tomls (rust_codegen.rs:912+ walkers) — that is not a workspace consumer.
  ALSO (this todo) mirror the executed plan: copy this file to `.sisyphus/plans/debt-zero-best-practices.md` (tracked canonical home; `.omo/` is gitignored) so contributors see it.
  Acceptance: root Cargo.toml has no tokio-tungstenite line; `cargo check --workspace` green; `.sisyphus/plans/debt-zero-best-practices.md` exists in git.
  QA: happy = manifest grep shows zero dependency entries + check green; failure-path = if any crate Cargo.toml DECLARES it as a dependency, STOP and record in evidence (pin stays, todo re-scoped).
  Commit: `w1: drop dead pin + mirror plan`.

### W2 — Lint/CI (branch `debt-zero/w2-lint-ci`)

- [ ] 4. Fix `pattern_guards.rs:98` clippy breaker (single assertion)
  References: `crates/buff-lang-parser/tests/pattern_guards.rs:98` = `.map(|g| g.span().start >= 0)` — unsigned `>= 0` triggers deny-by-default `absurd_extreme_comparisons` under `--all-targets`.
  Acceptance: exact drop-in replacement at line 98 — `.map(|g| g.span().start >= 0)` → `.map(|g| g.span().end > g.span().start)` (keep the surrounding `.unwrap_or(false)` chain untouched; do NOT use `Option::filter` here — it yields `Option<&Guard>` and breaks the chain). Intent preserved: the tautology currently reduces to "guard is Some"; `end > start` is strictly stronger and true for parser-produced guards. ONE line, no sibling edits; `cargo clippy -p buff-lang-parser --all-targets -- -D warnings` exits 0.
  QA: happy = clippy clean; failure-evidence = before/after clippy output saved to `task-4-clippy.txt`.
  Commit: `w2: fix pattern_guards absurd comparison`.
- [ ] 5. Capture FULL `--all-targets` warning inventory, then fix ALL surfaced warnings + gate the extraction crates
  References: known clusters: proptest unused imports (test code); `crates/buff-lang-codegen-rust/tests/channel_codegen.rs` dead helpers. Full list comes from the run. ALSO: `CI_CRATES` (ci.yml:15-32) currently OMITS `buff-lang-pipeline`, `buff-lang-check`, `buff-lang-fmt` — the very crates todos 19/21/22 modify; this todo adds them.
  Acceptance: derive the `-p` list from ci.yml `CI_CRATES` env PLUS `-p buff-lang-pipeline -p buff-lang-check -p buff-lang-fmt` (verify all three are clippy-clean; fix any warnings surfaced); run `cargo clippy <that list> --all-targets -- -D warnings` capturing output to `task-5-clippy-inventory.txt` (PowerShell note: use `*> task-5-clippy-inventory.txt` — PS 5.1 `2>` on native commands mangles stderr into ErrorRecord formatting). Command exits 0. Every warning fixed at its site (test-code fixes allowed — they are code, not deletions).
  QA: happy = command exit 0 with inventory file attached; failure-path = any warning that is a genuine lint-FP gets `#[allow]` + inline justification comment (never blanket-allow).
  Commit: `w2: clear all-targets warning inventory`.
- [ ] 6. Widen CI clippy gate to the exact string + add the extraction crates
  References: `.github/workflows/ci.yml:84` currently `cargo clippy ${{ env.CI_CRATES }} --lib -- -D warnings`; `CI_CRATES` env at ci.yml:15-32.
  Acceptance: line becomes `cargo clippy ${{ env.CI_CRATES }} --all-targets -- -D warnings` AND the `CI_CRATES` env list gains `-p buff-lang-pipeline -p buff-lang-check -p buff-lang-fmt` (clippy-clean per todo 5). NOT `--workspace` (the list deliberately excludes non-gated framework crates per the comment at ci.yml:12-14). CI green on the PR.
  QA: happy = PR checks pass; failure-proof = grep the workflow for ` --lib ` returns 0 clippy-gate hits (README's local-guidance `--workspace --all-targets` stays as-is for humans).
  Commit: `w2: widen CI clippy gate to --all-targets`.
- [ ] 7. Make self-host-check actually gate (exit-code fix)
  References: `.github/workflows/ci.yml:246-276` — loop counts FAIL but never exits non-zero (grep runs inside `if`, so `bash -e` never fires) despite "HARD GATE" comment at :235. Baseline first: current job log shows PASS==TOTAL (all self-host files pass `buff check`) — capture it.
  Acceptance: after the counting loop add `if [ "$FAIL" -gt 0 ]; then echo "::error::$FAIL self-host files failed"; exit 1; fi`; job still green on the PR. Baseline proof = CI green on the PR WITH the exit-1 code present (that IS the baseline demonstration — no separate log fetch needed; if you want the pre-fix counts, `gh run list --workflow=ci.yml --limit 5` then `gh run view <id> --log | grep "Self-host"` is the optional incantation).
  QA: happy = CI job passes WITH the exit-1 code present; failure-evidence = baseline PASS/TOTAL counts in `task-7-selfhost-baseline.txt`. If baseline is NOT all-pass, STOP: record counts, keep the notice-only behavior, and flag for W5 todo 29 (token.buff sync must restore green before gating).
  Commit: `w2: self-host-check exits nonzero on failure`.
- [ ] 8. Make the existing golden-outputs step actually gate (covers the 4 regression examples for free)
  References: the existing `Validate golden outputs` step in the `buff-validation` job (ci.yml:159-182) iterates `examples/*.buff.expected` — which ALREADY includes the 4 regression examples at the examples/ ROOT: `examples/while_loop.buff`, `examples/word_operators.buff`, `examples/match_layout.buff`, `examples/multistmt_lambda.buff` (each with a `.expected` sidecar; NOT under examples/use-cases/). That step has the SAME bug as todo 7's self-host-check: it increments `FAIL` at ci.yml:174 but never exits non-zero.
  Acceptance: add `if [ "$FAIL" -gt 0 ]; then echo "::error::$FAIL golden outputs diverged"; exit 1; fi` after the existing loop (mirror todo 7's protocol: if the step goes red on the PR because golden outputs ALREADY diverge on main, capture the diverging set to evidence, STOP, and surface it — do not "fix" goldens to match without diagnosing); step green on the PR with the exit-1 present.
  QA: happy = step passes WITH the gate; failure-proof = intentionally diverge one `.expected` locally, run the step's commands, observe non-zero, revert (record in `task-8-golden-proof.txt`).
  Commit: `w2: gate golden outputs in CI`.
- [ ] 9. Pre-commit hook truth
  References: `.githooks/pre-commit` (BOM fixed by todo 1). Windows-host breakage is the historical reason for `--no-verify`.
  Acceptance: `bash -n .githooks/pre-commit` OK; hook documented in CONTRIBUTING (install step `git config core.hookspath .githooks` + note that it rewrites AGENTS.md metadata lines). No change to worker `--no-verify` convention.
  QA: happy = doc grep finds the hookspath instruction.
  Commit: `w2: document pre-commit hook`.

### W3 — Docs truth pass (branch `debt-zero/w3-docs-truth`; covers BOTH book/ AND docs-site/ — they are separate hand-maintained trees)

- [ ] 10. Keyword-count truth (25 → 30) at every site
  References: audit enumeration: `book/src/chapter-0.md`, `chapter-1.md`, `chapter-6.md`; docs-site ×2 pages; `playground/index.html`; bufflings exercises; `docs/stability-tiers.md`. README already correct (30).
  Acceptance: `git grep -nE "25 (reserved )?keywords"` over tracked files = 0 hits (run first to capture the full list — includes any site beyond the enumeration — into `task-10-keyword-sites.txt`); every hit corrected to 30 (or the sentence rewritten to drop the count where a count adds nothing).
  SCOPE NOTE — code files are IN this todo's sweep, deliberately (they carry the same false claim): `crates/buff-lang-cli/src/scaffold.rs:10` (doc comment) and `:736` (`assert_eq!(KEYWORDS.len(), 25, ...)` — update the stale KEYWORDS list to the real 30 keywords and the assertion; behavior change: scaffold validation rejects 5 more names; `cargo test -p buff-lang-cli` must be green), `crates/buff-lang-lexer/src/lexer.rs:8` (doc comment), `crates/buff-lang-error/tests/error_messages.rs:33` (comment). This is the ONLY W3 todo authorized to edit Rust source.
  QA: happy = grep 0; evidence file lists before/after per site.
  Commit: `w3: keyword count 25→30 everywhere`.
- [ ] 11. while/match/operators claim truth (docs-site)
  References: `while` IS a keyword (`crates/buff-lang-lexer/src/token.rs:89` KwWhile; parser supports it — while_loop.buff is a golden example). Audit: 4 docs-site pages claim "while not in the language" / "match braces required" / "and/or/not not &&/||/!".
  Acceptance: `git grep -niE "while (is |isn.?t |not )(not )?(in|supported)" -- docs-site/ book/` (and pattern variants `no while`, `match .* braces (are )?required`, `&&|\|\|` claims) = 0 false-claim hits; each site corrected to current language truth (while exists; braces optional where parser allows; word operators and/or/not are the syntax).
  QA: happy = greps 0; capture pre-state in `task-11-claims.txt`.
  Commit: `w3: fix while/match/operator claims`.
- [ ] 12. Remove `const` teaching (feature does not exist; BUG-2 deferred)
  References: `docs-site/content/onboarding/rust-developers.md:108,619`; `docs-site/content/onboarding/javascript-developers.md:95` (+ the `const TABLE = @comptime` example).
  Acceptance: `git grep -nE "\bconst\b" -- docs-site/content` shows zero "Buff has const" teaching claims (scan limited to the docs-site tree — Rust/SVG mentions of `const` elsewhere are irrelevant); each site replaced with one line: "Buff has no `const` yet (tracked as BUG-2); use `let`/`@comptime` per current syntax."
  QA: happy = grep 0 teaching-claims.
  Commit: `w3: stop teaching nonexistent const`.
- [ ] 13. chapter-6 minimal truth-edits
  References: `book/src/chapter-6.md`: "parser accepts only func at top level" (stale — many top-level decls); "enum-value matching is a codegen gap" (BUG-14 FIXED); "type annotation required on public functions" (BUG-10 check-side FIXED; W5 todo 28 extends to codegen — keep wording future-proof: "annotations optional; inference covers public fns"); remove `from "path" import x` docs (deferred feature).
  Acceptance: each of the 4 claims corrected/removed (edit sentences, no restructuring); `git grep -nE "only .func. at top (level|of file)|annotation required on public|from \".path.\" import" -- book/` = 0 hits; for the enum-matching claim use `git grep -n "enum.*matching" -- book/src/chapter-6.md` and fix ONLY the sentence claiming it is a codegen gap (chapter-6's other legitimate "gap" mentions — the v0.5-era gap and the multi-file linking gap — are true statements and stay).
  QA: happy = grep 0.
  Commit: `w3: chapter-6 truth edits`.
- [ ] 14. Count unification across all docs
  References: canonical numbers + derivation: crates = 73 (`ls crates | wc -l`... use `(gci crates -Directory).Count`); compiler crates = 17 (`buff-lang-*` manifests, incl. ffi-guide); subcommands = 37 (count `Command::` enum variants in `crates/buff-lang-cli/src/cli.rs` — method pinned here; cli.rs = 1,469 lines verified); `rust_codegen.rs` = 10,519 lines (`(gc ...rust_codegen.rs | measure -Line).Lines`). Fix: root `AGENTS.md` ("70-crate", "14 core", "23 subcommands", "550-line cli.rs", "12,777"), root `Cargo.toml:5` comment ("69 workspace crates"), CONTRIBUTING, README counts, `.omo`-mirror NOT needed (gitignored).
  Acceptance: `git grep -nE "\b(69|70)\b.*(crate|workspace)|14 .*core|23 subcommands|550-line|12,?777"` over tracked docs = 0 stale hits; each doc states the canonical number.
  QA: happy = grep 0 + numbers consistent across README/AGENTS/CONTRIBUTING/book.
  Commit: `w3: unify crate/subcommand/line counts`.
- [ ] 15. CONTRIBUTING CI truth
  References: `CONTRIBUTING.md:34,71,75-78,205` (claims clippy `--all-targets` at "line 18" of ci.yml etc.). Post-W2 reality: hard gates = fmt --check, clippy `$CI_CRATES --all-targets -D warnings`, cargo-deny, docker-build, self-host-check (now real, todo 7), equivalence-check, buff-validation goldens (todo 8); test-core/test-framework/installer-lint = advisory.
  Acceptance: CONTRIBUTING's CI section matches ci.yml gate-for-gate; `cargo fmt --check` on CONTRIBUTING n/a (markdown) — verify by reading ci.yml top-to-bottom against the doc once.
  QA: happy = side-by-side diff table captured in `task-15-ci-truth.txt`.
  Commit: `w3: CONTRIBUTING matches real CI`.
- [ ] 16. README examples table + BUGS-FOUND tense
  References: README examples table (21 rows) missing the 4 regression examples: `while_loop`, `word_operators`, `match_layout`, `multistmt_lambda`. `examples/use-cases/BUGS-FOUND.md` detail sections still present-tense for FIXED bugs (3/4/5/6/9/10/11/12/13/14).
  Acceptance: table gains 4 rows (one per example, matching existing row format); BUGS-FOUND detail sections converted to past tense with fix-PR references; resolution table untouched (already correct).
  QA: happy = `git grep -n "while_loop" README.md` hits the table; BUGS-FOUND read-through shows past tense.
  Commit: `w3: README table + BUGS-FOUND tense`.
- [ ] 17. AGENTS.md structure truths
  References: root `AGENTS.md`: "dual bin+lib in THREE crates" → SIX (cli, lsp, registry, buffup, bufflings, mcp); "buff-lsp depends on buff-lang-cli for fmt" → depends on `buff-lang-fmt` (`crates/buff-lsp/Cargo.toml:28`); `naming_lint.rs` location → `crates/buff-lang-check/src/`; CODE MAP pipeline stage → `buff-lang-pipeline` crate (pipeline.rs no longer exists in CLI; lib.rs:46-54 re-exports); add cargo-deny + self-host-check + equivalence-check to the CI-gates list. (DO NOT touch the eval-duplication NOTES paragraph — todo 22 owns it.)
  Acceptance: `git grep -n "THREE crates\|buff-lang-cli for fmt\|buff-lang-cli/src/naming_lint\|cli/src/pipeline.rs" -- AGENTS.md` = 0 hits.
  QA: happy = grep 0.
  Commit: `w3: AGENTS.md structure truths`.

### W4 — Code rules (branch `debt-zero/w4-code-rules`)

- [ ] 18. Create the 3 missing AGENTS.md files
  References: `crates/buff-lang-pipeline/AGENTS.md`, `crates/buff-lang-check/AGENTS.md`, `crates/buff-lang-fmt/AGENTS.md` (all verified absent; `crates/buff-email/AGENTS.md` EXISTS — do NOT touch it).
  Acceptance: each new file follows the per-crate AGENTS pattern (purpose, key modules with line refs, DEPENDENCIES list from its Cargo.toml, DEFERRED section if applicable — mirror `crates/buff-lsp/AGENTS.md` shape); content documents what IS.
  QA: happy = 3 files exist, each names its real deps (pipeline: ast, codegen-buffhtml, codegen-rust, error, lexer, parser, types, buffhtml-parser).
  Commit: `w4: AGENTS.md for pipeline/check/fmt`.
- [ ] 19. Resolve TODO at `buff-lang-pipeline/src/lib.rs:443` ("move this fix into lower_func in the codegen")
  References: the TODO's own stated intent.
  Acceptance: EITHER the fix lives in codegen-rust's `lower_func` (smallest move; `cargo test -p buff-lang-codegen-rust -p buff-lang-pipeline` green; snapshots unchanged) OR — if the move touches >2 files — reword the TODO into a tracked deferral citing `.sisyphus/decisions/codegen-rust-god-functions-deferred.md`. Record which path was taken in evidence.
  QA: happy = TODO gone (moved or reworded-with-rationale); tests green.
  Commit: `w4: resolve pipeline lib.rs:443 TODO`.
- [ ] 20. Reword emitted T127 comment in `cargo_gen.rs:133`
  References: `crates/buff-lang-cli/src/config/cargo_gen.rs:133` emits `"\n# [registry-dependencies] (T127 — cargo wiring TODO)\n"` INTO USER-GENERATED projects. It is a template string, NOT a code TODO. Registry wiring stays OUT (Must-NOT-Have).
  Acceptance: emitted comment becomes `"\n# [registry-dependencies] (not yet wired — tracked as T127)\n"` or similar TODO-free wording; `cargo test -p buff-lang-cli` green (if a scaffold-output snapshot covers the old string and diverges: accept it via `cargo insta accept` or `INSTA_UPDATE=always cargo test` — do NOT merely delete the `.snap.new` file, that rejects the change and the test stays red).
  QA: happy = `git grep -n "TODO" -- crates/buff-lang-cli/src/config/cargo_gen.rs` = 0; scaffold snapshot accepted.
  Commit: `w4: de-TODO emitted scaffold comment`.
- [ ] 21. Convert 5 verified unwrap/expect sites + document 1 justified exception (per-site else behavior)
  References + target semantics (ALL byte-verified; these plus ONE documented exception are the ONLY non-test letter-violations — the audit's "registry ×3" is a phantom: every registry unwrap/expect is `#[cfg(test)]`-gated and legal):
  1. `crates/buff-config/src/lib.rs:190` — `iter.next().unwrap()` after `iter.peek()` Some-check → `if let Some(val) = iter.next() { ...insert... }` with NO else (fn returns `()`; skip insertion — unreachable in practice, preserves behavior).
  2. `crates/buff-lsp/src/handlers.rs:223` — inner `.parse().unwrap()` inside `unwrap_or_else` → replace the whole expression with `match format!(...).parse() { Ok(uri) => uri, Err(_) => return Vec::new() }` (or the enclosing fn's existing empty-return value) — Err arm unreachable for `buff://` URIs; behavior-preserving, satisfies the no-unwrap letter.
  3. `crates/buff-web3/src/lib.rs:250` — **JUSTIFIED EXCEPTION, do NOT convert.** Site sits in `fn inert_provider_fallback() -> EthProvider<Http>` (feeds `Provider::default()`); every constructor path returns `Result` and the fn cannot; no `Default`/zeroed value type-checks. The site already carries a ~20-line invariant justification. Mirrors the repo's accepted `TokenStream::expect(...)?` philosophy. Leave as-is; note it in this todo's evidence file as the 1 accepted exception.
  4. `crates/buff-lang-check/src/naming_lint.rs:105` — `chars().next().expect("checked non-empty")` after is_empty guard → `if let Some(first) = s.chars().next() { ...body... }` with else = early return of the enclosing fn's existing "skip this ident" path.
  5. `crates/buff-jupyter/src/kernel/display.rs:133` — same pattern, else = return the raw/unformatted fallback the caller already handles for empty input.
  6. `crates/buff-lang-cli/src/scaffold.rs:179` — same pattern, else = `return Ok(())`-equivalent for the enclosing fallible fn (no scaffold entry emitted for empty name — matches the is_empty guard's intent).
  Acceptance: `git grep -nE "\.unwrap\(\)|\.expect\(" -- crates/buff-config/src/lib.rs crates/buff-lsp/src/handlers.rs crates/buff-lang-check/src/naming_lint.rs crates/buff-jupyter/src/kernel/display.rs crates/buff-lang-cli/src/scaffold.rs` shows zero non-test hits (web3 DELIBERATELY absent from this grep — its 1 site is the accepted exception, verified by reading); `cargo test -p buff-config -p buff-lsp -p buff-lang-check -p buff-jupyter -p buff-lang-cli` green.
  QA: happy = grep + tests green; failure-path = each converted site's else-branch must preserve observable behavior (run the crate's tests; where a test doesn't cover the else path, add ONE small test exercising empty/None input through the public API).
  Commit: `w4: convert 5 unwrap sites + document 1 exception`.
- [ ] 22. buff-eval ↔ buff-lang-pipeline dedup (S2 — the ONLY mirrors in scope)
  References: DELETE in `crates/buff-eval/src/lib.rs`: `EvalLinker` + `resolve_eval_linker_flags` (:113-151), `with_exe_extension` (:787-798), `sccache_available` mirror (:170-172), and the mirror-comments (:109-116, :784-786). IMPORT from `buff_lang_pipeline` (already a dependency; the crate is clap/tokio-FREE — verified — so the original duplication rationale is dead): `with_exe_extension` (pub at :1800), `LinkerChoice` (:274), `linker_from_str` (:291). Shape mismatch resolution: ADD one small pure helper `pub fn linker_args(choice: &LinkerChoice) -> Vec<&'static str>` to `buff-lang-pipeline/src/lib.rs` (returns the rustc `-C linker...` args that `resolve_eval_linker_flags` produced; no new deps); buff-eval's call sites map `EvalLinker::Auto→LinkerChoice::Auto`, `System→LinkerChoice::System`. DO NOT touch `BackendChoice`/`cranelift` mirrors (:160-174 region beyond sccache) — out of scope.
  ALSO DELETE the now-false docs: buff-eval `AGENTS.md` "Pipeline is DUPLICATED inline… keep the two copies in sync" paragraph; root `AGENTS.md` NOTES "with_exe_extension + compile_rust_to_exe logic is DUPLICATED" paragraph (this todo owns them — W3 deliberately did not touch them).
  Acceptance: `git grep -nE "Mirrors buff_lang|two copies" -- crates/buff-eval AGENTS.md` = 0 hits (NOTE: root AGENTS.md pathspec is `AGENTS.md`, repo-root relative); `cargo test -p buff-eval -p buff-lang-pipeline -p buff-repl -p buff-jupyter` green; `cargo clippy -p buff-eval -p buff-lang-pipeline --all-targets -- -D warnings` green.
  QA: happy = greps 0 + suites green; failure-path = `linker_args` unit test asserting Auto produces the same args the old mirror produced (port the old fn's expectations as the test).
  Commit: `w4: dedup buff-eval into buff-lang-pipeline`.
- [ ] 23. Documented-duplicates note in root Cargo.toml
  References: transitive dup chains verified: windows-sys ×5, hashbrown ×5, tungstenite ×4, darling ×3, spin ×3 (all transitive; upgrade cost >> benefit).
  Acceptance: one comment block near `[workspace.dependencies]` listing the chains + "accepted; revisit on major-version bumps of their parents"; links the DR from todo 26.
  QA: happy = `cargo metadata` still parses (comment-only change).
  Commit: `w4: document known duplicate chains`.
- [ ] 24. Cheap dependency upgrades: scraper 0.22→0.27, dirs 5→6
  References: root `Cargo.toml:673` (scraper; its own comment says the 0.27 bump is a no-op), `:402` (dirs 5→6).
  Acceptance: pins bumped; `cargo update -p scraper -p dirs`; `cargo test -p buff-scrape -p buff-config` (dirs consumers: grep first — `git grep -l "dirs::" -- crates/*/src` — run each consumer's tests) green; workspace clippy green.
  QA: happy = targeted tests green; failure-path = if API breaks compile, fix call sites minimally (both upgrades are minor/major-with-small-delta per audit); record diff stats in evidence.
  Commit: `w4: bump scraper + dirs`.
- [ ] 25. Runtime panic-policy wording (T70)
  References: `crates/buff-lang-runtime/AGENTS.md` — current wording vs actual policy (runtime legitimately panics on contract violations; the blanket "no panic" reading is false).
  Acceptance: AGENTS.md states the real policy: "runtime panics only on violated contracts (documented per-module); buff-assertions panics are the assertion mechanism"; no code changes.
  QA: happy = doc review; grep `panic` mentions in runtime AGENTS coherent.
  Commit: `w4: runtime panic policy wording`.
- [ ] 26. Accepted-debt DR for ethers-3/oauth2-5
  References: fork #4 default (user did not override): the h2 0.3-removal upgrades are multi-day ecosystem migrations — formally deferred.
  Acceptance: new `.sisyphus/decisions/accepted-debt-ethers-oauth2.md` (ADR shape: decision, alternatives, revisit trigger); root `audit.toml:50-55` comment points at it.
  QA: happy = file exists + audit.toml cross-ref.
  Commit: `w4: DR for deferred dep upgrades`.

### W5 — Deferred-debt attacks (branch `debt-zero/w5-deferred-debt`)

- [ ] 27. BUG-15: Windows panic-path (candidate fix, bounded)
  References: `crates/buff-lang-debug-info/src/panic_hook.rs` — the single capture call is `Backtrace::capture()` at `:203` (`:193` is its doc-comment mention; update that comment alongside). Capture on the panic path recurses/overflows on Windows hosts. (NOTE: the crate is buff-lang-**debug-info**, NOT buff-lang-error.)
  Acceptance: guard the :203 call: `#[cfg(windows)]` → skip capture (emit location-only line, matching the non-backtrace path); update the :193-195 doc comment to describe the Windows behavior; `cargo test -p buff-lang-debug-info` green locally on Windows; CI advisory `test-core` windows job log saved to evidence (it exercises debug-info transitively even though advisory).
  QA: happy = local Windows suite green + no behavior change on unix (`#[cfg]` keeps unix path byte-identical); failure-bound = if overflow persists (it shouldn't — capture is the only recursion source), REVERT this todo's commit, record in evidence, keep BUG-15 documented-open in BUGS-FOUND.md. No more than 2 fix attempts.
  Commit: `w5: BUG-15 skip Backtrace::capture on windows`.
- [ ] 28. BUG-10 codegen follow-up: untyped-param inference through transpile (smallest TDD)
  References: RED test FIRST: new `crates/buff-lang-codegen-rust/tests/untyped_param.rs` — use the crate's ESTABLISHED helper pattern (matching neighboring tests): `buff_lang_lexer::tokenize(src)` → `buff_lang_parser::parse(...)` → `generate_rust(...)` (all three crates are already dev-dependencies of buff-lang-codegen-rust). Test source (inline string): a `fn` with untyped params used in a way that requires inference, asserting the emitted Rust annotates correctly (matching how `buff check` already infers per BUG-10's fixed side). Do NOT route through `buff_lang_pipeline::compile_to_rust` — (a) buff-lang-pipeline is not a dev-dependency here, (b) its signature is `compile_to_rust(file: &Path)` (reads from disk), and adding the dev-dep would drag salsa/anyhow into codegen-rust test builds. Run the test — it must FAIL (RED) before the fix; save RED output to evidence.
  Acceptance: smallest codegen change in `crates/buff-lang-codegen-rust/src/rust_codegen*` (or types inference hook already consulted at each `let` — extend to params) turns the test GREEN; existing snapshots unchanged EXCEPT any snapshot whose source legitimately contains an untyped-param fn — such an intentional update is accepted via `cargo insta accept` (or `INSTA_UPDATE=always cargo test`) and recorded in evidence (zero UNEXPLAINED `.snap.new` after `cargo test -p buff-lang-codegen-rust`); `cargo test --workspace` green.
  QA: happy = RED evidence + GREEN run + snapshot stasis; failure-bound = if the fix exceeds one function family, STOP, keep the RED test marked `#[ignore]` with a BUG-10 reference comment, record scope in evidence (test ships ignored, debt stays tracked).
  Commit: `w5: BUG-10 untyped-param codegen (TDD)`.
- [ ] 29. self-host token.buff minimal sync (KwWhile/KwImpl)
  References: `self-host/lexer/token.buff` (verified: NO While/Impl entries) vs `crates/buff-lang-lexer/src/token.rs:89,276` (KwWhile) and `:147,295` (KwImpl).
  Acceptance: add the two keyword variants (+ their match arms wherever token.buff dispatches keywords); `cargo run -p buff-lang-cli -- check self-host/lexer/token.buff` reports no issues; CI self-host-check job (now gating per todo 7) green.
  QA: happy = check clean + CI green; failure-path = if adding keywords cascades into parser-port gaps in the .buff token port, STOP after adding ONLY the token variants, record the cascade in evidence.
  Commit: `w5: sync token.buff KwWhile/KwImpl`.
- [ ] 30. Frozen-at-v1.39 note for the self-host corpus
  References: `docs/SELF_HOST_MIGRATION.md` + header comment of `self-host/lexer/token.buff`.
  Acceptance: both locations state: "self-host corpus reflects v1.39 keyword/grammar surface; frozen pending v1.40 self-host milestone (DR-014)"; no other corpus edits.
  QA: happy = grep finds the note in both files.
  Commit: `w5: freeze note on self-host corpus`.
- [ ] 31. audit.toml per-ID comments + resolve the ignore-not-honored mystery
  References: `audit.toml:12-55` — this is **cargo-AUDIT** config (security.yml installs cargo-audit and runs `cargo audit --ignore RUSTSEC-2026-0258` at security.yml:29). cargo-audit's schema is `ignore: Vec<Id>` — STRING IDS ONLY (reason-objects are cargo-deny syntax and would break `cargo audit` config parsing — do NOT use them here). The CLI `--ignore` flag duplicating the config file is the "mystery".
  Acceptance: (a) keep/verify the string-form ignore list with a comment above EACH ignored ID explaining why (comments largely exist — complete any gaps); (b) remove the redundant CLI `--ignore RUSTSEC-2026-0258` from security.yml so audit.toml is the single source of truth; (c) NEW test `crates/buff-lang-cli/tests/audit_toml_reasons.rs` reads `../../audit.toml` (via `CARGO_MANIFEST_DIR`), parses with the `toml` crate, and asserts: parse OK + every advisory ID in the ignore list has a non-empty comment line immediately above its entry; `cargo test -p buff-lang-cli audit_toml` green; CI security job (cargo-audit) green on the PR.
  QA: happy = test green + cargo-audit job green; failure-path = if the comment-detection heuristic (comment above entry) proves brittle for the file's layout, assert instead that each ignore ID string appears in a line also containing `RUSTSEC-` context OR within N lines below a comment containing the ID — keep the assertion mechanical, never human-judged.
  Commit: `w5: audit.toml per-ID comments + single ignore source + test`.

### W6 — Structure (branch `debt-zero/w6-structure`)

- [ ] 32. codegen-wgsl wiring trace (READ-ONLY)
  References: verified: NO production crate depends on `buff-lang-codegen-wgsl` (only `buff-lang-runtime` [dev-dependencies] consumes it), despite AGENTS.md's CODE MAP claiming a live GPU path.
  Acceptance: trace and write 1-3 paragraphs answering: who (if anyone) calls `generate_wgsl`; how GPU-dispatched fns reach `buff-lang-runtime` (or don't); verdict = wired | test-only | orphaned. Output goes INTO todo 33's ARCHITECTURE.md + `task-32-wgsl-trace.txt`. ZERO code/Cargo.toml changes (Must-NOT-Have).
  QA: happy = every claim in the trace cites a file:line.
  Commit: (none — evidence + content only; lands with todo 33).
- [ ] 33. Write ARCHITECTURE.md (repo root, tracked)
  References: rust-analyzer-style. Content (all verified facts): (a) pipeline diagram (lexer→parser→types→codegen→pipeline→rustc; RSX track; WGSL status from todo 32); (b) crate taxonomy table — all 73: 17 `buff-lang-*` compiler (incl. check/fmt/pipeline extraction, P0.26 arch-003), tooling set, 42 framework (3 intra-edges: science→tensor, ml→tensor, game→ecs), version tiers; (c) dependency-direction rules + known exceptions: parser⇄types dev-back-edge (test-only, accepted), `buff-plugins`←{check,cli,lsp} (documented exception), codegen-rust EMITS `buff_lang_runtime::` tokens into user programs → **`buff-lang-runtime` crate name is a stability surface — never rename without a compat story** (rust_codegen.rs:4594); (d) API-boundary invariants (error/ast = leaf hubs, 21/13 dependents; LSP never leaks protocol types — mirror rust-analyzer's rule); (e) flat-layout rationale citing Cargo Book workspaces recommendation + matklad's Large Rust Workspaces; (f) 6 dual bin+lib binaries census; buff-mcp = sole CLI-lib consumer; buff-registry/ui-dioxus = zero buff-dep.
  Acceptance: `ARCHITECTURE.md` at repo root; every structural claim matches the dep-graph facts above (this todo's spec IS the checklist); cross-linked from README's Architecture section (one line).
  QA: happy = file exists, README links it; spot-check 5 claims against Cargo.tomls.
  Commit: `w6: add ARCHITECTURE.md`.

### W7 — thiserror 1→2 (branch `debt-zero/w7-thiserror2`)

- [ ] 34. Migrate thiserror 1.0 → 2.0 across the workspace
  References: root `Cargo.toml:106` pin; blast radius = 62 files / ~40 crates (anchored by `crates/buff-lang-error/src/diagnostic.rs`).
  Acceptance: pin → `"2"`; `cargo update -p thiserror`; fix compile fallout only (thiserror 2 is source-compatible for this repo's derive shapes — `#[error]` interpolation unchanged); **Display strings byte-identical**: snapshot stasis — run EITHER `cargo insta test --workspace` (if cargo-insta installed; else `cargo install cargo-insta --locked` once) OR plain `cargo test --workspace` and confirm ZERO `.snap.new` files exist afterward (same stasis gate). Any snapshot churn = regression, investigate, do not accept blindly; local full-suite run recorded to `task-34-test-workspace.txt` (this is THE gate — CI tests are advisory).
  QA: happy = suites green + snapshot stasis; failure-path = rollback = revert THIS wave's single PR (pin + Cargo.lock restore); no partial merges.
  Commit: `w7: thiserror 2 (Display byte-identical)`.
- [ ] 35. Record W7 gate evidence + closeout
  References: rollback mechanism (R4): single-PR revert + local test log as pre-merge gate.
  Acceptance: evidence dir contains: full local `cargo test --workspace` output + snapshot-stasis proof (insta summary or zero-`.snap.new` listing) + the Cargo.lock diff stat; PR merged only with both attached.
  QA: happy = evidence files exist and show green.
  Commit: `w7: evidence closeout`.

## Final verification wave

- [ ] F1. Compliance re-run (hygiene + lint invariants hold on main)
  Commands: tracked-BOM scan (todo 1 command) = 0 files; `git grep -nE "â€|â†|Ã©|Ã¢"` = 0; `cargo clippy <CI_CRATES + pipeline/check/fmt> --all-targets -- -D warnings` = exit 0; unwrap letter-audit: the 5 converted sites' grep = 0 non-test hits AND web3 lib.rs:250 remains the sole documented exception; TODO census: `git grep -nE "TODO|FIXME" -- crates/*/src` delta vs baseline = only tracked/justified entries.
  Evidence: `task-F1-compliance.txt`.
- [ ] F2. Quality gates
  Commands: local `cargo test --workspace` green (recorded); CI on main: fmt, clippy-all-targets, deny, docker, self-host-check (gating), equivalence-check, buff-validation goldens — ALL green; snapshot set unchanged vs pre-plan except intentionally accepted scaffold snapshot (todo 20).
  Evidence: `task-F2-quality.txt` (CI job URLs + local log).
- [ ] F3. Docs audit
  Commands: re-run todos 10-17 acceptance greps = all 0 stale hits; counts consistent (73/17/37/1469/10519) across README+AGENTS+CONTRIBUTING+book; `ARCHITECTURE.md` exists, README-linked, spot-check 5 claims.
  Evidence: `task-F3-docs.txt`.
- [ ] F4. Scope/Must-NOT-Have audit
  Checks (run PER WAVE PR as it merges, not once at the end): WGSL path untouched (no dep changes to codegen-wgsl — `git diff main...<wave-branch> -- crates/buff-lang-codegen-wgsl/Cargo.toml` empty for every wave); no `TokenStream::expect` conversions (git grep count unchanged); no `#[cfg(test)]` unwrap edits (web3's single documented exception is the only non-test expect that remains); no chapter rewrites (book diff = sentence-level); W7 Display byte-identical (snapshot stasis); no ErrorCodes renumbered; no tests deleted/skipped (`git diff` shows no `#[ignore]` additions except todo 28's bounded fallback).
  Evidence: `task-F4-scope.txt`.

## Commit strategy

- One commit per todo, message = the `Commit:` line above (`git commit --no-verify -F <tempfile>`). Branch/PR per wave (`debt-zero/w{N}-{slug}`, `[debt-zero] W{N}: …`). Worker merges after CI hard gates green; rebases over dependabot if needed; updates `.sisyphus/boulder.json` at each wave merge. Evidence: `.sisyphus/evidence/task-{N}-{slug}.{ext}` per todo (not committed).

## Success criteria

1. `cargo clippy $CI_CRATES --all-targets -- -D warnings` is a green HARD gate in CI (and `--workspace --all-targets` passes locally).
2. Zero tracked BOM/mojibake files; 7 affected manifests parse cleanly; actions/cache warnings gone.
3. Every stale docs claim enumerated in this plan is corrected; counts canonical (73/17/37) everywhere; ARCHITECTURE.md is the single structural entry point.
4. All 6 verified unwrap/expect violations RESOLVED — 5 converted + 1 documented exception (web3); buff-eval mirrors are deleted (single source of truth in buff-lang-pipeline); duplication notes deleted from both AGENTS.md files.
5. BUG-15 fixed-or-bounded (evidence recorded); BUG-10 RED→GREEN (or bounded-ignore with tracking); token.buff synced; audit.toml carries per-ID comments + comment-assertion test (single source of truth).
6. thiserror 2 with byte-identical Display output; full local test suite green; all CI hard gates green on main.
7. Zero Must-NOT-Have violations (F4).

## Review receipts

- Metis gap analysis (pre-write): COMPLETE — 13 gaps + 8 risks + 11 decisions resolved; blockers folded into this plan (receipt: `.omo/drafts/debt-zero-best-practices.md`).
- Round 1 — Momus: REVISE (3 REQUIRED: todo 27 wrong crate; todo 8 wrong paths; todo 10 code-file sweep contradiction). Verdict basis: 27/30 factual claims byte-verified exact; completeness/ordering/consistency/safety/row-grammar all PASS. Full report: session bg_671fde53.
- Round 1 — Oracle: REVISE (7 REQUIRED: todo 28 infeasible test snippet [no dev-dep + `compile_to_rust(&Path)` signature]; todo 27 wrong crate; todo 21.3 unimplementable conversion → justified exception; todo 31 cargo-audit-vs-cargo-deny tool confusion [ignore is Vec<Id> strings, reason-objects would break parsing]; todo 8 wrong paths + existing golden loop ci.yml:159-182 never exits non-zero; todo 3 self-destructing pre-check; cargo-insta not installed on worker host). Full report: session bg_111635ea.
- Round 1 fixes applied: todos 3, 4, 5, 6, 7, 8, 10, 12, 13, 20, 21, 22, 27, 28, 31, 34, 35, F1, F4 + Verification-strategy bootstrap; OPTIONAL fixes also applied (todo 4 exact drop-in, CI_CRATES += pipeline/check/fmt, todo 21.2 exact shape, todo 22 pathspec, todo 12 paths, todo 13 scoped greps, todo 7 gh hint, PS `*>` note, F4 per-wave).
- Round 2 — Momus: **APPROVE** ("all Round-1 + Oracle REQUIRED fixes landed correctly and completely at their operative sites; no FAIL-class defects introduced; row grammar intact"). Session bg_2aa6f411.
- Round 2 — Oracle: **APPROVE** (all 7 items verified applied correctly against disk — dev-deps, signatures, ci.yml loop lines, exception grounding, cargo-audit schema, manifest grep, insta fallbacks; no new technical errors; row grammar intact). Session bg_d6e3decd.
- Round-2 cosmetic polish (both reviewers' OPTIONAL notes) applied post-approval: success-criterion 4 reworded (5 converted + 1 exception); todo 21 commit message; todo 31 + scope + success-criterion 5 "per-ID comments" (was "reason objects"); todo 8 commit label; todo 20 insta-accept mechanism corrected (never delete `.snap.new` to accept); todo 27 :193-doc-comment/:203-call-site clarified; todo 28 intentional-snapshot-update path authorized.
- ALL REVIEWERS AGREE (Metis pre-write folded; Momus APPROVE; Oracle APPROVE). Plan status: approved-for-handoff.
