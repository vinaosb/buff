# decisions � buff-post-v10-tooling


## [T116] 2026-07-19T16:57:57

- **No JS framework**: Plain HTML/CSS/vanilla JS per task spec. No build step, no bundler. Files can be served directly by any static host.
- **data-buff-source on anchors**: Buff source stored as HTML attributes (with entities for newlines). JS reads at runtime, encodes, and sets href. This avoids hardcoding base64 in HTML (which would be fragile and unreadable).
- **3-column grid for examples**: Rust (red tint) | Buff (green tint) | Why easier (blue tint). Each column has a distinct background color for visual distinction. Stacks to single column below 960px.
- **Quick start shows cargo install (local path)**: Not cargo install buff-cli since crates.io publishing isn't done yet. Matches current README install instructions.
- **Features strip**: A separate section showing 'what Buff removes' (strikethrough + note) reinforces the pitch without bloating examples.
- **Port 8093 for website tests**: Avoids port collision with playground on 8092. Both playwright configs use reuseExistingServer: true.

## [T118] 2026-07-19T19:32:17-03:00 - VSCode extension (editors/vscode/) shipped

**Decision 1: TextMate grammar over direct tree-sitter integration**.
VSCode's native syntax highlighting is TextMate-based (.tmLanguage.json). The proposed tree-sitter API (scode.treeSitterTokenVisual) is Insiders-only + requires --enable-proposed-api, which would force users onto a non-stable VSCode. We hand-derived syntaxes/buff.tmLanguage.json from 	ree-sitter-buff/queries/highlights.scm captures + grammar.js tokens. Same 25-keyword set, same literal patterns. The two grammars stay in sync manually; tree-sitter-buff remains the source of truth (mirrors how tree-sitter-buff itself is a derived approximation of the hand-rolled parser in crates/buff-lang-parser). If VSCode ever ships a stable tree-sitter API, the migration is import tree-sitter-buff + register the existing .scm queries, no parser changes needed.

**Decision 2: Buff comments are // and /* */, NOT #**.
Task spec said #; reality (every example file + tree-sitter grammar.js comment: choice(token(seq('//', /.*/)), token(seq('/*', ...)))) says // + /* */. The task ALSO said "MUST reflect Buff reality" - so reality wins. language-configuration.json uses // for lineComment and ["/*", "*/"] for blockComment. README + 12-step manual QA checklist reflect this.

**Decision 3: No test-electron in CI**.
@vscode/test-electron downloads a full VSCode instance (~150MB Windows .zip / .dmg) per run. Heavy for a tiny extension. The task spec explicitly allows "compile-time + package-time verification + a documented manual QA checklist" as an alternative. We took that path: tsc exit 0 + sce package exit 0 + 12-step manual QA checklist in README.md. Adding test-electron later is a one-file addition (	est/runTest.ts) + one devDependency.

**Decision 4: Binary auto-discovery over forced config**.
Neither uff.serverPath nor uff.binaryPath is required. The resolver tries (1) the configured path if it exists, (2) 	arget/release/<bin>[.exe] in each workspace folder (covers "open the repo root" workflow), (3) bare binary on PATH via where/which. This makes the extension work zero-config for the canonical "git clone + cargo build + open repo" workflow.

**Decision 5: uff.run uses Terminal, uff.build / uff.check use OutputChannel**.
Terminal handles interactive stdin (future Buff REPL) + preserves ANSI colors. OutputChannel captures compiler diagnostics in a copyable, scrollable, filterable surface (closer to how rust-analyzer surfaces cargo output).

**Decision 6: uff.formatOnSave mirrors to editor.formatOnSave for the [buff] language**.
Doesn't reimplement formatting - buff-lsp already exposes documentFormattingProvider. The flag just sets the language-specific override so users opt-in once in the Buff settings section instead of digging into language-overrides. Inspects globalLanguageValue (the 9.x-correct field) before updating to avoid infinite config-change loops.

**Decision 7: No icon field yet**.
A proper icons/buff.png requires design work (a 32x32 + 16x16, light + dark variants). The task scope said "Do NOT add custom editor themes/color schemes" - we extend that to icons. The extension still packages and installs cleanly without an icon (VSCode shows a generic language icon). A future PR can add the art without touching code.

**Decision 8: Version 1.2.0**.
Matches the v1.2 *Use Buff* release. The extension is the second task of v1.2 (T118, after T117 buff-lsp); when v1.2 ships, this version aligns with the workspace crate versions post-bump.
## [T120] 2026-07-19
- DECISION: uff build project mode transpiles .buff→.rs before cargo build (not after). This ensures the .rs files exist when cargo reads the manifest.
- DECISION: generate_cargo_toml always emits [[bin]] section. Without it, cargo build fails with 
o targets specified. The bin name matches the package name.
- DECISION: uff clean and uff update are thin wrappers (no flags). They can be extended later if needed.



## [T121b] 2026-07-19 — Dioxus codegen feasibility spike: VERDICT PASS

**Task:** T121b (plan L956-1015) — UI go/no-go gate.

**Verdict:** PASS. Buff syn/quote/prettyplease codegen emits valid #​[component] + sx!{} macros that compile to wasm32 AND render+react in headless Chrome.

**Pinned versions:**
- dioxus umbrella: =0.7.2 (sub-crates resolved to 0.7.9 transitively)
- wasm-bindgen 0.2.126 (host wasm-bindgen.exe matches)
- rustc 1.95.0 / target wasm32-unknown-unknown

**Key codegen finding:** prettyplease inserts whitespace INSIDE the rsx!
TokenStream (onclick : move | _ | count += 1 vs original onclick: move |_| count += 1)
but does NOT delete/reorder/re-tokenize. dioxus-rsx proc macro accepts
the massaged form. This is THE central risk retired.

**Browser render:** counter initial = Increment (count: 0); after click =
Increment (count: 1). Signal→event→reactive DOM update pipeline proven.
Screenshot: .sisyphus/evidence/task-121b-dioxus-counter.png
Method: hand-rolled wasm-bindgen --target web + python -m http.server.
No need for dx/	runk/wasm-pack install — the existing host wasm-bindgen.exe suffices.

**Error-mapping quality:**
- rustc/dioxus-rsx diagnostics point at exact generated-.rs line:col (verified on broken.rs).
- Filename translation (.rs→.buff) already exists in buff_lang_cli::error_mapper.
- GAP: rsx! body is opaque TokenStream; per-token spans point at generated Rust text,
  not Buff AST nodes. Granularity collapses inside macro bodies.
- Recommended v1.8 baseline: post-format // buff-line:N markers + line-scan reverse-map.
  Per-element localization deferred to v1.9+.

**Risk documented:** No language transpiles to Dioxus from non-Rust source —
no community prior art for transpiler-level error mapping, type-checking rsx tree,
hot-reload across transpile. PASS removes fundamental feasibility doubt;
integration ergonomics risk remains for v1.8 T130.

**Workspace integrity:** cargo check --workspace --all-targets exits 0 (verified).
Zero new workspace deps; only one test file added to buff-lang-codegen-rust.
Spike crate lives under %TEMP%\opencode\dioxus-spike\ (throwaway, outside buff repo).

**Deliverables:**
- crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs (7 tests, all pass)
- .sisyphus/decisions/dioxus-feasibility.md (full decision record)
- .sisyphus/evidence/task-121b-dioxus-counter.png (post-click screenshot)
- .sisyphus/evidence/task-121b-decision.txt (short verdict summary)

**Implication for plan:** v1.8 T130 proceeds as planned. UI foundation risk front-loaded and retired 5 releases early.

