//! Duplicate detection (TX-3, TX-6, task 21): duplicate event slugs must
//! name both files; duplicate category slugs must fail; a duplicate relation
//! `order` inside one event must fail.

use taxonomy::validator::validate_dir;

fn fixture_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn duplicate_event_slug_fails_naming_both_files() {
    // `duplicates/dup` holds two event files both declaring
    // slug `comprar-vehiculo`: the valid seed file plus `dup-event.yaml`.
    let errors = validate_dir(&fixture_dir("dup"));
    assert!(!errors.is_empty(), "duplicate event slugs must fail");
    let messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    let joined = messages.join(" | ");
    assert!(
        messages.iter().any(|m| m.contains("comprar-vehiculo")),
        "slug must be named: {joined}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("comprar-vehiculo.yaml") && m.contains("dup-event.yaml")),
        "both files must be named by one failure: {joined}"
    );
}

#[test]
fn duplicate_category_slug_fails() {
    let errors = validate_dir(&fixture_dir("dup"));
    assert!(!errors.is_empty(), "duplicate category slugs must fail");
    let messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    assert!(
        messages.iter().any(|m| m.contains("vehiculos")),
        "category slug must be named: {messages:?}"
    );
}

#[test]
fn duplicate_relation_order_fails() {
    // `duplicates/dup-order-dir` holds one event whose two relations both
    // declare `order: 1`.
    let errors = validate_dir(&fixture_dir("dup-order-dir"));
    assert!(!errors.is_empty(), "duplicate relation order must fail");
    let msg = errors[0].to_string();
    assert!(
        msg.contains("dup-order.yaml"),
        "offending file must be named: {msg}"
    );
    assert!(
        msg.contains("order"),
        "offending field must be named: {msg}"
    );
    assert!(msg.contains("1"), "offending value must be named: {msg}");
}
