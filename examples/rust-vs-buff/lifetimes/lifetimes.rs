// Rust: lifetimes. Functions returning references need 'a annotations tying
// the output lifetime to the input. Structs holding references need their
// own lifetime parameter that infects every downstream type.

// A struct with NO references is fine, but accessing a String field moves
// it out -- so we must return &str with a lifetime tied to &self.
struct Article {
    title: String,
    body: String,
}

// Accessor pattern: returns &str tied to the input's lifetime. The 'a is
// mandatory when lifetime elision rules do not apply.
fn title_of<'a>(a: &'a Article) -> &'a str {
    return &a.title;
}

// "Return one of two borrowed strings" -- the compiler forces 'a to tie
// both args and the return together. Without it: "lifetime may not live
// long enough."
fn longer<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() > b.len() {
        return a;
    }
    return b;
}

// Struct that holds a reference needs a lifetime parameter on the struct
// itself, infecting every type that uses it.
struct Wrapper<'a> {
    data: &'a str,
}

fn main() {
    let art = Article {
        title: "Hello Buff".to_string(),
        body: "No lifetimes here".to_string(),
    };

    // title_of borrows art; the returned &str is tied to art's lifetime.
    // We cannot let art drop while the &str is alive.
    println!("{}", title_of(&art));

    // String literals are &'static str, so they outlive everything.
    println!("{}", longer("short", "much longer string"));

    let w = Wrapper { data: "borrowed slice" };
    println!("{}", w.data);
}
