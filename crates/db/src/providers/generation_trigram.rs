//! Generation-scoped trigram candidate provider (S6 task 16, design §2.2):
//! scores `pg_trgm`'s `similarity()` against the **precomputed**
//! `generation_trigram_surface` written at build time — the same canonical
//! rules the legacy per-request provider composes today (name + positive
//! keywords with their canonical terms, negatives excluded), so candidate
//! similarity values are identical (provider equivalence tests in
//! `tests/providers.rs`).
//!
//! Threshold: `MIN_TRIGRAM_SIMILARITY/10` (0.3), strict `>` — the exact
//! semantics of the legacy provider. The GUC `pg_trgm.similarity_threshold`
//! (read by the index-compatible `surface_text % $1` predicate) is set with
//! `SET LOCAL` inside the provider's own transaction via `set_config(...,
//! is_local => true)`, so the threshold never depends on pool session state;
//! the explicit `similarity(...) > $2` belt enforces the same cut exactly.
//! The GIN `(surface_text gin_trgm_ops)` index (migration 0015) makes the
//! `%` predicate indexable.
//!
//! The legacy `TrigramProvider` stays intact for rollback (task 21 keeps the
//! legacy path until the snapshot route is verified).

use search::engine::{CandidateProvider, ProviderFuture};
use search::types::{Candidate, NormalizedQuery};
use sqlx::PgPool;
use uuid::Uuid;

use super::canonical_query_text;

/// The minimum trigram contribution in scaled units — the value scale is
/// `round(similarity * 10)` (at most 10 for an identical string), so a fuzzy
/// trigram hit stays one order below a real keyword match (design D-1).
pub const MIN_TRIGRAM_SIMILARITY: f64 = 3.0;

/// The DB-side threshold: `MIN_TRIGRAM_SIMILARITY / 10` = 0.3, applied
/// strictly (`>`), identical to the legacy provider's threshold.
pub const SIMILARITY_THRESHOLD: f64 = MIN_TRIGRAM_SIMILARITY / 10.0;

/// The threshold as the `f32` the SQL comparison and the GUC value bind to
/// (`similarity()` is `real` in PostgreSQL).
fn threshold_f32() -> f32 {
    SIMILARITY_THRESHOLD as f32
}

pub struct GenerationTrigramProvider {
    pool: PgPool,
}

impl GenerationTrigramProvider {
    pub fn new(pool: PgPool) -> Self {
        GenerationTrigramProvider { pool }
    }
}

impl CandidateProvider for GenerationTrigramProvider {
    fn rule_name(&self) -> &'static str {
        "TRIGRAM"
    }

    fn candidates<'a>(
        &'a self,
        generation_id: Uuid,
        query: &'a NormalizedQuery,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let text = canonical_query_text(query);
            if text.trim().is_empty() {
                return Ok(Vec::new());
            }
            let rule_name = self.rule_name();
            let threshold = threshold_f32();

            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|err| super::provider_failed(rule_name, err))?;
            // SET LOCAL pg_trgm.similarity_threshold = MIN_TRIGRAM_SIMILARITY/10
            // inside this transaction: the `%` operator reads the GUC, and
            // set_config(..., is_local = true) has exactly SET LOCAL's
            // transaction-scoped rollback semantics while remaining
            // parameterizable.
            sqlx::query!(
                "SELECT set_config('pg_trgm.similarity_threshold', $1, true) AS \"guc!\"",
                format!("{threshold}"),
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|err| super::provider_failed(rule_name, err))?;
            let rows = sqlx::query!(
                r#"SELECT g.slug AS "slug!",
                          similarity(g.surface_text, $1) AS "sim!"
                   FROM generation_trigram_surface g
                   JOIN generation_life_events e
                     ON e.generation_id = g.generation_id AND e.slug = g.slug
                   WHERE g.generation_id = $2 AND g.surface_text % $1
                     AND similarity(g.surface_text, $1) > $3
                     AND e.status = 'active'"#,
                text,
                generation_id,
                threshold,
            )
            .fetch_all(&mut *tx)
            .await
            .map_err(|err| super::provider_failed(rule_name, err))?;
            tx.commit()
                .await
                .map_err(|err| super::provider_failed(rule_name, err))?;

            Ok(rows
                .into_iter()
                .filter_map(|row| {
                    let value = (f64::from(row.sim) * super::trigram::VALUE_SCALE).round() as i64;
                    (value > 0).then(|| Candidate {
                        event_slug: row.slug,
                        rule_name: rule_name.to_string(),
                        value,
                    })
                })
                .collect())
        })
    }
}
