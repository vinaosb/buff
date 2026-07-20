// Rust: error handling. Custom error types need #[derive] plus impl Display
// and impl std::error::Error -- ~15 lines of boilerplate per error. Or you
// return Box<dyn Error> and pay a dynamic dispatch + allocation cost.

use std::error::Error;
use std::fmt;
use std::fmt::Display;

// Boilerplate: a custom error type needs a struct, derive, Display, and
// the Error trait impl (empty body, but mandatory).
#[derive(Debug, Clone)]
struct BuffError {
    message: String,
}

impl BuffError {
    fn new(message: &str) -> Self {
        BuffError { message: message.to_string() }
    }
}

impl Display for BuffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for BuffError {}

// Returning different error types forces Box<dyn Error> (or a manual enum).
fn half(n: i64) -> Result<i64, Box<dyn Error>> {
    if n < 2 {
        return Err(Box::new(BuffError::new("too small")));
    }
    Ok(n / 2)
}

fn double_half(n: i64) -> Result<i64, Box<dyn Error>> {
    // The ? operator propagates errors, but the type must coerce via Box.
    let h = half(n)?;
    Ok(h + 1)
}

fn add_one(n: i64) -> Result<i64, Box<dyn Error>> {
    let h = half(n)?;
    Ok(h * 2)
}

fn main() {
    let good = double_half(10);
    match good {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    }

    let bad = double_half(1);
    match bad {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    }

    let ok = half(8);
    match ok {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    }

    let fail = half(0);
    match fail {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    }

    let works = add_one(10);
    match works {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    }

    let fails = add_one(0);
    match fails {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    }
}
