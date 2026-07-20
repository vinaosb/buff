// Rust: collections. HashMap requires an import, explicit type annotations,
// .insert() to populate, and .get(&key) returning Option<&V> for lookup.

use std::collections::HashMap;

fn main() {
    // Vec<T> -- the only collection with a literal macro (vec![]).
    let mut v = vec![10, 20, 30];
    v.push(40);
    println!("{}", v.len());
    println!("{}", v[0]);

    let top = v.pop();
    match top {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }

    // .iter().map().collect() to transform a Vec.
    let scaled: Vec<i32> = vec![1, 2, 3]
        .iter()
        .map(|x| x * 10)
        .collect();
    println!("{}", scaled[0]);
    println!("{}", scaled[2]);

    // HashMap -- no literal syntax, must HashMap::new() and .insert(&k, v).
    // Type must be inferred from inserts or written explicitly.
    let mut scores: HashMap<i32, i32> = HashMap::new();
    scores.insert(1, 10);
    scores.insert(2, 20);
    scores.insert(3, 30);
    println!("{}", scores.len());

    // Lookup is .get(&key) returning Option<&i32>; no [] indexing on HashMap.
    // We must clone or copy out of the Option<&V> to use the value.
    if let Some(val) = scores.get(&2) {
        println!("{}", val);
    }
}
