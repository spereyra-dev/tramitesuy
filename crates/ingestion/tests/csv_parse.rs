//! Task 45 (IN-3, D-5): the CSV `FormatStrategy` must recover a quoted field
//! containing embedded newlines as one intact record — naive line splitting
//! MUST NOT be used.

use ingestion::format::csv::CsvStrategy;
use ingestion::ports::FormatStrategy;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} must be committed"))
}

#[test]
fn embedded_newline_field_recovered_intact() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_embedded_newline.csv"))
        .expect("valid fixture parses");

    // The fixture has 2 logical rows; the first quoted `ques_es` spans 3
    // physical lines. Naive line splitting would produce 4+ records.
    assert_eq!(rows.len(), 2, "embedded newlines must not split records");

    let first = &rows[0];
    assert_eq!(first.get("id"), Some("1001"));

    let ques_es = first.get("ques_es").expect("ques_es present");
    assert_eq!(
        ques_es.matches('\n').count(),
        2,
        "the quoted field keeps its two embedded newlines verbatim"
    );
    assert!(ques_es.contains("Renueva la libreta"));
    assert!(ques_es.contains("y actualiza datos"));
    assert!(ques_es.contains("del vehiculo."));
}

#[test]
fn second_row_is_unaffected_by_preceding_multiline_field() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_embedded_newline.csv"))
        .expect("valid fixture parses");

    let second = &rows[1];
    assert_eq!(second.get("id"), Some("1002"));
    assert_eq!(second.get("nombre_tramite"), Some("Pago de patente"));
    assert_eq!(
        second.get("ques_es"),
        Some("Descripcion del tramite 1002"),
        "record framing stays aligned after a multiline field"
    );
}

#[test]
fn utf8_and_quoting_are_honored() {
    // Fixture values carry Latin-1-ish Spanish domain words stored as UTF-8
    // and standard double-quote escaping; both must round-trip.
    let rows = CsvStrategy
        .parse(&fixture("tramites_embedded_newline.csv"))
        .expect("valid fixture parses");

    let first = &rows[0];
    assert_eq!(first.get("nombre_tramite"), Some("Cambio de libreta"));
    assert_eq!(first.get("tiene_costo"), Some("Si"));
}
