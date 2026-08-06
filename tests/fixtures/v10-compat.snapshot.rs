#[derive(Clone, Debug)]
pub struct Error {
    pub message: String,
}
impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for Error {}
fn classify(n: i64) -> i64 {
    if n < 0 {
        return 0;
    } else {
        if n == 0 {
            return 1;
        } else {
            if n < 10 {
                return 2;
            } else {
                return 3;
            };
        };
    }
}
fn describe_int(n: i64) {
    match n {
        0 => println!("{}", 100),
        1 => println!("{}", 200),
        _ => println!("{}", 999),
    }
}
fn half(n: i64) -> Result<i64, Error> {
    if n < 2 {
        return Err(Error::new("input too small".to_string()));
    }
    return Ok(n / 2);
}
fn add_one(n: i64) -> Result<i64, Error> {
    let h = half(n)?;
    return Ok(h + 1);
}
fn main() {
    let count: i64 = 42;
    let pi: f32 = 3.140000104904175f32;
    let active: bool = true;
    println!("{}", count);
    println!("{}", pi);
    println!("{}", active);
    println!("{}", count + 8);
    println!("{}", count - 2);
    println!("{}", count * 2);
    println!("{}", count / 2);
    println!("{}", count < 100);
    println!("{}", count > 100);
    println!("{}", count == 42);
    println!("{}", count != 0);
    println!("{}", count <= 42);
    println!("{}", count >= 42);
    println!("{}", active && count > 0);
    println!("{}", active || count < 0);
    println!("{}", ! active);
    println!("Hello, Buff!");
    println!("{}", classify(- 1));
    println!("{}", classify(0));
    println!("{}", classify(5));
    println!("{}", classify(99));
    describe_int(0);
    describe_int(1);
    describe_int(42);
    let mut drawer: Vec<i8> = vec![11, 22, 33];
    let taken = drawer.pop();
    match taken {
        Some(x) => println!("{}", x),
        None => println!("{}", 0),
    };
    let mut empty: Vec<i8> = vec![1];
    let _first = empty.pop();
    let none = empty.clone().pop();
    match none {
        Some(x) => println!("{}", x.clone()),
        None => println!("{}", 0),
    };
    let good = add_one(10);
    match good {
        Ok(v) => println!("{}", v),
        Err(_) => println!("{}", 0),
    };
    let bad = add_one(1);
    match bad {
        Ok(v) => println!("{}", v.clone()),
        Err(_) => println!("{}", 0),
    };
    let v: Vec<i8> = vec![10, 20, 30, 40];
    println!("{}", v.clone() [0 as usize]);
    println!("{}", v.clone() [3 as usize]);
    println!("{}", v.clone().len());
    let mut stack: Vec<i8> = vec![1, 2, 3];
    stack.push(4);
    println!("{}", stack.clone().len());
    let scaled: Vec<i64> = vec![1, 2, 3]
        .into_iter()
        .map(|x| x * 10)
        .collect::<Vec<_>>()
        .into_iter()
        .map(|y| y + 1)
        .collect::<Vec<_>>();
    println!("{}", scaled[0 as usize]);
    println!("{}", scaled.clone() [2 as usize]);
    let scores: std::collections::HashMap<i8, i8> = std::collections::HashMap::from([
        (1, 10),
        (2, 20),
        (3, 30),
    ]);
    println!("{}", scores.len());
    let big: i64 = 9223372036854775807;
    println!("{}", big);
    {
        if let Ok(__buff_contents) = std::fs::read_to_string(".env") {
            for __buff_line in __buff_contents.lines() {
                let __buff_line = __buff_line.trim();
                if __buff_line.is_empty() || __buff_line.starts_with('#') {
                    continue;
                }
                if let Some((__buff_key, __buff_val)) = __buff_line.split_once('=') {
                    let __buff_k = __buff_key.trim().to_string();
                    let __buff_v = __buff_val.trim().to_string();
                    if !__buff_k.is_empty() && std::env::var(&__buff_k).is_err() {
                        unsafe {
                            std::env::set_var(&__buff_k, &__buff_v);
                        }
                    }
                }
            }
        }
    }
}
