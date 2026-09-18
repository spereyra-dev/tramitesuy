//! Organization persistence over sqlx (IN-8, D-4): one row per source
//! `institucion_oid`, name from `institucion_nombre`, upserted by
//! `external_id`. Parent-organization fields stay inside
//! `procedures.raw_data` JSONB — no org hierarchy is modeled.

use sqlx::PgExecutor;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, FixedOffset};

/// Upserts one organization keyed by the source oid and returns its row id.
/// `created_at` is preserved across conflicts; the name and `updated_at`
/// follow the source row. Must be called inside the caller's transaction so
/// the batch stays atomic (design §4.1).
pub async fn upsert_organization<'e, E>(
    executor: E,
    external_id: &str,
    name: &str,
    at: DateTime<FixedOffset>,
) -> Result<Uuid, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    let row = sqlx::query!(
        "INSERT INTO organizations (external_id, name, updated_at) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (external_id) DO UPDATE \
           SET name = EXCLUDED.name, updated_at = EXCLUDED.updated_at \
         RETURNING id",
        external_id,
        name,
        at
    )
    .fetch_one(executor)
    .await?;
    Ok(row.id)
}
