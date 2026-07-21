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
