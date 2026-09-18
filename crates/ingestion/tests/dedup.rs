//! Task 48 (IN-5): duplicate external_id winner rule — most recent
//! `actualizado` wins; exact timestamp tie ⇒ lexicographically greater
//! SHA-256 hex of the raw serialization wins; deterministic across runs.

use ingestion::dedup::dedup;
use ingestion::format::csv::CsvStrategy;
use ingestion::ports::FormatStrategy;
use ingestion::row::RawRow;
use ingestion::summary::RunWarning;
use sha2::{Digest, Sha256};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} must be committed"))
}

fn digest_hex(row: &RawRow) -> String {
    let bytes: Vec<u8> = Sha256::digest(row.raw_serialization().as_bytes()).to_vec();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn most_recent_actualizado_wins() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_duplicate_ids.csv"))
        .expect("fixture parses");

    let outcome = dedup(rows);

    let winner_2001 = outcome
        .winners
        .iter()
        .find(|r| r.get("id") == Some("2001"))
        .expect("id 2001 survives dedup");
    assert_eq!(
        winner_2001.get("nombre_tramite"),
        Some("Cambio de libreta"),
        "the 2026-09-17 row (newer actualizado) wins over the 2026-09-16 row"
    );
    assert_eq!(winner_2001.get("actualizado"), Some("2026-09-17"));
}

#[test]
fn exact_timestamp_tie_breaks_by_greater_sha256_hex() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_duplicate_ids.csv"))
        .expect("fixture parses");

    // The two id-2002 rows share `actualizado` 2026-09-17 and differ only in
    // `nombre_tramite`/`valor`; the tie must be broken by content digest.
    let outcome = dedup(rows.clone());

    let group: Vec<&RawRow> = rows
        .iter()
        .filter(|r| r.get("id") == Some("2002"))
        .collect();
    assert_eq!(group.len(), 2, "fixture carries the tied pair");
    let winner_digest = group
        .iter()
        .map(|r| digest_hex(r))
        .max()
        .expect("tie pair non-empty");

    let winner_2002 = outcome
        .winners
        .iter()
        .find(|r| r.get("id") == Some("2002"))
        .expect("id 2002 survives dedup");
    assert_eq!(
        digest_hex(winner_2002),
        winner_digest,
        "the lexicographically greater SHA-256 hex wins the tie"
    );
}

#[test]
fn dedup_is_deterministic_across_runs() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_duplicate_ids.csv"))
        .expect("fixture parses");

    let first = dedup(rows.clone());
    let second = dedup(rows);

    let serial: Vec<String> = first
        .winners
        .iter()
        .map(|r| r.raw_serialization())
        .collect();
    let serial2: Vec<String> = second
        .winners
        .iter()
        .map(|r| r.raw_serialization())
        .collect();
    assert_eq!(serial, serial2, "identical input ⇒ identical winners");
    assert_eq!(first.warnings, second.warnings);
}

#[test]
fn outcome_is_a_warning_naming_id_winner_and_losers() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_duplicate_ids.csv"))
        .expect("fixture parses");

    let outcome = dedup(rows);

    // Two duplicate ids resolved; unique id 2003 raises no warning.
    assert_eq!(outcome.warnings.len(), 2);
    assert!(matches!(
        outcome.warnings[0],
        RunWarning::DuplicateId { .. }
    ));

    let ids: Vec<&str> = outcome.winners.iter().filter_map(|r| r.get("id")).collect();
    assert_eq!(ids, ["2001", "2002", "2003"], "winners are sorted by id");

    for warning in &outcome.warnings {
        let RunWarning::DuplicateId { id, winner, losers } = warning else {
            unreachable!("dedup only emits DuplicateId warnings")
        };
        assert!(!winner.is_empty(), "warning names the winner for {id}");
        assert_eq!(losers.len(), 1, "each duplicate pair has one loser");
        assert!(
            losers.iter().all(|l| l.contains("actualizado=")),
            "loser descriptions are informative"
        );
    }
}

#[test]
fn unique_ids_pass_through_untouched() {
    let rows = CsvStrategy
        .parse(&fixture("tramites_duplicate_ids.csv"))
        .expect("fixture parses");

    let outcome = dedup(rows);

    assert_eq!(outcome.winners.len(), 3, "3 unique ids after dedup");
    assert!(outcome.winners.iter().any(|r| r.get("id") == Some("2003")));
    assert!(
        !outcome.warnings.iter().any(|w| matches!(
            w,
            RunWarning::DuplicateId { id, .. } if id == "2003"
        )),
        "unique ids produce no duplicate warning"
    );
}

#[test]
fn findings_land_in_run_summary_as_warnings_not_errors() {
    // Task 49 (design §3 error strategy): duplicate/skip findings are
    // collected as RunSummary warnings; only structural problems are hard
    // errors. The summary API must accept them without failing.
    use ingestion::row::SkippedRow;
    use ingestion::summary::RunSummary;

    let rows = CsvStrategy
        .parse(&fixture("tramites_duplicate_ids.csv"))
        .expect("fixture parses");
    let outcome = dedup(rows);

    let mut summary = RunSummary {
        rows_read: 5,
        ..Default::default()
    };
    summary.record_duplicates(outcome.warnings.clone());
    summary.record_skips(&[SkippedRow {
        id: Some("9999".into()),
        reason: "missing required field(s): url".into(),
    }]);

    assert_eq!(summary.duplicates_resolved, 2);
    assert_eq!(summary.rows_skipped, 1);
    assert_eq!(summary.warnings.len(), 3);
    assert!(summary.warnings.iter().any(|w| matches!(
        w,
        RunWarning::DuplicateId { id, .. } if id == "2001"
    )));
}
