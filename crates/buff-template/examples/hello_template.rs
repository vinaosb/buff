// T19 example: compile a template from string, render with context.
//
// Demonstrates the basic `Template::from_string` + `template.render`
// pipeline. Uses handlebars `{{ variable }}` syntax.

use buff_template::Template;

fn main() {
    let t = Template::from_string("Hello {{name}}!").expect("compile template");
    let out = t.render(r#"{"name": "Buff"}"#).expect("render with name");
    println!("{out}");
    assert_eq!(out, "Hello Buff!");
}
