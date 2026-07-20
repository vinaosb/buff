// Rust: closures and the Fn/FnMut/FnOnce trait family. Capture mode matters,
// .iter() yields references (so *x), and iterator chains need .collect::<T>().

fn main() {
    // .iter() gives &i32, so the closure receives a reference. We need
    // .cloned() or explicit *x, plus .collect::<Vec<i32>>() turbofish.
    let doubled: Vec<i32> = vec![1, 2, 3, 4, 5]
        .iter()
        .map(|x| x * 2)
        .collect();
    println!("{}", doubled[0]);
    println!("{}", doubled[4]);

    let squared: Vec<i32> = vec![1, 2, 3, 4, 5]
        .iter()
        .map(|x| x * x)
        .collect();
    println!("{}", squared[2]);

    // Chained .map() calls: each closure takes &i32, returns i32. We collect
    // at the end to materialize a Vec<i32>.
    let plus_one: Vec<i32> = vec![1, 2, 3, 4, 5]
        .iter()
        .map(|x| x * 2)
        .map(|y| y + 1)
        .collect();
    println!("{}", plus_one[0]);
    println!("{}", plus_one[4]);

    let echo: Vec<i32> = vec![10, 20, 30]
        .iter()
        .map(|x| x + x)
        .collect();
    println!("{}", echo[1]);

    // .len() on the vector.
    let count = vec![1, 2, 3, 4, 5].len();
    println!("{}", count);
}
