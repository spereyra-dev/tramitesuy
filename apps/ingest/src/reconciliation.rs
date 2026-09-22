//! Worker-side manifest reconciliation (S8 task 23, design §2.3, OPT-02/
//! OPT-04, R10): the worker reconciles the durable manifest on a fixed
//! interval (60 seconds by default, configurable) — it reads the API's
//! adoption write-back to confirm a publication and to gate collection
//! (task 24), and raises the operational lag alert when the newest
//! confirmed publication has been un-adopted past the bound (10 minutes by
//! default).
//!
//! Reconciliation never deletes anything by itself: the only deletion path
//! is the gated collector, which runs off the request path and only after
//! the confirmed adoption + in-flight drain or the retention window
//! (task 24). The publication notification (`LISTEN`/`NOTIFY` hint) is only
//! an accelerator — the interval cadence is the correctness mechanism.

use std::time::Duration;

use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, Utc};
use uuid::Uuid;

/// The shared cross-process notification channel. The worker publishes a
/// hint here after each promotion; the API listens on the same channel
/// (`api::generation::reconcile::PUBLICATION_CHANNEL` — the cross-process
/// test asserts the two constants match). Acceleration only.
pub const PUBLICATION_CHANNEL: &str = "generation_published";

/// Worker reconciliation policy. Environment variables (all optional):
///
/// | Variable | Default | Meaning |
/// |---|---|---|
/// | `INGEST_RECONCILE_SECS` | 60 | manifest reconciliation interval |
/// | `INGEST_LAG_ALERT_SECS` | 600 | lagging-adoption alert bound |
/// | `INGEST_GENERATION_RETENTION` | 3 | retained generations |
/// | `INGEST_RETENTION_WINDOW_SECS` | 3600 | in-flight retention window |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileConfig {
    pub interval: Duration,
    pub lag_alert_after: Duration,
    pub retention: usize,
    pub retention_window: Duration,
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        ReconcileConfig {
            interval: Duration::from_secs(60),
            lag_alert_after: Duration::from_secs(600),
            retention: 3,
            retention_window: Duration::from_secs(3600),
        }
    }
}

/// A variable was present but its value cannot be used (unparseable or
/// zero). Same shape as the pool-config error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileConfigError {
    pub field: &'static str,
    pub value: String,
}

impl std::fmt::Display for ReconcileConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid value for {}: {:?}", self.field, self.value)
    }
}

impl std::error::Error for ReconcileConfigError {}

impl ReconcileConfig {
    /// Reads the policy from `std::env`.
    pub fn from_env() -> Result<ReconcileConfig, ReconcileConfigError> {
        ReconcileConfig::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads the policy from an injectable lookup (tests / file sources).
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<ReconcileConfig, ReconcileConfigError> {
        Ok(ReconcileConfig {
            interval: Duration::from_secs(parse_positive_secs(
                &lookup,
                "INGEST_RECONCILE_SECS",
                ReconcileConfig::default().interval,
            )?),
            lag_alert_after: Duration::from_secs(parse_positive_secs(
                &lookup,
                "INGEST_LAG_ALERT_SECS",
                ReconcileConfig::default().lag_alert_after,
            )?),
            retention: parse_positive(&lookup, "INGEST_GENERATION_RETENTION", 3)?,
            retention_window: Duration::from_secs(parse_positive_secs(
                &lookup,
                "INGEST_RETENTION_WINDOW_SECS",
                ReconcileConfig::default().retention_window,
            )?),
        })
    }
}

fn parse_positive_secs(
    lookup: &impl Fn(&str) -> Option<String>,
    field: &'static str,
    default: Duration,
) -> Result<u64, ReconcileConfigError> {
    parse_positive(lookup, field, default.as_secs())
}

/// Parses a required-positive integer from an optional variable: absent
/// keeps the default; present-but-unparseable or zero is a configuration
/// error (fail-fast at boot).
fn parse_positive<T>(
    lookup: &impl Fn(&str) -> Option<String>,
    field: &'static str,
    default: T,
) -> Result<T, ReconcileConfigError>
where
    T: std::str::FromStr + PartialOrd + Default + Copy,
{
    match lookup(field) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<T>()
            .ok()
            .filter(|parsed| *parsed > T::default())
            .ok_or(ReconcileConfigError { field, value: raw }),
    }
}

/// What one worker reconciliation pass observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerReport {
    /// The newest published generation, if any.
    pub newest_published: Option<Uuid>,
    /// Whether the newest published generation is confirmed adopted by the
    /// API's manifest write-back.
    pub adopted: bool,
    /// The lagging-adoption alert fired (un-adopted publication older than
    /// the bound).
    pub lag_alert: bool,
    /// The adoption was not confirmed: collection was deferred entirely.
    pub deferred_unadopted: bool,
    /// Generations whose projections were collected (task 24 gates).
    pub collected: Vec<Uuid>,
    /// Generations deferred because an in-flight holder still uses them.
    pub deferred_in_flight: Vec<Uuid>,
}

/// One worker reconciliation pass: confirm the adoption of the newest
/// publication, alert on the lagging bound, and — only when the adoption is
/// confirmed — run the gated retention collection (never on the request
/// path, never touching the active or previous generation).
pub async fn run_pass(
    pool: &PgPool,
    config: &ReconcileConfig,
) -> Result<WorkerReport, sqlx::Error> {
    let now = Utc::now();
    let newest = db::generations::adopt::newest_published(pool).await?;
    let adoption = db::generations::adopt::latest_adoption(pool).await?;

    let mut report = WorkerReport {
        newest_published: newest.as_ref().map(|newest| newest.generation_id),
        ..WorkerReport::default()
    };

    let confirmed = match (&newest, &adoption) {
        (Some(newest), Some(adoption)) => adoption.generation_id == newest.generation_id,
        _ => false,
    };
    report.adopted = confirmed;

    if !confirmed {
        // The lagging-API alert: the newest confirmed publication is older
        // than the bound and the API has not adopted it.
        if let Some(newest) = &newest
            && (now - newest.published_at) > chrono_of(config.lag_alert_after)
        {
            report.lag_alert = true;
            eprintln!(
                "worker reconciliation: ALERT publication lag — generation {} published at \
                 {} is not adopted past the bound",
                newest.generation_id, newest.published_at
            );
        }
        // A lagging API's projection is never deleted: the whole collection
        // pass defers until the adoption is confirmed.
        report.deferred_unadopted = true;
        return Ok(report);
    }

    // Adoption confirmed: the gated collection (off the request path).
    if let Some(adoption) = &adoption {
        let collection = db::generations::collect::collect_generations(
            pool,
            db::generations::collect::CollectionConfig {
                retention: config.retention,
                retention_window: config.retention_window,
            },
            adoption,
            now,
        )
        .await?;
        report.collected = collection.collected;
        report.deferred_in_flight = collection.deferred_in_flight;
    }
    Ok(report)
}

fn chrono_of(window: Duration) -> chrono::Duration {
    // Justified: `from_std` fails only beyond chrono's range (centuries);
    // such a configuration saturates instead of panicking the pass.
    chrono::Duration::from_std(window).unwrap_or(chrono::Duration::MAX)
}

/// The publication stamp of the newest published reference (test and
/// operator-facing helper for lag arithmetic).
pub fn published_at(reference: &db::generations::adopt::PublishedReference) -> DateTime<Utc> {
    reference.published_at
}

/// The one-line summary the daemon logs per pass (counts and references —
/// never query-derived text; the worker's operational observability).
pub fn summarize(report: &WorkerReport) -> String {
    format!(
        "newest_published={} adopted={} collected={} deferred_in_flight={} deferred_unadopted={}",
        report
            .newest_published
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_string()),
        report.adopted,
        report.collected.len(),
        report.deferred_in_flight.len(),
        report.deferred_unadopted,
    )
}
