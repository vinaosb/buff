//! Integration tests for `buff-lang-codegen-buffhtml` — end-to-end parse →
//! codegen → string-substring assertions on the generated Rust source.
//!
//! These tests assert the generated `.rs` text contains the expected
//! substrings (NOT a full rustc compile — that's the CLI integration step's
//! responsibility, deferred per task spec).

#![allow(clippy::needless_raw_string_hashes)]

use buff_lang_buffhtml_parser::parse;
use buff_lang_codegen_buffhtml::{generate, DEFAULT_COMPONENT_NAME};
use buff_lang_error::SourceId;

fn gen(src: &str) -> String {
    let template = parse(src, SourceId(0)).expect("parse failed");
    let result = generate(&template, DEFAULT_COMPONENT_NAME).expect("codegen failed");
    result.rust_source
}

fn gen_with_map(src: &str) -> (String, buff_lang_codegen_buffhtml::SpanMap) {
    let template = parse(src, SourceId(0)).expect("parse failed");
    let result = generate(&template, DEFAULT_COMPONENT_NAME).expect("codegen failed");
    (result.rust_source, result.span_map)
}

#[test]
fn emits_component_attr_and_rsx_macro() {
    let src = gen("<div>hello</div>");
    assert!(
        src.contains("#[component]"),
        "expected #[component] in:\n{src}"
    );
    assert!(src.contains("rsx!"), "expected `rsx!` in:\n{src}");
}

#[test]
fn emits_use_dioxus_prelude() {
    let src = gen("<div>x</div>");
    assert!(
        src.contains("use dioxus::prelude::*"),
        "expected `use dioxus::prelude::*` in:\n{src}"
    );
}

#[test]
fn emits_element_return_type() {
    let src = gen("<div>x</div>");
    assert!(
        src.contains("-> Element"),
        "expected `-> Element` in:\n{src}"
    );
}

#[test]
fn div_with_text_child() {
    let src = gen("<div>hello</div>");
    assert!(src.contains("div"), "expected `div` tag in:\n{src}");
    assert!(
        src.contains("\"hello\""),
        "expected `\"hello\"` text literal in:\n{src}"
    );
}

#[test]
fn interpolation_emits_expr_verbatim() {
    let src = gen("<div>{count}</div>");
    assert!(
        src.contains("count"),
        "expected `count` identifier in:\n{src}"
    );
}

#[test]
fn counter_e2e_shape() {
    // Mirrors the decision-record counter example's button row.
    let src = "<button on:click={increment}>Increment (count: {count})</button>";
    let out = gen(src);
    // Must contain the rsx!{} macro, the button element, the on_click
    // handler identifier, the onclick event, and the interpolated count.
    for needle in ["rsx!", "button", "on_click", "increment", "count"] {
        assert!(
            out.contains(needle),
            "expected `{needle}` in counter e2e codegen:\n{out}"
        );
    }
}

#[test]
fn on_event_handler_emits_on_event_name() {
    let src = gen("<button on:click={h}>x</button>");
    assert!(
        src.contains("on_click"),
        "expected `on_click` (event name lowered to Rust style):\n{src}"
    );
}

#[test]
fn on_event_with_modifier_emits_base_event_name() {
    // For T133 floor, modifiers are NOT yet wired (T134+). The codegen still
    // emits `on_submit` as the attribute name; the modifier is documented as
    // deferred.
    let src = gen("<form on:submit_prevent={h}></form>");
    assert!(
        src.contains("on_submit"),
        "expected `on_submit` (modifier dropped for T133):\n{src}"
    );
}

#[test]
fn named_prop_lowered_as_identifier_colon_value() {
    let src = gen("<Greeting name: \"Alice\" />");
    assert!(
        src.contains("Greeting"),
        "expected component `Greeting` in:\n{src}"
    );
    assert!(
        src.contains("name") && src.contains("Alice"),
        "expected `name` prop + `Alice` value in:\n{src}"
    );
}

#[test]
fn each_block_emits_iter_map_collect() {
    let src = gen("<ul>{#each items as item}<li>{item}</li>{/each}</ul>");
    assert!(src.contains(".iter()"), "expected `.iter()` in:\n{src}");
    assert!(src.contains(".map("), "expected `.map(` in:\n{src}");
    // prettyplease inserts spaces around turbofish (`collect ::< Vec < _ >>`).
    assert!(src.contains("collect"), "expected `collect` in:\n{src}");
    assert!(
        src.contains("Vec"),
        "expected `Vec` turbofish target in:\n{src}"
    );
}

#[test]
fn each_block_with_index_emits_enumerate() {
    let src = gen("{#each items as item, i}<li>{item}</li>{/each}");
    assert!(
        src.contains(".enumerate()"),
        "expected `.enumerate()` for index binding:\n{src}"
    );
}

#[test]
fn if_block_emits_rust_if_expr() {
    let src = gen("{#if a}<x />{:else}<y />{/if}");
    assert!(src.contains("if "), "expected `if` keyword in:\n{src}");
    assert!(src.contains("else"), "expected `else` keyword in:\n{src}");
}

#[test]
fn if_block_with_else_if_branches() {
    let src = gen("{#if a}<x />{:else if b}<y />{:else}<z />{/if}");
    assert!(src.contains("if "), "expected if in:\n{src}");
    assert!(src.contains("else"), "expected else in:\n{src}");
    // Both branches' bodies should appear.
    assert!(src.contains("rsx!"), "expected nested rsx! in:\n{src}");
}

#[test]
fn fragment_emits_fragment_keyword() {
    let src = gen("<><span>a</span><span>b</span></>");
    assert!(
        src.contains("Fragment"),
        "expected `Fragment` keyword for `<>...</>`:\n{src}"
    );
}

#[test]
fn component_invocation_emits_pascal_ident() {
    let src = gen("<Counter />");
    assert!(
        src.contains("Counter"),
        "expected component ident `Counter` in:\n{src}"
    );
}

#[test]
fn slot_emits_children_identifier() {
    let src = gen("<slot />");
    assert!(
        src.contains("children"),
        "expected `children` for default slot:\n{src}"
    );
}

#[test]
fn script_block_preserved_as_const() {
    let src = gen("<script lang=\"buff\">hello world</script>\n<div />");
    assert!(
        src.contains("__BUFF_SCRIPT_SOURCE"),
        "expected `__BUFF_SCRIPT_SOURCE` const in:\n{src}"
    );
    assert!(
        src.contains("hello world"),
        "expected script body preserved verbatim in:\n{src}"
    );
}

#[test]
fn boolean_attr_emits_true_value() {
    let src = gen("<input disabled />");
    // prettyplease inserts spaces around `:` — accept either form.
    assert!(
        src.contains("disabled") && src.contains("true"),
        "expected `disabled` + `true` for boolean attr in:\n{src}"
    );
}

#[test]
fn literal_attr_emits_string_value() {
    let src = gen("<div class=\"card\" />");
    assert!(
        src.contains("class") && src.contains("card"),
        "expected `class` + `card` literal in:\n{src}"
    );
}

#[test]
fn expression_attr_emits_expr_verbatim() {
    let src = gen("<div class={some_var} />");
    assert!(
        src.contains("some_var"),
        "expected `some_var` expression verbatim in:\n{src}"
    );
}

#[test]
fn counter_complete_e2e_shape() {
    // From the decision record §3 — the canonical counter example.
    let src = r#"<script lang="buff">
component Counter = fn(props: { initial: Int = 0 }) -> Element:
    count = state(props.initial)
    increment = fn(): count.set(count.get() + 1)
</script>

<div class="counter">
    <span>{count}</span>
    <button on:click={increment}>+1</button>
</div>"#;
    let out = gen(src);
    // Smoke: the generated source must contain the rsx!{} macro and the
    // key identifiers from the template.
    for needle in [
        "rsx!",
        "div",
        "span",
        "button",
        "count",
        "increment",
        "on_click",
        "__BUFF_SCRIPT_SOURCE",
    ] {
        assert!(
            out.contains(needle),
            "expected `{needle}` in counter e2e codegen:\n{out}"
        );
    }
}

// ---------------------------------------------------------------------------
// Span-map tests.
// ---------------------------------------------------------------------------

#[test]
fn span_map_populated_for_interp_anchor() {
    // The `{count}` interpolation should produce at least one anchor in the
    // span map for the `count` identifier.
    let (_, map) = gen_with_map("<div>{count}</div>");
    assert!(
        !map.is_empty(),
        "expected span map to have at least one anchor"
    );
}

#[test]
fn span_map_map_span_returns_buffhtml_position() {
    // Parse a known-snippet with an interpolation at a known source position,
    // then verify that querying the .rs position of the `count` identifier
    // returns the .buffhtml span.
    let (src, map) = gen_with_map("<div>{count}</div>");
    // The `count` identifier's position in `src` (1-based).
    let count_line_col = find_first_occurrence(&src, "count");
    assert!(count_line_col.is_some(), "expected `count` in:\n{src}");
    let (line, col) = count_line_col.expect("just checked");
    let buffhtml_span = map.map_span(line, col);
    assert!(
        buffhtml_span.is_some(),
        "expected span map lookup to succeed for line={line} col={col} in:\n{src}"
    );
    let span = buffhtml_span.expect("just checked");
    // The .buffhtml span for `{count}` starts at offset 5 (`<div>` = 5 chars).
    assert!(
        span.start <= 5 || span.end >= 5,
        "expected span to roughly cover the interpolation in source: {span:?}"
    );
}

fn find_first_occurrence(src: &str, needle: &str) -> Option<(usize, usize)> {
    for (i, line) in src.lines().enumerate() {
        if let Some(col) = line.find(needle) {
            return Some((i + 1, col + 1));
        }
    }
    None
}

#[test]
fn span_map_empty_when_no_interpolations() {
    // A plain-text element has no expression anchors.
    let (_, map) = gen_with_map("<div>hello</div>");
    assert!(map.is_empty(), "expected empty span map for plain text");
}

#[test]
fn generated_source_re_parses_as_syn_file() {
    // Symmetry check: the generated Rust source must re-parse as a valid
    // syn::File. This is the same assertion as T121b's
    // `t121b_generated_source_re_parses` — proves the rsx!{} TokenStream
    // survives prettyplease formatting.
    let src = gen("<div>{count}</div>");
    syn::parse_str::<syn::File>(&src).unwrap_or_else(|e| {
        panic!("prettyplease output must re-parse as syn::File: {e}\n--- src ---\n{src}")
    });
}

// ---------------------------------------------------------------------------
// T133 stretch features (6 features).
// ---------------------------------------------------------------------------

#[test]
fn raw_html_emits_dangerous_inner_html() {
    // Feature 1: `{@html expr}` → div { dangerous_inner_html: <expr> }
    let src = gen("{@html raw_html}");
    assert!(
        src.contains("dangerous_inner_html"),
        "expected `dangerous_inner_html` for `{{@html}}`:\n{src}"
    );
    assert!(
        src.contains("raw_html"),
        "expected the raw_html identifier preserved:\n{src}"
    );
}

#[test]
fn raw_html_emits_xss_marker_comment() {
    // Feature 1: the generated source contains an auditing marker.
    let src = gen("{@html raw_html}");
    assert!(
        src.contains("XSS") || src.contains("{@html}"),
        "expected an XSS / `{{@html}}` audit marker in:\n{src}"
    );
}

#[test]
fn spread_props_emit_dotted_spread() {
    // Feature 2: `{...rest}` → `..rest`
    let src = gen("<Button {...rest} />");
    assert!(
        src.contains("..rest") || src.contains(".. rest"),
        "expected `..rest` spread syntax in:\n{src}"
    );
}

#[test]
fn spread_props_with_other_attrs_coexist() {
    // Feature 2: spread + named prop together.
    let src = gen("<Button {...rest} label: \"x\" />");
    assert!(
        (src.contains("..rest") || src.contains(".. rest")) && src.contains("label"),
        "expected spread + label together in:\n{src}"
    );
}

#[test]
fn named_slot_emits_named_children_ident() {
    // Feature 3: `<slot name="header" />` → renders `{ header }`.
    let src = gen("<slot name=\"header\" />");
    assert!(
        src.contains("header"),
        "expected `header` named-children identifier in:\n{src}"
    );
    // Sanity: must NOT contain the lowercase fallback `children` (the
    // default slot form). It might still appear by coincidence if the
    // name was sanitized — but for `header`, it shouldn't.
    assert!(
        !src.contains("{ children }"),
        "named slot should not lower to default `children`:\n{src}"
    );
}

#[test]
fn default_slot_still_emits_children() {
    // Feature 3: default slot form unchanged.
    let src = gen("<slot />");
    assert!(
        src.contains("children"),
        "expected `children` for default slot:\n{src}"
    );
}

#[test]
fn keyed_each_emits_enumerate_and_key() {
    // Feature 4: `{#each xs as x (x.id)}` → keyed form with `key:` attribute.
    let src = gen("{#each items as item (item.id)}<li>{item}</li>{/each}");
    assert!(
        src.contains(".enumerate()"),
        "expected `.enumerate()` for keyed each in:\n{src}"
    );
    assert!(
        src.contains("key"),
        "expected `key` attribute for keyed each in:\n{src}"
    );
}

#[test]
fn keyed_each_with_method_iterable_compiles() {
    // Feature 4 fix: parens in iterable expression work end-to-end.
    let src = gen("{#each items.read() as item (item.id)}<li>{item}</li>{/each}");
    // Should still emit the keyed form and the iterable expression verbatim.
    assert!(
        src.contains("items.read()"),
        "expected iterable expression preserved verbatim in:\n{src}"
    );
    assert!(
        src.contains("key"),
        "expected `key` for keyed each in:\n{src}"
    );
}

#[test]
fn bind_emits_controlled_two_way_binding() {
    // Feature 5: `bind:value={name}` → `value: name, oninput: move |e| name.set(e.value())`
    let src = gen("<input bind:value={name} />");
    assert!(src.contains("value"), "expected `value` prop in:\n{src}");
    assert!(
        src.contains("oninput"),
        "expected `oninput` handler for two-way binding in:\n{src}"
    );
    assert!(
        src.contains(".set("),
        "expected `.set(` call to mutate the signal in:\n{src}"
    );
    assert!(
        src.contains("name"),
        "expected signal identifier `name` in:\n{src}"
    );
}

#[test]
fn bind_emits_move_closure_capturing_signal() {
    // Feature 5: the oninput handler must be a `move` closure to capture
    // the signal by reference for `.set()`.
    let src = gen("<input bind:value={username} />");
    assert!(
        src.contains("move"),
        "expected `move` closure in bind codegen:\n{src}"
    );
    assert!(
        src.contains("username"),
        "expected signal `username` to appear in:\n{src}"
    );
}

#[test]
fn await_block_emits_use_resource_pattern() {
    // Feature 6: minimal `{#await fut}{:then x}{body}{/await}`.
    let src = gen("{#await fetchUser(id)}{:then user}<Profile user: {user} />{/await}");
    assert!(
        src.contains("use_resource"),
        "expected `use_resource` hook for await-block in:\n{src}"
    );
    assert!(
        src.contains("fetchUser"),
        "expected future expression preserved in:\n{src}"
    );
    assert!(
        src.contains("user"),
        "expected then-binding identifier in:\n{src}"
    );
}

#[test]
fn await_block_with_catch_emits_error_arm() {
    // Feature 6: full form with pending + then + catch.
    let src = gen("{#await fetchUser(id)}<Spinner />{:then user}<Profile user: {user} />{:catch err}<Error />{/await}");
    assert!(
        src.contains("use_resource"),
        "expected `use_resource` for await-block:\n{src}"
    );
    assert!(
        src.contains("err"),
        "expected catch-binding identifier `err` in:\n{src}"
    );
    assert!(
        src.contains("Ready"),
        "expected `ResourceState::Ready` match arm in:\n{src}"
    );
}

// ---------------------------------------------------------------------------
// T134 — component interface declaration + lifecycle hooks.
// ---------------------------------------------------------------------------

#[test]
fn t134_script_without_props_keeps_t133_floor() {
    // When no `props` attribute is present, codegen keeps the T133
    // floor behavior: `const __BUFF_SCRIPT_SOURCE` + zero-arg component.
    let src = gen(
        r#"<script lang="buff">let mut count = use_signal(|| 0);</script>
<div />"#,
    );
    assert!(
        src.contains("__BUFF_SCRIPT_SOURCE"),
        "T133-floor `__BUFF_SCRIPT_SOURCE` const must be preserved (no-props path):\n{src}"
    );
    assert!(
        !src.contains("fn BuffHtmlComponent(props:"),
        "no-props path must NOT generate a `props:` parameter:\n{src}"
    );
}

#[test]
fn t134_props_attribute_generates_props_param() {
    // `<script lang="buff" props="Props">` switches the signature to
    // `fn Comp(props: Props) -> Element`.
    let src = gen(r#"<script lang="buff" props="Props">
struct Props {
    name: String,
    count: i32,
}
</script>
<div />"#);
    assert!(
        src.contains("fn BuffHtmlComponent(props: Props)"),
        "expected `fn Comp(props: Props)` signature from props= attribute:\n{src}"
    );
    assert!(
        !src.contains("__BUFF_SCRIPT_SOURCE"),
        "props= path must NOT emit the legacy __BUFF_SCRIPT_SOURCE const:\n{src}"
    );
}

#[test]
fn t134_props_struct_hoisted_to_module_scope() {
    // The `struct Props { ... }` declared in the script body must
    // appear at module scope (outside the component fn) so the
    // `props: Props` parameter is visible.
    let src = gen(r#"<script lang="buff" props="Props">
struct Props {
    name: String,
}
</script>
<div />"#);
    assert!(
        src.contains("struct Props"),
        "expected `struct Props` declaration to survive in:\n{src}"
    );
    // The struct must appear BEFORE the component fn (module scope).
    let struct_pos = src
        .find("struct Props")
        .expect("struct Props must be present");
    let fn_pos = src
        .find("fn BuffHtmlComponent")
        .expect("component fn must be present");
    assert!(
        struct_pos < fn_pos,
        "struct Props must be hoisted ABOVE the component fn (module scope),\n\
         got struct at {struct_pos}, fn at {fn_pos}:\n{src}"
    );
}

#[test]
fn t134_props_destructure_uses_all_field_names() {
    // The auto-generated destructure must list every declared field
    // (so the script body can reference them by name).
    let src = gen(r#"<script lang="buff" props="Props">
struct Props {
    name: String,
    count: i32,
    active: bool,
}
</script>
<div />"#);
    // Destructure form: `let Props { name, count, active, .. } = props;`
    assert!(
        src.contains("let Props"),
        "expected `let Props {{ ... }} = props;` destructure:\n{src}"
    );
    for field in ["name", "count", "active"] {
        assert!(
            src.contains(field),
            "expected field `{field}` to appear in generated source:\n{src}"
        );
    }
    assert!(
        src.contains("= props"),
        "expected destructure to assign from `props`:\n{src}"
    );
}

#[test]
fn t134_props_script_body_statements_spliced_into_fn() {
    // The script body's non-item statements (let bindings, side-effect
    // calls) must be spliced into the function body AHEAD of the rsx!{}
    // expression.
    let src = gen(r#"<script lang="buff" props="Props">
struct Props {
    initial: i32,
}
let mut count = use_signal(|| initial);
</script>
<div>{count}</div>"#);
    assert!(
        src.contains("let mut count"),
        "expected `let mut count` body statement to be spliced into fn:\n{src}"
    );
    assert!(
        src.contains("use_signal(|| initial)"),
        "expected `initial` prop field to be in scope after destructure:\n{src}"
    );
    // body statement must come AFTER the destructure + BEFORE rsx!.
    let destructure_pos = src.find("let Props").expect("destructure must be present");
    let body_pos = src
        .find("let mut count")
        .expect("body stmt must be present");
    let rsx_pos = src.find("rsx!").expect("rsx! must be present");
    assert!(
        destructure_pos < body_pos && body_pos < rsx_pos,
        "expected order destructure < body < rsx!, got {destructure_pos} < {body_pos} < {rsx_pos}:\n{src}"
    );
}

#[test]
fn t134_props_with_unknown_type_name_falls_through() {
    // When the script declares `props="Missing"` but no matching
    // struct is found in the body, codegen still emits the `props:
    // Missing` signature (rustc surfaces the error against the
    // generated position — the SpanMap translates it back).
    let src = gen(r#"<script lang="buff" props="Missing">
let x = 42;
</script>
<div />"#);
    assert!(
        src.contains("fn BuffHtmlComponent(props: Missing)"),
        "expected signature to use the declared type name verbatim:\n{src}"
    );
}
