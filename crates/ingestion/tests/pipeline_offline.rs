//! Task 50 (IN-1, D-5): a full fixture-driven run over a `FixtureFetcher`
//! (committed bytes + fixed manifest) completes
//! resolve → download → parse → validate → dedup → normalize → hash →
//! diff → persist with zero network access, persisting through the
//! in-memory `ProcedureRepository` port.

use ingestion::error::IngestionError;
use ingestion::format::csv::CsvStrategy;
use ingestion::pipeline::run;
use ingestion::ports::{FormatStrategy, ProcedureRepository, SourceFetcher};
use ingestion::summary::{RunStamp, UpsertCounts};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
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

/// Per-method call counter for the recording repository double below.
#[derive(Default)]
struct CallLog {
    upserts: usize,
    closes: usize,
    deactivates: usize,
    reactivates: usize,
    touches: usize,
}

/// The canonical in-memory repository wrapped with a call log: a run can be
/// proven to have issued no state-changing port call, independently of
/// whether the in-memory state happened to change (e.g. a no-op call with an
/// empty argument list).
struct RecordingRepo {
    inner: InMemoryRepo,
    log: RefCell<CallLog>,
}

impl Default for RecordingRepo {
    fn default() -> Self {
        Self {
            inner: InMemoryRepo::default(),
            log: RefCell::new(CallLog::default()),
        }
    }
}

impl RecordingRepo {
    /// `(upserts, closes, deactivates, reactivates, touches)` call counts.
    fn calls(&self) -> (usize, usize, usize, usize, usize) {
        let log = self.log.borrow();
        (
            log.upserts,
            log.closes,
            log.deactivates,
            log.reactivates,
            log.touches,
        )
    }

    fn reset_log(&self) {
        *self.log.borrow_mut() = CallLog::default();
    }
}

impl ProcedureRepository for RecordingRepo {
    fn latest_hashes(&self) -> Result<HashMap<String, String>, ingestion::error::RepoError> {
        self.inner.latest_hashes()
    }

    fn upsert_procedures(
        &self,
        rows: &[ingestion::summary::ProcedureUpsert],
        at: RunStamp,
    ) -> Result<UpsertCounts, ingestion::error::RepoError> {
        self.log.borrow_mut().upserts += 1;
        self.inner.upsert_procedures(rows, at)
    }

    fn close_versions(
        &self,
        ids: &[(String, String)],
        at: RunStamp,
    ) -> Result<(), ingestion::error::RepoError> {
        self.log.borrow_mut().closes += 1;
        self.inner.close_versions(ids, at)
    }

    fn deactivate_missing(
        &self,
        present_ids: &BTreeSet<String>,
        at: RunStamp,
    ) -> Result<usize, ingestion::error::RepoError> {
        self.log.borrow_mut().deactivates += 1;
        self.inner.deactivate_missing(present_ids, at)
    }

    fn reactivate_present(
        &self,
        ids: &[String],
        at: RunStamp,
    ) -> Result<usize, ingestion::error::RepoError> {
        self.log.borrow_mut().reactivates += 1;
        self.inner.reactivate_present(ids, at)
    }

    fn touch_last_seen(
        &self,
        ids: &[String],
        at: RunStamp,
    ) -> Result<(), ingestion::error::RepoError> {
        self.log.borrow_mut().touches += 1;
        self.inner.touch_last_seen(ids, at)
    }

    fn all_external_ids(&self) -> Result<Vec<String>, ingestion::error::RepoError> {
        self.inner.all_external_ids()
    }
}

/// A minimal header carrying the five required source columns; the body is
/// supplied per test. Used instead of a committed fixture so the batch can
/// be made empty or fully invalid inline.
const REQUIRED_HEADER: &[u8] = b"id,nombre_tramite,ques_es,institucion_nombre,url\n";

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

/// An empty source batch (header only, zero rows) is a structural failure:
/// the run returns `EmptyBatch` and issues no state-changing port call, so a
/// populated catalog is never deactivated by a truncated response.
#[test]
fn empty_source_batch_returns_empty_batch_error_and_changes_no_state() {
    let repo = RecordingRepo::default();
    // Seed a populated catalog so any state change would be observable.
    let seed = FixtureFetcher {
        manifest: fixed_manifest(),
        bytes: fixture("tramites_pipeline.csv"),
    };
    run(&seed, &CsvStrategy, &repo, "2026-09-18T03:00:00Z".into())
        .expect("seed run persists the catalog");
    let before = repo.inner.state_report();
    let seeded_upserts = repo.inner.upserts().len();
    repo.reset_log();

    let empty = FixtureFetcher {
        manifest: fixed_manifest(),
        bytes: REQUIRED_HEADER.to_vec(),
    };
    let err = run(&empty, &CsvStrategy, &repo, "2026-09-18T04:00:00Z".into())
        .expect_err("an empty batch must not run to completion");
    match err {
        IngestionError::EmptyBatch { rows_read, skipped } => {
            assert_eq!((rows_read, skipped), (0, 0));
        }
        other => panic!("expected EmptyBatch, got {other:?}"),
    }

    assert_eq!(
        repo.calls(),
        (0, 0, 0, 0, 0),
        "no upsert/close/deactivate/reactivate/touch call may happen for an empty batch"
    );
    assert_eq!(repo.inner.state_report(), before, "state is untouched");
    assert_eq!(repo.inner.upserts().len(), seeded_upserts);
}

/// A batch where every row is invalid (all skipped) is just as dangerous as
/// an empty one: the winners set is empty, so the guard fires before any
/// state-changing call and the catalog is preserved.
#[test]
fn fully_invalid_batch_returns_empty_batch_error_and_changes_no_state() {
    let repo = RecordingRepo::default();
    let seed = FixtureFetcher {
        manifest: fixed_manifest(),
        bytes: fixture("tramites_pipeline.csv"),
    };
    run(&seed, &CsvStrategy, &repo, "2026-09-18T03:00:00Z".into())
        .expect("seed run persists the catalog");
    let before = repo.inner.state_report();
    repo.reset_log();

    // The only row is missing the required `nombre_tramite`, so it is
    // skipped and no winner survives.
    let invalid = FixtureFetcher {
        manifest: fixed_manifest(),
        bytes: b"id,nombre_tramite,ques_es,institucion_nombre,url\n\
                 9001,,Descripcion del tramite 9001,Ministerio,https://www.gub.uy/tramite/9001\n"
            .to_vec(),
    };
    let err = run(&invalid, &CsvStrategy, &repo, "2026-09-18T04:00:00Z".into())
        .expect_err("a fully invalid batch must not run to completion");
    match err {
        IngestionError::EmptyBatch { rows_read, skipped } => {
            assert_eq!((rows_read, skipped), (1, 1));
        }
        other => panic!("expected EmptyBatch, got {other:?}"),
    }

    assert_eq!(
        repo.calls(),
        (0, 0, 0, 0, 0),
        "no upsert/close/deactivate/reactivate/touch call may happen for an invalid batch"
    );
    assert_eq!(repo.inner.state_report(), before, "state is untouched");
}
