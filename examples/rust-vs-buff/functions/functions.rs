// Rust: functions. Explicit type annotations on every parameter, format!()
// macro for string concatenation, semicolons on every statement, braces on
// every block. No default parameter values -- you fake them with Option<T>.

fn add(a: i64, b: i64) -> i64 {
    return a + b;
}

fn factorial(n: i64) -> i64 {
    if n <= 1 {
        return 1;
    }
    return n * factorial(n - 1);
}

fn classify(n: i64) -> i64 {
    if n == 0 {
        return 100;
    }
    if n == 1 {
        return 200;
    }
    return 999;
}

// No defaults: a "greet" with an optional name forces &str + Option + unwrap_or,
// or two functions. Buff supports `name: String = "world"` directly.
fn greet(name: &str) -> String {
    return format!("Hello, {}!", name);
}

fn main() {
    println!("{}", add(3, 4));
    println!("{}", factorial(10));
    println!("{}", classify(0));
    println!("{}", classify(1));
    println!("{}", classify(5));

    // String concatenation via format!() -- heavier than `+` in Buff.
    let message = greet("World");
    println!("{}", message);
}
