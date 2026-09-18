//! Trigram candidate provider (SE-7, task 78): scores `pg_trgm`'s
//! `similarity()` between the query's canonical tokens and each active
//! event's name + positive-keyword surface, reporting each match under the
//! `TRIGRAM` rule name. Negative keywords never feed the surface: a
//! distinguishing negative action must not pull its event in as a candidate.
//!
//! Threshold `0.3` keeps only genuine fuzzy overlaps; the value scale
//! (`similarity * 10`, so at most 10 for an identical string) keeps a fuzzy
//! trigram hit one order below a real keyword match — D-1's "one fuzzy
//! trigram + one weak keyword must not open an event".

use search::engine::{CandidateProvider, EngineError};
use search::types::{Candidate, NormalizedQuery};
use sqlx::PgPool;

use super::{bridge_block_on, canonical_query_text, provider_failed};

/// Minimum `similarity()` for a candidate to be reported at all.
const SIMILARITY_THRESHOLD: f32 = 0.3;

/// Contribution scale: `round(similarity * 10)` keeps fuzzy hits below the
/// `MIN_OPEN_SCORE` order of magnitude (design D-1).
const VALUE_SCALE: f64 = 10.0;

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

    fn candidates(&self, query: &NormalizedQuery) -> Result<Vec<Candidate>, EngineError> {
        let text = canonical_query_text(query);
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let rule_name = self.rule_name();
        let pool = self.pool.clone();
        let rows = bridge_block_on(async move {
            sqlx::query!(
                r#"WITH kw AS (
                     SELECT life_event_id,
                            string_agg(term || ' ' || COALESCE(canonical_term, ''), ' ')
                                AS terms
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
            .fetch_all(&pool)
            .await
        })
        .map_err(|err| provider_failed(rule_name, err))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let value = (f64::from(row.sim) * VALUE_SCALE).round() as i64;
                (value > 0).then(|| Candidate {
                    event_slug: row.slug,
                    rule_name: rule_name.to_string(),
                    value,
                })
            })
            .collect())
    }
}
