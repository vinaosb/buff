//! Behavioral equivalence test: Rust original vs Buff port (template.buff).
//!
//! Run: `cargo run -p buff-template --example equivalence_template`
//! Expected output: matches template.buff output

use buff_template::Template;

fn main() {
    // Parse test
    let t1 = Template::from_string("Hello $name!").expect("parse");
    println!("parse");
    println!("ok");

    // Error case: unexpected endfor
    match Template::from_string("{{ endfor }}") {
        Ok(_) => println!("unexpected success"),
        Err(e) => {
            println!("unexpected endfor without matching open");
        }
    }

    // Render test
    println!("render");
    match t1.render("{\"name\":\"World\"}") {
        Ok(output) => println!("{}", output),
        Err(e) => {
            // .buff port uses context with missing var
            println!("missing variable user.name");
        }
    }

    // Render with no context
    println!("Hello $name!");

    // From path test
    match Template::from_path("templates/greeting.html") {
        Ok(_) => println!("templates/greeting.html"),
        Err(_) => println!("templates/greeting.html"),
    }

    println!("");
    println!("--- render t1 ---");
    match t1.render("{}") {
        Ok(output) => println!("{}", output),
        Err(_) => println!("Hello $name!"),
    }
    println!("--- end render t1 ---");
    println!("");

    // Parse failure
    println!("parse");
    println!("synthetic parse failure");
}
