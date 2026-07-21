# buff-lang-codegen-buffhtml

Lowers `RsxTemplateFile` AST → Rust source containing `rsx!{}` macro invocations (Dioxus 0.7). Reuses the T121b-proven syn/quote/prettyplease emission pattern from `buff-lang-codegen-rust`.

## STRUCTURE

```
src/
├── lib.rs         # 961 lines — entry (generate), file assembly, AST→TokenStream lowering
├── prop_check.rs  # 847 lines — T134 prop pre-checker: extract_interface, check_props, type-matching
├── span_map.rs    # 233 lines — post-format SpanMap side-table (binary-search lookup)
└── error.rs       # 9 lines — BuffHtmlCodegenError (UnsupportedConstruct variant)

tests/
└── codegen_tests.rs   # snapshot tests (insta)
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Lower a new RsxNode variant to Rust | `lib.rs` — add match arm in `lower_node()` |
| Lower a new attribute kind | `lib.rs` — add match arm in `lower_attr()` |
| Add component lifecycle hook | `prop_check.rs` + `lib.rs::build_file_items` (body_stmts insertion) |
| Tune span mapping accuracy | `span_map.rs` — anchor selection + finalize scan |
| Add prop validation rule | `prop_check.rs` — `check_component_invocation` or `type_mismatch` |
| SSR lowering (`render_to_string`) | `lib.rs` — T135 `dioxus-ssr` path (separate entry from `generate`) |

## CONVENTIONS (this crate only)

- **HARD RULE: every Rust construct via `syn` types.** Same as `buff-lang-codegen-rust`. Single string producer is `prettyplease::unparse`. NO raw-string codegen.
- **`quote!` + `parse2` OK** — `syn::parse_str::<syn::Expr>(expr_src)` is used for verbatim expression emission. On parse failure, falls back to raw token emission (rustc surfaces the error; SpanMap maps it back).
- **Entry point**: `generate(template, component_name) -> Result<CodegenResult, BuffHtmlCodegenError>`. Returns both formatted Rust source and a `SpanMap`.
- **Script-block integration (T134)**: when `props="TypeName"` is present on `<script>`, codegen parses the script as a Rust block, hoists top-level items (struct, use) to module scope, splices body statements into the component fn, and inserts `let <Type> { fields, .. } = props;` destructure first.
- **No-props fallback (T133 floor)**: script source preserved as `const __BUFF_SCRIPT_SOURCE: &str = "..."` for CLI downstream pass.
- **Deterministic output**: same AST → byte-identical Rust. Snapshot tests enforce this.
- **Tests**: `tests/codegen_tests.rs` with insta snapshots in `tests/snapshots/`.

## SPAN MAP (post-format side-table)

prettyplease loses syn span info during formatting. The side-table recovers it:

1. During lowering, `SpanMapBuilder::add_anchor(text, buffhtml_span)` records stable substrings (identifiers, literals).
2. After `prettyplease::unparse`, `SpanMapBuilder::finalize(rs_source)` scans the formatted text line-by-line for each anchor, building a sorted `Vec<(RsLineCol, Span)>`.
3. At diagnostic time, `SpanMap::map_span(line, col)` binary-searches for the nearest preceding anchor.

Evidence: `.sisyphus/evidence/task-133-span-mapping-spike.txt`.

## PROP PRE-CHECKER (T134)

Runs after parse + codegen, before rustc. For each `<Component prop: value>` in a parent template:

- **`extract_interface(template, tag_name)`** — parses the child's script for the declared Props struct.
- **`check_props(parent, registry)`** — walks the parent AST, validates every component invocation: missing required props, unknown props, literal-type mismatch.
- **`PropInterfaceRegistry`** — caller builds this by walking all `.buffhtml` files in scope.
- Components without a `props="..."` attribute are skipped (backward-compat with T133 floor).
- `{...spread}` bypasses all checks (static analysis can't see inside).
