//! Task 46 (IN-4): a row missing any required source field MUST be skipped
//! and reported (naming its `id`) without aborting the run.

use ingestion::format::csv::CsvStrategy;
use ingestion::ports::FormatStrategy;
use ingestion::row::{REQUIRED_COLUMNS, validate_rows};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} must be committed"))
}

#[test]
fn required_columns_are_exactly_the_spec_set() {
    assert_eq!(
        REQUIRED_COLUMNS,
        [
            "id",
            "nombre_tramite",
            "institucion_nombre",
            "url",
            "ques_es"
        ],
        "spec IN-4 fixes the required-field list"
    );
}

#[test]
fn row_with_empty_nombre_tramite_is_skipped_and_reported() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_missing_required.csv"))
        .expect("fixture parses");

    let (valid, skipped) = validate_rows(rows);

    assert_eq!(skipped.len(), 1, "exactly one row is invalid");
    assert_eq!(
        skipped[0].id.as_deref(),
        Some("1002"),
        "the report names the offending row id"
    );
    assert!(
        skipped[0].reason.contains("nombre_tramite"),
        "the report names the missing required column"
    );

    let ids: Vec<&str> = valid.iter().map(|r| r.get("id").unwrap()).collect();
    assert_eq!(ids, ["1001", "1003"], "valid rows survive the same run");
}

#[test]
fn empty_required_id_is_skipped_with_unnamed_report() {
    // A row whose `id` itself is empty cannot be named; the skip report must
    // still fire (id: None) and the run must continue.
    let good = sample_row("1001", "Cambio de libreta");
    let mut bad_no_id = sample_row("", "Sin id");
    bad_no_id.set("url", "");
    let mut bad_no_url = sample_row("1002", "Falta url");
    bad_no_url.set("url", "");

    let (valid, skipped) = validate_rows(vec![good, bad_no_id, bad_no_url]);

    assert_eq!(valid.len(), 1);
    assert_eq!(skipped.len(), 2, "both malformed rows are reported");
    assert!(
        skipped.iter().any(|s| s.id.is_none()),
        "a row without an id is reported unnamed"
    );
    assert!(skipped.iter().any(|s| s.reason.contains("url")));
}

#[test]
fn valid_rows_pass_untouched() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_missing_required.csv"))
        .expect("fixture parses");
    let (valid, skipped) = validate_rows(rows);
    assert_eq!(valid.len(), 2);
    assert_eq!(skipped.len(), 1);
    assert_eq!(
        valid[0].get("ques_es"),
        Some("Descripcion del tramite 1001")
    );
}

fn sample_row(id: &str, nombre: &str) -> ingestion::row::RawRow {
    let values: Vec<(String, String)> = ingestion::row::SOURCE_COLUMNS
        .iter()
        .map(|c| ((*c).to_string(), String::new()))
        .collect();
    let mut row = ingestion::row::RawRow::new(values);
    row.set("id", id);
    row.set("nombre_tramite", nombre);
    row.set("institucion_nombre", "Ministerio");
    row.set("url", "https://www.gub.uy/tramite/x");
    row.set("ques_es", "Descripcion");
    row
}
