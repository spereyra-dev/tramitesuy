//! FTS candidate provider (SE-7, task 78): matches the query's canonical
//! tokens against `life_events.generated_tsvector` (migration 0011: weighted
//! `simple`-config tsvector over name='A' + description='B') via
//! `plainto_tsquery`, reporting each match under the `FTS_TEXT` rule name.
//!
//! Known limitation (recorded, apply progress): the generated column does
//! not `unaccent` the stored text, so only de-accented lexemes can match the
//! engine's de-accented query tokens; fuzzy coverage of accented surfaces is
//! the trigram provider's job. Changing the generated column would be a
//! data-model migration and is deliberately out of this unit's scope.
//!
//! Value scale: `ts_rank` sits in [0, ~0.1) for this column shape; the
//! contribution is `(rank * 100).round()` so a real FTS hit lands in the
//! same order as a keyword weight while never dominating an ACTION+ENTITY
//! match on its own.

use search::engine::{CandidateProvider, EngineError};
use search::types::{Candidate, NormalizedQuery};
use sqlx::PgPool;

use super::{bridge_block_on, canonical_query_text, provider_failed};

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

    fn candidates(&self, query: &NormalizedQuery) -> Result<Vec<Candidate>, EngineError> {
        let text = canonical_query_text(query);
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let rule_name = self.rule_name();
        let pool = self.pool.clone();
        let rows = bridge_block_on(async move {
            sqlx::query!(
                r#"WITH q AS (SELECT plainto_tsquery('simple', $1) AS tsq)
                   SELECT e.slug AS "slug!", ts_rank(e.generated_tsvector, q.tsq) AS "rank!"
                   FROM life_events e, q
                   WHERE e.status = 'active'
                     AND e.generated_tsvector @@ q.tsq"#,
                text,
            )
            .fetch_all(&pool)
            .await
        })
        .map_err(|err| provider_failed(rule_name, err))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let value = (f64::from(row.rank) * 100.0).round() as i64;
                (value > 0).then(|| Candidate {
                    event_slug: row.slug,
                    rule_name: rule_name.to_string(),
                    value,
                })
            })
            .collect())
    }
}
