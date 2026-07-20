// Rust: enums. Variants must be qualified (Shape::Circle, not Circle) at
// every use site. #[derive] macros are manual. Methods need impl blocks with
// &self receivers.

#[derive(Clone, Debug, PartialEq)]
enum Shape {
    Circle(f64),
    Rectangle(f64, f64),
    Point,
}

// Methods require an `impl Shape { ... }` block and &self receivers.
impl Shape {
    fn describe(&self) -> i32 {
        // Match arms must qualify each variant with Shape:: -- noise that
        // gets worse with nested or imported enums.
        match self {
            Shape::Circle(_r) => 1,
            Shape::Rectangle(_w, _h) => 2,
            Shape::Point => 0,
        }
    }
}

fn main() {
    // Construction also requires Shape:: prefix on every variant.
    let c = Shape::Circle(3.0);
    let r = Shape::Rectangle(2.0, 4.0);
    let p = Shape::Point;

    println!("{}", c.describe());
    println!("{}", r.describe());
    println!("{}", p.describe());

    // Direct match on the enum -- fully qualified everywhere.
    match c.clone() {
        Shape::Circle(r) => println!("{}", r),
        Shape::Rectangle(w, h) => println!("{} {}", w, h),
        Shape::Point => println!("{}", 0),
    }

    // Built-in Option<T> works the same way: Some/None qualification required.
    let mut v = vec![1, 2, 3];
    let top = v.pop();
    match top {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    }
}
