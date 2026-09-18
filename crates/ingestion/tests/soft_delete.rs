//! Task 52 (IN-7): a procedure absent from a later run becomes
//! `status = inactive` with `deactivated_at` set and is NEVER deleted;
//! present rows advance `last_seen_at`; `first_seen_at` is preserved.

use ingestion::format::csv::CsvStrategy;
use ingestion::in_memory::ProcedureStatus;
use ingestion::pipeline::run;
use ingestion::ports::DatasetManifest;
use support::InMemoryRepo;

mod support;

use support::FixtureFetcher;

const NOW1: &str = "2026-09-18T03:00:00Z";
const NOW2: &str = "2026-09-19T03:00:00Z";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} must be committed"))
}

fn fetcher(bytes: Vec<u8>) -> FixtureFetcher {
    FixtureFetcher {
        manifest: DatasetManifest {
            resource_id: "00000000-0000-0000-0000-000000000000".to_string(),
            last_modified: "2026-09-17T00:00:00Z".to_string(),
            hash: "fixture-sha".to_string(),
        },
        bytes,
    }
}

#[test]
fn missing_row_is_deactivated_and_never_deleted() {
    let full = fetcher(fixture("tramites_pipeline.csv"));
    let reduced = fetcher(fixture("tramites_pipeline_reduced.csv"));
    let repo = InMemoryRepo::default();

    run(&full, &CsvStrategy, &repo, NOW1.to_string()).expect("first run");
    let summary = run(&reduced, &CsvStrategy, &repo, NOW2.to_string()).expect("second run");

    assert_eq!(summary.rows_read, 1, "the reduced fixture holds one row");
    assert_eq!(summary.created, 0);
    assert_eq!(summary.updated, 0);
    assert_eq!(summary.deactivated, 1, "the disappeared row is deactivated");

    let procedures = repo.procedures();
    assert_eq!(
        procedures.len(),
        2,
        "no procedure row was deleted (IN-7 never-delete clause)"
    );

    let removed = procedures
        .iter()
        .find(|p| p.external_id == "3001")
        .expect("3001 still exists");
    assert_eq!(removed.status, ProcedureStatus::Inactive);
    assert_eq!(removed.deactivated_at, Some(NOW2.to_string()));
    assert_eq!(
        removed.first_seen_at, NOW1,
        "first_seen_at is preserved from initial ingestion"
    );
    assert_eq!(
        removed.last_seen_at, NOW1,
        "an absent row is not touched by the later run"
    );

    let present = procedures
        .iter()
        .find(|p| p.external_id == "3002")
        .expect("3002 still exists");
    assert_eq!(present.status, ProcedureStatus::Active);
    assert_eq!(present.deactivated_at, None);
    assert_eq!(present.first_seen_at, NOW1, "first_seen_at preserved");
    assert_eq!(
        present.last_seen_at, NOW2,
        "present rows advance last_seen_at"
    );
}

#[test]
fn an_already_inactive_row_is_not_deactivated_twice() {
    // TRIANGULATE: a third identical run must not re-deactivate anything —
    // only rows that leave the source while still active count.
    let full = fetcher(fixture("tramites_pipeline.csv"));
    let reduced = fetcher(fixture("tramites_pipeline_reduced.csv"));
    let repo = InMemoryRepo::default();

    run(&full, &CsvStrategy, &repo, NOW1.to_string()).expect("run 1");
    run(&reduced, &CsvStrategy, &repo, NOW2.to_string()).expect("run 2");
    let summary = run(
        &reduced,
        &CsvStrategy,
        &repo,
        "2026-09-20T03:00:00Z".to_string(),
    )
    .expect("run 3");

    assert_eq!(summary.deactivated, 0, "nothing new left the source");
    let procedures = repo.procedures();
    assert_eq!(procedures.len(), 2);
    assert_eq!(
        procedures
            .iter()
            .find(|p| p.external_id == "3001")
            .unwrap()
            .deactivated_at,
        Some(NOW2.to_string()),
        "deactivated_at keeps its original run stamp"
    );
}
