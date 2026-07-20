// Rust: the borrow checker. Every value has one owner; passing or assigning
// it MOVES ownership. To reuse, you sprinkle .clone() everywhere or annotate
// lifetimes and references (&, &mut, 'a).

fn main() {
    let v = vec![1, 2, 3, 4, 5];

    // Indexing borrows implicitly; printing the element is fine.
    println!("{}", v[0]);
    println!("{}", v[4]);

    // Reuse after move: `let v2 = v` would MOVE v. We must .clone() to keep
    // both usable. The compiler forces this ceremony.
    let v2 = v.clone();
    println!("{}", v2[0]);

    // A mutable owned vector: pushing and popping mutate in place.
    let mut stack = vec![10, 20, 30];
    stack.push(40);
    println!("{}", stack.len());
    println!("{}", stack[3]);

    // .pop() returns Option<i32>; we must match to unwrap safely.
    let top = stack.pop();
    match top {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }
}
