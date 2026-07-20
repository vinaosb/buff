// Rust: structs. Boilerplate everywhere:
//   - #[derive(Clone, Debug)] on every struct for basic usability.
//   - `pub` on every field (private by default).
//   - Methods require a separate `impl Struct { }` block.
//   - Methods take &self / &mut self explicitly.
//   - String field access moves out of the struct -- so .clone() everywhere.

#[derive(Clone, Debug)]
struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug)]
struct Person {
    pub name: String,
    pub age: i64,
}

// Methods live in a separate impl block; &self receivers are mandatory.
impl Point {
    fn x(&self) -> f64 {
        return self.x;
    }
    fn y(&self) -> f64 {
        return self.y;
    }
}

impl Person {
    // Returning a String field by value moves it out of self, so we must
    // either return &str (lifetime-bound) or .clone() the field.
    fn name(&self) -> String {
        return self.name.clone();
    }
    fn age(&self) -> i64 {
        return self.age;
    }
}

fn main() {
    // Construction: literal syntax with field: value pairs.
    // `let mut` is required if we want to mutate later.
    let p = Point { x: 3.0, y: 4.0 };
    println!("{}", p.x());
    println!("{}", p.y());

    let person = Person {
        name: "Alice".to_string(),
        age: 30,
    };

    // .clone() to take the name out without moving; otherwise person.name
    // becomes unusable for the rest of the scope.
    println!("{}", person.name());
    println!("{}", person.age());
}
