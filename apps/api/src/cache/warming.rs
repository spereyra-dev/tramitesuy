//! Cache warming (S10 task 32, search-cache delta "Cache warming from a
//! static non-sensitive list", design §3.5): after an adoption, a
//! background task walks a short static committed list of non-sensitive
//! example queries (`apps/api/warming_queries.txt`) and runs the NORMAL
//! computation path — real providers, real ranking, results inserted into
//! the active generation's cache — WITHOUT fabricating user logs (no
//! `search_logs` row is ever written for a warmed query).
//!
//! Warming is never a publication condition: the caller spawns it as a
//! background task after the swap is confirmed, a per-query failure is
//! reported operationally and dropped (no panic, no cache pollution), and
//! serving correctness is untouched by warming succeeding, failing, or
//! being abandoned.

use crate::handlers::search::{CacheWrite, cache_write_commit, lookup_or_compute};
use crate::state::AppState;

/// The committed, non-sensitive warming list: embedded at compile time
/// (`include_str!`) so the list travels with the repo exactly as it will
/// serve. One query per line; `#` comments and blank lines are ignored.
pub fn committed_queries() -> Vec<String> {
    const WARMING_QUERIES: &str = include_str!("../../warming_queries.txt");
    WARMING_QUERIES
        .lines()
        .map(|line| line.split('#').next().unwrap_or(line).trim().to_string())
        .filter(|query| !query.is_empty())
        .collect()
}

/// Warms the active generation's cache with `queries`, through the normal
/// computation path (`handlers::search::lookup_or_compute` — the same
/// lookup/join-or-lead/compute pipeline the handlers serve): a warmed
/// query that is already cached serves as a hit (nothing computed, nothing
/// inserted); a fresh one computes and its pending cache write is
/// committed directly — the warming path writes NO user log, so no
/// fabricated `search_logs` row exists.
///
/// Returns the number of entries this pass computed (zero on an
/// already-warm cache). Failures are reported operationally (no
/// query-derived text ever leaves this module) and contained: a failing
/// warming neither panics nor blocks serving, and the caller decides
/// whether to retry.
pub async fn run(state: &AppState, queries: &[String]) -> usize {
    let generation = state.active.load_full();
    let mut computed = 0usize;
    for query in queries {
        let effective = query.trim();
        if effective.is_empty() {
            continue;
        }
        match lookup_or_compute(state, &generation, effective).await {
            // Computed fresh (or a wait window elapsed and this warming
            // recomputed): commit the pending write exactly like a real
            // miss would after its log. The oversized-drop and eviction
            // rules are the cache's own (`SearchCache::insert_shared`).
            // A cache hit commits nothing and is NOT a computation.
            Ok((_outcome, write, _provider_ops)) => {
                if matches!(write, CacheWrite::Pending { .. }) {
                    computed += 1;
                }
                cache_write_commit(write, &generation);
            }
            // A failed warming is operational signal only (design §3.5):
            // reported, then dropped — serving is unaffected and the query
            // simply stays uncached.
            Err(_error) => {
                state
                    .metrics
                    .observe_operational_alert(crate::metrics::OperationalAlert::WarmingFailed);
            }
        }
    }
    computed
}

/// Warms the active generation's cache from the committed list. Spawned as
/// a background task after every confirmed adoption; its failure never
/// blocks or conditions the publication that already completed.
pub async fn warm(state: &AppState) -> usize {
    run(state, &committed_queries()).await
}

/// Spawns the warming task as a background task (the callers after every
/// confirmed adoption — boot and reconciliation): the publication already
/// completed and this task's failure never conditions it. Runs only when
/// warming is enabled in the serving limits (the production default; a
/// deployment that does not want warming turns it off with
/// `API_CACHE_WARMING=0`).
pub fn spawn(state: &AppState) {
    if !state.limits.cache_warming {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        warm(&state).await;
    });
}
