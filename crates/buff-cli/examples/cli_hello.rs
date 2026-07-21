// T32 example: minimal hello-world CLI.
//
// Demonstrates the smallest possible buff-cli app: an App with a
// version flag, a --name option, and one positional arg. Parses the
// arguments and prints a greeting.

use buff_cli::App;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let app = App::new("hello".to_string())
        .version("0.1.0".to_string())
        .about("Says hello to a name".to_string())
        .option(
            "name".to_string(),
            "n".to_string(),
            "Name to greet (default: world)".to_string(),
        )
        .arg(
            "greeting".to_string(),
            "Optional greeting override".to_string(),
        );

    let parsed = match app.parse(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let name = parsed
        .option("name")
        .unwrap_or_else(|| "world".to_string());
    let greeting = parsed
        .arg("greeting")
        .unwrap_or_else(|| "Hello".to_string());
    println!("{greeting}, {name}!");
}
