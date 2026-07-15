//! Example property test template for the lexer.
//!
//! When the lexer (T6) is implemented, replace this with real roundtrip properties.

use proptest::prelude::*;

proptest! {
    #[test]
    fn example_always_true_string_doesnt_crash(s in "[a-zA-Z]{1,10}") {
        // When lexer exists: lex(&s) should never panic
        // For now just ensure strings don't crash
        let _ = s.len();
    }

    #[test]
    fn example_integer_literal_roundtrips(n in 0i64..1000) {
        // When lexer exists: lex(n.to_string()) should produce single IntLit(n)
        let s = n.to_string();
        let parsed_back: i64 = s.parse().unwrap();
        prop_assert_eq!(n, parsed_back);
    }
}
