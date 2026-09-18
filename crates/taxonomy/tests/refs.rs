//! Reference checks (TX-3, D-2, task 22): a relation referencing an
//! `external_id` absent from the snapshot fails naming the event file and
//! the orphan id; a reference to an undefined category slug fails naming
//! the file and the value.

use taxonomy::validator::validate_dir_against_snapshot;

fn fixture_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn refs_failures() -> Vec<String> {
    let data_dir = fixture_dir("refs");
    validate_dir_against_snapshot(&data_dir, &data_dir.join("external_ids.txt"))
        .into_iter()
        .map(|e| e.to_string())
        .collect()
}

#[test]
fn orphan_external_id_fails_naming_file_and_value() {
    let failures = refs_failures();
    assert!(
        failures.iter().any(|m| m.contains("orphan-relation.yaml")),
        "offending file must be named: {failures:?}"
    );
    assert!(
        failures
            .iter()
            .any(|m| m.contains("orphan-relation.yaml") && m.contains("orphan-proc-999")),
        "orphan external_id must be named with its file: {failures:?}"
    );
}

#[test]
fn unknown_category_reference_fails_naming_file_and_value() {
    let failures = refs_failures();
    assert!(
        failures
            .iter()
            .any(|m| m.contains("unknown-category.yaml") && m.contains("categoria-inexistente")),
        "file and offending category slug must be named: {failures:?}"
    );
}

#[test]
fn refs_fixture_produces_exactly_the_two_expected_failures() {
    let failures = refs_failures();
    assert_eq!(
        failures.len(),
        2,
        "orphan relation + unknown category, nothing else: {failures:?}"
    );
}
