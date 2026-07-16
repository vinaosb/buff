# Draft: Rename Project from "Deox" to [NEW NAME]

## Project Context
- **Location**: `C:\Users\vsbb1\source\repos\Deox`
- **Nature**: Appears to be a programming language that transpiles to Rust (simpler than Rust, does heavy lifting automatically)
- **Reason for rename**: "Deox" is already used elsewhere in the tech ecosystem

## Naming Research (from user's conversation with another AI)
The user explored names inspired by:
1. **Chemistry/electrical angle** (removing oxidation, like "Deox" implies)
2. **Sleek refining terms** (stripping impurities from metal)
3. **Biological/fungal names** (honoring Rust's fungus origin)

### Candidate Names & Availability
| Name | Availability | Notes |
|------|--------------|-------|
| **Cathode** | 🟢 100% free | Safest bet. No language conflicts. |
| **Slag** | 🟢 free | Slang connotation in some dialects |
| **Anode** | 🟡 minor conflict | Android IDE runtime exists, not a language |
| **Skim** | 🟡 minor conflict | macOS PDF reader exists |
| **Hypha** | 🟡 minor conflict | Distributed computing framework (Hypha RPC) |
| Flux | 🔴 avoid | Heavily used (InfluxDB, Facebook Flux, FluxCD) |
| Mycel | 🔴 avoid | Existing data-flow runtime |
| Galva | 🔴 avoid | Existing scripting language |
| Deox | (current) | Already used elsewhere |

## Requirements (confirmed)
- [CONFIRMED - FINAL]: New name = **Buff** (mascot: Buff the Crab with buffing wheel)
- [CONFIRMED]: Crate prefix = **`buff-lang-`** (with hyphen) — e.g., `buff-lang-ast` → module `buff_language_ast`
  - Wait: Rust crate `buff-lang-ast` → module name `buff_lang_ast` (hyphens→underscores). CONFIRMED prefix.
- [CONFIRMED]: Scope = FULL rename
- [CONFIRMED]: File extension = `.buff` (NOT `.bf` — Brainfuck conflict)
- [CONFIRMED]: Binary name = `buff`
- [CONFIRMED]: Domain = `buff.rs` (primary), `buff-lang.dev` (alt)
- [CONFIRMED]: GitHub = org `buff-lang` (repo move from vsbb1/Deox)
- [CONFIRMED]: Backward compat = NONE (pre-release, clean break)
- [DEFAULT APPLIED]: Test strategy = tests-after (run existing `cargo test` suite to verify rename didn't break anything; mechanical rename doesn't need new tests)

## Research Findings
- (pending explore agent results: bg_9fbe5f88)

### Codebase Map (from explore bg_9fbe5f88 — EXHAUSTIVE)
**Project identity**:
- "Deox" = "Deoxidizer" (removes Rust's oxidation/complexity)
- Tagline: *"Rust performance with Go productivity"*
- 3 pillars: (1) transpile-don't-reimplement, (2) hide memory mgmt (no `&`, `mut`, lifetimes visible), (3) invisible heterogeneous computing (CPU+GPU via WGSL shaders)
- v0.1 milestone codename: "Olá, Deox"
- Status: in-progress (Wave-based dev, v0.1 not yet shipped)

**Structure**: Rust workspace, 9 crates under `crates/`:
- `deox-ast` — AST definitions
- `deox-lexer` — tokenizer
- `deox-parser` — parser
- `deox-types` — type system + inference
- `deox-codegen-rust` — Rust code generator
- `deox-codegen-wgsl` — WGSL shader generator (GPU!)
- `deox-runtime` — runtime support
- `deox-error` — diagnostics
- `deox-cli` — CLI binary (`name = "deox"` is the executable)

**Reference inventory (73 files total)**:
| Category | Count | Notes |
|----------|-------|-------|
| Crate directories | 9 + 1 notepad dir | `crates/deox-*` + `.sisyphus/notepads/deox-master` |
| Package IDs (Cargo.toml) | 10 | workspace + 9 crates, `name = "deox-*"`, binary `name = "deox"` |
| Source identifiers | 59+ files | `use deox_*::*`, `DeoxError`, `__deox_tmp_N` temp vars, `test.deox` paths |
| String literals | 50+ | CLI help, banners, `"Olá, Deox!"`, error msgs |
| Config (Cargo.toml) | 11 | workspace deps + per-crate |
| Documentation | 15+ md | README.md (203 lines), 7 plan files in `.sisyphus/plans/deox-*.md`, TESTING.md |
| CI/CD | 1 | `.github/workflows/ci.yml` — generic cargo cmds, minimal impact |
| Binary name | 1 CRITICAL | `[[bin]] name = "deox"` in deox-cli/Cargo.toml |
| Tests | 8+ files | cli_run_tests, cli_build_tests, span_test, infer_tests + fixtures |
| File extension | `.deox` | examples/*.deox, tests/fixtures/*/*.deox, CLI refs |
| Snapshots | auto-gen | will regenerate |

**Special identifiers to handle**:
- `__deox_tmp_N` — generated temp var prefix (in codegen-rust context.rs) — must rename to `__<newname>_tmp_N`
- `DeoxError` enum — rename to `<NewName>Error`
- `test.deox`, `run_ola.deox` — test fixture path strings

**No ambiguous references** — all "deox" occurrences are intentional project refs.

**Rename is CLEAN** — pre-release, no published crates to migrate, no external users.

## Open Questions
1. ~~Which name has the user chosen?~~ → **Buff** (mascot: crab with buffing wheel)
2. Should the git repository / folder itself be renamed? → YES (full scope confirmed)
3. Are there published packages/crates that need migration? → NO (pre-release, clean break)
4. Is there a `.deox` file extension? → YES, will change to `.buff`
5. External references? → GitHub repo (vsbb1/Deox), README clone URL

## Scope Boundaries
- INCLUDE: All 73 files with "deox" refs — crates, Cargo.toml, source identifiers, string literals, docs, tests/fixtures, file extension, binary name, GitHub repo/folder, .deox→.buff files
- EXCLUDE: target/ build artifacts (auto-regenerated), snapshot files (auto-regenerated), backward-compat shims (pre-release)

## AVAILABILITY VERIFICATION (Buff) — DONE

| Target | Status | Evidence |
|--------|--------|----------|
| Language name "Buff" | ✅ CLEAR | No language/compiler conflict found |
| `crates.io/crates/buff` (bare) | 🔴 TAKEN | "Traits for buffer" — exists |
| `crates.io/crates/buff-lang` | ✅ FREE | API 404 |
| `crates.io/crates/bufflang` | ✅ FREE | API 404 |
| `crates.io/crates/buff-cli` | ✅ FREE | API 404 |
| GitHub org `buff-lang` | ✅ FREE | 404 |
| Domain `buff.rs` | ✅ FREE | transport error (unregistered) — THEMATIC! |
| Domain `buff-lang.dev` | ✅ FREE | transport error (unregistered) |
| Domain `buff.dev` | 🟡 PARKED | premium domain for sale ($$) |
| File ext `.bf` | 🔴 CONFLICT | Brainfuck uses `.bf` |
| File ext `.buff` | ✅ CLEAN | matches name, 4 chars |

### RECOMMENDED NAMING SCHEME
- **Language name**: Buff
- **Mascot**: Buff the Crab (copper/steel crab with buffing wheel, polishing Rust)
- **File extension**: `.buff` (NOT `.bf` — Brainfuck conflict)
- **Crate prefix**: `bufflang-` → `bufflang-ast`, `bufflang-cli`, `bufflang-codegen-rust`, `bufflang-codegen-wgsl`, `bufflang-error`, `bufflang-lexer`, `bufflang-parser`, `bufflang-runtime`, `bufflang-types`
  - (bare `buff` taken on crates.io; `bufflang` prefix is clean & short, module names → `bufflang_ast` etc.)
- **Binary name**: `buff` (the command users type: `buff run hello.buff`)
- **Temp var prefix**: `__buff_tmp_N` (was `__deox_tmp_N`)
- **Error type**: `BuffError` (was `DeoxError`)
- **Primary domain**: `buff.rs` (thematic, available)
- **GitHub**: org `buff-lang`, repo `buff-lang/buff` (or move under existing user)

### Identifiers mapping (Deox → Buff)
- `deox` → `buff` (binary, language name in docs)
- `deox-*` crates → `bufflang-*` crates
- `deox_*` modules → `bufflang_*` modules
- `DeoxError` → `BuffError`
- `__deox_tmp_` → `__buff_tmp_`
- `.deox` files → `.buff` files
- `"Olá, Deox!"` → `"Olá, Buff!"`
- v0.1 milestone "Olá, Deox" → "Olá, Buff"
