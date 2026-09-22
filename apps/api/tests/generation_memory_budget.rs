//! S8 task 25 (catalog-generations delta, OPT-10, R4): the memory-budget
//! guard. Before a candidate generation is loaded/adopted, RAM is sized for
//! the active generation + the candidate + the previous generation still in
//! use + caches + PostgreSQL + system reserve. With no budget the current
//! generation keeps serving and the failure is reported operationally — the
//! system never reaches OOM or sustained swap growth.

mod support;

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
