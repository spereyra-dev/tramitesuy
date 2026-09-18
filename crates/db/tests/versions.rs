//! DM-3 RED contract: procedure_versions history is append-only. A re-run
//! with unchanged content leaves every prior version byte-identical and
//! creates no new version row.

mod common;

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
