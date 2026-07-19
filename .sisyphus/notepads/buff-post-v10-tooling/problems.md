# problems — buff-post-v10-tooling


## [T118] 2026-07-19T19:33:10-03:00 - No blockers

**Status**: COMPLETED - zero blocking problems.

**Minor friction (all resolved)**:
1. languageValue field doesn't exist in vscode 1.80+ ConfigurationInspect type. Fixed: use globalLanguageValue.
2. ServerOptions.debug with options: { execArgv: [] } forced TypeScript to pick the NodeModule variant of the union (instead of Executable), causing command property errors. Fixed: drop the options field - we don't need Node debug args for a binary server.
3. Task spec said # comments; reality says // + /* */. Used reality. Documented in decisions.md + issues.md.
4. PowerShell NativeCommandError noise on npm/tsc/vsce stderr warnings. Ignored - judge by exit codes.

**Verification (all pass)**:
- npm install: 293 packages, 0 vulnerabilities, exit 0.
- npm run compile (tsc -p ./): exit 0, 0 TypeScript errors.
- npx @vscode/vsce package --no-dependencies: exit 0, produces buff-vscode-1.2.0.vsix (14.6 KB, 10 files).
- Manual QA checklist documented in editors/vscode/README.md (12 steps).
- No git commit/tag (per task spec).
