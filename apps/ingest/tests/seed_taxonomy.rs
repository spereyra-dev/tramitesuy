//! Task 69 (TX-6, DM-1, D-6): `ingest seed-taxonomy` loads the YAML taxonomy
//! and projects it into the database tables — categories, events, keywords,
//! synonyms, and relations with `order_index` preserved — idempotent per
//! slug on a second run. Integration against the compose Postgres via a
//! scratch database. The real `data/` seed (9 Vehículos events, provisional
//! external ids 100001–100022) is used end to end, with procedures seeded
//! from the committed snapshot so every relation finds its FK target.

mod common;

use std::path::Path;
use std::process::Command;

fn workspace_root() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../..")
}

fn seed_taxonomy_command(database_url: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ingest"));
    command
        .arg("seed-taxonomy")
        .arg("--data-dir")
        .arg(format!("{}/data", workspace_root()))
        .arg("--snapshot")
        .arg(format!(
            "{}/data/external_ids.snapshot.txt",
            workspace_root()
        ))
        .arg("--database-url")
        .arg(database_url);
    command
}

async fn relation_orders(pool: &sqlx::PgPool, event_slug: &str) -> Vec<i32> {
    sqlx::query_scalar(
        "SELECT r.order_index FROM life_event_procedures r \
         JOIN life_events e ON e.id = r.life_event_id \
         WHERE e.slug = $1 ORDER BY r.order_index",
    )
    .bind(event_slug)
    .fetch_all(pool)
    .await
    .expect("relations readable")
}

#[tokio::test(flavor = "multi_thread")]
async fn seed_taxonomy_writes_every_projection_and_is_idempotent() {
    let (pool, db_name) = common::fresh_migrated_db().await;
    let url = format!("postgres://postgres:postgres@localhost:5432/{db_name}");

    // Procedures first: the relations reference the provisional external
    // ids from the committed snapshot (task 68 blocker).
    common::seed_procedures_from_snapshot(
        &pool,
        Path::new(&format!(
            "{}/data/external_ids.snapshot.txt",
            workspace_root()
        )),
    )
    .await;

    // First run: everything is written.
    let first = seed_taxonomy_command(&url).output().expect("binary runs");
    assert!(
        first.status.success(),
        "first seed must succeed; stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let category_count: i64 = sqlx::query_scalar("SELECT count(*) FROM categories")
        .fetch_one(&pool)
        .await
        .expect("count");
    let event_count: i64 = sqlx::query_scalar("SELECT count(*) FROM life_events")
        .fetch_one(&pool)
        .await
        .expect("count");
    let keyword_count: i64 = sqlx::query_scalar("SELECT count(*) FROM life_event_keywords")
        .fetch_one(&pool)
        .await
        .expect("count");
    let synonym_count: i64 = sqlx::query_scalar("SELECT count(*) FROM synonyms")
        .fetch_one(&pool)
        .await
        .expect("count");
    let relation_count: i64 = sqlx::query_scalar("SELECT count(*) FROM life_event_procedures")
        .fetch_one(&pool)
        .await
        .expect("count");

    assert_eq!(category_count, 1, "the vehiculos category must be seeded");
    assert_eq!(
        event_count, 9,
        "all nine Vehículos events must be seeded (TX-5)"
    );
    assert!(keyword_count > 0, "typed keywords must be projected");
    assert_eq!(
        synonym_count, 14,
        "the synonyms projection must match the YAML seed"
    );
    assert!(
        relation_count > 0,
        "relations must be written when procedures exist"
    );

    // order_index preserved per event (TX-6): vender-vehiculo declares
    // relations at orders 1..3 with required flags.
    assert_eq!(
        relation_orders(&pool, "vender-vehiculo").await,
        vec![1, 2, 3]
    );
    let required_flags: Vec<bool> = sqlx::query_scalar(
        "SELECT r.required FROM life_event_procedures r \
         JOIN life_events e ON e.id = r.life_event_id \
         WHERE e.slug = 'vender-vehiculo' ORDER BY r.order_index",
    )
    .fetch_all(&pool)
    .await
    .expect("flags");
    assert_eq!(
        required_flags,
        vec![true, false, false],
        "required flags must be preserved (TX-6)"
    );

    // Second run: idempotent per slug — no new rows anywhere, order stays.
    let second = seed_taxonomy_command(&url).output().expect("binary runs");
    assert!(second.status.success(), "second seed must succeed");
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("inserted=0") || stdout.contains("0 inserted"),
        "the second run must report zero inserts; got: {stdout}"
    );
    let category_after: i64 = sqlx::query_scalar("SELECT count(*) FROM categories")
        .fetch_one(&pool)
        .await
        .expect("count");
    let event_after: i64 = sqlx::query_scalar("SELECT count(*) FROM life_events")
        .fetch_one(&pool)
        .await
        .expect("count");
    let keyword_after: i64 = sqlx::query_scalar("SELECT count(*) FROM life_event_keywords")
        .fetch_one(&pool)
        .await
        .expect("count");
    let synonym_after: i64 = sqlx::query_scalar("SELECT count(*) FROM synonyms")
        .fetch_one(&pool)
        .await
        .expect("count");
    let relation_after: i64 = sqlx::query_scalar("SELECT count(*) FROM life_event_procedures")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(
        category_count, category_after,
        "categories idempotent per slug"
    );
    assert_eq!(event_count, event_after, "events idempotent per slug");
    assert_eq!(keyword_count, keyword_after, "keywords idempotent");
    assert_eq!(synonym_count, synonym_after, "synonyms idempotent");
    assert_eq!(
        relation_count, relation_after,
        "relations idempotent per (event, procedure)"
    );
    assert_eq!(
        relation_orders(&pool, "vender-vehiculo").await,
        vec![1, 2, 3]
    );

    common::drop_test_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn relations_whose_procedure_is_absent_are_skipped_with_a_warning() {
    let (pool, db_name) = common::fresh_migrated_db().await;
    let url = format!("postgres://postgres:postgres@localhost:5432/{db_name}");

    // No procedures seeded: every relation is pending the ingestion run.
    let output = seed_taxonomy_command(&url).output().expect("binary runs");
    assert!(
        output.status.success(),
        "seed must not fail while ingestion has not run yet (make dev order)"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.to_lowercase().contains("pending"),
        "missing-procedure relations must be reported as pending, not fatal; got: {combined}"
    );
    let relation_count: i64 = sqlx::query_scalar("SELECT count(*) FROM life_event_procedures")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(
        relation_count, 0,
        "no relation row may reference a missing procedure"
    );

    common::drop_test_db(&db_name).await;
}
