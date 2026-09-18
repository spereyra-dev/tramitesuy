//! Task 51 (IN-6, DM-3): a changed `valor` produces exactly one new version
//! with `content_hash = SHA-256(normalized_payload)` and closes the prior
//! version's `valid_until` at the run timestamp, while unchanged rows produce
//! no version row.

use ingestion::format::csv::CsvStrategy;
use ingestion::pipeline::run;
use ingestion::ports::{DatasetManifest, FormatStrategy};
use sha2::{Digest, Sha256};
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

fn manifest() -> DatasetManifest {
    DatasetManifest {
        resource_id: "00000000-0000-0000-0000-000000000000".to_string(),
        last_modified: "2026-09-17T00:00:00Z".to_string(),
        hash: "fixture-sha".to_string(),
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// SHA-256 of the normalized payload (`raw_data` JSON) of the row with the
/// given `id` inside a fixture.
fn hash_of_row(bytes: &[u8], id: &str) -> String {
    let rows = CsvStrategy.parse(bytes).expect("fixture parses");
    let row = rows
        .iter()
        .find(|r| r.get("id") == Some(id))
        .unwrap_or_else(|| panic!("row {id} present"));
    digest_hex(
        serde_json::to_string(&row.to_raw_data_json())
            .unwrap()
            .as_bytes(),
    )
}

#[test]
fn changed_valor_creates_exactly_one_version_and_closes_the_prior() {
    let fetcher = FixtureFetcher {
        manifest: manifest(),
        bytes: fixture("tramites_pipeline.csv"),
    };
    let repo = InMemoryRepo::default();
    run(&fetcher, &CsvStrategy, &repo, NOW1.to_string()).expect("first run");

    let second = FixtureFetcher {
        manifest: manifest(),
        bytes: fixture("tramites_pipeline_valor_changed.csv"),
    };
    let summary = run(&second, &CsvStrategy, &repo, NOW2.to_string()).expect("second run");

    assert_eq!(summary.rows_read, 2);
    assert_eq!(summary.created, 0, "no row is new in the second run");
    assert_eq!(summary.updated, 1, "only the changed row re-persists");
    assert_eq!(summary.unchanged, 1);
    assert_eq!(summary.deactivated, 0);

    let baseline = hash_of_row(&fixture("tramites_pipeline.csv"), "3001");
    let changed = hash_of_row(&fixture("tramites_pipeline_valor_changed.csv"), "3001");
    assert_ne!(baseline, changed, "the valor edit must change the hash");

    let versions = repo.versions("3001");
    assert_eq!(versions.len(), 2, "changed row → exactly one new version");

    assert_eq!(versions[0].content_hash, baseline);
    assert_eq!(versions[0].valid_from, NOW1);
    assert_eq!(
        versions[0].valid_until,
        Some(NOW2.to_string()),
        "prior version's valid_until closes at the run timestamp"
    );

    assert_eq!(versions[1].content_hash, changed);
    assert_eq!(versions[1].valid_from, NOW2);
    assert_eq!(versions[1].valid_until, None, "the new version stays open");
}

#[test]
fn unchanged_rows_produce_no_version_row() {
    let fetcher = FixtureFetcher {
        manifest: manifest(),
        bytes: fixture("tramites_pipeline.csv"),
    };
    let repo = InMemoryRepo::default();
    run(&fetcher, &CsvStrategy, &repo, NOW1.to_string()).expect("first run");
    run(&fetcher, &CsvStrategy, &repo, NOW2.to_string()).expect("identical second run");

    // Only the initial version exists for the untouched row: no version row
    // was appended on the identical re-run (IN-6 unchanged clause, DM-3).
    assert_eq!(repo.versions("3002").len(), 1);
    assert_eq!(repo.versions("3001").len(), 1);
    assert_eq!(repo.procedures().len(), 2);
}
