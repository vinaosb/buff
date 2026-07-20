// Rust: pattern matching. Powerful but verbose: matching over &Option<T>
// requires Some(&x) or Some(x) depending on ownership; Result wrapping
// nests with ?; arms must agree on type to use match as an expression.

use std::error::Error;
use std::fmt;

#[derive(Debug, Clone)]
struct BuffError {
    message: String,
}

impl fmt::Display for BuffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for BuffError {}

fn half(n: i64) -> Result<i64, Box<dyn Error>> {
    if n < 2 {
        return Err(Box::new(BuffError { message: "too small".to_string() }));
    }
    Ok(n / 2)
}

fn classify(n: i64) -> i64 {
    if n == 0 {
        return 0;
    }
    if n == 1 {
        return 1;
    }
    return 99;
}

fn main() {
    // Match on Result -- Ok(v) and Err(_) arms must agree on output type.
    let good = half(10);
    match good {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    }

    let bad = half(1);
    match bad {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    }

    // Match on Option<i32> from .pop().
    let mut stack = vec![10, 20, 30];
    let top = stack.pop();
    match top {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    let _ = stack.pop();
    let empty = stack.pop();
    match empty {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    // Matching over a borrowed Option<&T> would force Some(&x) or Some(ref x)
    // in the pattern. The .clone() or *deref dance is mandatory in Rust.
    let borrowed: Option<&i32> = Some(&42);
    match borrowed {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    println!("{}", classify(0));
    println!("{}", classify(1));
    println!("{}", classify(5));
}
