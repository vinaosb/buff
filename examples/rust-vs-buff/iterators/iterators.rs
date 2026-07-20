// Rust: iterators. Verbosity everywhere:
//   - .iter() borrows, .into_iter() consumes -- you must choose.
//   - Iterator chains need .collect::<Vec<T>>() with explicit type.
//   - Vectors are indexed by usize; mixed integer types need `as usize`.
//   - Closures over .iter() receive &T, requiring *x or |&x| patterns.

fn main() {
    let mut v = vec![10, 20, 30];
    v.push(40);
    println!("{}", v.len());

    // Indexing: i32 literal 0 works as usize by inference, but mixed
    // arithmetic types force `as usize` casts.
    let idx: i32 = 3;
    println!("{}", v[0]);
    println!("{}", v[idx as usize]);

    // .pop() returns Option<T>; must match to use.
    let top = v.pop();
    match top {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    // .iter().map().collect() with turbofish or type annotation.
    let doubled: Vec<i32> = vec![1, 2, 3, 4, 5]
        .iter()
        .map(|x| x * 2)
        .collect();
    println!("{}", doubled[0]);
    println!("{}", doubled[4]);

    // Chained maps: each closure takes &i32 (note `*y` would be needed if
    // we wrote the body without auto-deref on operator).
    let result: Vec<i32> = vec![1, 2, 3]
        .iter()
        .map(|x| x * 10)
        .map(|y| y + 1)
        .collect();
    println!("{}", result[0]);
    println!("{}", result[2]);
}
