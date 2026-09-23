//! FTS candidate provider (SE-7, task 78): matches the query's canonical
//! tokens against the requested generation's weighted FTS vector via
//! `plainto_tsquery`, reporting each match under the `FTS_TEXT` rule name.
//!
//! Since WU-5a (audit F15 / task T12) the provider is generation-scoped:
//! for a durable generation it ranks `generation_fts_text.fts_tsvector`
//! filtered by `generation_id`, never the mutable
//! `life_events.generated_tsvector` — a rollback to an older generation must
//! not rank the current taxonomy. The projection column is written by
//! `db::generations::build::write_projections` with the exact legacy
//! weighting (name='A' + description='B', `simple` config, unaccented
//! through `public.unaccent_immutable`), so new builds rank identically.
//! Generations captured before 0019 are backfilled from their own immutable
//! fts_text: searchable, but unweighted until rebuilt; mutable life_events
//! cannot safely reconstruct their original name/description weights.
//!
//! The legacy sentinel (`LEGACY_GENERATION_ID`, the cold-start baseline
//! before any durable generation is adopted) keeps the documented legacy
//! path over the dual-written `life_events` table; there is no generation to
//! isolate there. build.rs mints durable IDs with `Uuid::now_v7()`, never nil:
//! real generation content cannot reach this mutable fallback.
//!
//! Value scale: `ts_rank` sits in [0, ~0.1) for this column shape; the
//! contribution is `(rank * 100).round()` so a real FTS hit lands in the
//! same order as a keyword weight while never dominating an ACTION+ENTITY
//! match on its own.
//!
//! S4b (task 11): the provider is an async implementation awaited directly
//! from the orchestration layer — no synchronous bridging.

use search::engine::{CandidateProvider, ProviderFuture};
use search::types::{Candidate, NormalizedQuery};
use sqlx::PgPool;
use uuid::Uuid;

use super::{canonical_query_text, provider_failed};

pub struct FtsProvider {
    pool: PgPool,
}

impl FtsProvider {
    pub fn new(pool: PgPool) -> Self {
        FtsProvider { pool }
    }
}

impl CandidateProvider for FtsProvider {
    fn rule_name(&self) -> &'static str {
        "FTS_TEXT"
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
            // The two branches select the same (slug, rank) shape; collect to
            // a single `Vec` so the mapping below is shared.
            let rows: Vec<(String, f32)> = if generation_id == super::LEGACY_GENERATION_ID {
                // Cold-start baseline: no durable generation exists yet, so
                // the dual-written legacy table is the only surface.
                sqlx::query!(
                    r#"WITH q AS (SELECT plainto_tsquery('simple', $1) AS tsq)
                   SELECT e.slug AS "slug!", ts_rank(e.generated_tsvector, q.tsq) AS "rank!"
                   FROM life_events e, q
                   WHERE e.status = 'active'
                     AND e.generated_tsvector @@ q.tsq"#,
                    text,
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|err| provider_failed(rule_name, err))?
                .into_iter()
                .map(|row| (row.slug, row.rank))
                .collect()
            } else {
                // Generation-scoped ranking: the requested generation's own
                // immutable weighted vector, active events only.
                sqlx::query!(
                    r#"WITH q AS (SELECT plainto_tsquery('simple', $2) AS tsq)
                       SELECT g.slug AS "slug!", ts_rank(g.fts_tsvector, q.tsq) AS "rank!"
                       FROM generation_fts_text g
                       JOIN generation_life_events e
                         ON e.generation_id = g.generation_id AND e.slug = g.slug
                       CROSS JOIN q
                       WHERE g.generation_id = $1
                         AND e.status = 'active'
                         AND g.fts_tsvector @@ q.tsq"#,
                    generation_id,
                    text,
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|err| provider_failed(rule_name, err))?
                .into_iter()
                .map(|row| (row.slug, row.rank))
                .collect()
            };

            Ok(rows
                .into_iter()
                .filter_map(|(slug, rank)| {
                    let value = (f64::from(rank) * 100.0).round() as i64;
                    (value > 0).then(|| Candidate {
                        event_slug: slug,
                        rule_name: rule_name.to_string(),
                        value,
                    })
                })
                .collect())
        })
    }
}
