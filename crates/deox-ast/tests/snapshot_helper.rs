//! Snapshot helper utility — reusable across deox-ast test files.
//!
//! Provides a convenience function for asserting Display snapshots
//! with a named snapshot (avoids inline-snapshot boilerplate).

use std::fmt::Display;

/// Assert that `value`'s `Display` output matches the named snapshot.
///
/// # Example
///
/// ```ignore
/// use deox_ast::{Expr, Literal, Span};
/// use snapshot_helper::assert_display_snapshot;
///
/// let e = Expr::Literal(Literal::Int(42), Span::dummy());
/// assert_display_snapshot(&e, "int_literal_42");
/// ```
pub fn assert_display_snapshot<T: Display>(value: &T, name: &str) {
    insta::assert_snapshot!(name, format!("{}", value));
}

#[cfg(test)]
mod tests {
    use super::*;
    use deox_ast::{Expr, Literal, Span};

    fn span() -> Span {
        Span::dummy()
    }

    /// Example test demonstrating the helper.
    #[test]
    fn example_using_helper() {
        let e = Expr::Literal(Literal::Int(42), span());
        assert_display_snapshot(&e, "helper_example_int_42");
    }
}
