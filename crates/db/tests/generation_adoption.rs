//! S8 task 23 (catalog-generations delta, OPT-02/OPT-04): the adoption
//! manifest write-back. After every swap the API records
//! `active_generation_id` + `adopted_at` (+ the in-flight generation report)
//! on the adopted manifest row; the worker's reconciler and collector read
//! that record back — adoption confirmation is what gates collection, and a
//! lagging API is observable through a missing/older adoption row.

#[path = "c2support/mod.rs"]
mod c2support;

use c2support::*;

use sqlx::types::uuid::Uuid;

/// Inserts one complete published manifest row so adoption write-backs have
/// a row to land on. `published_age` spaces the rows in time so "latest
/// adoption" and "newest published" are deterministic.
async fn insert_published_manifest(pool: &sqlx::PgPool, id: Uuid, published_age: &str) {
    sqlx::query(
        "INSERT INTO catalog_generations \
         (generation_id, status, content_hash, taxonomy_version, engine_version, \
          source_synced_at, event_count, procedure_count, projection_status, published_at) \
         VALUES ($1, 'published', 'hash', 'tax-v1', 'engine-v1', now(), 2, 4, 'complete', \
                 now() - $2::interval)",
    )
    .bind(id)
    .bind(published_age)
    .execute(pool)
    .await
    .expect("published manifest row inserted");
}

#[tokio::test]
async fn confirm_adoption_writes_active_generation_adopted_at_and_inflight() {
    let (pool, db_name) = fresh_migrated_db().await;
    let generation = Uuid::now_v7();
    insert_published_manifest(&pool, generation, "1 hour").await;

    let in_flight = vec![Uuid::now_v7()];
    db::generations::adopt::confirm_adoption(&pool, generation, &in_flight)
        .await
        .expect("adoption write-back");

    let (active, adopted_at, inflight): (Uuid, Option<String>, Vec<Uuid>) = sqlx::query_as(
        "SELECT generation_id, adopted_at::text, inflight_generation_ids \
         FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(generation)
    .fetch_one(&pool)
    .await
    .expect("manifest row readable");
    assert_eq!(
        active, generation,
        "the adopted manifest row self-identifies as the active generation"
    );
    assert!(
        adopted_at.is_some(),
        "the adoption carries its confirmation timestamp"
    );
    assert_eq!(
        inflight, in_flight,
        "the in-flight generation report travels with the adoption record"
    );

    drop_db(&db_name).await;
}

#[tokio::test]
async fn latest_adoption_returns_the_most_recently_adopted_row() {
    let (pool, db_name) = fresh_migrated_db().await;
    let older = Uuid::now_v7();
    let newer = Uuid::now_v7();
    insert_published_manifest(&pool, older, "2 hours").await;
    insert_published_manifest(&pool, newer, "30 minutes").await;

    assert!(
        db::generations::adopt::latest_adoption(&pool)
            .await
            .expect("read")
            .is_none(),
        "before any adoption there is no adoption record"
    );

    db::generations::adopt::confirm_adoption(&pool, older, &[])
        .await
        .expect("adopt older");
    // Microsecond-resolution timestamps: a short pause makes the ordering
    // deterministic without a clock dependency in the production path.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    db::generations::adopt::confirm_adoption(&pool, newer, &[])
        .await
        .expect("adopt newer");

    let adoption = db::generations::adopt::latest_adoption(&pool)
        .await
        .expect("read")
        .expect("an adoption record exists");
    assert_eq!(
        adoption.generation_id, newer,
        "the latest adoption is the currently served generation"
    );

    drop_db(&db_name).await;
}

#[tokio::test]
async fn newest_published_identifies_the_reference_the_api_must_adopt() {
    let (pool, db_name) = fresh_migrated_db().await;
    let older = Uuid::now_v7();
    let newer = Uuid::now_v7();
    insert_published_manifest(&pool, older, "1 hour").await;
    insert_published_manifest(&pool, newer, "10 minutes").await;

    let newest = db::generations::adopt::newest_published(&pool)
        .await
        .expect("read")
        .expect("a published generation exists");
    assert_eq!(
        newest.generation_id, newer,
        "the newest published generation is the adoption candidate"
    );

    // A never-published manifest never becomes the candidate.
    sqlx::query(
        "INSERT INTO catalog_generations \
         (generation_id, status, content_hash, taxonomy_version, engine_version, \
          source_synced_at, event_count, procedure_count, projection_status) \
         VALUES ($1, 'building', 'hash', 'tax-v1', 'engine-v1', now(), 0, 0, 'partial')",
    )
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .expect("building manifest row inserted");
    let newest_after = db::generations::adopt::newest_published(&pool)
        .await
        .expect("read")
        .expect("published row still found");
    assert_eq!(
        newest_after.generation_id, newer,
        "a building manifest is never the adoption candidate"
    );

    drop_db(&db_name).await;
}

#[tokio::test]
async fn reactivate_republishes_only_a_retained_published_generation() {
    let (pool, db_name) = fresh_migrated_db().await;
    let previous = Uuid::now_v7();
    let defective = Uuid::now_v7();
    insert_published_manifest(&pool, previous, "2 hours").await;
    insert_published_manifest(&pool, defective, "1 hour").await;

    let reactivated = db::generations::adopt::reactivate(&pool, previous)
        .await
        .expect("reactivation runs");
    assert!(
        reactivated,
        "the retained previous generation is re-promoted (rollback path)"
    );

    // A never-published candidate is never reactivated.
    let building = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO catalog_generations \
         (generation_id, status, content_hash, taxonomy_version, engine_version, \
          source_synced_at, event_count, procedure_count, projection_status) \
         VALUES ($1, 'validated', 'hash', 'tax-v1', 'engine-v1', now(), 0, 0, 'complete')",
    )
    .bind(building)
    .execute(&pool)
    .await
    .expect("validated manifest row inserted");
    let not_reactivated = db::generations::adopt::reactivate(&pool, building)
        .await
        .expect("reactivation runs");
    assert!(
        !not_reactivated,
        "only a retained published generation can be reactivated"
    );

    drop_db(&db_name).await;
}
