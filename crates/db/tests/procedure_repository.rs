//! Task 58 (DM-2, IN-6/IN-7/IN-9, D-5): the sqlx `ProcedureRepository`
//! implementation in `crates/db` must satisfy the exact port contract proven
//! by the canonical `InMemoryProcedureRepository` in unit B3 — same fixture
//! semantics, single-transaction batches, and compile-time-checked queries,
//! exercised against the compose Postgres via scratch databases.

mod common;

use common::*;
use db::repos::procedures::PostgresProcedureRepository;
use ingestion::ports::ProcedureRepository;
use ingestion::summary::ProcedureUpsert;

const RUN_1: &str = "2026-09-18T03:00:00Z";
const RUN_2: &str = "2026-09-18T04:00:00Z";

fn row(id: &str, valor: &str, org: &str, org_name: &str, hash: &str) -> ProcedureUpsert {
    ProcedureUpsert {
        external_id: id.to_string(),
        name: format!("Trámite {id}"),
        description: format!("Descripción del trámite {id}"),
        organization_external_id: org.to_string(),
        organization_name: org_name.to_string(),
        official_url: format!("https://www.gub.uy/tramite/{id}"),
        content_hash: hash.to_string(),
        raw_data: serde_json::json!({ "id": id, "valor": valor }),
    }
}

async fn counts(pool: &sqlx::PgPool) -> (i64, i64, i64, i64) {
    let procedures: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE status = 'active') FROM procedures",
    )
    .fetch_one(pool)
    .await
    .expect("procedures counted");
    let versions: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE valid_until IS NULL) FROM procedure_versions",
    )
    .fetch_one(pool)
    .await
    .expect("versions counted");
    let orgs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations")
        .fetch_one(pool)
        .await
        .expect("organizations counted");
    (procedures.0, procedures.1, versions.0, orgs)
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_batch_creates_procedures_versions_and_organizations() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());

    let counts_out = repo
        .upsert_procedures(
            &[
                row("1001", "100", "O-1", "Ministerio", "hash-1001"),
                row("1002", "200", "O-2", "Intendencia", "hash-1002"),
            ],
            RUN_1.to_string(),
        )
        .expect("batch upserts");
    assert_eq!(counts_out.inserted, 2, "both rows are new");
    assert_eq!(counts_out.updated, 0);

    // Two active procedures, one open version each, one org per source oid.
    assert_eq!(counts(&pool).await, (2, 2, 2, 2));

    // first_seen/last_seen stamped at the run; soft-delete fields clear.
    let record: (String, String, Option<String>) = sqlx::query_as(
        "SELECT to_char(first_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(last_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(deactivated_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1001'",
    )
    .fetch_one(&pool)
    .await
    .expect("procedure 1001 exists");
    assert_eq!(record.0, "2026-09-18T03:00:00Z");
    assert_eq!(record.1, "2026-09-18T03:00:00Z");
    assert_eq!(record.2, None, "no deactivation stamp on a fresh row");

    // raw_data preserves the source payload (IN-8, D-4).
    let raw: serde_json::Value =
        sqlx::query_scalar("SELECT raw_data FROM procedures WHERE external_id = '1001'")
            .fetch_one(&pool)
            .await
            .expect("raw_data present");
    assert_eq!(raw, serde_json::json!({ "id": "1001", "valor": "100" }));

    // Organization mapped by source oid with the source name (IN-8).
    let org: (String, String) =
        sqlx::query_as("SELECT external_id, name FROM organizations WHERE external_id = 'O-1'")
            .fetch_one(&pool)
            .await
            .expect("organization row exists");
    assert_eq!(org, ("O-1".to_string(), "Ministerio".to_string()));

    // latest_hashes exposes the open hash per external id (IN-6 diff input).
    let hashes = repo.latest_hashes().expect("latest hashes");
    assert_eq!(hashes.len(), 2);
    assert_eq!(hashes.get("1001").map(String::as_str), Some("hash-1001"));

    // all_external_ids feeds export-ids: every id, sorted.
    let ids = repo.all_external_ids().expect("external ids");
    assert_eq!(ids, vec!["1001".to_string(), "1002".to_string()]);

    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unchanged_upsert_updates_row_but_creates_no_second_version() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_1.to_string(),
    )
    .expect("first upsert");

    // Same content re-upserted (as the pipeline never does for unchanged rows,
    // but the repository contract must still match the in-memory reference):
    // the row updates, no version opens (IN-9 at the repository layer).
    let counts_out = repo
        .upsert_procedures(
            &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
            RUN_2.to_string(),
        )
        .expect("second upsert");
    assert_eq!(counts_out.updated, 1);
    assert_eq!(counts(&pool).await, (1, 1, 1, 1), "no second version row");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_content_creates_one_version_and_closes_the_prior_at_the_run_stamp() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-v1")],
        RUN_1.to_string(),
    )
    .expect("v1 upsert");

    repo.upsert_procedures(
        &[row("1001", "150", "O-1", "Ministerio", "hash-v2")],
        RUN_2.to_string(),
    )
    .expect("v2 upsert");

    // Repository-layer semantics (in-memory reference parity): the upsert
    // opens exactly one new version; closing the predecessor is the separate
    // `close_versions` step the pipeline issues right after the batch.
    let (total, _open): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE valid_until IS NULL) FROM procedure_versions",
    )
    .fetch_one(&pool)
    .await
    .expect("versions counted");
    assert_eq!(total, 2, "exactly one new version row");

    repo.close_versions(
        &[("1001".to_string(), "hash-v1".to_string())],
        RUN_2.to_string(),
    )
    .expect("close call");
    let open: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM procedure_versions WHERE valid_until IS NULL")
            .fetch_one(&pool)
            .await
            .expect("open count");
    assert_eq!(open, 1, "only the new version stays open after closing");

    let prior: (String, Option<String>) = sqlx::query_as(
        "SELECT to_char(valid_from, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(valid_until, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedure_versions WHERE content_hash = 'hash-v1'",
    )
    .fetch_one(&pool)
    .await
    .expect("prior version exists");
    assert_eq!(prior.0, "2026-09-18T03:00:00Z");
    assert_eq!(
        prior.1,
        Some("2026-09-18T04:00:00Z".to_string()),
        "prior version closed at run 2 (DM-3)"
    );
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn close_versions_closes_only_the_matching_open_version() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-v1")],
        RUN_1.to_string(),
    )
    .expect("v1 upsert");

    repo.close_versions(
        &[("1001".to_string(), "hash-v1".to_string())],
        RUN_2.to_string(),
    )
    .expect("close call");
    let open: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM procedure_versions WHERE valid_until IS NULL")
            .fetch_one(&pool)
            .await
            .expect("open count");
    assert_eq!(open, 0, "the open version is closed at the run stamp");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deactivate_missing_marks_absent_inactive_and_never_deletes() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[
            row("1001", "100", "O-1", "Ministerio", "hash-1001"),
            row("1002", "200", "O-2", "Intendencia", "hash-1002"),
        ],
        RUN_1.to_string(),
    )
    .expect("initial batch");

    let deactivated = repo
        .deactivate_missing(
            &["1001".to_string()].into_iter().collect(),
            RUN_2.to_string(),
        )
        .expect("deactivate call");
    assert_eq!(deactivated, 1, "only 1002 is absent from the source");

    let absent: (String, Option<String>) = sqlx::query_as(
        "SELECT status, to_char(deactivated_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1002'",
    )
    .fetch_one(&pool)
    .await
    .expect("row still exists (never deleted)");
    assert_eq!(absent.0, "inactive");
    assert_eq!(absent.1, Some("2026-09-18T04:00:00Z".to_string()));

    // Idempotent: an already-inactive row is not deactivated twice.
    let again = repo
        .deactivate_missing(
            &["1001".to_string()].into_iter().collect(),
            RUN_2.to_string(),
        )
        .expect("second deactivate call");
    assert_eq!(again, 0, "soft delete flips each row once");
    assert_eq!(counts(&pool).await.0, 2, "no row was ever deleted");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_reactivates_an_inactive_row_without_opening_a_version() {
    // B3 recorded edge: a row present in the source is re-activated in place
    // (status back to active, deactivation stamp cleared); unchanged content
    // opens no new version.
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_1.to_string(),
    )
    .expect("initial upsert");
    repo.deactivate_missing(&Default::default(), RUN_2.to_string())
        .expect("deactivate all");

    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_2.to_string(),
    )
    .expect("re-upsert");

    let state: (String, Option<String>) = sqlx::query_as(
        "SELECT status, to_char(deactivated_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1001'",
    )
    .fetch_one(&pool)
    .await
    .expect("row exists");
    assert_eq!(state.0, "active", "present row re-activated");
    assert_eq!(state.1, None, "stale deactivation stamp cleared");
    assert_eq!(
        counts(&pool).await.2,
        1,
        "unchanged content opens no version"
    );
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn touch_last_seen_advances_only_last_seen() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_1.to_string(),
    )
    .expect("initial upsert");

    repo.touch_last_seen(&["1001".to_string()], RUN_2.to_string())
        .expect("touch call");

    let seen: (String, String) = sqlx::query_as(
        "SELECT to_char(first_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(last_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1001'",
    )
    .fetch_one(&pool)
    .await
    .expect("row exists");
    assert_eq!(seen.0, "2026-09-18T03:00:00Z", "first_seen_at preserved");
    assert_eq!(seen.1, "2026-09-18T04:00:00Z", "last_seen_at advanced");

    // The stamp conversion boundary: a malformed RunStamp is a typed error.
    let err = repo
        .touch_last_seen(&["1001".to_string()], "not-a-stamp".to_string())
        .expect_err("malformed run stamp rejected");
    assert!(
        err.to_string().contains("run stamp"),
        "error names the malformed run stamp: {err}"
    );
    drop_test_db(&name).await;
}
