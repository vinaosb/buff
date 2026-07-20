# decisions — buff-post-v10-tooling


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
- DECISION: uff build project mode transpiles .buffâ†’.rs before cargo build (not after). This ensures the .rs files exist when cargo reads the manifest.
- DECISION: generate_cargo_toml always emits [[bin]] section. Without it, cargo build fails with 
o targets specified. The bin name matches the package name.
- DECISION: uff clean and uff update are thin wrappers (no flags). They can be extended later if needed.

