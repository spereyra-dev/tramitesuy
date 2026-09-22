//! Publication detection and adoption (S8 task 23, catalog-generations
//! delta "Publication detection tolerates lost notifications", OPT-02/
//! OPT-04, R10): the API reconciles the durable manifest every
//! `reconciliation_interval` (60 seconds by default, configurable) and
//! adopts the newest published generation through the same validated load +
//! atomic swap path as boot. Adoption is confirmed with the manifest
//! write-back (`active_generation_id` + `adopted_at` + the in-flight
//! report). A cross-process notification (PostgreSQL `LISTEN`/`NOTIFY`) may
//! accelerate detection but is never the correctness mechanism: a
//! publication whose notification is lost is adopted within one
//! reconciliation cycle.
//!
//! Reconciliation NEVER deletes anything by itself — collection is a
//! separate worker-side concern gated on the confirmed adoption (task 24).
//! A failed or invalid load never changes the served generation (task 22).

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use crate::metrics::OperationalAlert;
use crate::state::AppState;

/// The cross-process notification channel (PostgreSQL `LISTEN`/`NOTIFY`):
/// the worker publishes a hint here after each promotion. Only an
/// accelerator — never the correctness mechanism (task 23).
pub const PUBLICATION_CHANNEL: &str = "generation_published";

/// Reconciliation policy: the manifest-reconciliation cadence and the
/// operational alert bound for a lagging adoption (a published generation
/// the API has not adopted within the bound after its publication stamp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileConfig {
    pub interval: Duration,
    pub lag_alert_after: Duration,
}

/// What one reconciliation tick did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickReport {
    /// The generation adopted and confirmed this tick, if any.
    pub adopted: Option<Uuid>,
    /// The newest published generation was already the served one.
    pub already_current: bool,
    /// The lagging-adoption alert fired (newest publication older than the
    /// bound while the API still serves an older generation).
    pub lagging: bool,
    /// The candidate was rejected by the memory-budget guard (task 25): the
    /// current generation keeps serving.
    pub memory_budget_rejected: bool,
    /// The newest publication failed to load (invalid/rejected candidate):
    /// the served generation is never changed by a failed load.
    pub load_rejected: Option<String>,
}

/// One reconciliation pass: detect the newest published manifest, adopt it
/// if it is not already served, and raise the lagging alert past the bound.
/// Never deletes anything; a DB error propagates so the loop can log it and
/// keep ticking (the manifest stays durable and the next cycle retries).
pub async fn tick(state: &AppState) -> Result<TickReport, sqlx::Error> {
    let now = Utc::now();
    let newest = db::generations::adopt::newest_published(&state.pool).await?;
    let current = state.active.load_full();
    let current_id = current.generation_id();

    let mut report = TickReport::default();
    let Some(newest) = newest else {
        // Nothing published yet: the API stays cold until the first valid
        // publication (task 22 semantics; the boot load already handled it).
        return Ok(report);
    };

    // Operational alert: the active generation is lagging behind a
    // confirmed publication past the configurable bound.
    if newest.generation_id != current_id
        && (now - newest.published_at) > chrono_of(state.limits.lag_alert_after)
    {
        state
            .metrics
            .observe_operational_alert(OperationalAlert::PublicationLag);
        report.lagging = true;
    }

    if newest.generation_id == current_id {
        report.already_current = true;
        return Ok(report);
    }

    // Memory-budget guard (task 25): the candidate is sized BEFORE loading
    // (coarse projection from the manifest counts over the active
    // snapshot's per-item rates) so an over-budget adoption never
    // materializes a second snapshot.
    if let Some(budget) = state.limits.memory_budget {
        let footprint = super::memory_budget::estimate(&current);
        let projected = super::memory_budget::project_candidate(
            &current,
            newest.event_count,
            newest.procedure_count,
        );
        let previous_in_use = state.retained_in_use_bytes();
        if !budget.permits(
            footprint.owned_bytes,
            projected,
            previous_in_use,
            footprint.shared_bytes,
        ) {
            state
                .metrics
                .observe_operational_alert(OperationalAlert::MemoryBudget);
            report.memory_budget_rejected = true;
            return Ok(report);
        }
    }

    // Adopt through the durable loader (newest-first with the previous
    // generation as fallback): an invalid or failed load never installs.
    // The configured cache limits travel with the snapshot (S9 task 27).
    match super::load_published_with_bundle_and_limits(
        &state.pool,
        &state.bundle,
        state.provider_fetch,
        state.limits.cache,
    )
    .await
    {
        Ok(Some(generation)) => {
            let adopted_id = generation.generation_id();
            if adopted_id == current_id {
                // The newest publication was rejected and the loader fell
                // back to the previous generation — already the served one:
                // no adoption happened, the served generation is unchanged.
                report.already_current = true;
                return Ok(report);
            }
            state.install(Arc::new(generation));
            // Write-back after the swap: the manifest records the adoption
            // (the worker's correctness signal) with the in-flight report.
            db::generations::adopt::confirm_adoption(
                &state.pool,
                adopted_id,
                &state.retained_inflight_ids(),
            )
            .await?;
            // Cache warming (S10 task 32): a background task after the
            // confirmed adoption — never a publication condition.
            crate::cache::warming::spawn(state);
            report.adopted = Some(adopted_id);
        }
        Ok(None) => {
            report.load_rejected = Some("no loadable published generation".to_string());
        }
        Err(error) => {
            report.load_rejected = Some(error.to_string());
        }
    }
    Ok(report)
}

/// The reconciliation background loop: ticks every configured interval, and
/// additionally wakes on the cross-process notification when the
/// `LISTEN`/`NOTIFY` channel is available. The notification is only an
/// accelerator — the interval tick is the correctness mechanism, so a lost
/// notification is adopted within one cycle.
pub fn spawn(state: AppState, channel: &'static str) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // The listener is best-effort: without it the loop still reconciles
        // on the interval (correctness never depends on the notification).
        let mut listener = match sqlx::postgres::PgListener::connect_with(&state.pool).await {
            Ok(mut listener) => match listener.listen(channel).await {
                Ok(()) => Some(listener),
                Err(error) => {
                    eprintln!("api reconciliation: LISTEN unavailable: {error}");
                    None
                }
            },
            Err(error) => {
                eprintln!("api reconciliation: notification listener unavailable: {error}");
                None
            }
        };
        loop {
            if let Err(error) = tick(&state).await {
                eprintln!("api reconciliation tick: {error}");
            }
            let sleep = tokio::time::sleep(state.limits.reconciliation_interval);
            tokio::select! {
                _ = sleep => {}
                notification = async {
                    match listener.as_mut() {
                        // Justified: the pending future never resolves, so a
                        // missing listener degrades to the interval cadence.
                        Some(listener) => listener.recv().await.map(|_| ()),
                        None => std::future::pending().await,
                    }
                } => {
                    if let Err(error) = notification {
                        eprintln!("api reconciliation listener: {error}");
                    }
                }
            }
        }
    })
}

fn chrono_of(window: Duration) -> chrono::Duration {
    // Justified: `from_std` fails only beyond chrono's range (centuries);
    // such a configuration saturates instead of panicking the loop.
    chrono::Duration::from_std(window).unwrap_or(chrono::Duration::MAX)
}
