//! sqlx implementation of the ingestion [`ProcedureRepository`] port
//! (task 58: DM-2, IN-6/IN-7/IN-9, D-5). All queries are compile-time-checked
//! (`sqlx::query!` against the committed `.sqlx` metadata). Each batch runs
//! in a single transaction (design §4.1); rows are never deleted, only
//! deactivated (IN-7).

use crate::repos::orgs::upsert_organization;
use ingestion::error::RepoError;
use ingestion::ports::ProcedureRepository;
use ingestion::summary::{ProcedureUpsert, RunStamp, UpsertCounts};
use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, FixedOffset, Utc};
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

/// Postgres-backed persistence for procedures, versions, and organizations.
///
/// The ingestion port is synchronous (the pipeline is a deterministic,
/// fixture-driven core), while sqlx is async: this adapter bridges the two
/// with its own runtime. Called from a synchronous context, it uses a shared
/// internal runtime; called from within a multi-thread tokio runtime (the
/// `apps/ingest` worker), it blocks via `block_in_place`. Current-thread
/// runtimes cannot host it (tokio forbids blocking there).
pub struct PostgresProcedureRepository {
    pool: PgPool,
}

impl PostgresProcedureRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Converts the port's RFC 3339 [`RunStamp`] into a bindable timestamptz
    /// at this repository boundary (recorded B2/B3 deviation: the port stays
    /// deterministic and clock-free; conversion happens here).
    fn stamp(at: &RunStamp) -> Result<DateTime<FixedOffset>, RepoError> {
        DateTime::parse_from_rfc3339(at)
            .map_err(|e| RepoError::Failed(format!("invalid run stamp {at:?}: {e}")))
    }

    fn block_on<F: std::future::Future>(&self, fut: F) -> F::Output {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => shared_runtime().block_on(fut),
        }
    }
}

/// Shared runtime for synchronous callers outside any tokio context (the
/// ingestion pipeline is sync). Single worker: the repository serializes
/// batches anyway.
fn shared_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("ingestion repository runtime")
    })
}

fn repo_err(err: impl std::fmt::Display) -> RepoError {
    RepoError::Failed(err.to_string())
}

impl ProcedureRepository for PostgresProcedureRepository {
    fn latest_hashes(&self) -> Result<HashMap<String, String>, RepoError> {
        self.block_on(async {
            let rows = sqlx::query!(
                "SELECT DISTINCT ON (p.external_id) p.external_id, v.content_hash \
                 FROM procedure_versions v \
                 JOIN procedures p ON p.id = v.procedure_id \
                 WHERE v.valid_until IS NULL \
                 ORDER BY p.external_id, v.valid_from DESC",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(repo_err)?;
            Ok(rows
                .into_iter()
                .map(|r| (r.external_id, r.content_hash))
                .collect())
        })
    }

    fn upsert_procedures(
        &self,
        rows: &[ProcedureUpsert],
        at: RunStamp,
    ) -> Result<UpsertCounts, RepoError> {
        let at = Self::stamp(&at)?;
        self.block_on(async {
            let mut tx = self.pool.begin().await.map_err(repo_err)?;
            let mut counts = UpsertCounts::default();

            for row in rows {
                // Organization upsert first (IN-8): the procedure row's FK
                // target exists inside the same transaction.
                let organization_id = upsert_organization(
                    &mut *tx,
                    &row.organization_external_id,
                    &row.organization_name,
                    at,
                )
                .await
                .map_err(repo_err)?;

                // Present rows (active or inactive) update in place: a row
                // present in the source is active again with the deactivation
                // stamp cleared (recorded B3 re-activation edge).
                let existing = sqlx::query!(
                    "SELECT id FROM procedures WHERE external_id = $1 \
                     ORDER BY (status = 'active') DESC, created_at DESC LIMIT 1",
                    row.external_id,
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(repo_err)?;
                let existing = existing.map(|record| record.id);

                let procedure_id = match existing {
                    Some(existing) => {
                        counts.updated += 1;
                        sqlx::query!(
                            "UPDATE procedures SET name = $2, description = $3, \
                             organization_id = $4, official_url = $5, status = 'active', \
                             raw_data = $6, last_seen_at = $7, deactivated_at = NULL, \
                             updated_at = $7 WHERE id = $1",
                            existing,
                            row.name,
                            row.description,
                            organization_id,
                            row.official_url,
                            row.raw_data,
                            at,
                        )
                        .execute(&mut *tx)
                        .await
                        .map_err(repo_err)?;
                        existing
                    }
                    None => {
                        counts.inserted += 1;
                        sqlx::query!(
                            "INSERT INTO procedures \
                             (external_id, name, description, organization_id, official_url, \
                              status, raw_data, first_seen_at, last_seen_at, deactivated_at, \
                              updated_at) \
                             VALUES ($1, $2, $3, $4, $5, 'active', $6, $7, $7, NULL, $7) \
                             RETURNING id",
                            row.external_id,
                            row.name,
                            row.description,
                            organization_id,
                            row.official_url,
                            row.raw_data,
                            at,
                        )
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(repo_err)?
                        .id
                    }
                };

                // Append-only versioning (DM-3): a version opens only when
                // the incoming content hash differs from the open one, so
                // unchanged re-upserts create nothing.
                let open_hash: Option<String> = sqlx::query!(
                    "SELECT content_hash FROM procedure_versions \
                     WHERE procedure_id = $1 AND valid_until IS NULL \
                     ORDER BY valid_from DESC LIMIT 1",
                    procedure_id,
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(repo_err)?
                .map(|r| r.content_hash);

                if open_hash.as_deref() != Some(row.content_hash.as_str()) {
                    sqlx::query!(
                        "INSERT INTO procedure_versions (procedure_id, content_hash, payload, \
                         valid_from) VALUES ($1, $2, $3, $4)",
                        procedure_id,
                        row.content_hash,
                        row.raw_data,
                        at,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(repo_err)?;
                }
            }

            tx.commit().await.map_err(repo_err)?;
            Ok(counts)
        })
    }

    fn close_versions(&self, ids: &[(String, String)], at: RunStamp) -> Result<(), RepoError> {
        let at = Self::stamp(&at)?;
        self.block_on(async move {
            let mut tx = self.pool.begin().await.map_err(repo_err)?;
            for (external_id, content_hash) in ids {
                sqlx::query!(
                    "UPDATE procedure_versions v SET valid_until = $1 \
                     FROM procedures p \
                     WHERE v.procedure_id = p.id AND p.external_id = $2 \
                       AND v.content_hash = $3 AND v.valid_until IS NULL",
                    at,
                    external_id,
                    content_hash,
                )
                .execute(&mut *tx)
                .await
                .map_err(repo_err)?;
            }
            tx.commit().await.map_err(repo_err)?;
            Ok(())
        })
    }

    fn deactivate_missing(
        &self,
        present_ids: &BTreeSet<String>,
        at: RunStamp,
    ) -> Result<usize, RepoError> {
        let at = Self::stamp(&at)?;
        let present: Vec<String> = present_ids.iter().cloned().collect();
        self.block_on(async move {
            let result = sqlx::query!(
                "UPDATE procedures SET status = 'inactive', deactivated_at = $1 \
                 WHERE status = 'active' AND NOT (external_id = ANY($2))",
                at,
                present as Vec<String>,
            )
            .execute(&self.pool)
            .await
            .map_err(repo_err)?;
            Ok(result.rows_affected() as usize)
        })
    }

    fn touch_last_seen(&self, ids: &[String], at: RunStamp) -> Result<(), RepoError> {
        let at = Self::stamp(&at)?;
        let ids = ids.to_vec();
        self.block_on(async move {
            sqlx::query!(
                "UPDATE procedures SET last_seen_at = $1 WHERE external_id = ANY($2)",
                at,
                ids as Vec<String>,
            )
            .execute(&self.pool)
            .await
            .map_err(repo_err)?;
            Ok(())
        })
    }

    fn all_external_ids(&self) -> Result<Vec<String>, RepoError> {
        self.block_on(async {
            let rows =
                sqlx::query!("SELECT DISTINCT external_id FROM procedures ORDER BY external_id",)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(repo_err)?;
            Ok(rows.into_iter().map(|r| r.external_id).collect())
        })
    }
}

// ---------------------------------------------------------------------------
// Read-side queries for the API read surface (tasks 71/73; design §7 maps
// GET /events/:slug to `repos::procedures::by_event` and GET /procedures/:id
// to `repos::procedures::by_external_id`). Justified crates/db addition: D-5
// forbids SQL in apps/api, and the allowed-surface note for C1 requires any
// db addition to be justified here — these are exactly the read queries the
// C1 endpoint contracts need.
// ---------------------------------------------------------------------------

/// Event metadata for the event page (task 71).
#[derive(Debug)]
pub struct EventMeta {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub category_slug: String,
}

/// One related procedure on the event page, ordered by `order_index` (TX-6).
#[derive(Debug)]
pub struct EventProcedure {
    pub external_id: String,
    pub name: String,
    pub official_url: Option<String>,
    pub raw_data: Option<serde_json::Value>,
    pub last_seen_at: DateTime<Utc>,
    pub order_index: i32,
    pub required: bool,
}

/// The full event-page projection: event metadata plus its relations.
#[derive(Debug)]
pub struct EventProcedures {
    pub event: EventMeta,
    pub procedures: Vec<EventProcedure>,
}

/// One procedure with its organization name, for the detail endpoint (task
/// 73). Inactive procedures remain fetchable (API-8): the active row wins
/// when several rows share an external_id, and the latest row otherwise.
#[derive(Debug)]
pub struct ProcedureDetail {
    pub external_id: String,
    pub name: String,
    pub description: Option<String>,
    pub organization_name: Option<String>,
    pub official_url: Option<String>,
    pub status: String,
    pub raw_data: Option<serde_json::Value>,
    pub last_seen_at: DateTime<Utc>,
}

/// Loads the event-page projection for one slug, or None for an unknown
/// slug. Relations are ordered by `order_index` (TX-6); deactivated
/// procedures keep their relation (rows are never deleted, IN-7) and stay
/// on the page with their attribution intact.
pub async fn by_event(pool: &PgPool, slug: &str) -> Result<Option<EventProcedures>, sqlx::Error> {
    let meta = sqlx::query!(
        "SELECT e.slug, e.name, e.description, c.slug AS category_slug \
         FROM life_events e JOIN categories c ON c.id = e.category_id \
         WHERE e.slug = $1",
        slug,
    )
    .fetch_optional(pool)
    .await?;
    let Some(meta) = meta else {
        return Ok(None);
    };

    let rows = sqlx::query!(
        "SELECT p.external_id, p.name, p.official_url, p.raw_data, p.last_seen_at, \
                r.order_index, r.required \
         FROM life_event_procedures r \
         JOIN procedures p ON p.id = r.procedure_id \
         JOIN life_events e ON e.id = r.life_event_id \
         WHERE e.slug = $1 \
         ORDER BY r.order_index",
        slug,
    )
    .fetch_all(pool)
    .await?;

    Ok(Some(EventProcedures {
        event: EventMeta {
            slug: meta.slug,
            name: meta.name,
            description: meta.description,
            category_slug: meta.category_slug,
        },
        procedures: rows
            .into_iter()
            .map(|r| EventProcedure {
                external_id: r.external_id,
                name: r.name,
                official_url: r.official_url,
                raw_data: r.raw_data,
                last_seen_at: r.last_seen_at,
                order_index: r.order_index,
                required: r.required,
            })
            .collect(),
    }))
}

/// Loads one procedure by external id with its organization name, or None
/// for an unknown id. Active rows win over inactive ones sharing the id
/// (the partial unique index only constrains active rows), matching the
/// upsert's present-row selection; an inactive-only id still resolves so a
/// deactivated procedure stays fetchable with `status: "inactive"` (API-8).
pub async fn by_external_id(
    pool: &PgPool,
    external_id: &str,
) -> Result<Option<ProcedureDetail>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT p.external_id, p.name, p.description, \
                o.name AS \"organization_name: Option<String>\", \
                p.official_url, p.status, p.raw_data, p.last_seen_at \
         FROM procedures p LEFT JOIN organizations o ON o.id = p.organization_id \
         WHERE p.external_id = $1 \
         ORDER BY (p.status = 'active') DESC, p.created_at DESC \
         LIMIT 1",
        external_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| ProcedureDetail {
        external_id: r.external_id,
        name: r.name,
        description: r.description,
        organization_name: r.organization_name,
        official_url: r.official_url,
        status: r.status,
        raw_data: r.raw_data,
        last_seen_at: r.last_seen_at,
    }))
}

/// One transition card (task 7, OPT-06): the fields the search payload uses
/// for its event-procedure summary (slug, name, order, required, official
/// URL, last-seen attribution stamp, reported-cost text) plus the projection
/// fields the stage-3 generation card table will store (importance,
/// organization short name, status). No `raw_data` is transported: the
/// missing-cost rule (API-3) is evaluated once here in SQL, exactly matching
/// the API's `cost_fields_from_raw` — reported only when BOTH source columns
/// are non-empty JSON strings, in which case `cost` is the verbatim `valor`.
#[derive(Debug)]
pub struct EventCard {
    pub slug: String,
    pub name: String,
    pub order_index: i32,
    pub importance: Option<String>,
    pub required: bool,
    pub organization_short_name: Option<String>,
    pub cost: Option<String>,
    pub status: String,
    pub official_url: Option<String>,
    pub last_seen_at: DateTime<Utc>,
}

/// Loads the transition cards for one slug in a SINGLE statement, or None
/// for an unknown slug or an event with no relations (the search payload
/// serves an empty procedures summary for both, exactly like today's
/// `by_event` path, which stays intact for rollback). Relations are ordered
/// by `order_index` then external id (TX-6); deactivated procedures keep
/// their card with their current status (IN-7).
pub async fn cards_by_event(
    pool: &PgPool,
    slug: &str,
) -> Result<Option<Vec<EventCard>>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT r.order_index, r.importance, r.required, \
                p.external_id AS \"slug\", p.name, p.status, p.official_url, p.last_seen_at, \
                o.short_name AS \"organization_short_name\", \
                CASE \
                    WHEN jsonb_typeof(p.raw_data -> 'tiene_costo') = 'string' \
                         AND btrim(p.raw_data ->> 'tiene_costo') <> '' \
                         AND jsonb_typeof(p.raw_data -> 'valor') = 'string' \
                         AND btrim(p.raw_data ->> 'valor') <> '' \
                    THEN p.raw_data ->> 'valor' \
                END AS \"cost\" \
         FROM life_event_procedures r \
         JOIN procedures p ON p.id = r.procedure_id \
         JOIN life_events e ON e.id = r.life_event_id \
         LEFT JOIN organizations o ON o.id = p.organization_id \
         WHERE e.slug = $1 \
         ORDER BY r.order_index, p.external_id",
        slug,
    )
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        rows.into_iter()
            .map(|r| EventCard {
                slug: r.slug,
                name: r.name,
                order_index: r.order_index,
                importance: r.importance,
                required: r.required,
                organization_short_name: r.organization_short_name,
                cost: r.cost,
                status: r.status,
                official_url: r.official_url,
                last_seen_at: r.last_seen_at,
            })
            .collect(),
    ))
}
