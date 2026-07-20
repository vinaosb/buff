// Rust: recursion. Same algorithm as Buff, more ceremony:
//   - Integer type zoo: u8/u16/u32/u64/i32/i64/usize/isize. Pick one.
//   - println!("...{}", x) format macro syntax instead of print(x).
//   - Semicolons on every statement, braces on every block.

// fib(10) = 55. Using u64 to avoid overflow on larger inputs.
fn fib(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn factorial(n: u64) -> u64 {
    if n <= 1 {
        return 1;
    }
    return n * factorial(n - 1);
}

fn power(base: u64, exp: u64) -> u64 {
    if exp == 0 {
        return 1;
    }
    return base * power(base, exp - 1);
}

fn main() {
    // println! is a macro with format-string syntax, not a function call.
    println!("{}", fib(10));
    println!("{}", factorial(10));
    println!("{}", power(2, 10));

    // The integer-type choice bites at call sites: passing a u32 where u64
    // is expected needs `as u64`. Buff has a single Int type.
    let small: u32 = 10;
    println!("{}", fib(small as u64));
}
