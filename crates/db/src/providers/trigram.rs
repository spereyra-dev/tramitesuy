//! Trigram candidate provider (SE-7, task 78): scores `pg_trgm`'s
//! `similarity()` between the query's canonical tokens and each active
//! event's name + positive-keyword surface, reporting each match under the
//! `TRIGRAM` rule name. Negative keywords never feed the surface: a
//! distinguishing negative action must not pull its event in as a candidate.
//!
//! Since WU-5a (audit F15 / task T12) the provider is generation-scoped: for
//! a durable generation it scores the requested generation's immutable
//! `generation_trigram_surface` filtered by `generation_id`, never the
//! mutable `life_events` + `life_event_keywords`. The projection replicates
//! the legacy surface content (name plus positive canonical keyword terms,
//! negatives excluded). Both aggregates pin keyword order by (term, type)
//! for new builds; older immutable generations retain their captured bytes.
//! The legacy sentinel (`LEGACY_GENERATION_ID`, the cold-start baseline)
//! keeps the documented legacy path; there is no generation to isolate there.
//! Durable IDs are minted with `Uuid::now_v7()` in build.rs, never nil, so
//! real generation content cannot reach this mutable fallback.
//!
//! Threshold `0.3` keeps only genuine fuzzy overlaps; the value scale
//! (`similarity * 10`, so at most 10 for an identical string) keeps a fuzzy
//! trigram hit one order below a real keyword match — D-1's "one fuzzy
//! trigram + one weak keyword must not open an event".
//!
//! S4b (task 11): the provider is an async implementation awaited directly
//! from the orchestration layer — no synchronous bridging.

use search::engine::{CandidateProvider, ProviderFuture};
use search::types::{Candidate, NormalizedQuery};
use sqlx::PgPool;
use uuid::Uuid;

use super::{canonical_query_text, provider_failed};

/// Minimum `similarity()` for a candidate to be reported at all.
const SIMILARITY_THRESHOLD: f32 = 0.3;

/// Contribution scale: `round(similarity * 10)` keeps fuzzy hits below the
/// `MIN_OPEN_SCORE` order of magnitude (design D-1).
pub const VALUE_SCALE: f64 = 10.0;

pub struct TrigramProvider {
    pool: PgPool,
}

impl TrigramProvider {
    pub fn new(pool: PgPool) -> Self {
        TrigramProvider { pool }
    }
}

impl CandidateProvider for TrigramProvider {
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
            // The two branches select the same (slug, similarity) shape;
            // collect to a single `Vec` so the mapping below is shared.
            let rows: Vec<(String, f32)> = if generation_id == super::LEGACY_GENERATION_ID {
                // Cold-start baseline: no durable generation exists yet, so
                // the dual-written legacy tables are the only surface.
                sqlx::query!(
                    r#"WITH kw AS (
                     SELECT life_event_id,
                            string_agg(term || ' ' || COALESCE(canonical_term, ''), ' '
                                       ORDER BY term, type) AS terms
                     FROM life_event_keywords
                     WHERE NOT negative
                     GROUP BY life_event_id
                   )
                   SELECT e.slug AS "slug!",
                          similarity(e.name || ' ' || COALESCE(kw.terms, ''), $1) AS "sim!"
                   FROM life_events e
                   LEFT JOIN kw ON kw.life_event_id = e.id
                   WHERE e.status = 'active'
                     AND similarity(e.name || ' ' || COALESCE(kw.terms, ''), $1) > $2"#,
                    text,
                    SIMILARITY_THRESHOLD,
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|err| provider_failed(rule_name, err))?
                .into_iter()
                .map(|row| (row.slug, row.sim))
                .collect()
            } else {
                // Generation-scoped ranking: the requested generation's own
                // immutable surface, active events only.
                sqlx::query!(
                    r#"SELECT g.slug AS "slug!",
                              similarity(g.surface_text, $2) AS "sim!"
                       FROM generation_trigram_surface g
                       JOIN generation_life_events e
                         ON e.generation_id = g.generation_id AND e.slug = g.slug
                       WHERE g.generation_id = $1
                         AND e.status = 'active'
                         AND similarity(g.surface_text, $2) > $3"#,
                    generation_id,
                    text,
                    SIMILARITY_THRESHOLD,
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|err| provider_failed(rule_name, err))?
                .into_iter()
                .map(|row| (row.slug, row.sim))
                .collect()
            };

            Ok(rows
                .into_iter()
                .filter_map(|(slug, sim)| {
                    let value = (f64::from(sim) * VALUE_SCALE).round() as i64;
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
