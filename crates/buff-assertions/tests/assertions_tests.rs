use buff_assertions::assertThat;

#[test]
fn hello_assertions_basic() {
    assertThat(42).isEqualTo(42);
    assertThat(10).isGreaterThan(5);
    assertThat("hello".to_string()).startsWith("hel");
}

#[test]
fn hello_assertions_option() {
    assertThat(Some(99)).isSome().isEqualTo(99);
    let none: Option<i32> = None;
    assertThat(none).isNone();
}

#[test]
fn hello_assertions_vec() {
    assertThat(vec![1, 2, 3]).hasSize(3).containsItem(&2);
    let empty: Vec<i32> = Vec::new();
    assertThat(empty).isEmpty();
}

#[test]
fn hello_assertions_fluent_chain() {
    assertThat("hello world".to_string())
        .startsWith("hello")
        .contains(" ")
        .endsWith("world");
}
