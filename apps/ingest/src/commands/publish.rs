//! `ingest publish` (S6 task 18, OPT-02, R13, ingestion delta, design §6.3):
//! the mandatory publication flow — build → validate → persist complete
//! artifacts → mark `validated` → promote the reference.
//!
//! Guarantees:
//! - **Exclusion**: the same PostgreSQL advisory lock scheduled and manual
//!   ingestion runs share (`hashtext('tramitesuy:ingestion')`). A run that
//!   cannot acquire it records a `skipped` status and is not queued — the
//!   API keeps serving the current generation throughout.
//! - **Idempotency/retryable**: identical content reuses the recorded
//!   generation id (`content_hash` lookup) and never duplicates artifacts;
//!   projection writes are idempotent per `(generation_id, slug)`; a
//!   promotion retry over a `validated` candidate completes idempotently.
//! - **Durable artifacts before the reference is promoted**: the legacy
//!   (working) tables stay written by the ingestion pipeline's dual-write,
//!   so a failure after the working-table updates never leaves those tables
//!   as the sole copy of the live version — the manifest governs what is
//!   live, and the promotion only advances a `validated` candidate with
//!   complete projections.
//! - **Run record** (`ingestion_runs`): start/end timestamps, status,
//!   counts (jsonb), candidate and published generation references,
//!   attempt; error paths record a terminal `failed` status.

use crate::errors::PublishError;
use crate::exclusion::IngestionExclusion;
use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, Utc};
use std::path::Path;
use uuid::Uuid;

/// Which run kind is publishing (matches `ingestion_runs.trigger`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Scheduled,
    Manual,
    Recovery,
}

impl Trigger {
    fn as_str(self) -> &'static str {
        match self {
            Trigger::Scheduled => "scheduled",
            Trigger::Manual => "manual",
            Trigger::Recovery => "recovery",
        }
    }
}

/// The outcome of one publish invocation.
#[derive(Debug, Clone)]
pub struct PublishReport {
    pub run_id: Uuid,
    /// `success` | `validation_failed` | `skipped`.
    pub status: String,
    pub candidate_generation_id: Option<Uuid>,
    pub published_generation_id: Option<Uuid>,
    /// The published generation was already live (identical content).
    pub already_published: bool,
    pub counts: serde_json::Value,
    pub validation_failures: Vec<String>,
}

impl PublishReport {
    /// Deterministic report the worker prints (the summary artifact).
    pub fn summary(&self) -> String {
        let published = self
            .published_generation_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_string());
        format!(
            "publish status={} run_id={} published_generation={} failures={}\n",
            self.status,
            self.run_id,
            published,
            self.validation_failures.len(),
        )
    }
}

/// CLI entry: run one publish pass and print the report.
pub fn run(data_dir: &str, database_url: Option<&str>) {
    match run_once(data_dir, database_url) {
        Ok(()) => {}
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    }
}

fn run_once(data_dir: &str, database_url: Option<&str>) -> Result<(), PublishError> {
    let pool = crate::support::connect_pool(&crate::support::database_url(database_url));
    let report = crate::support::block_on(publish(&pool, Path::new(data_dir), Trigger::Manual))?;
    println!("{}", report.summary());
    Ok(())
}

/// The full promotion flow over one pool. The ingestion exclusion is
/// acquired first (transaction-scoped, released with the guard on every
/// exit path); a run that cannot acquire it records a `skipped` status and
/// is not queued. A validation failure is recorded on the run record, not
/// propagated as a panic; other failures mark the run `failed` before
/// propagating.
pub async fn publish(
    pool: &PgPool,
    data_dir: &Path,
    trigger: Trigger,
) -> Result<PublishReport, PublishError> {
    // The taxonomy actually used in the build: loaded from the YAML files
    // (TX-1) for the validation gate, and hashed as the manifest's
    // `taxonomy_version`.
    let taxonomy = taxonomy::loader::load_data_dir(data_dir)
        .map_err(|e| PublishError::Taxonomy(e.to_string()))?;
    let taxonomy_version = crate::support::compute_taxonomy_version(data_dir)?;

    // The ingestion exclusion is held for the whole flow on the guard's
    // transaction (released with the guard — commit, rollback, drop, or an
    // unwind — so no path leaves a stuck exclusion).
    let held = IngestionExclusion::try_acquire(pool)
        .await
        .map_err(|e| PublishError::Exclusion(e.to_string()))?;
    let Some(exclusion) = held else {
        let run_id = crate::run_records::record_terminal_run(
            pool,
            trigger.as_str(),
            "skipped",
            serde_json::json!({ "reason": "ingestion exclusion held by another run" }),
            1,
        )
        .await
        .map_err(|e| PublishError::RunRecord(e.to_string()))?;
        return Ok(PublishReport {
            run_id,
            status: "skipped".to_string(),
            candidate_generation_id: None,
            published_generation_id: None,
            already_published: false,
            counts: serde_json::json!({ "reason": "exclusion held" }),
            validation_failures: Vec::new(),
        });
    };

    let result = publish_locked(pool, &taxonomy, &taxonomy_version, trigger, 1).await;
    drop(exclusion);
    result
}

/// The promotion flow over a directory for a run that ALREADY holds the
/// ingestion exclusion (the daemon's scheduled cycle acquires it once
/// around ingest + publish): the flow body with the run record carrying
/// the cycle's attempt number.
pub async fn publish_with_exclusion_held(
    pool: &PgPool,
    data_dir: &Path,
    trigger: Trigger,
    attempt: i16,
) -> Result<PublishReport, PublishError> {
    let taxonomy = taxonomy::loader::load_data_dir(data_dir)
        .map_err(|e| PublishError::Taxonomy(e.to_string()))?;
    let taxonomy_version = crate::support::compute_taxonomy_version(data_dir)?;
    publish_locked(pool, &taxonomy, &taxonomy_version, trigger, attempt).await
}

/// The flow body, called with the exclusion held.
async fn publish_locked(
    pool: &PgPool,
    taxonomy: &taxonomy::model::Taxonomy,
    taxonomy_version: &str,
    trigger: Trigger,
    attempt: i16,
) -> Result<PublishReport, PublishError> {
    let run_id = Uuid::now_v7();
    let started_at = Utc::now();
    let result = publish_run(
        pool,
        taxonomy,
        taxonomy_version,
        trigger,
        attempt,
        run_id,
        started_at,
    )
    .await;
    if let Err(error) = &result {
        // Error paths mark the run `failed`: no run record is left `running`.
        let counts = serde_json::json!({ "error": error.to_string() });
        let _ = sqlx::query!(
            "UPDATE ingestion_runs SET finished_at = $1, status = 'failed', counts = $2 \
             WHERE run_id = $3 AND status = 'running'",
            Utc::now(),
            counts,
            run_id,
        )
        .execute(pool)
        .await;
    }
    result
}

async fn publish_run(
    pool: &PgPool,
    taxonomy: &taxonomy::model::Taxonomy,
    taxonomy_version: &str,
    trigger: Trigger,
    attempt: i16,
    run_id: Uuid,
    started_at: DateTime<Utc>,
) -> Result<PublishReport, PublishError> {
    sqlx::query!(
        "INSERT INTO ingestion_runs (run_id, trigger, started_at, status, attempt) \
         VALUES ($1, $2, $3, 'running', $4)",
        run_id,
        trigger.as_str(),
        started_at,
        attempt,
    )
    .execute(pool)
    .await
    .map_err(|e| PublishError::RunRecord(e.to_string()))?;

    // 1. Build (reads the same source ingestion writes, under the exclusion
    //    — no partial update is captured).
    let built = db::generations::build::build_generation(pool, taxonomy_version)
        .await
        .map_err(|e| PublishError::Build(e.to_string()))?;
    let counts = serde_json::json!({
        "content_hash": built.content_hash,
        "events": built.event_count,
        "procedures": built.procedure_count,
    });

    // Identical content already published: no new content version. A
    // taxonomy-only YAML change (content_hash unchanged) re-validates the
    // published manifest against the CURRENT taxonomy and re-stamps its
    // `taxonomy_version`: the API's load gate (pinned to the boot YAML)
    // would otherwise reject the manifest forever. The re-stamp is a
    // manifest metadata correction — the content-derived generation_id and
    // every projection row are untouched (task 23 reconciliation decision,
    // design §6.4; recorded in the change's apply-progress).
    if built.already_published {
        let recorded: String = sqlx::query_scalar!(
            "SELECT taxonomy_version FROM catalog_generations WHERE generation_id = $1",
            built.generation_id,
        )
        .fetch_one(pool)
        .await
        .map_err(|e| PublishError::Promotion(e.to_string()))?;
        if recorded != taxonomy_version {
            let restamp_gate = db::generations::validate::validate_generation(
                pool,
                built.generation_id,
                Some(taxonomy),
            )
            .await
            .map_err(|e| PublishError::Validation(e.to_string()))?;
            if !restamp_gate.passed() {
                let counts = serde_json::json!({
                    "content_hash": built.content_hash,
                    "events": built.event_count,
                    "procedures": built.procedure_count,
                    "validation_failures": restamp_gate
                        .failures
                        .iter()
                        .map(|f| format!("{}: {}", f.kind, f.detail))
                        .collect::<Vec<_>>(),
                });
                finish_run(
                    pool,
                    run_id,
                    "validation_failed",
                    counts.clone(),
                    Some(built.generation_id),
                    None,
                )
                .await?;
                return Ok(PublishReport {
                    run_id,
                    status: "validation_failed".to_string(),
                    candidate_generation_id: Some(built.generation_id),
                    published_generation_id: None,
                    already_published: true,
                    counts,
                    validation_failures: restamp_gate
                        .failures
                        .iter()
                        .map(|f| format!("{}: {}", f.kind, f.detail))
                        .collect(),
                });
            }
            sqlx::query!(
                "UPDATE catalog_generations SET taxonomy_version = $2 \
                 WHERE generation_id = $1 AND status = 'published'",
                built.generation_id,
                taxonomy_version,
            )
            .execute(pool)
            .await
            .map_err(|e| PublishError::Promotion(e.to_string()))?;
        }
        finish_run(
            pool,
            run_id,
            "success",
            counts.clone(),
            Some(built.generation_id),
            Some(built.generation_id),
        )
        .await?;
        return Ok(PublishReport {
            run_id,
            status: "success".to_string(),
            candidate_generation_id: Some(built.generation_id),
            published_generation_id: Some(built.generation_id),
            already_published: true,
            counts,
            validation_failures: Vec::new(),
        });
    }

    // 2. Validate the persisted artifacts (the publication validation gate).
    let gate =
        db::generations::validate::validate_generation(pool, built.generation_id, Some(taxonomy))
            .await
            .map_err(|e| PublishError::Validation(e.to_string()))?;
    let failure_messages: Vec<String> = gate
        .failures
        .iter()
        .map(|f| format!("{}: {}", f.kind, f.detail))
        .collect();
    if !gate.passed() {
        let counts = serde_json::json!({
            "content_hash": built.content_hash,
            "events": built.event_count,
            "procedures": built.procedure_count,
            "validation_failures": failure_messages,
        });
        finish_run(
            pool,
            run_id,
            "validation_failed",
            counts.clone(),
            Some(built.generation_id),
            None,
        )
        .await?;
        return Ok(PublishReport {
            run_id,
            status: "validation_failed".to_string(),
            candidate_generation_id: Some(built.generation_id),
            published_generation_id: None,
            already_published: false,
            counts,
            validation_failures: failure_messages,
        });
    }

    // 3. Promote the reference: the manifest moves to `published` only from
    //    a `validated` candidate with complete projections — artifacts were
    //    persisted before the reference is promoted, and the legacy tables
    //    (dual-write) keep their copy regardless of any later failure.
    let promoted: Option<Option<DateTime<Utc>>> = sqlx::query_scalar!(
        "UPDATE catalog_generations SET status = 'published', published_at = now() \
         WHERE generation_id = $1 AND status = 'validated' AND projection_status = 'complete' \
         RETURNING published_at",
        built.generation_id,
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| PublishError::Promotion(e.to_string()))?;
    if promoted.flatten().is_none() {
        return Err(PublishError::Promotion(format!(
            "generation {} is not a validated candidate with complete projections",
            built.generation_id
        )));
    }

    finish_run(
        pool,
        run_id,
        "success",
        counts.clone(),
        Some(built.generation_id),
        Some(built.generation_id),
    )
    .await?;

    notify_publication(pool, built.generation_id).await;

    Ok(PublishReport {
        run_id,
        status: "success".to_string(),
        candidate_generation_id: Some(built.generation_id),
        published_generation_id: Some(built.generation_id),
        already_published: false,
        counts,
        validation_failures: Vec::new(),
    })
}

/// The cross-process notification hint (S8 task 23, `LISTEN`/`NOTIFY`):
/// published after a successful promotion so a listening API detects the
/// publication faster than the next reconciliation tick. Only an
/// accelerator — a lost or failed hint changes nothing: the durable
/// manifest is the correctness source and the reconciliation interval the
/// fallback. The hint is therefore best-effort: a failure is logged and the
/// run's recorded outcome (already terminal) is untouched.
async fn notify_publication(pool: &PgPool, generation_id: Uuid) {
    let payload = generation_id.to_string();
    let result = sqlx::query!(
        "SELECT pg_notify($1, $2) AS \"notified!\"",
        crate::reconciliation::PUBLICATION_CHANNEL,
        payload,
    )
    .fetch_optional(pool)
    .await;
    if let Err(error) = result {
        eprintln!(
            "publish: notification hint failed (accelerator only, the reconciliation \
             interval covers it): {error}"
        );
    }
}

/// Finishes one run record with a terminal status, timestamps, counts, and
/// the generation references.
async fn finish_run(
    pool: &PgPool,
    run_id: Uuid,
    status: &str,
    counts: serde_json::Value,
    candidate: Option<Uuid>,
    published: Option<Uuid>,
) -> Result<(), PublishError> {
    sqlx::query!(
        "UPDATE ingestion_runs SET finished_at = $1, status = $2, counts = $3, \
         candidate_generation_id = $4, published_generation_id = $5 \
         WHERE run_id = $6",
        Utc::now(),
        status,
        counts,
        candidate,
        published,
        run_id,
    )
    .execute(pool)
    .await
    .map_err(|e| PublishError::RunRecord(e.to_string()))?;
    Ok(())
}
