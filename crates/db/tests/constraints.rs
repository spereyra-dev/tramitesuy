//! DM-2 RED contract: the schema's FK and uniqueness constraints are enforced
//! by the database. Fixture insert helpers live in `common`.

mod common;

use common::{is_fk_violation, is_unique_violation};

fn is_check_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23514"))
}

#[tokio::test]
async fn domain_check_constraints_are_enforced() {
    let (pool, name) = common::fresh_migrated_db().await;
    let seed = common::seed_minimal(&pool).await;

    // procedures.status CHECK in active|inactive
    let err = sqlx::query(
        "INSERT INTO procedures (external_id, name, organization_id, status) VALUES ('x2', 'X', $1, 'archived')",
    )
    .bind(seed.organization_id)
    .execute(&pool)
    .await
    .expect_err("status 'archived' must be rejected");
    assert!(
        is_check_violation(&err),
        "procedures.status CHECK not enforced: {err:?}"
    );

    // life_event_keywords.type CHECK in ACTION|ENTITY|MODIFIER|CONTEXT
    let err = sqlx::query(
        "INSERT INTO life_event_keywords (life_event_id, term, type, weight) VALUES ($1, 'x', 'VERB', 5)",
    )
    .bind(seed.event_id)
    .execute(&pool)
    .await
    .expect_err("keyword type 'VERB' must be rejected");
    assert!(
        is_check_violation(&err),
        "keyword type CHECK not enforced: {err:?}"
    );

    // life_event_keywords.weight > 0
    let err = sqlx::query(
        "INSERT INTO life_event_keywords (life_event_id, term, type, weight) VALUES ($1, 'x', 'ACTION', 0)",
    )
    .bind(seed.event_id)
    .execute(&pool)
    .await
    .expect_err("weight 0 must be rejected");
    assert!(
        is_check_violation(&err),
        "weight > 0 CHECK not enforced: {err:?}"
    );

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn migrations_are_idempotent_when_rerun() {
    let (pool, name) = common::fresh_migrated_db().await;
    let tables_before_rerun: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tablename != '_sqlx_migrations'",
    )
    .fetch_one(&pool)
    .await
    .expect("count application tables before rerun");
    assert_eq!(
        tables_before_rerun, 11,
        "the embedded migration set must create all expected application tables"
    );

    // A second full application must be a no-op (sqlx tracks versions).
    common::apply_migrations(&pool).await;
    let tables_after_rerun: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tablename != '_sqlx_migrations'",
    )
    .fetch_one(&pool)
    .await
    .expect("count application tables after rerun");
    assert_eq!(
        tables_after_rerun, tables_before_rerun,
        "re-running migrations must not change the application table inventory"
    );
    common::drop_test_db(&name).await;
}

/// Nil uuid: guaranteed absent as an FK target in the fixtures (audited).
const MISSING: sqlx::types::Uuid = sqlx::types::Uuid::nil();

#[tokio::test]
async fn duplicate_life_event_procedure_pair_is_rejected() {
    let (pool, name) = common::fresh_migrated_db().await;
    let seed = common::seed_minimal(&pool).await;

    let result = sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) VALUES ($1, $2, 2, TRUE)",
    )
    .bind(seed.event_id)
    .bind(seed.procedure_id)
    .execute(&pool)
    .await;

    let err = result.expect_err("duplicate (life_event_id, procedure_id) pair must be rejected");
    assert!(
        is_unique_violation(&err),
        "expected a uniqueness violation, got: {err:?}"
    );

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn second_active_procedure_with_same_external_id_is_rejected() {
    let (pool, name) = common::fresh_migrated_db().await;
    let seed = common::seed_minimal(&pool).await;

    let result = sqlx::query(
        "INSERT INTO procedures (external_id, name, organization_id, status) VALUES ('100001', 'Duplicado', $1, 'active')",
    )
    .bind(seed.organization_id)
    .execute(&pool)
    .await;

    let err =
        result.expect_err("a second active procedure with the same external_id must be rejected");
    assert!(
        is_unique_violation(&err),
        "expected a uniqueness violation, got: {err:?}"
    );

    // The soft-delete path must free the external_id for a new active row:
    // the partial unique index only covers status='active'.
    sqlx::query(
        "UPDATE procedures SET status='inactive', deactivated_at=now() WHERE external_id='100001'",
    )
    .execute(&pool)
    .await
    .expect("deactivate original procedure");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, organization_id, status) VALUES ('100001', 'Reencarnado', $1, 'active')",
    )
    .bind(seed.organization_id)
    .execute(&pool)
    .await
    .expect("a fresh active procedure may reuse the external_id of an inactive one");

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn duplicate_open_content_hash_for_same_procedure_is_rejected() {
    let (pool, name) = common::fresh_migrated_db().await;
    let seed = common::seed_minimal(&pool).await;

    // The seeded version is open (valid_until NULL) with hash 'hash-a';
    // a second open version with the same hash for the same procedure must
    // violate the unique (procedure_id, content_hash) constraint.
    let result = sqlx::query(
        "INSERT INTO procedure_versions (procedure_id, content_hash, payload) VALUES ($1, 'hash-a', '{}'::jsonb)",
    )
    .bind(seed.procedure_id)
    .execute(&pool)
    .await;

    let err =
        result.expect_err("a duplicate open content_hash for the same procedure must be rejected");
    assert!(
        is_unique_violation(&err),
        "expected a uniqueness violation, got: {err:?}"
    );

    // Closing the open version frees the hash pair for a new version row.
    sqlx::query("UPDATE procedure_versions SET valid_until=now() WHERE id=$1")
        .bind(seed.version_id)
        .execute(&pool)
        .await
        .expect("close the open version");
    sqlx::query(
        "INSERT INTO procedure_versions (procedure_id, content_hash, payload) VALUES ($1, 'hash-a', '{}'::jsonb)",
    )
    .bind(seed.procedure_id)
    .execute(&pool)
    .await
    .expect("the same hash is allowed again once the prior open version is closed");

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn every_cross_table_reference_is_a_real_foreign_key() {
    let (pool, name) = common::fresh_migrated_db().await;
    let seed = common::seed_minimal(&pool).await;

    // life_events.category_id → categories
    let err =
        sqlx::query("INSERT INTO life_events (slug, name, category_id) VALUES ('x', 'X', $1)")
            .bind(MISSING)
            .execute(&pool)
            .await
            .expect_err("nonexistent category_id must be rejected");
    assert!(
        is_fk_violation(&err),
        "life_events.category_id is not enforced as FK: {err:?}"
    );

    // life_event_keywords.life_event_id → life_events (ON DELETE CASCADE)
    let err = sqlx::query(
        "INSERT INTO life_event_keywords (life_event_id, term, type, weight) VALUES ($1, 'comprar', 'ACTION', 10)",
    )
    .bind(MISSING)
    .execute(&pool)
    .await
    .expect_err("nonexistent life_event_id must be rejected");
    assert!(
        is_fk_violation(&err),
        "life_event_keywords.life_event_id is not enforced as FK: {err:?}"
    );

    // procedures.organization_id → organizations
    let err = sqlx::query(
        "INSERT INTO procedures (external_id, name, organization_id, status) VALUES ('x1', 'X', $1, 'active')",
    )
    .bind(MISSING)
    .execute(&pool)
    .await
    .expect_err("nonexistent organization_id must be rejected");
    assert!(
        is_fk_violation(&err),
        "procedures.organization_id is not enforced as FK: {err:?}"
    );

    // procedure_versions.procedure_id → procedures
    let err = sqlx::query(
        "INSERT INTO procedure_versions (procedure_id, content_hash, payload) VALUES ($1, 'h', '{}'::jsonb)",
    )
    .bind(MISSING)
    .execute(&pool)
    .await
    .expect_err("nonexistent procedure_id must be rejected");
    assert!(
        is_fk_violation(&err),
        "procedure_versions.procedure_id is not enforced as FK: {err:?}"
    );

    // life_event_procedures → life_events and → procedures
    let err = sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) VALUES ($1, $2, 1, TRUE)",
    )
    .bind(MISSING)
    .bind(seed.procedure_id)
    .execute(&pool)
    .await
    .expect_err("nonexistent life_event_id in relation must be rejected");
    assert!(
        is_fk_violation(&err),
        "life_event_procedures.life_event_id is not enforced as FK: {err:?}"
    );
    let err = sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) VALUES ($1, $2, 1, TRUE)",
    )
    .bind(seed.event_id)
    .bind(MISSING)
    .execute(&pool)
    .await
    .expect_err("nonexistent procedure_id in relation must be rejected");
    assert!(
        is_fk_violation(&err),
        "life_event_procedures.procedure_id is not enforced as FK: {err:?}"
    );

    // search_logs.event FKs → life_events (nullable)
    let err = sqlx::query("INSERT INTO search_logs (query, normalized_query, selected_event_id) VALUES ('q', 'q', $1)")
        .bind(MISSING)
        .execute(&pool)
        .await
        .expect_err("nonexistent selected_event_id must be rejected");
    assert!(
        is_fk_violation(&err),
        "search_logs.selected_event_id is not enforced as FK: {err:?}"
    );

    // search_feedback → search_logs and → life_events
    let err = sqlx::query(
        "INSERT INTO search_feedback (search_log_id, event_id, correct) VALUES ($1, $2, TRUE)",
    )
    .bind(MISSING)
    .bind(seed.event_id)
    .execute(&pool)
    .await
    .expect_err("nonexistent search_log_id must be rejected");
    assert!(
        is_fk_violation(&err),
        "search_feedback.search_log_id is not enforced as FK: {err:?}"
    );
    let err = sqlx::query(
        "INSERT INTO search_feedback (search_log_id, event_id, correct) VALUES ($1, $2, TRUE)",
    )
    .bind(seed.log_id)
    .bind(MISSING)
    .execute(&pool)
    .await
    .expect_err("nonexistent feedback event_id must be rejected");
    assert!(
        is_fk_violation(&err),
        "search_feedback.event_id is not enforced as FK: {err:?}"
    );

    // Prove CASCADE on life_event_keywords (specced behavior).
    sqlx::query("INSERT INTO life_event_keywords (life_event_id, term, type, weight) VALUES ($1, 'comprar', 'ACTION', 10)")
        .bind(seed.event_id)
        .execute(&pool)
        .await
        .expect("insert keyword");
    sqlx::query("DELETE FROM life_events WHERE id = $1")
        .bind(seed.event_id)
        .execute(&pool)
        .await
        .expect("delete event");
    let keywords: i64 =
        sqlx::query_scalar("SELECT count(*) FROM life_event_keywords WHERE life_event_id = $1")
            .bind(seed.event_id)
            .fetch_one(&pool)
            .await
            .expect("count keywords after event delete");
    assert_eq!(keywords, 0, "keywords must cascade with their life_event");

    common::drop_test_db(&name).await;
}
