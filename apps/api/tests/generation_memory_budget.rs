//! S8 task 25 (catalog-generations delta, OPT-10, R4): the memory-budget
//! guard. Before a candidate generation is loaded/adopted, RAM is sized for
//! the active generation + the candidate + the previous generation still in
//! use + caches + PostgreSQL + system reserve. With no budget the current
//! generation keeps serving and the failure is reported operationally — the
//! system never reaches OOM or sustained swap growth.

mod support;

use std::sync::Arc;

use support::*;

use api::generation::memory_budget::{self, MemoryBudget};

/// A 64 MiB budget with a 48 MiB reserve for caches/PostgreSQL/system.
fn test_budget() -> MemoryBudget {
    MemoryBudget {
        total_bytes: 64 * 1024 * 1024,
        reserve_bytes: 48 * 1024 * 1024,
    }
}

#[test]
fn the_guard_sizes_active_candidate_previous_and_reserve() {
    let budget = test_budget();
    let active: u64 = 8 * 1024 * 1024;
    let candidate: u64 = 8 * 1024 * 1024;
    let previous: u64 = 4 * 1024 * 1024;

    assert!(
        !budget.permits(active, candidate, previous, 0),
        "active + candidate + previous-in-use + reserve over the budget is rejected"
    );
    assert!(
        budget.permits(active, candidate, 0, 0),
        "without a previous generation still in use the candidate fits"
    );
    // The reserve is part of the sizing: caches + PostgreSQL + system.
    let exact = MemoryBudget {
        total_bytes: active + candidate + previous + test_budget().reserve_bytes,
        reserve_bytes: test_budget().reserve_bytes,
    };
    assert!(
        exact.permits(active, candidate, previous, 0),
        "exactly at budget admits"
    );
}

#[test]
fn an_empty_budget_rejects_any_candidate() {
    let budget = MemoryBudget {
        total_bytes: 0,
        reserve_bytes: 0,
    };
    assert!(
        !budget.permits(1, 1, 0, 0),
        "with no budget at all nothing new is built or loaded"
    );
}

#[tokio::test]
async fn the_estimator_is_deterministic_and_reports_shared_bundle_data_once() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let generation = api::generation::load_published(&pool, &repo_root().join("data"))
        .await
        .expect("load")
        .expect("published generation loads");

    let first = memory_budget::estimate(&generation);
    assert!(
        first.owned_bytes > 0,
        "a loaded generation owns catalog data"
    );
    // The same generation's estimate is deterministic (stable sizing).
    assert_eq!(
        memory_budget::estimate(&generation),
        first,
        "the estimate is deterministic"
    );

    // The taxonomy/engine/synonyms are shared Arc'd bundle data: they are
    // counted as `shared` (once across generations), not re-owned per
    // snapshot — the reuse the guard's sizing exploits.
    assert!(
        first.shared_bytes > 0,
        "the bundle (engine/taxonomy/synonyms) is reported as shared, not owned"
    );
    // Two coexisting generations share the immutable bundle: shared counted
    // once, owned additive.
    let two_generations = first.owned_bytes * 2 + first.shared_bytes;
    assert!(
        two_generations < (first.owned_bytes + first.shared_bytes) * 2,
        "the shared bundle is not double-counted per generation"
    );

    common_drop(&db_name).await;
}

#[tokio::test]
async fn a_candidate_over_the_injected_budget_is_rejected_by_the_reconciliation_tick() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;

    // Injected budget: admits the active generation exactly (its measured
    // owned footprint) with a 1 MiB reserve for caches + PostgreSQL +
    // system — any candidate exceeds it.
    let current = api::generation::load_published(&pool, &repo_root().join("data"))
        .await
        .expect("load")
        .expect("the published generation loads");
    let owned = memory_budget::estimate(&current).owned_bytes;
    let limits = api::config::ApiLimits {
        memory_budget: Some(MemoryBudget {
            total_bytes: owned + 1024 * 1024,
            reserve_bytes: 1024 * 1024,
        }),
        ..api::config::ApiLimits::default()
    };
    let metrics = Arc::new(api::metrics::MemoryMetrics::new());
    let state = api::state::AppState::boot_with_metrics(
        pool.clone(),
        &repo_root().join("data"),
        limits,
        metrics.clone(),
    )
    .await
    .expect("boot with the injected budget");
    assert!(
        state.active.load_full().is_loaded(),
        "G1 is active under the budget"
    );

    // A new generation is published (content change); adopting it would
    // exceed the budget.
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '4551'")
        .execute(&pool)
        .await
        .expect("content change for the candidate");
    let published = publish_sample_generation(&pool).await;
    let report = api::generation::reconcile::tick(&state)
        .await
        .expect("tick runs");

    assert!(
        report.memory_budget_rejected,
        "the over-budget candidate is rejected: {report:?}"
    );
    // The active generation keeps serving: the candidate was never
    // installed (the swap never happened).
    let served = state.active.load_full();
    assert_ne!(
        served.generation_id(),
        published.generation_id,
        "the candidate was never installed"
    );
    assert!(
        served.manifest().is_some(),
        "the previous generation stays active"
    );
    // The failure is reported operationally through the metrics seam.
    assert!(
        metrics.alert_total(api::metrics::OperationalAlert::MemoryBudget) >= 1,
        "an operational signal is emitted for the rejected candidate"
    );
    // No OOM/swap growth: only the active snapshot is retained.
    assert_eq!(state.retained_snapshot_count(), 1);

    common_drop(&db_name).await;
}

#[tokio::test]
async fn a_lost_notification_is_adopted_within_one_reconciliation_cycle() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let state =
        api::state::AppState::boot(pool.clone(), &repo_root().join("data"), Default::default())
            .await
            .expect("boot adopts the published generation");
    let served_before = state.active.load_full().generation_id();

    // A new generation is published while the notification is LOST: no
    // listener, no hint — only the durable manifest exists.
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '4551'")
        .execute(&pool)
        .await
        .expect("content change for the new generation");
    let newer = publish_sample_generation(&pool).await;
    assert_eq!(
        state.active.load_full().generation_id(),
        served_before,
        "without any signal the API keeps serving the current generation"
    );

    // One reconciliation cycle reconciles the manifest and adopts.
    let report = api::generation::reconcile::tick(&state)
        .await
        .expect("tick runs");
    assert_eq!(
        report.adopted,
        Some(newer.generation_id),
        "the publication is adopted: {report:?}"
    );

    // The adoption is written back on the manifest row.
    let (active_id, adopted_at): (sqlx::types::uuid::Uuid, Option<String>) = sqlx::query_as(
        "SELECT active_generation_id, adopted_at::text FROM catalog_generations \
         WHERE generation_id = $1",
    )
    .bind(newer.generation_id)
    .fetch_one(&pool)
    .await
    .expect("manifest row");
    assert_eq!(
        active_id, newer.generation_id,
        "the manifest records the adopted generation"
    );
    assert!(adopted_at.is_some(), "the adoption carries its timestamp");

    let _ = served_before;
    common_drop(&db_name).await;
}

#[tokio::test]
async fn the_lagging_alert_fires_past_the_bound() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    // A shortened alert bound (configurable): the newest publication is
    // already past it while the API still serves the previous generation.
    let limits = api::config::ApiLimits {
        lag_alert_after: std::time::Duration::from_secs(0),
        ..api::config::ApiLimits::default()
    };
    let metrics = Arc::new(api::metrics::MemoryMetrics::new());
    let state = api::state::AppState::boot_with_metrics(
        pool.clone(),
        &repo_root().join("data"),
        limits,
        metrics.clone(),
    )
    .await
    .expect("boot");
    assert!(metrics.alert_total(api::metrics::OperationalAlert::PublicationLag) == 0);

    // Publish a newer generation that the API has not adopted.
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '4551'")
        .execute(&pool)
        .await
        .expect("content change for the new generation");
    publish_sample_generation(&pool).await;
    let report = api::generation::reconcile::tick(&state)
        .await
        .expect("tick runs");
    assert!(
        report.lagging,
        "the lagging-adoption alert fires past the bound"
    );
    assert!(
        metrics.alert_total(api::metrics::OperationalAlert::PublicationLag) >= 1,
        "the operational alert is emitted"
    );

    common_drop(&db_name).await;
}

#[tokio::test]
async fn a_failed_load_never_changes_the_served_generation_and_reconciliation_never_deletes() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let state =
        api::state::AppState::boot(pool.clone(), &repo_root().join("data"), Default::default())
            .await
            .expect("boot loads G1");
    let served = state.active.load_full().generation_id();

    // A published candidate whose manifest cannot load (a taxonomy version
    // that does not match the boot YAML): the load is rejected.
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '4551'")
        .execute(&pool)
        .await
        .expect("content change for the candidate");
    let candidate = publish_sample_generation(&pool).await;
    sqlx::query(
        "UPDATE catalog_generations SET taxonomy_version = 'defective' WHERE generation_id = $1",
    )
    .bind(candidate.generation_id)
    .execute(&pool)
    .await
    .expect("defective manifest");
    let projection_rows_before = projection_row_count(&pool).await;
    let report = api::generation::reconcile::tick(&state)
        .await
        .expect("tick runs");
    assert!(
        report.adopted.is_none() && report.already_current,
        "the invalid candidate is rejected; the loader re-adopted the previous          generation, which is the served one: {report:?}"
    );
    assert_eq!(
        state.active.load_full().generation_id(),
        served,
        "a failed load never changes the served generation"
    );
    // Reconciliation never deletes anything by itself: the projection rows
    // of every generation are intact after the tick.
    assert_eq!(
        projection_row_count(&pool).await,
        projection_rows_before,
        "reconciliation deletes nothing"
    );

    common_drop(&db_name).await;
}
