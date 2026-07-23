//! Render-focused integration tests for `buff-template`.
//!
//! Covers handlebars template syntax: variable substitution, loops,
//! conditionals, nested objects, and edge cases.

use buff_template::Template;

#[test]
fn render_nested_object() {
    let t = Template::from_string("{{user.name}} is {{user.age}}").expect("compile");
    let out = t
        .render(r#"{"user": {"name": "Alice", "age": 30}}"#)
        .expect("render nested");
    assert_eq!(out, "Alice is 30");
}

#[test]
fn render_html_escaping() {
    let t = Template::from_string("{{content}}").expect("compile");
    let out = t
        .render(r#"{"content": "<script>alert('xss')</script>"}"#)
        .expect("render html");
    // handlebars auto-escapes HTML by default
    assert_eq!(out, "&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;");
}

#[test]
fn render_with_comment() {
    let t = Template::from_string("before{{! this is a comment }}after").expect("compile");
    let out = t.render("{}").expect("render comment");
    assert_eq!(out, "beforeafter");
}

#[test]
fn render_with_helpers() {
    let t = Template::from_string("{{#if show}}visible{{/if}}").expect("compile");
    let out = t.render(r#"{"show": true}"#).expect("render if true");
    assert_eq!(out, "visible");
    let out2 = t.render(r#"{"show": false}"#).expect("render if false");
    assert_eq!(out2, "");
}

#[test]
fn render_with_each_else() {
    let t = Template::from_string("{{#each items}}{{this}}{{else}}empty{{/each}}")
        .expect("compile each-else");
    let out = t
        .render(r#"{"items": ["a", "b"]}"#)
        .expect("render with items");
    assert_eq!(out, "ab");
    let out2 = t.render(r#"{"items": []}"#).expect("render empty items");
    assert_eq!(out2, "empty");
}

#[test]
fn render_trim_whitespace() {
    let t = Template::from_string("  {{~name~}}  ").expect("compile");
    let out = t.render(r#"{"name": "trimmed"}"#).expect("render trim");
    assert_eq!(out, "trimmed");
}

#[test]
fn render_multiple_variables() {
    let t = Template::from_string("{{a}} {{b}} {{c}}").expect("compile");
    let out = t
        .render(r#"{"a": "x", "b": "y", "c": "z"}"#)
        .expect("render multi");
    assert_eq!(out, "x y z");
}

#[test]
fn render_boolean_context() {
    let t = Template::from_string("{{#if flag}}yes{{/if}}").expect("compile");
    assert_eq!(t.render(r#"{"flag": true}"#).expect("true"), "yes");
    assert_eq!(t.render(r#"{"flag": false}"#).expect("false"), "");
}
