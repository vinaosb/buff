// Rust: no null, but Option<T> has its own friction. .unwrap() panics on
// None, if let Some(x) adds indentation and a second code path, and you
// must pick between match / if let / unwrap / unwrap_or / map / and_then.

fn main() {
    let mut v = vec![10, 20, 30];

    // .pop() returns Option<T>. We must choose how to extract the value.
    // .unwrap() would panic on None -- the very thing Option was meant to
    // prevent. So we write out a full match.
    let top = v.pop();
    match top {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    let second = v.pop();
    match second {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    // Popping from an empty vec yields None -- the dangerous case.
    let third = v.pop();
    match third {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    let nothing = v.pop();
    match nothing {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    // More Option<T> from another vector.
    let mut data = vec![100, 200];
    let a = data.pop();
    match a {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    // The .unwrap() temptation: in a hurry you might write data.pop().unwrap().
    // That panics on None. Buff's match-only discipline forces safety.
    let b = data.pop();
    match b {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    let c = data.pop();
    match c {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }
}
