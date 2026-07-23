// T32 example: flags + options + positionals.
//
// Demonstrates a moderately complex CLI: boolean flags, value
// options with short forms, and positional args. Mirrors typical
// tools like `ls` (with -a, -l, --sort=NAME, PATH...).

use buff_cli::App;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let app = App::new("ls-like".to_string())
        .about("Demo: flags + options + positionals".to_string())
        .flag(
            "all".to_string(),
            "a".to_string(),
            "show hidden".to_string(),
        )
        .flag(
            "long".to_string(),
            "l".to_string(),
            "long format".to_string(),
        )
        .option(
            "sort".to_string(),
            "s".to_string(),
            "sort by: name|size|time".to_string(),
        )
        .arg("path".to_string(), "directory to list".to_string());

    let parsed = match app.parse(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let show_all = parsed.flag("all");
    let long_format = parsed.flag("long");
    let sort = parsed.option("sort").unwrap_or_else(|| "name".to_string());
    let path = parsed.arg("path").unwrap_or_else(|| ".".to_string());

    println!("listing {path}");
    println!("  show all: {show_all}");
    println!("  long:     {long_format}");
    println!("  sort by:  {sort}");
}
