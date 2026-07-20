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

## [T124c] 2026-07-20 01:21:50 -03:00

- **Decision: PreludeType::Log added as a namespace-only variant
  (NOT a new Type::Log variant).** The Buff Type enum gains NO new
  variant — Log is never a runtime value, only a namespace for
  assoc fns (Log.debug/info/warn/error). uff_type() returns
  Type::Void for Log. The existing is_prelude_datetime() Type
  predicate is unchanged (Void returns false correctly). This is the
  precedent for future namespace modules.

- **Decision: source-order field emission for tracing macros.**
  Log.info("msg", z: 1, a: 2) → 	racing::info!(z = 1, a = 2, "msg").
  NOT alphabetical sort. Rationale: (a) matches what the user wrote,
  (b) tracing itself preserves insertion order in event records,
  (c) simpler implementation (no sort pass).

- **Decision: subscriber init via 	ry_init() (panic-free) over
  init() (panics on duplicate).** Buff's "no panicking generated
  code" stance is the deciding factor. The let _ = ... discard
  swallows the Result.

- **Decision: dev/release split via cfg!(debug_assertions) RUNTIME
  check (not #[cfg(...)] compile-time).** One binary works in both
  modes. The runtime check evaluates to a constant 	rue/alse
  that the optimiser folds away, so there's no perf cost.

- **Decision: subscriber init ONLY in main, not in helpers.** The
  global subscriber is per-binary, not per-function. Emitting it in
  helpers would either (a) double-install (panic) or (b) be silently
  swallowed by 	ry_init(). Restricting to main is the canonical
  pattern. Library-style Buff programs that use Log but don't define
  main get no auto-init — they'll have silent events (no subscriber).
  This is the same trade-off T31 made for #[tokio::main] (only
  emitted on main).

- **Decision: tracing + tracing-subscriber recorded in extern_crates
  BTreeSet, NOT linked by single-file rustc path.** Same codegen-only
  linking boundary as chrono (T124b) and tokio (T31). Acceptance
  criterion is snapshot-verified codegen + registry wired + workspace
  green. Cargo-project wiring is deferred (the future Cargo-project
  pipeline will write <name> = "<version>" lines into generated
  Cargo.toml).

- **Decision: Log method calls bypass the T105 named-arg resolution
  in lower_expr's MethodCall arm.** Standard resolution drops arg
  names (correct for user methods where param order is positional).
  Log NEEDS the names (each k: v becomes a tracing field k = v).
  Intercepts Log BEFORE resolution, passes original args (with
  NamedArg nodes) to lower_prelude_type_assoc_fn. Other prelude
  types (DateTime, Duration, ...) have no named-arg usage in practice
  so they continue through the standard path.

- **Decision (T124d): Regex instance methods via the existing
  prelude-types registry — NO infer.rs edit needed.** The T124b
  registry consult in infer.rs Expr::MethodCall arm already
  routes Type.method(args) through ssoc_fn_lookup AND
  ecv.method(args) (when recv is a typed value) through
  instance_fn_lookup generically. Adding Regex to the registry
  was sufficient — no infer.rs edit. This validates the registry
  extensibility claim from T124b ("future v1.4 stdlib tasks extend
  without rewriting the inferencer"). The instance-method dispatch
  works because the codegen consults its own TypeInferencer to get
  the receiver's resolved Type, then matches against the registry.

- **Decision (T124d): egex::Regex::new(...).unwrap_or_else(|_|
  regex::Regex::new(r"a^").unwrap()) for Regex.compile.** T124b's
  DateTime.parse used unwrap_or(chrono::Utc::now()) — the fallback
  was an infallible function call. The regex crate has NO infallible
  constructor (Regex::new is the only path and returns Result). The
  codegen generates an inner .unwrap() on the provably-valid
  literal "a^" (an 'a' followed by start-of-string anchor —
  syntactically valid, semantically never matches anything). This is
  the established Rust idiom for infallible fallback from a fallible
  ctor when no const-fn constructor exists (regex has no
  Regex::empty() or const constructor; chrono's Utc::now() is
  the equivalent trusted call in the T124b precedent).

- **Decision (T124d): egex.captures lowering uses a BLOCK
  expression with let __buff_caps = ...; to bind the captures
  result ONCE.** The block evaluates to the populated HashMap.
  Binding the result avoids re-evaluating the receiver (which may
  have side effects) and avoids moving the receiver into the first
  .captures() call (allowing subsequent .name(...) lookups).
  Numbered groups iterate via caps.iter().enumerate() (index
  order); named groups via ecv.capture_names().flatten() (source
  order). Both are deterministic at runtime — the iteration order
  of the resulting HashMap is NOT deterministic, but that's a Rust
  HashMap property (lookups by key still work). Keyed as "0" for
  full match, "1"/"2"/... for numbered groups, source names for
  named groups. The block is built via quote! + syn::parse2 so
  the single string producer remains prettyplease.

- **Decision (T124d): separate Type::is_prelude_regex() predicate
  instead of extending is_prelude_datetime().** T124b's predicate
  covers only the chrono family (DateTime/Date/Time/Duration/
  Instant). Regex is a runtime value but NOT a datetime, so a
  separate predicate captures the runtime-value-but-not-datetime
  case. Future runtime-value types (Url, Hasher, Connection, ...)
  should each get their own predicate — the alternative (a single
  is_prelude_value_type() predicate covering all) would lose the
  granularity the codegen dispatch needs (each type lowers to a
  DIFFERENT Rust crate path).

- **Decision (T124d): regex crate recorded in extern_crates BTreeSet,
  NOT linked by single-file rustc path.** Same codegen-only linking
  boundary as chrono (T124b), tracing (T124c), and tokio (T31).
  Acceptance = snapshot-verified codegen + registry wired + workspace
  green. Cargo-project wiring is deferred.

## 2026-07-20 T124f — Math/Random/Sort/Strings utility modules

### Decision: separate `PreludeAssocConst` registry (not reuse `PreludeAssocFn`)

`Math.PI` / `Math.E` are accessed WITHOUT parens (`Math.PI`, not
`Math.PI()`). The parser produces a zero-arg `MethodCall` for both
`obj.field` and `obj.field()`. Reusing `PreludeAssocFn` would have
required special-casing the args.is_empty() path inside the existing
assoc-fn dispatch — cleaner to have a separate `PreludeAssocConst`
enum + `assoc_const_lookup` / `assoc_const_return_type` helpers.
Dispatch in `lower_method_call` BEFORE the T26 field-access heuristic
so `Math.PI` is rewritten to `std::f64::consts::PI` (NOT the bare
field access `Math.PI` which wouldn't compile).

### Decision: Sort = instance methods on existing Vector (NOT a new PreludeType)

`vec.sort()` / `vec.sort_by(cmp)` mirror `.len()` / `.pop()` / `.map()`
dispatch — they live in the string-method mapping block in
`lower_method_call`, NOT in the prelude-types registry. The codegen
lowers them to a `{ let mut __v = recv; __v.sort[_by](...); __v }`
block so the surface stays functional (`[3,1,2].sort() -> [1,2,3]`).
The `__v` name is `__`-prefixed to avoid colliding with user vars
(mirrors the `__recv` placeholder precedent).

### Decision: rand 0.8 (NOT 0.9)

rand 0.8 is the long-standing stable API surface the broader Rust
ecosystem targets. rand 0.9 renamed the API (`rng()` instead of
`thread_rng()`, `random_range()` instead of `gen_range()`,
`IndexedRandom` partial split from `SliceRandom`). Pinning 0.8 keeps
the codegen-lowered calls stable + matches the chrono/toml/regex/
tracing "pin pre-1.0 major" precedent. API used:
- `rand::thread_rng().gen_range(min..=max)` (inclusive range).
- `rand::thread_rng().gen::<f64>()`.
- `rand::seq::SliceRandom::{choose, shuffle}`.

### Decision: Math/Strings NO extern crate (std-only)

Math wraps `f64` methods + `std::f64::consts`; Strings wraps `str`/
`String` methods. Both are pure Rust std — no `program_uses_X` walker
needed, no extern crate registration. Only Random needs the walker
(wraps the `rand` crate).

### Decision: chrono walker narrowed to datetime family only

Pre-T124f `expr_uses_chrono` flagged ANY `is_prelude_type(id.name)`
Ident receiver as chrono usage. After T124c-f that list includes
Log/Regex/Toml/Math/Random/Strings (none lower to chrono), causing
false-positive chrono registration. Narrowed to
`prelude_type_lookup(...).buff_type().is_prelude_datetime()`
(flags only DateTime/Date/Time/Duration/Instant). Caught by the
`math_codegen_no_extern_crate_registered` test.

## 2026-07-20 T124g (Args/Env/input/sleep)

### 1. Args.Get and Env.Get share ONE PreludeAssocFn variant (Get)

Mirroring the existing Parse precedent (DateTime.parse / Date.parse / Toml.parse all share PreludeAssocFn::Parse), the new Get variant is shared between Args.get and Env.get. Initially considered two distinct variants (ArgsGet, EnvGet) but that broke the existing prelude_assoc_fn_no_duplicates invariant test (duplicate name "get") AND required rewriting ssoc_fn_lookup to scan-validate instead of find-validate-once. Consolidation into one variant preserves the no-duplicates invariant AND keeps the lookup logic simpler. Dispatch on the (type, method) pair handles the semantic difference (Args.get→String vs Env.get→Option<String>).

### 2. ssoc_fn_lookup rewritten to scan-validate

Original: ind(|f| f.name() == method)? then validate-once. With two same-named variants this returns the FIRST (ArgsGet) and fails validation on (Env, ArgsGet) → lookup returns None → Env.get would not be recognised. New: iterate all variants matching the name, return first whose (type, method) pair validates. Single source of truth = ssoc_fn_return_type matrix.

### 3. program_uses_tokio walker is NARROW (only flags sleep(...))

Per T124f gotcha: the chrono walker was originally over-broad (flagged namespace modules). The new tokio walker flags ONLY FuncCall { callee: Ident("sleep") } — NOT every async fn, NOT every 	okio::* path fragment in the lowering. The lowering is a codegen-private concern; the walker is a USER-INTENT detector. Same conservative strategy as the rand walker flagging Random.<method>(...).

### 4. 	okio enters extern_crates for the FIRST time

The v1.0 async lowering (	okio::spawn, 	okio::runtime::Runtime, #[tokio::main]) does NOT register tokio in extern_crates — single-file uff run rustc path never linked tokio either (mirrors chrono/regex/toml/rand codegen-only boundary). T124g is the FIRST time 	okio enters extern_crates; the existing async codegen paths don't need updating because their 	okio::* paths compile iff tokio is in the (deferred) Cargo project's [dependencies], which is exactly what this walker signals.

### 5. Duration.seconds(N) AST-shape detection in lower_sleep

	okio::time::sleep requires std::time::Duration — NOT chrono::TimeDelta (which T124b's Duration.seconds would normally produce; TimeDelta doesn't impl Into<std::time::Duration> without explicit conversion). To keep the sleep path chrono-independent AND the surface ergonomic, the codegen detects the MethodCall { receiver: Ident("Duration"), method: Ident(<unit>), args: [N] } AST shape and rewrites it to std::time::Duration::from_<unit>(N) directly. Plain Int args are treated as seconds (per spec's "plain int-seconds form" fallback). Other arg shapes pass through unchanged.

### 6. quote! doesn't support ## token paste

Initial attempt used quote! { std::time::Duration::from_##std_unit(#n) } which failed with "prefix rom_ is unknown". Fixed by building the method name via proc_macro2::Ident::new(&format!("from_{std_unit}"), Span::call_site()) and splicing via #ctor_name.

### 7. set_var is unsafe in Rust 2024

Buff emits Rust 2021 edition, so std::env::set_var(k, v) is safe today. The lowering is a bare call. A future edition bump will need an unsafe { ... } wrapper — flagged here, tracked for the post-v1.4 edition-migration task.

## [2026-07-20 T124h] T124h decisions
URL.parse fallback: unwrap_or_else(|_| Url::parse('about:blank').unwrap()) keeps the value-type total (no Result surfaced to Buff user yet). url crate reused from existing workspace pin (T117 LSP transitive) - not re-added. Per-crate narrow extern_crates walkers mirror program_uses_tokio/rand; URL walker gated on buff_type().is_prelude_url() (narrow, avoids chrono over-broad bug).
== T124k (crypto modules: Hash + HMAC) ==

SHAPE DECISIONS:
- Namespace-only modules mirroring T124i (Yaml/Csv) + T124j (Dir/Tempfile).
  Both Hash + HMAC have buff_type() -> Type::Void, is_namespace_only() = true.
  NO new ty.rs variant (all assoc fns return String).
- PreludeAssocFn::Sha256 is SHARED between Hash.sha256 and HMAC.sha256
  (mirrors Parse being shared between DateTime/Date/Toml/URL/UUID, Create
  being shared between Dir/Tempfile, Encode/Decode shared by Base64/Hex/
  URLEncode). Same algorithm SHA-256, different receiver type; dispatched
  on the (type, method) pair.
- Sha512 / Md5 are Hash-only (HMAC surface is SHA-256 only in T124k).
- HMAC spelled all-caps (mirrors UUID + URL convention; canonical acronym
  surfaced as PascalCase module name).

CODEGEN SHAPES (verified via accepted insta snapshots):
- Hash.sha256(d) -> { use sha2::Digest; hex::encode(sha2::Sha256::digest
  (d.as_bytes())) } - block-scoped use brings Digest trait method into
  scope (digest is a trait method, NOT inherent on Sha256).
- Hash.sha512(d) -> identical shape but Sha512.
- Hash.md5(d) -> hex::encode(md5::compute(d.as_bytes()).0) - NO use needed
  (md5::compute is a free function; .0 is tuple-struct field access).
- HMAC.sha256(k, d) -> { use hmac::Mac; hmac::Hmac::<sha2::Sha256>
  ::new_from_slice(k.as_bytes()).map(|mut mac| { mac.update(d.as_bytes
  ()); hex::encode(mac.finalize().into_bytes()) }).unwrap_or_default() }
  - block-scoped use for Mac trait methods; panic-free via .map().
  unwrap_or_default() (new_from_slice returns Result).

NARROW WALKERS (chrono-over-broad gotcha avoidance):
- program_uses_sha2 flags ONLY: (Hash, sha256) / (Hash, sha512) / (HMAC,
  sha256). Method-aware + receiver-aware. NOT a generic Hash-namespace
  walker (would over-register sha2 for Hash.md5-only programs).
- program_uses_md5 flags ONLY: (Hash, md5).
- program_uses_hmac flags ONLY: (HMAC, sha256).
- HMAC.sha256 also records sha2 (cross-crate coupling handled in
  generate() caller, NOT the walker - walker stays minimal one-crate).
- hex recorded alongside every Hash/HMAC call (every digest/MAC lowers
  to hex::encode).

VERIFICATION: all 4 gates green. cargo build/clippy/test all EXIT=0.
buff-lang-codegen-rust full test suite green (no regressions).


## T124l (Process / OS prelude modules) - 2026-07-20

### Process value-shape: Option<std::process::Child>

Chose to surface the spawned value as Option<std::process::Child> rather than
std::process::Child or Result<Process, Error>. Rationale:

- Buff's "no panicking generated code" rule means Process.spawn CANNOT use
  .expect() / .unwrap() on the fallible Command::spawn() -> io::Result<Child>.
- The Option wrapper collapses spawn failure to None via .spawn().ok().
- .wait() / .id() then chain .map(...).unwrap_or_default() through the
  Option so spawn-failure is observable as default Int (0) without panicking.
- A future task may surface spawn failure via a Result<Process, Error> if a
  use case emerges; the Option wrapper is the minimum panic-free shape today.

### buff_type_to_syn for Process: emit the Option wrapper explicitly

Type::Process => Option<std::process::Child> (via make_generic_path_type).
This makes the generated let p: Option<std::process::Child> = ... annotation
visible in snapshots, communicating the wrapper shape to readers. Without this,
let p = ...spawn().ok(); would compile (Rust infers Option) but the surface
type contract would be invisible.

### Hostname via env vars (NO hostname crate)

OS.hostname() lowers to std::env::var("COMPUTERNAME").or_else(|_|
std::env::var("HOSTNAME")).unwrap_or_default(). This is the bare-minimum
approach covering Windows (COMPUTERNAME) + Unix (HOSTNAME). Empty String when
neither env var is set. NO hostname crate added (spec explicitly forbids it;
the codegen-only linking boundary limit is observed).

### num_cpus narrow walker (program_uses_num_cpus)

Narrow method-aware walker flags ONLY the (OS, cpus) combination (mirrors
program_uses_dir_walk from T124j, program_uses_sha2 from T124k). A generic
program_uses_namespace("OS") would over-register num_cpus for programs using
only OS.name/arch/hostname (those use std::env::consts + env-var - std-only).
Process.* uses std::process::* (std-only - NO extern crate recorded, mirrors
Path / Dir.list / Tempfile.dir stance).

### Process.spawn arg shape: separate command + args (NO shell)

Process.spawn(cmd, args) lowers to Command::new(cmd).args(args).spawn().ok()
- the command and args are passed SEPARATELY to Command::new + .args,
NOT through a shell. This eliminates the shell-injection vector (spec safety
stance). The args param is a Vector<String> (codegen splices whatever
expression the user provides; the type-inference layer enforces the surface
contract).

### T122 — Path-form git deps (not Cargo { git = "..." } form)

generate_cargo_toml emits git-deps as 
ame = { path = "<checkout>" } pointing at
~/.buff/git/<sha256(url)[..16]>/. Chosen over Cargo's native { git = "URL" }
form because:
1. OFFLINE-FRIENDLY — cargo never re-fetches; the single canonical checkout per URL
   is shared across all projects.
2. MATCHES THE "clone to ~/.buff/git" DESIGN — one place to inspect/patch the dep.
3. INSPECTABLE — users can read/modify the checkout directly; uff update will
   re-clone on demand in a future task.
The trade-off: cargo can't auto-update the dep (the path is fixed). For T122 this is
acceptable; future uff update will re-clone to refresh the checkout in place.

### T122 — GitDependency stores all 3 qualifiers; clone-time precedence is rev > tag > branch

The buff.toml schema stores git, ranch, 	ag, ev independently (matching
Cargo's schema). At clone time, precedence is: if rev is set, plain clone +
git checkout <rev>; else if tag is set, --branch <tag> (git accepts tags as
--branch refs); else if branch is set, --branch <branch>. Stored fields preserve
user intent regardless of which one was active at clone time.

### T122 — BUFF_HOME env override for test isolation (no new --buff-home CLI flag)

buff_home_dir() reads  first, falls back to  (Windows) or
C:\Users\vsbb1 (Unix). This lets integration tests isolate the checkout cache to a per-test
tempdir without mutating process-wide env vars. NO --buff-home CLI flag was added
(overengineering for v1.0; users get the default ~/.buff behavior).

### T122 — Transitive dep parse is logged, not resolved (v1.6 work)

uff add reads the cloned repo's buff.toml and prints transitive [dependencies]
to stderr. It does NOT recursively clone them — that's a registry task (v1.6).
The T122 acceptance gate is "parse + log", not "recursive resolution".


### T123 - Workspace as cargo passthrough (NOT reinvented)

The Buff CLI does NOT implement workspace dependency-dedup, shared-target/, or member-fan-out. It emits a virtual [workspace] Cargo.toml and lets cargo do ALL of that. The Buff-specific work is:
1. Parse [workspace] from buff.toml (Buff's schema).
2. Emit the matching [workspace] block in the generated Cargo.toml (Buff's codegen).
3. Transpile each member's .buff files to .rs (cargo doesn't know about .buff).
4. Shell out to cargo build / cargo test at the workspace root (cargo fans out).

Anything else (dep resolution, target/ sharing, build ordering) is cargo's job. Reinventing any of it would be a violation of the passthrough principle.

### T123 - BuffConfig::package is Option<PackageSection> (mutually-exclusive with workspace)

Cargo virtual manifests have NO [package] section. To represent this in Buff's schema, BuffConfig::package was made Optional. Mutually-exclusive with the new workspace: Option<WorkspaceSection> field. parse() enforces "exactly one of package/workspace is Some" via post-deserialize cross-field validation (serde can't easily do cross-field rules). The validation returns ConfigError::Layout (not Parse) because the failure is structural, not syntactic.

### T123 - Workspace resolver defaults to "2" at emission time

Cargo's resolver = "2" is the modern default (since Rust 1.51). WorkspaceSection.resolver is Option<String> in the schema (user can override to "1"). generate_workspace_cargo_toml uses ws.resolver.as_deref().unwrap_or("2") so absent resolver still emits a valid virtual manifest. We do NOT inherit the resolver from the parent workspace's Cargo.toml because buff.toml is the single source of truth for Buff projects.

### T123 - uff test (no file) -> cargo test at root (project OR workspace)

Made Command::Test { file: Option<PathBuf> } (was required PathBuf). When file is None, commands::test::run_project() emits Cargo.toml (idempotent) and invokes cargo test at the cwd. Buff does NOT loop members at the cargo level — cargo fans out itself. This mirrors how Build already handled project mode since T120.

## T124 — error code catalog (resume)

- **Separate `styles.css` over inline `<style>` on every page.** The spec wording was ambiguous (`inline <style>`); the strongest constraint was "match playground/website", which both use a separate `styles.css`. A shared file also keeps the page count low and lets future maintainers restyle the whole site by editing one file. Trade-off: each page needs an HTTP request for the CSS (fine for any real static host; matters only on `file://`).
- **Committed generator as an example binary**, not a build script or a test. Rationale: (1) the generator IS legitimate `buff-lang-error` API surface (exercises `ErrorCode::all()`, `code_str()`, `title()`, `explanation()`); (2) it stays out of the build graph (no `build.rs` cost); (3) re-running it is an explicit maintainer action, not a side-effect of `cargo test`; (4) it can use `panic!` / `expect` freely (the "no panic in non-test code" rule applies to library code, not examples).
- **E14xx block reserved, not pre-allocated.** The runtime crate (`buff-lang-runtime`) does not currently emit any `RuntimeError` with a stable code. Reserving the block documents the intent without inventing speculative codes (T124 hard rule: "no speculative/aspirational codes"). When the runtime starts emitting diagnostics, they get the next free `E14xx`.
- **Warning code (E1303 AsyncBlockDeadlock) reuses the same `error[E1xxx]:` prefix** even though it is a Warning severity. The render format is `[Warning] error[E1303]: <msg>`. Alternative considered: `warning[E1303]:` — rejected because (a) rustc uses `error[Exxxx]:` for some lints too, (b) tooling that parses codes should not have to special-case severity, (c) the code itself is severity-agnostic.
- **§19 in conventions.md is intentionally long and prescriptive** (6 numbered non-negotiable rules + 3 attach-or-not bullets). Rationale: stability guarantees need to spell out every forbidden action (renumber, reuse, silently delete) so a future contributor cannot claim "the spec didn't say I couldn't". Mirrors rustc's published E0xxx policy.
- **Pragmatic-code-attachment bar** documented in §19: codes attached at named constructors (`LexerError::unexpected_char`) + central helpers (`TokenStream::expect`, type-checker operator paths, codegen `unsupported()`), NOT at every internal `Diagnostic::error(...)` site. This covers the user-facing surface without inventing a code per parse-rule.

## T125-prep - buff-eval shared evaluation core

- **Evaluate via the real rustc pipeline, not a hand-rolled interpreter.** eval() composes accumulated state into a self-contained Buff program, runs tokenize -> parse -> generate_rust -> `rustc --edition 2021 -O` -> spawn, and captures the child's stdout/stderr via `std::process::Command::output`. Rationale: type inference currently lives INSIDE codegen, so any standalone interpreter would duplicate/diverge from real semantics. Trade-off: every eval invokes rustc (slow), acceptable for a REPL/Jupyter cadence and keeps output byte-identical to `buff run`.
- **stdout capture mechanism = Command::output().** Buff `print` lowers to Rust `println!` -> child stdout; `output()` captures into Vec<u8> without touching the parent process stdout. This is exactly what T129b (Jupyter iopub) and T138c (Bufflings output compare) need.
- **Snippet classification** via `parse_expression` (strict, all-tokens-consumed) then `parse`: Empty | BareExpr(is_print flag) | BodyStmt | TopLevelDecl(non-main) | FullProgram(has main). BareExpr wrapped as `print(<expr>)` to surface value, UNLESS it is itself a print/println call (Void return -> would be `print(print())` rustc type error). FullProgram NOT accumulated (user owns main; else duplicate-main error).
- **State accumulation stored as verbatim source strings** (top_level_src + body_stmts_src), re-parsed each call so codegen always sees a complete program. type_of() is side-effect-free (reads accumulated env, does not mutate).
- **No CLI dep** for with_exe_extension - reimplemented the one helper to avoid pulling clap/tokio into the eval crate.


## [T125a] 2026-07-20T12:00:00-03:00 - REPL core (crates/buff-repl) shipped

**Decision 1: pure `evaluate_and_format` over a `Repl::handle_line` method.**
The formatting layer is a free function `pub fn evaluate_and_format(ev: &mut Evaluator, input: &str) -> String`, NOT a `&mut self` method on `Repl`. This is the testability keystone: tests construct their own `Evaluator` and call the fn directly — no TTY, no rustyline, no Editor<...> generic args. The `Repl` struct owns the TTY-bound state (DefaultEditor + Evaluator + prompt) and its `run()` method calls the free fn internally. Mirrors how buff-eval exposes pure `classify` + `compose_program_body` so the broader pipeline can be tested without spawning `rustc`.

**Decision 2: rustyline 15, NOT 18.**
Latest is 18.0.1 (2026-06-24) but the API surface we depend on (`DefaultEditor::new()`, `readline(prompt) -> Result<String, ReadlineError>`, `add_history_entry(line)`, `ReadlineError::{Eof,Interrupted}` variants) is identical across 13.x/14.x/15.x/16.x/17.x/18.x. Pinning 15 (released 2024-11-15, over a year of ecosystem adoption) matches the rand-0.8 / chrono-0.4 conservative-pin philosophy already used in this workspace for codegen-adjacent tooling. `with-file-history` feature is NOT enabled — disk history is explicitly T125c territory.

**Decision 3: REPL renders diagnostics via `Diagnostic::Display`, NOT `Diagnostic::render(&source)`.**
T124 added a rustc-style renderer that takes raw source text and emits carets. The REPL has no canonical source text — diagnostics from rustc / spawn failures carry `Span::dummy()`, and the user's interactive input doesn't map cleanly to a multi-line source buffer. `Display` emits `[Severity] message` + `note: ...` lines, span-agnostic, which is the right level of detail for a one-line REPL exchange. `buff check` keeps using `render(source)` because it operates on file-based input.

**Decision 4: CLI `commands/repl.rs` is a 10-line shim.**
The real logic lives in `crates/buff-repl/`. `commands/repl.rs` constructs `Repl::new()` and calls `.run()`, mapping `ReadlineError` to `anyhow::Error` via `e.to_string()`. This follows the existing `commands/clean.rs` / `commands/update.rs` thin-wrapper pattern (cargo-subprocess shims that fit in 30 lines). NO new logic, NO new errors, NO buff-eval direct dep at the CLI layer — `buff-lang-cli` depends on `buff-repl`, which transitively pulls `buff-eval`.

**Decision 5: stdout/stderr/value ordering in `format_eval_result`.**
Order: stdout (verbatim) → stderr (verbatim, when non-empty) → diagnostic OR `= value` line. The diagnostic is ALWAYS last so the user sees the structured error at the bottom of the output (where their eye lands after a failed run). The `= value` line is suppressed when `value.trim() == stdout.trim()` to avoid duplicating what `print(2+3)` already wrote. The runtime-panic case (partial stdout + stderr + diagnostic) preserves all three for debuggability — matches rustc's behavior of showing partial output before the panic message.

**Decision 6: Ctrl-C prints a leading newline, Ctrl-D does not.**
On `ReadlineError::Interrupted`, we write `\n` + `bye.` + `\n`. The leading `\n` ensures `bye.` starts on a fresh row (Ctrl-C typically leaves the cursor mid-line after `^C`). On `ReadlineError::Eof` (Ctrl-D), no leading `\n` — Ctrl-D typically leaves the cursor at end-of-line. Both write `bye.` so the user has visual confirmation the session ended cleanly (mirrors `python -i`'s `exit()` confirmation).

**Decision 7: NO `Default` impl on `Repl`.**
`Default::default()` can't return `Result`. Since `Repl::new()` is fallible (`DefaultEditor::new()` can fail without a TTY), we drop `Default` entirely. Tests that want a `Repl` without a TTY should call `evaluate_and_format(ev, input)` directly — that's the testability contract.


## [T125b] 2026-07-20T14:30:00-03:00 - REPL `:type` meta-command + state-persistence lock-in

**Decision 1: separate `dispatch_line` from `evaluate_and_format`.**
T125a's `evaluate_and_format` is the bare-eval-and-format path. T125b adds `dispatch_line` as the REPL's TRUE entry point — it routes meta-commands (`:type`) and forwards everything else. `evaluate_and_format` stays public for backward compat (T125a tests use it directly) and as the documented "non-meta-command" path. The interactive `Repl::run_with_writer` loop now calls `dispatch_line`, NOT `evaluate_and_format` — so behavior is identical between TTY and the test harness.

**Decision 2: `:type` is matched by literal prefix + whitespace-or-EOL.**
`dispatch_line` strips leading whitespace, then checks `strip_prefix(":type")` AND verifies what follows is either empty OR starts with `is_whitespace`. This rejects `:typex` (no separator) which would otherwise be misinterpreted as the meta-command. A constant `TYPE_CMD = ":type"` is the single source of truth for both the dispatcher check and the usage-hint string. `:foo` (unknown meta-command) is NOT intercepted — it falls through to the lexer, which surfaces a parse diagnostic. T125c will add the generic dispatcher.

**Decision 3: `:type` surfaces `Type::Display` verbatim — NO normalization.**
The task spec said `:type x` prints `Int`; reality is `Int<64>` (Buff's default Int width). Rather than strip the width annotation in the REPL, we surface `Type`'s Display form unchanged. Three reasons: (a) the width is real semantic information the user may want; (b) the REPL is a debug tool, hiding info makes it less useful; (c) any future `:type --verbose` or `:type --brief` mode can layer on top of the raw Display form. Tests use `contains("Int")` to stay robust to width-inference tuning.

**Decision 4: empty `:type` arg is a USAGE HINT, not a diagnostic.**
`:type` with no expression (or whitespace-only arg) prints `:type requires an expression, e.g. \`:_type x\` or \`:type 2 + 3\`` — a USAGE hint, NOT a `[Error]` diagnostic. The distinction matters: usage hints are user-error recovery (the REPL is teaching the user the syntax), diagnostics are pipeline failures (lex/parse/codegen/rustc/spawn). Mixing them would make the REPL feel hostile. The hint is intentionally NOT a `Diagnostic::info()` because it should not look like a compiler message.

**Decision 5: `type_of` returning `None` prints a one-line inference-failure message.**
`Evaluator::type_of` returns `None` on ANY lex/parse/inference failure (it deliberately does not surface a diagnostic — the buff-eval contract). The REPL formats this as `cannot infer type of \`<expr>\``. This mirrors the `python -c "type(x)"` UX: when the type can't be determined, the REPL tells you what it tried. The expression is echoed back in backticks so the user can copy it for re-editing. No `[Error]` tag because there's no Diagnostic object involved — this is REPL-side formatting of an Option<Type>.

**Decision 6: ALL `:type` unit tests live in src/lib.rs (not tests/).**
`type_of` is a pure lex+parse+infer pass — NO rustc spawn. So `:type` tests are FAST (<1ms each) and hermetic. Placing them in `src/lib.rs` next to the other formatting-layer unit tests keeps the test pyramid honest: fast unit tests for the formatting/dispatch layer, slower integration tests (with rustc spawn) for state-persistence and shadowing in `tests/repl_tests.rs`. Mirrors how buff-eval splits its tests.

**Decision 7: the dispatcher takes `&mut Evaluator` even though `:type` only needs `&`.**
`Evaluator::type_of` is `&self` (read-only). But `dispatch_line` is `&mut Evaluator` because the OTHER branch (`evaluate_and_format`) requires `&mut` to accumulate `let`/`func` state. The single dispatcher signature accommodates both paths without caller-side branching. Cost: a `&mut` borrow that's never exercised for the `:type` path. Benefit: ONE call site in `run_with_writer`, uniform behavior between TTY and tests.

## T125c
- Accepted T125c as-is on disk (57 tests green, clippy/fmt clean, panic-free). :load routes through eval_line accumulation; ~/.buff_history via dirs::home_dir() best-effort; multi-line via blank-line terminator + needs_continuation heuristic. No source change over subagent output -- only re-verified after correcting the smoke-test invocation.

## T126
- User chose 'build code + local tests only' because the registry needs external infra I cannot provision (Postgres, object storage, paid hosting, GitHub OAuth, live domain). Delivered the axum server + in-memory backend + full local integration tests; deploy/OAuth/live-domain/Postgres/S3/ops-runbook are USER ACTIONS.
- Did NOT add diesel/postgres (would need libpq on Windows) nor any [features] section (repo hard rule) -- so the in-memory backend is the only shipped Storage impl for now.

## T127 acceptance (2026-07-20)
- 4/5 acceptance items checked (login/add/publish/install — all proven by in-process integration tests).
- "Consumed package's functions callable" LEFT UNCHECKED: honest gap — CLI compiles one .buff file at a time (README modules note); cross-package/multi-file linking is the same v0.5 codegen-only gap, deferred post-v1.0. Not claimed as done.
- Registry base URL env: `BUFF_REGISTRY_URL` (default http://127.0.0.1:7878). Live remote deploy remains a USER ACTION.

## T128 (262b845)
- Treated [quick]-tagged T128 as a full feature (real tree render + registry round-trip tests, 13 integration tests) per orchestrator standard.
- uff deps renders only the 3 modern dep kinds (rust-deps, git-dependencies, registry-dependencies); legacy [dependencies] section omitted from output per spec scope.
- uff outdated targets registry-dependencies only (git/rust deps have no registry version concept). uff audit/security deferred to v2.0 as specified.

## T129a - zeromq dependency choice

- Chose `zeromq = "0.4"` (resolves 0.4.1), the PURE-RUST ZeroMQ impl (RouterSocket/PubSocket/RepSocket), NOT the C-linked `zmq`/`libzmq` crate. Rationale: this Windows host cannot reliably compile arbitrary C shims (same class of issue that forced hand-rolled lexer/parser and removal of chumsky). Pure-Rust zeromq builds cleanly with no libzmq. Also added `bytes` to workspace deps for ZmqMessage frame construction. Kernel socket I/O is behind a `ZmqTransport` trait so it is swappable + unit-testable via MockTransport.

- T129b (d40a3f7): Jupyter execution engine wires execute_request -> buff_eval::Evaluator::eval_line (persistent session, reused from T125). Text out via iopub stream/execute_result; errors as Jupyter error msg + kernel survives; execution_count increments. buff-repl NOT consumed (avoids rustyline). Live nbconvert acceptance = USER ACTION (no Jupyter on build host).

- T129c (3045659): Chose HTML <table> for matrices/vectors over GPU→PNG image rendering — no raster renderer exists in buff-lang-runtime today; image/png deferred. MIME bundle ALWAYS pairs text/html with text/plain fallback per Jupyter display_data spec.
- T130 (b841512): Chose caret pin `dioxus = "0.7"` over exact `=0.7.2` for production workspace dep. T121b spike used exact pin for reproducibility; production uses caret and re-tests on minor bumps per decision doc. T121b PASS verdict (commit 6b2235f) is the precondition precedent; live browser render classified USER ACTION under build-code+local-tests-only mode (identical to T129a-c nbconvert precedent).
- T131 (98c7a63): Chose axum 0.8 over warp/hyper for dev server because (a) already workspace-pinned from T126 buff-registry, (b) `ws` feature gives WebSocket for free, (c) ServiceExt::oneshot test pattern already proven in buff-registry integration tests. Chose notify 6.x major-pin over 7.x/8.x to match conservative-pin philosophy and avoid MSRV bumps for the pinned 1.95.0 toolchain. Chose in-tree 200ms debounce over notify-debouncer-mini to minimize dep surface. LIVE RELOAD (not true HMR) is the v1.8 scope decision — true HMR deferred to v1.9+.
- T132 (59116d0+49f7bae): Chose to keep tauri as a TEMPLATE-ONLY dep (not a buff-lang-cli runtime dep) to keep the CLI lightweight. Trade-off: `buff ui build --desktop` cannot build natively without `cargo install tauri-cli` (USER ACTION) — acceptable under build-code+local-tests-only mode. Alternative considered: bundling tauri-cli as a workspace dep — rejected (heavy dep tree, platform-specific native SDKs, violates the CLI's pure-Rust-no-native-deps posture). The .gitignore `-f` fix (49f7bae) is a one-time scaffold-shipping correction, not a recurring decision.
