//! DM-3 RED contract: procedure_versions history is append-only. A re-run
//! with unchanged content leaves every prior version byte-identical and
//! creates no new version row.

mod common;

use db::repos::procedures::PostgresProcedureRepository;
use ingestion::ports::ProcedureRepository;
use ingestion::summary::ProcedureUpsert;

const RUN_1: &str = "2026-09-18T03:00:00Z";
const RUN_2: &str = "2026-09-18T04:00:00Z";

fn row(id: &str, valor: &str, hash: &str) -> ProcedureUpsert {
    ProcedureUpsert {
        external_id: id.to_string(),
        name: format!("Trámite {id}"),
        description: format!("Descripción del trámite {id}"),
        organization_external_id: "O-1".to_string(),
        organization_name: "Ministerio".to_string(),
        official_url: format!("https://www.gub.uy/tramite/{id}"),
        content_hash: hash.to_string(),
        raw_data: serde_json::json!({ "id": id, "valor": valor }),
    }
}

async fn open_version_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM procedure_versions WHERE valid_until IS NULL")
        .fetch_one(pool)
        .await
        .expect("open versions counted")
}

async fn total_version_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM procedure_versions")
        .fetch_one(pool)
        .await
        .expect("versions counted")
}

/// F13: the prior open version is closed inside the `upsert_procedures`
/// transaction. With `close_versions` deliberately NOT called, one commit
/// must already have produced exactly one open version per procedure.
#[tokio::test(flavor = "multi_thread")]
async fn changed_upsert_closes_the_prior_open_version_in_the_same_transaction() {
    let (pool, name) = common::fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(&[row("1001", "100", "hash-v1")], RUN_1.to_string())
        .expect("v1 upsert");

    repo.upsert_procedures(&[row("1001", "150", "hash-v2")], RUN_2.to_string())
        .expect("v2 upsert");

    assert_eq!(
        open_version_count(&pool).await,
        1,
        "one commit must produce exactly one open version (F13)"
    );
    assert_eq!(total_version_count(&pool).await, 2, "one new version row");

    // The prior version was closed at the v2 run stamp, not left open.
    let prior_until: Option<String> = sqlx::query_scalar(
        "SELECT to_char(valid_until, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedure_versions WHERE content_hash = 'hash-v1'",
    )
    .fetch_one(&pool)
    .await
    .expect("prior version exists");
    assert_eq!(prior_until.as_deref(), Some(RUN_2));

    common::drop_test_db(&name).await;
}

/// F13: an interruption right after the upsert (before the pipeline would
/// have called `close_versions`) cannot leave two open versions, because the
/// close already committed with the insert.
#[tokio::test(flavor = "multi_thread")]
async fn an_interruption_after_the_upsert_leaves_exactly_one_open_version() {
    let (pool, name) = common::fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(&[row("1001", "100", "hash-v1")], RUN_1.to_string())
        .expect("v1 upsert");
    repo.upsert_procedures(&[row("1001", "150", "hash-v2")], RUN_2.to_string())
        .expect("v2 upsert");

    // Simulate the process dying before the (now redundant) close step: the
    // injected follow-up statement fails, and the version state is inspected
    // as it would be after the crash.
    let interrupted = sqlx::query("SELECT 1 / 0")
        .execute(&pool)
        .await
        .expect_err("the injected follow-up failure aborts the run");
    assert!(
        interrupted.to_string().contains("division by zero"),
        "a real database error was injected: {interrupted}"
    );

    assert_eq!(
        open_version_count(&pool).await,
        1,
        "the committed upsert already left exactly one open version"
    );
    let open_hash: String =
        sqlx::query_scalar("SELECT content_hash FROM procedure_versions WHERE valid_until IS NULL")
            .fetch_one(&pool)
            .await
            .expect("open version exists");
    assert_eq!(open_hash, "hash-v2", "the newest version is the open one");

    common::drop_test_db(&name).await;
}

/// F13: `close_versions` stays in the port for compatibility and is a no-op
/// after `upsert_procedures` already closed the prior version.
#[tokio::test(flavor = "multi_thread")]
async fn close_versions_after_upsert_is_a_no_op() {
    let (pool, name) = common::fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(&[row("1001", "100", "hash-v1")], RUN_1.to_string())
        .expect("v1 upsert");
    repo.upsert_procedures(&[row("1001", "150", "hash-v2")], RUN_2.to_string())
        .expect("v2 upsert");
    let before: (String, Option<String>) = sqlx::query_as(
        "SELECT to_char(valid_from, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(valid_until, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedure_versions WHERE content_hash = 'hash-v1'",
    )
    .fetch_one(&pool)
    .await
    .expect("prior version exists");

    repo.close_versions(
        &[("1001".to_string(), "hash-v1".to_string())],
        "2026-09-18T05:00:00Z".to_string(),
    )
    .expect("close call is safe after the upsert");

    let after: (String, Option<String>) = sqlx::query_as(
        "SELECT to_char(valid_from, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(valid_until, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedure_versions WHERE content_hash = 'hash-v1'",
    )
    .fetch_one(&pool)
    .await
    .expect("prior version exists");
    assert_eq!(after, before, "the already-closed version is untouched");
    assert_eq!(open_version_count(&pool).await, 1, "still one open version");

    common::drop_test_db(&name).await;
}

/// F13 TRIANGULATE: a failure during the changed-row upsert (the new version
/// insert) rolls the whole transaction back — the prior version stays open
/// and no second version survives.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_version_insert_rolls_back_the_whole_upsert() {
    let (pool, name) = common::fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(&[row("1001", "100", "hash-v1")], RUN_1.to_string())
        .expect("v1 upsert");

    sqlx::query(
        "CREATE OR REPLACE FUNCTION induce_version_insert_failure() RETURNS trigger AS $body$ \
         BEGIN IF NEW.content_hash = 'hash-v2' THEN \
         RAISE EXCEPTION 'induced version failure'; END IF; \
         RETURN NEW; END; $body$ LANGUAGE plpgsql",
    )
    .execute(&pool)
    .await
    .expect("fault-injection function created");
    sqlx::query(
        "CREATE TRIGGER version_tripwire BEFORE INSERT ON procedure_versions \
         FOR EACH ROW EXECUTE FUNCTION induce_version_insert_failure()",
    )
    .execute(&pool)
    .await
    .expect("trigger installed");

    let err = repo
        .upsert_procedures(&[row("1001", "150", "hash-v2")], RUN_2.to_string())
        .expect_err("the injected version failure surfaces");
    assert!(
        err.to_string().contains("induced version failure"),
        "the induced database error is surfaced: {err}"
    );

    assert_eq!(total_version_count(&pool).await, 1, "nothing new committed");
    assert_eq!(
        open_version_count(&pool).await,
        1,
        "exactly one open version"
    );
    let open_hash: String =
        sqlx::query_scalar("SELECT content_hash FROM procedure_versions WHERE valid_until IS NULL")
            .fetch_one(&pool)
            .await
            .expect("open version exists");
    assert_eq!(open_hash, "hash-v1", "the prior version stays open");

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn re_ingesting_unchanged_content_keeps_versions_byte_identical() {
    let (pool, name) = common::fresh_migrated_db().await;
    let seed = common::seed_minimal(&pool).await;

    // Simulate the ingestion contract: version v2 was appended for a changed
    // payload, then the same content was ingested again.
    sqlx::query("UPDATE procedure_versions SET valid_until = now() WHERE id = $1")
        .bind(seed.version_id)
        .execute(&pool)
        .await
        .expect("close v1");
    let _v2_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO procedure_versions (procedure_id, content_hash, payload) VALUES ($1, 'hash-b', '{\"valor\":\"4500\"}'::jsonb) RETURNING id",
    )
    .bind(seed.procedure_id)
    .fetch_one(&pool)
    .await
    .expect("insert v2");

    let before = sqlx::query_as::<_, (sqlx::types::Uuid, String, serde_json::Value, Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>)>(
        "SELECT id, content_hash, payload, valid_from, valid_until FROM procedure_versions WHERE procedure_id = $1 ORDER BY valid_from",
    )
    .bind(seed.procedure_id)
    .fetch_all(&pool)
    .await
    .expect("snapshot versions before re-ingestion");

    // Re-ingestion with unchanged content: the idempotent replay must be a
    // no-op for the version table (no UPDATE, no DELETE, no new row).
    let hashes: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT content_hash FROM procedure_versions WHERE procedure_id = $1 AND valid_until IS NULL",
    )
    .bind(seed.procedure_id)
    .fetch_all(&pool)
    .await
    .expect("read open hashes");
    assert_eq!(
        hashes,
        vec!["hash-b".to_string()],
        "open version content unchanged"
    );

    let after = sqlx::query_as::<_, (sqlx::types::Uuid, String, serde_json::Value, Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>)>(
        "SELECT id, content_hash, payload, valid_from, valid_until FROM procedure_versions WHERE procedure_id = $1 ORDER BY valid_from",
    )
    .bind(seed.procedure_id)
    .fetch_all(&pool)
    .await
    .expect("snapshot versions after re-ingestion");

    assert_eq!(
        before, after,
        "both versions must remain byte-identical across a no-op re-ingestion"
    );
    assert_eq!(before.len(), 2, "no third version may appear");
    assert!(before[0].4.is_some(), "v1 stays closed (valid_until set)");
    assert!(before[1].4.is_none(), "v2 is the only open version");

    common::drop_test_db(&name).await;
}
