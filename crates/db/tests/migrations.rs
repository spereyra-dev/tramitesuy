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
