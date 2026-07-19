## F3: Real Manual QA (final-wave) — 2026-07-16T00:17:00-03:00

### Scope
Executed ALL QA scenarios from T1-T6 of the rename-to-buff plan, starting from `cargo clean` (removed 7828 files, 1.7GiB). MSVC LIB env fix re-applied in every shell (`$env:LIB = "$msvc;$sdk\um\x64;$sdk\ucrt\x64"`). Independent re-run (not relying on prior T6 evidence).

### Scenarios executed (17 total)
- **T1 (2)**: sisyphus-integrity, link-audit -> PASS, PASS
- **T2 (2)**: readme-audit, ascii-diagram -> PASS, PASS
- **T3 (4)**: build-test, residual-audit, binary-identity, error-mapper-coupled -> PASS x4
- **T4 (3)**: extension-rename, path-strings, example-content -> PASS x3
- **T5 (2)**: snapshot-regen, snapshot-content -> PASS, PASS
- **T6 (4)**: e2e-buff-run, residual-audit, metadata, edge-cases -> PASS x4

### Key verification facts
- `cargo build --workspace`: exit 0, 9 buff-lang-* crates compiled in 12.06s, 0 errors.
- `cargo test --workspace`: 503 passed, 0 failed, 4 ignored.
- `buff run examples/ola.buff`: stdout raw bytes `4F 6C C3 A1 2C 20 42 75 66 66 21 0A` = "Olá, Buff!\n" (C3 A1 = correct UTF-8 for á), exit 0. Definitive byte-level proof.
- `buff --version`: "buff 0.1.0", exit 0.
- `buff --help`: "Buff language compiler — transpiles .buff to Rust and runs rustc"; NO deox/Deox.
- `buff build examples/ola.buff`: "Built examples/ola.exe", exit 0.
- Missing-file: `buff run nonexistent.buff` -> graceful "Error: failed to read source file `nonexistent.buff`", exit 1 (no panic).
- Extra args: `buff run examples/ola.buff -- extra args` -> exit 0, prints "Olá, Buff!".
- cargo metadata: 9 buff-lang-* packages, 0 deox-*, binary "buff"; 9 buff-lang-* dirs on disk, 0 deox-*.
- Residual audit (clean-room product grep): README.md, Cargo.toml, Cargo.lock, crates/, examples/, tests/, LICENSE = 0 deox matches. Cross-validated by T2-S1/T3-S2/T4-S2/T5-S2.
- Snapshots: 0 stale *_deox*.snap, 2 *_buff*.snap + 1 AST snap; 0 deox in any .snap content.
- error_mapper coupled: 0 prog.deox, 15 prog.buff; 58 CLI tests pass (logic + assertions aligned).

### Allowlisted meta-doc residuals (NOT product failures, per F3 directive)
- .sisyphus/ matches (407 across 5 files): rename-to-buff.md plan (self-doc of old name), rename-to-buff + buff-master notepads (historical dev logs), boulder.json (T6 task_title echoing plan text).
- git history: 9 commits mention deox (correctly preserved, never rewritten per guardrails).
- Local folder `repos/Deox/` in boulder.json active_plan absolute path (T7 external/manual scope, not F3).

### Methodology notes (PowerShell gotchas encountered + worked around)
- PowerShell `Get-Content`/`-match` mangles UTF-8 accented chars (á) and box-drawing chars in display. Worked around with: (a) `[System.IO.File]::ReadAllText` + UTF8 encoding for README diagram analysis; (b) cmd.exe raw redirection + byte-level hex comparison for buff run stdout (definitive).
- `cargo metadata` JSON is compact format (`name`:`x` no space); flexible regex `"name"\s*:\s*` needed instead of literal with space.
- git grep on very long notepad lines can truncate the path prefix, breaking `-like '.sisyphus*'` categorization. Authoritative fix: explicit clean-room grep limited to product paths (returned 0).
- Build artifacts (examples/ola.exe, ola.pdb) produced by `buff build` edge-case verification were cleaned up (not committed).

### Evidence files (under .sisyphus/evidence/final-qa/)
t1-sisyphus-integrity.txt, t1-link-audit.txt, t2-readme-audit.txt, t2-ascii-diagram.txt, t3-build-test.txt (+ t3-build-stdout.txt, t3-test-stdout.txt supporting), t3-residual-audit.txt, t3-binary-identity.txt, t3-error-mapper-coupled.txt, t4-extension-rename.txt (T4 S1+S2+S3 consolidated), t5-snapshot-regen.txt (T5 S1+S2 consolidated), t6-e2e-buff-run.txt (T6 S1+S4 consolidated), t6-residual-audit.txt, t6-metadata.txt.

### T7 (external/manual) — NOT executed per task scope
GitHub repo rename + git remote URL + local folder rename remain user actions (out of F3 scope).

### VERDICT
Scenarios [17/17 pass] | Integration [4/4] | Edge Cases [2 tested] | VERDICT APPROVE

The Deox->Buff rename is complete and verified end-to-end from a clean build. All DoD criteria met.
