use buff_assertions::assertThat;

fn main() {
    // Basic equality
    assertThat(42).isEqualTo(42);
    println!("PASS: 42 == 42");

    // Numeric comparisons
    assertThat(10).isGreaterThan(5).isLessThan(20);
    println!("PASS: 10 > 5 && 10 < 20");

    // String assertions
    assertThat("hello world".to_string())
        .startsWith("hello")
        .contains(" ")
        .endsWith("world");
    println!("PASS: string fluent chain");

    // Option assertions
    assertThat(Some(99)).isSome().isEqualTo(99);
    println!("PASS: Option isSome");

    let none: Option<i32> = None;
    assertThat(none).isNone();
    println!("PASS: Option isNone");

    // Vector assertions
    assertThat(vec![1, 2, 3]).hasSize(3).containsItem(&2);
    println!("PASS: vector assertions");

    println!("\nAll assertions passed!");
}
