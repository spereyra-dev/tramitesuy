//! Migration contracts: the embedded migrations preserve the ten base tables,
//! add catalog-generation projections, and create no extensions (portability,
//! design §6 / D-6).

mod common;

use sqlx::Row;

#[tokio::test]
async fn migrations_preserve_base_tables_and_add_catalog_generation_manifest() {
    let (pool, name) = common::fresh_migrated_db().await;

    // Explicit allowlist: the ten DM-1 tables plus the additive catalog
    // generation manifest (and sqlx's internal bookkeeping table).
    let mut expected: Vec<String> = vec![
        "categories",
        "organizations",
        "life_events",
        "life_event_keywords",
        "procedures",
        "procedure_versions",
        "life_event_procedures",
        "synonyms",
        "search_logs",
        "search_feedback",
        "catalog_generations",
        "ingestion_runs",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    expected.push("_sqlx_migrations".to_string());
    expected.sort();

    let mut actual: Vec<String> = sqlx::query(
        "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
    )
    .fetch_all(&pool)
    .await
    .expect("list tables")
    .into_iter()
    .map(|row| row.get::<String, _>(0))
    .collect();
    actual.sort();

    assert_eq!(
        actual, expected,
        "schema after migrations must preserve the ten base tables and add only the catalog-generation manifest"
    );

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn catalog_generation_manifest_has_required_columns_and_status_constraint() {
    let (pool, name) = common::fresh_migrated_db().await;

    let mut columns: Vec<String> = sqlx::query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'catalog_generations' \
         ORDER BY ordinal_position",
    )
    .fetch_all(&pool)
    .await
    .expect("list catalog generation columns")
    .into_iter()
    .map(|row| row.get::<String, _>(0))
    .collect();
    columns.sort();

    let mut expected = vec![
        "generation_id",
        "status",
        "content_hash",
        "taxonomy_version",
        "engine_version",
        "source_synced_at",
        "created_at",
        "published_at",
        "retired_at",
        "event_count",
        "procedure_count",
        "projection_status",
        "active_generation_id",
        "adopted_at",
    ];
    expected.sort();
    assert_eq!(
        columns, expected,
        "manifest columns must match the contract"
    );

    let generation_id_type: String = sqlx::query_scalar(
        "SELECT data_type FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'catalog_generations' \
         AND column_name = 'generation_id'",
    )
    .fetch_one(&pool)
    .await
    .expect("read generation ID type");
    assert_eq!(generation_id_type, "uuid", "generation ID must be a UUID");

    let invalid_status = sqlx::query(
        "INSERT INTO catalog_generations \
         (generation_id, status, content_hash, taxonomy_version, engine_version, source_synced_at, event_count, procedure_count, projection_status) \
         VALUES (gen_random_uuid(), 'invalid', 'hash', 'taxonomy', 'engine', now(), 0, 0, 'pending')",
    )
    .execute(&pool)
    .await
    .expect_err("manifest status constraint must reject values outside the lifecycle");
    assert!(
        matches!(&invalid_status, sqlx::Error::Database(db) if db.code().as_deref() == Some("23514")),
        "unexpected error for invalid manifest status: {invalid_status:?}"
    );

    common::apply_migrations(&pool).await;
    let applied: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE version = 13")
            .fetch_one(&pool)
            .await
            .expect("count manifest migration records");
    assert_eq!(applied, 1, "re-running migrations must not reapply 0013");

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn ingestion_runs_enforce_contract_and_preserve_committed_records_on_rollback() {
    let (pool, name) = common::fresh_migrated_db().await;

    let mut columns: Vec<String> = sqlx::query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'ingestion_runs' \
         ORDER BY ordinal_position",
    )
    .fetch_all(&pool)
    .await
    .expect("list ingestion run columns")
    .into_iter()
    .map(|row| row.get::<String, _>(0))
    .collect();
    columns.sort();

    let mut expected = vec![
        "run_id",
        "trigger",
        "started_at",
        "finished_at",
        "status",
        "counts",
        "candidate_generation_id",
        "published_generation_id",
        "attempt",
    ];
    expected.sort();
    assert_eq!(
        columns, expected,
        "run-record columns must match the contract"
    );

    let generation_reference_columns: Vec<(String, String, String)> = sqlx::query(
        "SELECT column_name, data_type, is_nullable FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'ingestion_runs' \
         AND column_name IN ('counts', 'candidate_generation_id', 'published_generation_id') \
         ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .expect("read ingestion run contract column metadata")
    .into_iter()
    .map(|row| {
        (
            row.get("column_name"),
            row.get("data_type"),
            row.get("is_nullable"),
        )
    })
    .collect();
    assert_eq!(
        generation_reference_columns,
        vec![
            (
                "candidate_generation_id".to_string(),
                "uuid".to_string(),
                "YES".to_string()
            ),
            ("counts".to_string(), "jsonb".to_string(), "NO".to_string()),
            (
                "published_generation_id".to_string(),
                "uuid".to_string(),
                "YES".to_string()
            ),
        ],
        "counts must be JSONB and generation references must remain nullable"
    );

    let candidate_generation_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO catalog_generations \
         (generation_id, content_hash, taxonomy_version, engine_version, source_synced_at, event_count, procedure_count, projection_status) \
         VALUES (gen_random_uuid(), 'candidate-hash', 'taxonomy', 'engine', now(), 0, 0, 'pending') \
         RETURNING generation_id",
    )
    .fetch_one(&pool)
    .await
    .expect("create candidate generation");
    let published_generation_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO catalog_generations \
         (generation_id, status, content_hash, taxonomy_version, engine_version, source_synced_at, event_count, procedure_count, projection_status) \
         VALUES (gen_random_uuid(), 'published', 'published-hash', 'taxonomy', 'engine', now(), 0, 0, 'complete') \
         RETURNING generation_id",
    )
    .fetch_one(&pool)
    .await
    .expect("create published generation");

    let run_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO ingestion_runs \
         (run_id, trigger, finished_at, status, counts, candidate_generation_id, published_generation_id, attempt) \
         VALUES (gen_random_uuid(), 'manual', now(), 'skipped', '{\"read\": 0}'::jsonb, $1, $2, 1) \
         RETURNING run_id",
    )
    .bind(candidate_generation_id)
    .bind(published_generation_id)
    .fetch_one(&pool)
    .await
    .expect("a skipped run record is accepted");

    let invalid_trigger = sqlx::query(
        "INSERT INTO ingestion_runs (run_id, trigger, status, counts, attempt) \
         VALUES (gen_random_uuid(), 'invalid', 'skipped', '{}'::jsonb, 1)",
    )
    .execute(&pool)
    .await
    .expect_err("trigger constraint must reject values outside the contract");
    assert!(
        matches!(&invalid_trigger, sqlx::Error::Database(db) if db.code().as_deref() == Some("23514")),
        "unexpected error for invalid trigger: {invalid_trigger:?}"
    );

    let invalid_attempt = sqlx::query(
        "INSERT INTO ingestion_runs (run_id, trigger, status, counts, attempt) \
         VALUES (gen_random_uuid(), 'manual', 'skipped', '{}'::jsonb, 4)",
    )
    .execute(&pool)
    .await
    .expect_err("attempt constraint must reject values outside 1..3");
    assert!(
        matches!(&invalid_attempt, sqlx::Error::Database(db) if db.code().as_deref() == Some("23514")),
        "unexpected error for invalid attempt: {invalid_attempt:?}"
    );

    let missing_generation = sqlx::query(
        "INSERT INTO ingestion_runs \
         (run_id, trigger, status, counts, candidate_generation_id, attempt) \
         VALUES (gen_random_uuid(), 'manual', 'skipped', '{}'::jsonb, gen_random_uuid(), 1)",
    )
    .execute(&pool)
    .await
    .expect_err("generation references must be foreign keys");
    assert!(
        common::is_fk_violation(&missing_generation),
        "unexpected error for missing generation: {missing_generation:?}"
    );

    let missing_published_generation = sqlx::query(
        "INSERT INTO ingestion_runs \
         (run_id, trigger, status, counts, published_generation_id, attempt) \
         VALUES (gen_random_uuid(), 'manual', 'skipped', '{}'::jsonb, gen_random_uuid(), 1)",
    )
    .execute(&pool)
    .await
    .expect_err("published generation reference must be a foreign key");
    assert!(
        common::is_fk_violation(&missing_published_generation),
        "unexpected error for missing published generation: {missing_published_generation:?}"
    );

    let mut transaction = pool.begin().await.expect("begin run update transaction");
    sqlx::query("UPDATE ingestion_runs SET status = 'failed', counts = '{\"read\": 1}'::jsonb WHERE run_id = $1")
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .expect("stage partial run update");
    transaction
        .rollback()
        .await
        .expect("roll back partial run update");

    let (status, counts): (String, serde_json::Value) =
        sqlx::query_as("SELECT status, counts FROM ingestion_runs WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .expect("committed run record survives rollback");
    assert_eq!(status, "skipped");
    assert_eq!(counts, serde_json::json!({"read": 0}));

    common::drop_test_db(&name).await;
}


#[tokio::test]
async fn migrations_create_no_extensions() {
    let (pool, name) = common::fresh_provisioned_db().await;
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_extension")
        .fetch_one(&pool)
        .await
        .expect("count extensions before migrations");
    common::apply_migrations(&pool).await;
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_extension")
        .fetch_one(&pool)
        .await
        .expect("count extensions after migrations");
    assert_eq!(
        before, after,
        "migrations must create no extensions; pg_trgm/unaccent stay in docker/init/01-extensions.sql (D-6 portability)"
    );
    common::drop_test_db(&name).await;
}
