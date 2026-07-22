use buff_assertions::assertThat;

fn main() {
    // Chained numeric assertions
    assertThat(42)
        .isGreaterThan(0)
        .isGreaterThanOrEqualTo(42)
        .isLessThan(100)
        .isLessThanOrEqualTo(42)
        .isNotEqualTo(43);
    println!("PASS: numeric fluent chain");

    // Chained string assertions
    assertThat("the quick brown fox".to_string())
        .startsWith("the")
        .contains("quick")
        .contains("brown")
        .endsWith("fox");
    println!("PASS: string fluent chain");

    // Chained option assertions
    assertThat(Some("nested".to_string()))
        .isSome()
        .startsWith("nest")
        .endsWith("ed");
    println!("PASS: option fluent chain");

    // Chained vector assertions
    assertThat(vec![10, 20, 30, 40])
        .hasSize(4)
        .containsItem(&20)
        .containsItem(&40);
    println!("PASS: vector fluent chain");

    println!("\nAll fluent chains passed!");
}
