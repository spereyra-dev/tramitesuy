//! DM-1 RED contract: the embedded migrations build the schema, exactly the
//! ten specced application tables exist, and the migrations create no
//! extensions (portability, design §6 / D-6).

mod common;

use sqlx::Row;

#[tokio::test]
async fn migrations_create_exactly_the_ten_specified_tables() {
    let (pool, name) = common::fresh_migrated_db().await;

    // Explicit allowlist: exactly the ten DM-1 tables (sqlx's internal
    // `_sqlx_migrations` bookkeeping table is the only extra, non-application
    // table permitted in a migrated database).
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
        "schema after migrations must contain exactly the ten specced tables (+ _sqlx_migrations); no other application table may exist"
    );

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
