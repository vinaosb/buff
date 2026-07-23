// T32 example: subcommands (greet + count).
//
// Demonstrates `App.command(name, about)` returning a child App whose
// own builder calls configure the subcommand. The parsed result's
// `subcommand()` and `subcommand_args()` are then dispatched.

use buff_cli::App;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let app = App::new("multi".to_string()).about("Subcommand demo".to_string());

    let greet = app.command("greet".to_string(), "Say hello to NAME".to_string());
    greet.option(
        "name".to_string(),
        "n".to_string(),
        "Who to greet".to_string(),
    );

    let count = app.command("count".to_string(), "Count to N".to_string());
    count.arg("n".to_string(), "How high to count".to_string());

    let parsed = match app.parse(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    match parsed.subcommand().as_deref() {
        Some("greet") => {
            let sub = parsed.subcommand_args();
            let name = sub.option("name").unwrap_or_else(|| "world".to_string());
            println!("Hello, {name}!");
        }
        Some("count") => {
            let sub = parsed.subcommand_args();
            let n: u64 = sub.arg("n").and_then(|s| s.parse().ok()).unwrap_or(3);
            for i in 1..=n {
                println!("{i}");
            }
        }
        other => {
            eprintln!("unknown or missing subcommand: {other:?}");
            eprintln!("try: multi --help");
            std::process::exit(1);
        }
    }
}
