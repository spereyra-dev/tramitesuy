//! Task 50 (IN-1, D-5): a full fixture-driven run over a `FixtureFetcher`
//! (committed bytes + fixed manifest) completes
//! resolve → download → parse → validate → dedup → normalize → hash →
//! diff → persist with zero network access, persisting through the
//! in-memory `ProcedureRepository` port.

use ingestion::format::csv::CsvStrategy;
use ingestion::pipeline::run;
use ingestion::ports::{FormatStrategy, ProcedureRepository, SourceFetcher};
use ingestion::summary::{RunStamp, UpsertCounts};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};

mod support;

use support::{FixtureFetcher, InMemoryRepo};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} must be committed"))
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn fixed_manifest() -> ingestion::ports::DatasetManifest {
    ingestion::ports::DatasetManifest {
        resource_id: "00000000-0000-0000-0000-000000000000".to_string(),
        last_modified: "2026-09-17T00:00:00Z".to_string(),
        hash: "fixture-sha".to_string(),
    }
}

#[test]
fn full_offline_pipeline_persists_every_stage() {
    let fetcher = FixtureFetcher {
        manifest: fixed_manifest(),
        bytes: fixture("tramites_pipeline.csv"),
    };
    let repo = InMemoryRepo::default();

    let now: RunStamp = "2026-09-18T03:00:00Z".to_string();
    let summary = run(&fetcher, &CsvStrategy, &repo, now).expect("offline run completes");

    // Stage evidence: parse (2 rows), validate (0 skipped), dedup (0 dups),
    // diff (2 new external ids), persist (2 inserted).
    assert_eq!(summary.rows_read, 2);
    assert_eq!(summary.rows_skipped, 0);
    assert_eq!(summary.duplicates_resolved, 0);
    assert_eq!(summary.created, 2);
    assert_eq!(summary.updated, 0);

    // Persist landed through the repository port with the content hash of
    // the normalized payload (SHA-256 over the raw_data JSON, sorted keys).
    let upserts = repo.upserts();
    assert_eq!(upserts.len(), 2);
    let first = &upserts[0];
    assert_eq!(first.external_id, "3001");
    assert_eq!(first.name, "Cambio de libreta");
    assert_eq!(first.description, "Descripcion del tramite 3001");
    assert_eq!(first.organization_external_id, "O-1");
    assert_eq!(first.organization_name, "Ministerio");
    assert_eq!(first.official_url, "https://www.gub.uy/tramite/3001");

    let expected_hash = {
        let rows = CsvStrategy
            .parse(&fixture("tramites_pipeline.csv"))
            .expect("fixture parses");
        let row = rows
            .iter()
            .find(|r| r.get("id") == Some("3001"))
            .expect("row present");
        let payload = row.to_raw_data_json();
        digest_hex(serde_json::to_string(&payload).unwrap().as_bytes())
    };
    assert_eq!(first.content_hash, expected_hash);
    assert_eq!(first.raw_data.as_object().map(|o| o.len()), Some(31));

    let hashes = repo.latest_hashes().expect("in-memory repo");
    assert_eq!(hashes.get("3001"), Some(&expected_hash));
    assert!(hashes.contains_key("3002"));

    // last_seen touched for every persisted row at the run stamp.
    assert_eq!(
        repo.touched(),
        vec![
            ("3001".to_string(), "2026-09-18T03:00:00Z".to_string()),
            ("3002".to_string(), "2026-09-18T03:00:00Z".to_string())
        ]
    );
}

#[test]
fn second_identical_run_persists_nothing_new() {
    let fetcher = FixtureFetcher {
        manifest: fixed_manifest(),
        bytes: fixture("tramites_pipeline.csv"),
    };
    let repo = InMemoryRepo::default();

    let now = "2026-09-18T03:00:00Z".to_string();
    run(&fetcher, &CsvStrategy, &repo, now.clone()).expect("first run");
    let after_first = repo.upserts().len();

    let summary = run(&fetcher, &CsvStrategy, &repo, now).expect("second run");

    assert_eq!(summary.created, 0, "identical input creates nothing");
    assert_eq!(summary.updated, 0);
    assert_eq!(
        repo.upserts().len(),
        after_first,
        "no additional procedure rows"
    );
}

#[test]
fn skip_and_duplicate_findings_surface_in_summary() {
    let fetcher = FixtureFetcher {
        manifest: fixed_manifest(),
        bytes: fixture("tramites_missing_required.csv"),
    };
    let repo = InMemoryRepo::default();

    let summary = run(&fetcher, &CsvStrategy, &repo, "2026-09-18T03:00:00Z".into())
        .expect("run completes despite a skipped row");

    assert_eq!(summary.rows_read, 3);
    assert_eq!(summary.rows_skipped, 1);
    assert_eq!(summary.created, 2);
    assert!(
        summary
            .warnings
            .iter()
            .any(|w| matches!(w, ingestion::summary::RunWarning::SkippedRow {
                id: Some(id),
                ..
            } if id == "1002")),
        "the skip is reported naming its id"
    );
}

#[test]
fn fetcher_never_downloads_an_unresolved_resource() {
    let fetcher = FixtureFetcher {
        manifest: fixed_manifest(),
        bytes: fixture("tramites_pipeline.csv"),
    };
    let err = fetcher
        .download_resource("different-resource-id")
        .expect_err("foreign resource id must not be served");
    assert!(err.to_string().contains("different-resource-id"));
}

#[test]
fn in_memory_repo_counts_are_consistent() {
    let repo = InMemoryRepo::default();
    let empty: Vec<ingestion::summary::ProcedureUpsert> = Vec::new();
    let counts: UpsertCounts = repo
        .upsert_procedures(&empty, "now".to_string())
        .expect("empty batch");
    assert_eq!(counts, UpsertCounts::default());

    let hashes: HashMap<String, String> = repo.latest_hashes().expect("repo");
    assert!(hashes.is_empty());
    let ids: BTreeSet<String> = BTreeSet::new();
    assert_eq!(
        repo.deactivate_missing(&ids, "now".into()).expect("repo"),
        0
    );
}
