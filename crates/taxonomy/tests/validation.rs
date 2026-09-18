//! Strict schema validation (TX-2, task 20): unknown fields, invalid keyword
//! types, and missing required fields must fail naming the offending file
//! and value.

use taxonomy::validator::validate_dir;

fn fixture_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn unknown_field_is_rejected_with_file_and_field_named() {
    let errors = validate_dir(&fixture_dir("unknown-field-only"));
    assert_eq!(errors.len(), 1, "one failure expected: {errors:?}");
    let msg = errors[0].to_string();
    assert!(
        msg.contains("unknown-field.yaml"),
        "file must be named: {msg}"
    );
    assert!(
        msg.contains("unexpected_field"),
        "field must be named: {msg}"
    );
}

#[test]
fn invalid_keyword_type_is_rejected() {
    let errors = validate_dir(&fixture_dir("keyword-type-only"));
    assert_eq!(errors.len(), 1, "one failure expected: {errors:?}");
    let msg = errors[0].to_string();
    assert!(
        msg.contains("keyword-type.yaml"),
        "file must be named: {msg}"
    );
    assert!(msg.contains("VERB"), "offending type must be named: {msg}");
}

#[test]
fn missing_required_event_field_is_rejected() {
    let errors = validate_dir(&fixture_dir("missing-category-only"));
    assert_eq!(errors.len(), 1, "one failure expected: {errors:?}");
    let msg = errors[0].to_string();
    assert!(
        msg.contains("missing-category.yaml"),
        "file must be named: {msg}"
    );
    assert!(
        msg.contains("category"),
        "missing field must be named: {msg}"
    );
}

#[test]
fn missing_required_keyword_field_is_rejected() {
    let errors = validate_dir(&fixture_dir("missing-weight-only"));
    assert_eq!(errors.len(), 1, "one failure expected: {errors:?}");
    let msg = errors[0].to_string();
    assert!(
        msg.contains("missing-keyword-weight.yaml"),
        "file must be named: {msg}"
    );
    assert!(msg.contains("weight"), "missing field must be named: {msg}");
}

#[test]
fn valid_fixture_yields_zero_failures() {
    let errors = validate_dir(&fixture_dir("valid"));
    assert!(errors.is_empty(), "valid fixture must pass: {errors:?}");
}
