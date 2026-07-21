// T19 example: template with loop and conditional.
//
// Demonstrates handlebars `{% for %}` and `{% if %}` syntax.

use buff_template::Template;

fn main() {
    // Template with a loop
    let t = Template::from_string(
        "{% for item in items %}{{item}} {% endfor %}",
    )
    .expect("compile loop template");
    let out = t
        .render(r#"{"items": ["a", "b", "c"]}"#)
        .expect("render loop");
    println!("loop result: '{out}'");
    assert_eq!(out, "a b c ");

    // Template with a conditional
    let t2 = Template::from_string(
        "{% if ok %}yes{% else %}no{% endif %}",
    )
    .expect("compile conditional template");
    let out_true = t2.render(r#"{"ok": true}"#).expect("render true");
    println!("conditional true: '{out_true}'");
    assert_eq!(out_true, "yes");

    let out_false = t2.render(r#"{"ok": false}"#).expect("render false");
    println!("conditional false: '{out_false}'");
    assert_eq!(out_false, "no");
}
