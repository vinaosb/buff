
## T1: Rename .sisyphus plan/notepad files + repair internal links (2026-07-15 22:43:20)

### Summary
- Renamed 7 plan files via git mv (deox-*.md -> buff-*.md), all tracked as R (rename) not D+A.
- Renamed notepad dir .sisyphus/notepads/deox-master/ -> .sisyphus/notepads/buff-master/ (4 files: decisions/issues/learnings/problems).
- Applied 554 case-sensitive text replacements across the 7 renamed plan files using ordered regex rules.
- Repaired 20+ internal markdown cross-links between plan files (all resolve correctly).
- Repaired 6 README.md plan-filename link targets (display text + URL) to point to buff-*.md.
- boulder.json: ZERO lowercase deox matches. Only Deox (capital) in absolute path epos/Deox/ - that's the local folder name, T7's scope (external/manual). Orchestrator already set active_plan to rename-to-buff.md and plan_name to rename-to-buff - untouched as required.

### Replacement Strategy (case-sensitive, ordered)
Order matters! Most specific patterns first to avoid prefix collisions:
1. Plan filename roots (deox-master, deox-v01-mvp, ..., deox-conventions) -> buff-* (NOT buff-lang-*)
2. Workspace crate names with hyphen (deox-ast, deox-lexer, ..., deox-cli) -> buff-lang-*
3. Workspace crate modules with underscore (deox_ast, deox_lexer, ..., deox_cli) -> buff_lang_*
4. DeoxError -> BuffError
5. deox.workspace.toml, deox.toml, deox.lock -> buff.* (longest first)
6. .deox -> .buff (file extension)
7. Deox(?!idizer) -> Buff (negative lookahead to preserve 'Deoxidizer' historical name-origin references)
8. deox -> buff (lowercase, final - catches standalone binary name + CLI commands like deox run)

### Key Gotchas
- **Plan filename prefix collision**: deox-master.md and deox-ast both start with deox-. Must replace plan filename roots FIRST with uff-* mapping, BEFORE crate names with uff-lang-* mapping. Wrong order = uff-lang-master.md (broken).
- **Deoxidizer preservation**: Deox is a prefix of Deoxidizer. Used negative lookahead Deox(?!idizer) to skip the historical name-origin narrative. "Buff (Deoxidizer)" reads correctly as "Buff, formerly Deoxidizer concept".
- **deox_test edge case**: In buff-project-structure.md, deox_test = "1.0" is a hypothetical package name in a TOML example (NOT a workspace crate module). Falls through to final deox -> buff step -> becomes uff_test (correct). If we'd used blanket deox_* -> buff_lang_* rule, it would wrongly become uff_lang_test.
- **Case-sensitive matching required**: PowerShell -replace is case-insensitive by default. Used [regex]::Replace(content, pattern, replacement, [System.Text.RegularExpressions.RegexOptions]::None) for explicit case-sensitive matching so Deox -> Buff and deox -> buff are distinct operations.
- **UTF-8 handling**: Used [System.IO.File]::ReadAllText(path, [System.Text.Encoding]::UTF8) and WriteAllText(path, content, New-Object System.Text.UTF8Encoding(False)) to preserve UTF-8 (no BOM). The Olá accented character was preserved byte-for-byte.
- **Active plan file is sacrosanct**: .sisyphus/plans/rename-to-buff.md contains the QA verification patterns themselves (literal strings like \[text\](./deox-*.md) describing what to grep for). These show up as matches in the audit but are NOT broken links - they're meta-documentation. DO NOT touch this file.
- **boulder.json absolute path**: Contains epos/Deox/ in active_plan absolute path. This is the local folder name; T7 (external/manual) renames it. The orchestrator explicitly set active_plan with this path - MUST NOT change per task spec, even though case-insensitive grep flags it.

### Parallel Task Observations
- T3 (crates rename) ran in parallel - all 9 crate dirs were already renamed (deox-* -> buff-lang-*) by the time T1 finished git mv operations. No file overlap with T1 (.sisyphus/ vs crates/).
- T2 (README rewrite) ran in parallel - had already updated README prose (Deox->Buff, ASCII diagram realigned, install URL buff-lang/buff). T1 only updated the 6 plan-filename link targets (display + URL); the prose changes were T2's scope.

### QA Verification Results
- Scenario 1 (Sisyphus state integrity): PASS - all 5 assertions pass (R renames, boulder.json clean, paths correct)
- Scenario 2 (No broken internal links): PASS - all renamed plan file links resolve to buff-*.md; README links resolve; 0 stale deox-* link targets in renamed files
- Residual audit: 0 lowercase deox matches in 7 renamed plan files. Only Deoxidizer (intentional historical reference) remains.

### Files Tingerprint
- buff-master.md: 37 replacements (cross-links to all phase plans + numeric-system + project-structure + conventions)
- buff-v01-mvp.md: 220 replacements (largest - detailed TDD task specs with crate names, CLI commands, file paths)
- buff-v05-language.md: 136 replacements
- buff-v10-production.md: 85 replacements
- buff-numeric-system.md: 2 replacements (only title + 1 inline reference; mostly type-system spec)
- buff-conventions.md: 14 replacements
- buff-project-structure.md: 60 replacements (deox.toml schema, deox CLI commands, file tree with .deox extension)

## T2: Rewrite README.md for Buff (2026-07-15)

### Summary
- Rewrote README.md (216 lines -> 217 lines): all Deox->Buff prose, .deox->.buff extension, clone URL vsbb1/Deox -> buff-lang/buff, cargo install package name deox-cli -> buff-lang-cli.
- Manually realigned ASCII architecture diagram: all 8 crate names widened by +5 chars uniformly (deox- 5 chars -> buff-lang- 10 chars prefix). Center column at col 52; codegen branches at cols 26 (left) and 78 (right).
- Diagram splice done programmatically via PowerShell (Space/Dash helper functions) to eliminate human space-counting errors.

### Width Math (the critical part)
- All 8 crates gain exactly +5 display columns: "deox-" (5) -> "buff-lang-" (10).
- buff-lang-lexer=15, buff-lang-parser=16, buff-lang-ast=13, buff-lang-types=15, buff-lang-codegen-rust=22, buff-lang-codegen-wgsl=22, buff-lang-runtime=17, buff-lang-cli=13.
- Chose center vertical line at col 52 (= center of buff-lang-ast at cols 46-58).
- Codegen split: left branch at col 26 (center of buff-lang-codegen-rust at cols 15-36), right branch at col 78 (center of buff-lang-codegen-wgsl at cols 67-88).
- Split/merge bars span cols 26-78 (width 53): ┌@26 + 25 dashes + ┴@52 + 25 dashes + ┐@78.

### Key Gotchas
- **Hand-counting long space runs is unreliable**: First Write attempt had off-by-one bugs (center │/▼ at col 51 instead of 52; right ▼/│ on rows 160/164 at col 76 instead of 78). Fixed by generating diagram with PowerShell " " *  helper functions and verifying U+XXXX@col positions programmatically before splicing.
- **PowerShell backtick escaping**: Regex "^`\s*$" for matching markdown fences fails because backticks are PowerShell escape chars. Used 4-backtick (`` `` ) temporary fences as unambiguous splice markers, then converted to 3-backtick in the splice pass. Single-quoted comparisons ($line -eq '`') work fine for detection.
- **PowerShell SP alias collision**: Function named SP collided with built-in Set-ItemProperty alias. Renamed to Space.
- **UTF-8 file I/O**: Get-Content/Set-Content default to system codepage (mojibake on box-drawing chars). Must use [System.IO.File]::ReadAllBytes + [System.Text.Encoding]::UTF8.GetString for reads, and [System.IO.File]::WriteAllText with New-Object System.Text.UTF8Encoding(False) (no BOM) for writes.
- **T1 overlap on README link targets**: T1 (parallel) reported updating 6 README plan-filename link targets. My T2 Write independently set the same correct targets (buff-v01-mvp.md etc.). Residual audit confirms 0 stale deox-* links; no conflict in final committed content.
- **Tagline "(Deoxidizer)" dropped**: Per task spec, dropped the parenthetical "(Deoxidizer)" from tagline. Kept the continuation line "Removes the rust (complexity), leaving pure performance" since the buffing/polishing metaphor still fits (buffing removes rust).

### QA Verification Results
- Residual Deox (case-sensitive): 0 matches
- Residual deox (lowercase): 0 matches
- Residual .deox: 0 matches
- buff-lang- count: 12 (>= 9 required)
- github.com/buff-lang/buff clone URL: 1 match
- buff-v plan links: 6 matches (3 links x 2 = display text + URL)
- Ola Buff: 4 matches
- .buff extension: 16 matches
- Diagram alignment: ALL box chars at correct columns (verified U+XXXX@col for every structural char). Center line at col 52, branches at cols 26/78, split/merge bars span 26-78.

### Parallel Task Observations
- T1 (.sisyphus) completed: 7 plan files renamed, boulder.json updated, cross-links repaired.
- T3 (crates) running in parallel: 9 crate dirs renamed deox-* -> buff-lang-*.
- No file overlap with T1 (touching .sisyphus/ + README link targets only) or T3 (touching crates/).