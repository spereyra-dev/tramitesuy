//! Task 47 (IN-8, D-4): every one of the 31 source columns is preserved in
//! `RawRow`, and the `institucion_padre_organizacional_*` fields ride into
//! the `raw_data` JSONB value destined for `procedures.raw_data`.

use ingestion::format::csv::CsvStrategy;
use ingestion::ports::FormatStrategy;
use ingestion::row::SOURCE_COLUMNS;
use std::collections::BTreeSet;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} must be committed"))
}

#[test]
fn source_column_catalog_holds_exactly_31_names() {
    assert_eq!(SOURCE_COLUMNS.len(), 31);
}

#[test]
fn parsed_row_preserves_the_full_column_name_set() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_embedded_newline.csv"))
        .expect("fixture parses");

    let first = &rows[0];
    let got: BTreeSet<&str> = first.column_names().collect();
    let expected: BTreeSet<&str> = SOURCE_COLUMNS.iter().copied().collect();

    assert_eq!(
        got, expected,
        "the parsed row's column set must equal SOURCE_COLUMNS exactly"
    );
}

#[test]
fn parent_organization_fields_survive_into_raw_data() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_embedded_newline.csv"))
        .expect("fixture parses");
    let first = &rows[0];

    // D-4: parent-org semantics stay unresolved; the fields must still be
    // preserved, destined for `procedures.raw_data` JSONB.
    for col in [
        "institucion_padre_organizacional_id",
        "institucion_padre_organizacional_nombre",
    ] {
        let value = first.get(col).expect("parent-org column present");
        assert!(!value.is_empty(), "{col} must be preserved verbatim");
    }

    let raw = first.to_raw_data_json();
    let obj = raw.as_object().expect("raw_data is a JSON object");
    assert_eq!(obj.len(), 31, "raw_data carries every source column");
    assert!(obj.contains_key("institucion_padre_organizacional_id"));
    assert!(obj.contains_key("institucion_padre_organizacional_nombre"));
    assert_eq!(
        obj.get("id").and_then(|v| v.as_str()),
        Some("1001"),
        "raw_data values match the row"
    );
}
