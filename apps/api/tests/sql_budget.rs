//! S14 task 44 (operations delta "SQL budget per request", OPT-06, spec §7
//! test 10): the SQL-budget acceptance suite. In normal operation —
//! excluding publication controls and metrics — the budgets are:
//!
//! - catalog read (category, event, procedure): **0** statements;
//! - cache-hit search: **1** (the consolidated log insert);
//! - new search with PostgreSQL providers: **≤3** (FTS, trigram, log);
//! - intermediate no-snapshot `open`: **≤4** (adds the dedicated cards
//!   query).
//!
//! The task names `crates/db/tests/sql_budget.rs` **or** the API-level
//! counter harness from task 2; the budgets are asserted over the real HTTP
//! surface, which only the API harness can serve, so this file uses the
//! task-2 instrument (`SqlCounter` counting pool) in `apps/api/tests`.
//!
//! RED leg: the same budget checker is applied to the PRE-optimization
//! baseline numbers recorded in task 4 (`tests/load/BASELINE.md`: open
//! search 7, catalog read 2) and MUST be violated by them — the acceptance
//! thresholds are not vacuous, and the recorded pre-change path failed
//! every budget. The GREEN legs assert the budgets hold on the optimized
//! path (stages 2–4).
//!
//! Accounting rule (task 2/44 instrument): transaction control (`BEGIN`/
//! `COMMIT`) and the generation trigram provider's transaction-local
//! similarity threshold (`set_config`, mandated by design §2.2 inside the
//! provider transaction, never pool session state) are charged to the ONE
//! trigram operation the delta budgets — the same "real data statements"
//! accounting `sql_ops_baseline` records for the intermediate ≤4 budget.
//! The traced statement totals are recorded there, not re-asserted here.
//!
//! TRIANGULATE: the `/search/debug` (read-only) and cache-hit paths are
//! covered by their dedicated suites — `apps/api/tests/search_debug.rs`,
//! `cache_log_guarantee.rs` (cache-hit = 1 statement, debug shares it) and
//! `sql_ops_baseline.rs::debug_with_snapshot_uses_the_captured_generation_
//! and_logs` — referenced, not duplicated.

mod support;

use axum::http::StatusCode;
use support::*;

/// The SQL budget per request in normal operation (operations delta).
#[derive(Debug, Clone, Copy)]
enum Budget {
    /// Catalog reads execute no SQL.
    Catalog,
    /// Cache-hit search: exactly the consolidated log insert.
    CacheHit,
    /// New search with PostgreSQL providers: at most three statements.
    NewSearch,
    /// Intermediate no-snapshot `open`: at most four statements.
    IntermediateOpen,
}

impl Budget {
    fn limit(self) -> u64 {
        match self {
            Budget::Catalog => 0,
            Budget::CacheHit => 1,
            Budget::NewSearch => 3,
            Budget::IntermediateOpen => 4,
        }
    }

    /// Whether an observed statement count violates the budget.
    fn violated_by(self, observed: u64) -> bool {
        match self {
            // Cache-hit is an EXACT budget: one statement, not fewer (the
            // log must persist) and not more.
            Budget::CacheHit => observed != self.limit(),
            _ => observed > self.limit(),
        }
    }
}

/// RED leg (task 44): the budgets REJECT the pre-optimization baseline
/// numbers recorded in task 4 — open search 7 statements, catalog read 2,
/// disambiguation 4 (FTS + trigram + top-event lookup + consolidated log).
/// This proves the thresholds are not vacuous: they fail against exactly
/// the numbers the baseline recorded, and the stages 2–4 optimizations are
/// what brought the served path inside them.
#[test]
fn the_budget_checker_rejects_the_pre_optimization_baseline_numbers() {
    // tests/load/BASELINE.md (task 4), pre-optimization serving path.
    let baseline_open_search: u64 = 7;
    let baseline_catalog_read: u64 = 2;
    let baseline_disambiguation: u64 = 4;

    assert!(
        Budget::NewSearch.violated_by(baseline_open_search),
        "the pre-optimization open search (7 statements) must violate the \
         new-search ≤3 budget"
    );
    assert!(
        Budget::IntermediateOpen.violated_by(baseline_open_search),
        "the pre-optimization open search (7 statements) must violate the \
         intermediate ≤4 budget"
    );
    assert!(
        Budget::Catalog.violated_by(baseline_catalog_read),
        "the pre-optimization catalog read (2 statements) must violate the \
         catalog-0 budget"
    );
    assert!(
        Budget::NewSearch.violated_by(baseline_disambiguation),
        "the pre-optimization disambiguation (4 statements) must violate \
         the ≤3 budget"
    );

    // The checker still accepts numbers inside the budgets (negative
    // control on the other side of each boundary).
    assert!(!Budget::Catalog.violated_by(0));
    assert!(!Budget::CacheHit.violated_by(1));
    assert!(!Budget::NewSearch.violated_by(3));
    assert!(!Budget::IntermediateOpen.violated_by(4));
    assert!(
        Budget::CacheHit.violated_by(0) && Budget::CacheHit.violated_by(2),
        "the cache-hit budget is exact: neither 0 nor 2 statements satisfy it"
    );
}

/// GREEN: catalog reads execute ZERO statements against a loaded
/// generation snapshot (categories, event, procedure).
#[tokio::test(flavor = "multi_thread")]
async fn catalog_reads_execute_zero_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool).await;

    for uri in [
        "/api/v1/categories",
        "/api/v1/events/comprar-vehiculo",
        "/api/v1/procedures/4551",
    ] {
        section.reset();
        let (status, body) = request(&app, "GET", uri).await;
        let count = section.count();

        assert_eq!(
            status,
            StatusCode::OK,
            "{uri} must be served from the snapshot: {body}"
        );
        assert_eq!(
            count, 0,
            "the catalog read {uri} costs {count} statements — the budget is \
             ZERO (the snapshot answers, no SQL)"
        );
    }
}

/// GREEN: a cache-hit search executes EXACTLY one statement (the
/// consolidated log insert with integrated slug resolution).
#[tokio::test(flavor = "multi_thread")]
async fn a_cache_hit_search_costs_exactly_one_statement() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_search_fixture(&pool).await;
    let app = spawn_app_with_generation(pool.clone()).await;

    // The miss computes and warms the cache for its generation.
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mode"], "open");

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["mode"], "open",
        "the repeated query answers from cache"
    );
    assert_eq!(
        count, 1,
        "a cache-hit search executes EXACTLY one statement — the \
         consolidated log insert (observed {count})"
    );
}

/// GREEN: a new (uncached) search with the PostgreSQL providers stays
/// within THREE statements (FTS, trigram, consolidated log) against a
/// loaded snapshot.
#[tokio::test(flavor = "multi_thread")]
async fn new_provider_search_stays_within_three_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_search_fixture(&pool).await;
    let app = spawn_app_with_generation(pool).await;

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    let data = section.data_count();

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mode"], "open", "the recorded path is the open one");
    assert!(
        !body["results"][0]["procedures"]
            .as_array()
            .expect("cards")
            .is_empty(),
        "the open cards come from the snapshot"
    );
    assert!(
        data <= Budget::NewSearch.limit(),
        "a new PostgreSQL-provider search executed {data} data statements — \
         the budget is ≤3 (FTS + trigram + consolidated log; the trigram \
         provider's transaction-local threshold ceremony is charged to its \
         one trigram operation)"
    );
    assert!(
        data >= 1,
        "the new search still persists its consolidated log (log-before-\
         respond): {data} statements observed"
    );
}

/// GREEN: the intermediate no-snapshot `open` search stays within FOUR
/// statements (FTS, trigram, consolidated log, dedicated cards query) —
/// the budget held open until the snapshot route lands.
#[tokio::test(flavor = "multi_thread")]
async fn intermediate_open_search_stays_within_four_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_search_fixture(&pool).await;
    // No published generation: the legacy no-snapshot path serves.
    let app = spawn_app(pool);

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mode"], "open", "the recorded path is the open one");
    assert!(
        !body["results"][0]["procedures"]
            .as_array()
            .expect("cards")
            .is_empty(),
        "the intermediate open path still serves its cards query"
    );
    assert!(
        count <= Budget::IntermediateOpen.limit(),
        "the intermediate-phase open search executed {count} statements — \
         the budget is ≤4 (FTS + trigram + consolidated log + cards query)"
    );
}
