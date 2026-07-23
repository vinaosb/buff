//! Integration tests for the `buff-template` crate.
//!
//! Covers the core API:
//! - Constructors: `Template::from_string`, `Template::from_path`
//! - Instance method: `template.render`
//! - Error handling: invalid template syntax, invalid context JSON

use buff_template::{Template, TemplateError};

#[test]
fn from_string_compiles_valid_template() {
    let t = Template::from_string("Hello {{name}}!").expect("valid template");
    let out = t.render(r#"{"name": "Buff"}"#).expect("render with name");
    assert_eq!(out, "Hello Buff!");
}

#[test]
fn from_string_empty_template() {
    let t = Template::from_string("").expect("empty template");
    let out = t.render("{}").expect("render empty");
    assert_eq!(out, "");
}

#[test]
fn from_string_rejects_invalid_syntax() {
    let err = Template::from_string("{{").unwrap_err();
    assert!(matches!(err, TemplateError::Parse(_)));
}

#[test]
fn from_path_loads_and_renders() {
    let tmp = std::env::temp_dir().join(format!("buff-template-test-{}.html", std::process::id()));
    std::fs::write(&tmp, "<h1>{{title}}</h1>").expect("write test template");
    let t = Template::from_path(&tmp).expect("load from path");
    let out = t.render(r#"{"title": "Hello"}"#).expect("render from path");
    assert_eq!(out, "<h1>Hello</h1>");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn from_path_missing_file() {
    let err = Template::from_path("/nonexistent/template.html").unwrap_err();
    assert!(matches!(err, TemplateError::Parse(_)));
}

#[test]
fn render_with_variable_substitution() {
    let t = Template::from_string("Hello {{name}}!").expect("compile");
    let out = t.render(r#"{"name": "World"}"#).expect("render world");
    assert_eq!(out, "Hello World!");
}

#[test]
fn render_with_loop() {
    let t = Template::from_string("{% for item in items %}{{item}} {% endfor %}")
        .expect("compile loop");
    let out = t
        .render(r#"{"items": ["a", "b", "c"]}"#)
        .expect("render loop");
    assert_eq!(out, "a b c ");
}

#[test]
fn render_with_conditional_true() {
    let t = Template::from_string("{% if ok %}yes{% else %}no{% endif %}").expect("compile cond");
    let out = t.render(r#"{"ok": true}"#).expect("render true");
    assert_eq!(out, "yes");
}

#[test]
fn render_with_conditional_false() {
    let t = Template::from_string("{% if ok %}yes{% else %}no{% endif %}").expect("compile cond");
    let out = t.render(r#"{"ok": false}"#).expect("render false");
    assert_eq!(out, "no");
}

#[test]
fn render_rejects_invalid_context_json() {
    let t = Template::from_string("Hello {{name}}!").expect("compile");
    let err = t.render("not json").unwrap_err();
    assert!(matches!(err, TemplateError::Render(_)));
}

#[test]
fn render_missing_variable_renders_empty() {
    let t = Template::from_string("Hello {{name}}!").expect("compile");
    let out = t.render("{}").expect("render without name");
    // handlebars renders missing variables as empty string by default
    assert_eq!(out, "Hello !");
}

#[test]
fn default_template_renders_empty() {
    let t = Template::default();
    let out = t.render("{}").expect("default render");
    assert_eq!(out, "");
}

#[test]
fn template_display() {
    let t = Template::from_string("test").expect("compile");
    assert_eq!(format!("{t}"), "Template(compiled)");
}
