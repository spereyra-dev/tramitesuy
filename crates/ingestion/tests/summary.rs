//! Task 54 (IN-10): summary counts derived from source rows sum to rows
//! read, and identical input yields a byte-identical summary report.

use ingestion::format::csv::CsvStrategy;
use ingestion::pipeline::run;
use ingestion::ports::DatasetManifest;
use support::InMemoryRepo;

mod support;

use support::FixtureFetcher;

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
fn counts_sum_to_rows_read_with_skipped_rows() {
    let fetcher = fetcher(fixture("tramites_missing_required.csv"));
    let repo = InMemoryRepo::default();

    let summary = run(
        &fetcher,
        &CsvStrategy,
        &repo,
        "2026-09-18T03:00:00Z".to_string(),
    )
    .expect("run completes despite skipped rows");

    assert_eq!(summary.rows_read, 3);
    assert_eq!(summary.rows_skipped, 1);
    assert_eq!(summary.created, 2);
    assert_eq!(summary.updated, 0);
    assert_eq!(summary.unchanged, 0);
    assert_eq!(summary.duplicates_resolved, 0);
    assert_eq!(summary.deactivated, 0);
    assert_eq!(
        summary.rows_skipped
            + summary.created
            + summary.updated
            + summary.unchanged
            + summary.duplicates_resolved,
        summary.rows_read,
        "row-derived counts sum to rows read (IN-10)"
    );
}

#[test]
fn counts_sum_to_rows_read_with_duplicate_resolution() {
    // 5 source rows, 3 unique ids, 2 losers resolved by the IN-5 rule.
    let fetcher = fetcher(fixture("tramites_duplicate_ids.csv"));
    let repo = InMemoryRepo::default();

    let summary = run(
        &fetcher,
        &CsvStrategy,
        &repo,
        "2026-09-18T03:00:00Z".to_string(),
    )
    .expect("run completes");

    assert_eq!(summary.rows_read, 5);
    assert_eq!(summary.created, 3);
    assert_eq!(summary.duplicates_resolved, 2);
    assert_eq!(
        summary.rows_skipped
            + summary.created
            + summary.updated
            + summary.unchanged
            + summary.duplicates_resolved,
        summary.rows_read
    );
}

#[test]
fn identical_input_yields_a_byte_identical_report() {
    let fetcher = fetcher(fixture("tramites_duplicate_ids.csv"));

    let repo_a = InMemoryRepo::default();
    let summary_a = run(
        &fetcher,
        &CsvStrategy,
        &repo_a,
        "2026-09-18T03:00:00Z".to_string(),
    )
    .expect("run A completes");
    let repo_b = InMemoryRepo::default();
    let summary_b = run(
        &fetcher,
        &CsvStrategy,
        &repo_b,
        "2026-09-18T03:00:00Z".to_string(),
    )
    .expect("run B completes");

    assert_eq!(summary_a, summary_b, "same input → equal summaries");
    assert_eq!(
        summary_a.report(),
        summary_b.report(),
        "same input → byte-identical report"
    );
    assert_eq!(summary_a.warnings.len(), 2);
    assert!(
        summary_a.report().starts_with(
            "rows_read=5 rows_skipped=0 duplicates_resolved=2 created=3 updated=0 unchanged=0 deactivated=0\n"
        ),
        "the report's count line shape is stable: {}",
        summary_a.report()
    );
    assert!(
        summary_a.report().lines().count() == 3,
        "one count line + one line per warning"
    );
}
