//! Task 53 (IN-9): the second identical run is a complete no-op — zero new
//! procedures, zero new versions, zero duplicate organizations, no status
//! changes; only `last_seen_at` and the run record advance. (The run record
//! is the returned `RunSummary`; the DB `search_ops` divergence is resolved
//! as stdout-only by task 61, outside this unit.)

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
fn second_identical_run_creates_nothing_and_changes_no_statuses() {
    let fetcher = fetcher(fixture("tramites_pipeline.csv"));
    let repo = InMemoryRepo::default();

    run(&fetcher, &CsvStrategy, &repo, NOW1.to_string()).expect("first run");

    let summary = run(&fetcher, &CsvStrategy, &repo, NOW2.to_string()).expect("second run");
    assert_eq!(summary.rows_read, 2, "the run record reports rows again");
    assert_eq!(summary.rows_skipped, 0);
    assert_eq!(summary.duplicates_resolved, 0);
    assert_eq!(summary.created, 0, "zero new procedures");
    assert_eq!(summary.updated, 0, "zero new versions");
    assert_eq!(summary.unchanged, 2);
    assert_eq!(summary.deactivated, 0, "no status changed to inactive");

    let procedures = repo.procedures();
    assert_eq!(procedures.len(), 2);
    for procedure in &procedures {
        assert_eq!(procedure.status, ProcedureStatus::Active);
        assert_eq!(procedure.first_seen_at, NOW1, "first_seen_at preserved");
        assert_eq!(procedure.last_seen_at, NOW2, "only last_seen_at advances");
        assert_eq!(
            procedure.deactivated_at, None,
            "no deactivated_at is stamped on a present row"
        );
    }

    let version_rows: usize = procedures
        .iter()
        .map(|p| repo.versions(&p.external_id).len())
        .sum();
    assert_eq!(version_rows, 2, "zero new version rows");

    assert_eq!(repo.organizations().len(), 2, "no duplicate organizations");
}

#[test]
fn a_third_identical_run_still_advances_only_last_seen() {
    // TRIANGULATE: the no-op guarantee holds on every repeated run, and
    // first_seen_at keeps the very first run's stamp.
    let fetcher = fetcher(fixture("tramites_pipeline.csv"));
    let repo = InMemoryRepo::default();

    run(&fetcher, &CsvStrategy, &repo, NOW1.to_string()).expect("run 1");
    run(&fetcher, &CsvStrategy, &repo, NOW2.to_string()).expect("run 2");
    let summary = run(
        &fetcher,
        &CsvStrategy,
        &repo,
        "2026-09-20T03:00:00Z".to_string(),
    )
    .expect("run 3");

    assert_eq!(summary.created, 0);
    assert_eq!(summary.updated, 0);
    assert_eq!(summary.unchanged, 2);
    assert_eq!(summary.deactivated, 0);
    for procedure in repo.procedures() {
        assert_eq!(procedure.last_seen_at, "2026-09-20T03:00:00Z");
        assert_eq!(procedure.first_seen_at, NOW1);
        assert_eq!(repo.versions(&procedure.external_id).len(), 1);
    }
}
