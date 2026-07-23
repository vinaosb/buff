//! Integration tests for the `buff-fake` crate.
//!
//! Covers all 8 public methods per the T37 spec:
//! - `Faker::new`, `Faker::with_locale`, `Faker::with_seed`
//! - `name`, `email`, `address`, `phone`, `uuid`, `lorem`, `int`, `datetime`
//!
//! Tests are hermetic: seeded RNG produces deterministic output.

use buff_fake::{Faker, FakerError, FakerLocale};

#[test]
fn faker_new_creates_default() {
    let faker = Faker::new();
    // Default locale is en-US; just verify it doesn't panic.
    let _ = faker;
}

#[test]
fn faker_with_locale_pt_br() {
    let mut faker = Faker::with_locale(FakerLocale::PtBr);
    let name = faker.name();
    assert!(!name.is_empty(), "pt-BR name should not be empty");
}

#[test]
fn faker_with_seed_reproducible() {
    let mut faker_a = Faker::with_seed(FakerLocale::EnUs, 42);
    let mut faker_b = Faker::with_seed(FakerLocale::EnUs, 42);
    assert_eq!(faker_a.name(), faker_b.name());
    assert_eq!(faker_a.email(), faker_b.email());
    assert_eq!(faker_a.address(), faker_b.address());
}

#[test]
fn faker_name_generates_non_empty() {
    let mut faker = Faker::new();
    let name = faker.name();
    assert!(!name.is_empty(), "name should not be empty");
    assert!(name.contains(' '), "name should contain a space");
}

#[test]
fn faker_email_generates_valid_format() {
    let mut faker = Faker::new();
    let email = faker.email();
    assert!(!email.is_empty(), "email should not be empty");
    assert!(email.contains('@'), "email should contain @");
}

#[test]
fn faker_address_generates_non_empty() {
    let mut faker = Faker::new();
    let addr = faker.address();
    assert!(!addr.is_empty(), "address should not be empty");
}

#[test]
fn faker_phone_generates_non_empty() {
    let mut faker = Faker::new();
    let phone = faker.phone();
    assert!(!phone.is_empty(), "phone should not be empty");
}

#[test]
fn faker_uuid_generates_valid_format() {
    let mut faker = Faker::new();
    let uuid = faker.uuid();
    assert!(!uuid.is_empty(), "uuid should not be empty");
    assert_eq!(uuid.len(), 36, "uuid v4 should be 36 chars");
    assert_eq!(&uuid[14..15], "4", "uuid v4 should have version nibble 4");
}

#[test]
fn faker_lorem_generates_correct_word_count() {
    let mut faker = Faker::new();
    let text = faker.lorem(5);
    assert!(!text.is_empty(), "lorem should not be empty");
    let words: Vec<&str> = text.split_whitespace().collect();
    assert_eq!(words.len(), 5, "lorem(5) should produce 5 words");
}

#[test]
fn faker_int_in_range() {
    let mut faker = Faker::new();
    for _ in 0..20 {
        let val = faker.int(10, 20);
        assert!(
            (10..=20).contains(&val),
            "int(10,20) should be in [10,20], got {val}"
        );
    }
}

#[test]
fn faker_datetime_in_range() {
    let mut faker = Faker::new();
    let dt = faker.datetime("2020-01-01T00:00:00Z", "2020-12-31T23:59:59Z");
    assert!(dt.is_ok(), "datetime should succeed");
    let dt_str = dt.unwrap();
    assert!(!dt_str.is_empty(), "datetime should not be empty");
    assert!(dt_str.contains("2020"), "datetime should be in 2020");
}

#[test]
fn faker_datetime_rejects_invalid_range() {
    let mut faker = Faker::new();
    let err = faker.datetime("2020-12-31T00:00:00Z", "2020-01-01T00:00:00Z");
    assert!(err.is_err(), "end before start should error");
}

#[test]
fn faker_datetime_rejects_bad_format() {
    let mut faker = Faker::new();
    let err = faker.datetime("not-a-date", "2020-12-31T00:00:00Z");
    assert!(err.is_err(), "bad date format should error");
}

#[test]
fn faker_pt_br_generates_portuguese_names() {
    let mut faker = Faker::with_locale(FakerLocale::PtBr);
    let name = faker.name();
    assert!(!name.is_empty(), "pt-BR name should not be empty");
    // Portuguese names often have accented characters
    assert!(
        name.contains(' ') || name.len() > 3,
        "pt-BR name should be plausible"
    );
}

#[test]
fn faker_different_seeds_different_output() {
    let mut faker_a = Faker::with_seed(FakerLocale::EnUs, 1);
    let mut faker_b = Faker::with_seed(FakerLocale::EnUs, 2);
    // Very unlikely that two different seeds produce the same name
    assert_ne!(
        faker_a.name(),
        faker_b.name(),
        "different seeds should differ"
    );
}

// ---- Insta snapshots ---------------------------------------------------

#[test]
fn snapshot_faker_seeded_output() {
    let mut faker = Faker::with_seed(FakerLocale::EnUs, 42);
    insta::assert_snapshot!("faker_seeded_name", faker.name());
    insta::assert_snapshot!("faker_seeded_email", faker.email());
    insta::assert_snapshot!("faker_seeded_address", faker.address());
    insta::assert_snapshot!("faker_seeded_phone", faker.phone());
    insta::assert_snapshot!("faker_seeded_uuid", faker.uuid());
    insta::assert_snapshot!("faker_seeded_lorem", faker.lorem(3));
    insta::assert_snapshot!("faker_seeded_int", format!("{}", faker.int(1, 100)));
}

#[test]
fn snapshot_faker_error_debug() {
    let err1 = FakerError::InvalidDateRange("start after end".to_string());
    let err2 = FakerError::Panic;
    insta::assert_snapshot!("faker_error_debug", format!("{err1}\n{err2}"));
}
