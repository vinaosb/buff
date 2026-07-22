pub mod error;

use std::fmt;

pub struct AssertThat<T> {
    actual: T,
}

impl<T> AssertThat<T> {
    pub fn new(actual: T) -> Self {
        Self { actual }
    }

    pub fn isEqualTo(self, expected: T) -> Self
    where
        T: PartialEq + fmt::Debug,
    {
        if self.actual != expected {
            panic!(
                "assertThat({:?}).isEqualTo({:?}) failed",
                self.actual, expected
            );
        }
        self
    }

    pub fn isNotEqualTo(self, unexpected: T) -> Self
    where
        T: PartialEq + fmt::Debug,
    {
        if self.actual == unexpected {
            panic!(
                "assertThat({:?}).isNotEqualTo({:?}) failed",
                self.actual, unexpected
            );
        }
        self
    }

    pub fn isGreaterThan(self, other: T) -> Self
    where
        T: PartialOrd + fmt::Debug,
    {
        if !(self.actual > other) {
            panic!(
                "assertThat({:?}).isGreaterThan({:?}) failed",
                self.actual, other
            );
        }
        self
    }

    pub fn isGreaterThanOrEqualTo(self, other: T) -> Self
    where
        T: PartialOrd + fmt::Debug,
    {
        if !(self.actual >= other) {
            panic!(
                "assertThat({:?}).isGreaterThanOrEqualTo({:?}) failed",
                self.actual, other
            );
        }
        self
    }

    pub fn isLessThan(self, other: T) -> Self
    where
        T: PartialOrd + fmt::Debug,
    {
        if !(self.actual < other) {
            panic!(
                "assertThat({:?}).isLessThan({:?}) failed",
                self.actual, other
            );
        }
        self
    }

    pub fn isLessThanOrEqualTo(self, other: T) -> Self
    where
        T: PartialOrd + fmt::Debug,
    {
        if !(self.actual <= other) {
            panic!(
                "assertThat({:?}).isLessThanOrEqualTo({:?}) failed",
                self.actual, other
            );
        }
        self
    }

    pub fn isNull(self)
    where
        T: fmt::Debug,
    {
        panic!(
            "assertThat({:?}).isNull() is not supported for non-Option types; use isNone()",
            self.actual
        );
    }

    pub fn isNotNull(self)
    where
        T: fmt::Debug,
    {
    }
}

impl<T: fmt::Debug> AssertThat<Option<T>> {
    pub fn isSome(self) -> AssertThat<T> {
        match self.actual {
            Some(v) => AssertThat::new(v),
            None => panic!("assertThat(None).isSome() failed: value was None"),
        }
    }

    pub fn isNone(self) {
        if self.actual.is_some() {
            panic!(
                "assertThat({:?}).isNone() failed: value was Some",
                self.actual
            );
        }
    }
}

impl AssertThat<String> {
    pub fn startsWith(self, prefix: &str) -> Self {
        if !self.actual.starts_with(prefix) {
            panic!(
                "assertThat({:?}).startsWith({:?}) failed",
                self.actual, prefix
            );
        }
        self
    }

    pub fn endsWith(self, suffix: &str) -> Self {
        if !self.actual.ends_with(suffix) {
            panic!(
                "assertThat({:?}).endsWith({:?}) failed",
                self.actual, suffix
            );
        }
        self
    }

    pub fn contains(self, substr: &str) -> Self {
        if !self.actual.contains(substr) {
            panic!(
                "assertThat({:?}).contains({:?}) failed",
                self.actual, substr
            );
        }
        self
    }

    pub fn matches(self, pattern: &str) -> Self {
        if !self.actual.contains(pattern) {
            panic!(
                "assertThat({:?}).matches({:?}) failed",
                self.actual, pattern
            );
        }
        self
    }
}

impl<T: fmt::Debug> AssertThat<Vec<T>> {
    pub fn containsItem(self, item: &T) -> Self
    where
        T: PartialEq,
    {
        if !self.actual.contains(item) {
            panic!(
                "assertThat({:?}).containsItem({:?}) failed",
                self.actual, item
            );
        }
        self
    }

    pub fn hasSize(self, expected: usize) -> Self {
        let len = self.actual.len();
        if len != expected {
            panic!(
                "assertThat({:?}).hasSize({}) failed: actual size is {}",
                self.actual, expected, len
            );
        }
        self
    }

    pub fn isEmpty(self) -> Self {
        if !self.actual.is_empty() {
            panic!(
                "assertThat({:?}).isEmpty() failed: has {} elements",
                self.actual,
                self.actual.len()
            );
        }
        self
    }
}

pub fn assertThat<T>(actual: T) -> AssertThat<T> {
    AssertThat::new(actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_equal_to_passes() {
        assertThat(42).isEqualTo(42);
    }

    #[test]
    #[should_panic(expected = "isEqualTo")]
    fn is_equal_to_fails() {
        assertThat(42).isEqualTo(43);
    }

    #[test]
    fn is_not_equal_to_passes() {
        assertThat(42).isNotEqualTo(43);
    }

    #[test]
    #[should_panic(expected = "isNotEqualTo")]
    fn is_not_equal_to_fails() {
        assertThat(42).isNotEqualTo(42);
    }

    #[test]
    fn is_greater_than_passes() {
        assertThat(10).isGreaterThan(5);
    }

    #[test]
    #[should_panic(expected = "isGreaterThan")]
    fn is_greater_than_fails() {
        assertThat(5).isGreaterThan(10);
    }

    #[test]
    fn is_less_than_passes() {
        assertThat(5).isLessThan(10);
    }

    #[test]
    fn is_greater_than_or_equal_to_passes() {
        assertThat(5).isGreaterThanOrEqualTo(5);
        assertThat(6).isGreaterThanOrEqualTo(5);
    }

    #[test]
    fn is_less_than_or_equal_to_passes() {
        assertThat(5).isLessThanOrEqualTo(5);
        assertThat(4).isLessThanOrEqualTo(5);
    }

    #[test]
    fn string_starts_with_passes() {
        assertThat("hello world".to_string()).startsWith("hello");
    }

    #[test]
    #[should_panic(expected = "startsWith")]
    fn string_starts_with_fails() {
        assertThat("hello world".to_string()).startsWith("world");
    }

    #[test]
    fn string_ends_with_passes() {
        assertThat("hello world".to_string()).endsWith("world");
    }

    #[test]
    fn string_contains_passes() {
        assertThat("hello world".to_string()).contains("lo wo");
    }

    #[test]
    fn option_is_some_passes() {
        assertThat(Some(42)).isSome().isEqualTo(42);
    }

    #[test]
    #[should_panic(expected = "isSome")]
    fn option_is_some_fails() {
        let opt: Option<i32> = None;
        assertThat(opt).isSome();
    }

    #[test]
    fn option_is_none_passes() {
        let opt: Option<i32> = None;
        assertThat(opt).isNone();
    }

    #[test]
    #[should_panic(expected = "isNone")]
    fn option_is_none_fails() {
        assertThat(Some(42)).isNone();
    }

    #[test]
    fn vec_contains_item_passes() {
        assertThat(vec![1, 2, 3]).containsItem(&2);
    }

    #[test]
    fn vec_has_size_passes() {
        assertThat(vec![1, 2, 3]).hasSize(3);
    }

    #[test]
    fn vec_is_empty_passes() {
        let v: Vec<i32> = Vec::new();
        assertThat(v).isEmpty();
    }

    #[test]
    fn fluent_chain_passes() {
        assertThat(10)
            .isGreaterThan(5)
            .isLessThan(20)
            .isNotEqualTo(15);
    }

    #[test]
    fn string_fluent_chain_passes() {
        assertThat("hello beautiful world".to_string())
            .startsWith("hello")
            .contains("beautiful")
            .endsWith("world");
    }
}
