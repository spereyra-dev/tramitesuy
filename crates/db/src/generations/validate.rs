//! Publication validation gate (S6 task 17, OPT-02/OPT-03, R8, design §6.3):
//! every publication is validated before its reference is promoted. The gate
//! reads the durable generation artifacts only:
//!
//! - **manifest**: the generation exists and its projections report complete
//!   (`projection_status = 'complete'`) — an interrupted build (design §6.4)
//!   is never a publication candidate;
//! - **empty catalog**: an accidentally empty source (zero events, zero
//!   procedures, or zero *active* procedures) is rejected;
//! - **relation integrity**: every projected card resolves to a projected
//!   procedure detail and to a projected event (no dangling relations);
//! - **schema**: every projected JSON artifact carries the keys the API
//!   surface reads;
//! - **search-projection availability**: every declared event has its FTS
//!   and trigram rows (OPT-07: providers never rank against a partial
//!   surface);
//! - **taxonomy**: when the caller passes the YAML taxonomy actually used in
//!   the build, the projected events/keywords must align with it.
//!
//! Individual invalid source rows keep the ingestion pipeline's
//! skip-and-report policy: they never fail the generation's validation —
//! the gate rejects structural failures, not data quirks the run already
//! reported. A passing gate advances `status` to `validated` (idempotent);
//! a failing gate never advances the manifest status, and a `published` row
//! is never touched by the gate.

use sqlx::PgPool;
use taxonomy::model::Taxonomy;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationFailure {
    /// The gate that failed: `manifest` | `incomplete_projections` |
    /// `empty_catalog` | `empty_active_catalog` | `relation_integrity` |
    /// `schema` | `search_projection` | `taxonomy`.
    pub kind: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub generation_id: Uuid,
    pub failures: Vec<ValidationFailure>,
}

impl ValidationReport {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Validates one generation and, on success, advances its manifest status to
/// `validated` (idempotent: re-validating a `validated` or `published`
/// generation changes nothing). The failures are returned to the caller —
/// recording them on the matching `ingestion_runs` row belongs to the
/// publish flow (task 18), keeping this gate a pure db-layer check.
pub async fn validate_generation(
    pool: &PgPool,
    generation_id: Uuid,
    taxonomy: Option<&Taxonomy>,
) -> Result<ValidationReport, sqlx::Error> {
    let manifest = sqlx::query!(
        "SELECT status, projection_status, event_count, procedure_count \
         FROM catalog_generations WHERE generation_id = $1",
        generation_id,
    )
    .fetch_optional(pool)
    .await?;

    let Some(manifest) = manifest else {
        return Ok(ValidationReport {
            generation_id,
            failures: vec![ValidationFailure {
                kind: "manifest",
                detail: format!("no manifest row for generation {generation_id}"),
            }],
        });
    };

    // An interrupted build is never a publication candidate (design §6.4):
    // complete artifacts must be persisted before their reference is promoted.
    if manifest.projection_status != "complete" {
        return Ok(ValidationReport {
            generation_id,
            failures: vec![ValidationFailure {
                kind: "incomplete_projections",
                detail: format!(
                    "projection_status = {:?}; a build with incomplete projections never validates",
                    manifest.projection_status
                ),
            }],
        });
    }

    let mut failures: Vec<ValidationFailure> = Vec::new();

    // An accidentally empty catalog is rejected (ingestion delta, OPT-02).
    if manifest.event_count == 0 {
        failures.push(ValidationFailure {
            kind: "empty_catalog",
            detail: "the built catalog declares zero events".to_string(),
        });
    }
    if manifest.procedure_count == 0 {
        failures.push(ValidationFailure {
            kind: "empty_catalog",
            detail: "the built catalog declares zero procedures".to_string(),
        });
    }
    if !failures.is_empty() {
        return Ok(ValidationReport {
            generation_id,
            failures,
        });
    }

    // The manifest counters count every projected procedure, inactive rows
    // included, so a catalog whose procedures are all inactive passes the
    // checks above. A publication with no usable procedure is just as broken
    // as one with none at all, so require at least one active procedure from
    // the generation's own immutable projection — the manifest cannot tell.
    let active_procedure_count: i64 = sqlx::query_scalar!(
        "SELECT count(*) AS \"count!\" FROM generation_procedure_details \
         WHERE generation_id = $1 AND details ->> 'status' = 'active'",
        generation_id,
    )
    .fetch_one(pool)
    .await?;
    if active_procedure_count == 0 {
        failures.push(ValidationFailure {
            kind: "empty_active_catalog",
            detail: "the candidate declares no active procedure".to_string(),
        });
        // Accumulate and keep checking: a corrupted candidate may also carry
        // dangling relations or schema drift, and the report must name every
        // applicable failure instead of short-circuiting on the first one.
    }

    validate_search_projections(pool, generation_id, &mut failures).await?;
    validate_projection_schema(pool, generation_id, &mut failures).await?;
    // Relation SQL expands card arrays and requires a string slug. Schema
    // errors already reject publication; never let malformed JSON abort the
    // gate before it can return that report.
    if !failures.iter().any(|failure| failure.kind == "schema") {
        validate_relation_integrity(pool, generation_id, &mut failures).await?;
    }
    if let Some(taxonomy) = taxonomy {
        validate_taxonomy(pool, generation_id, taxonomy, &mut failures).await?;
    }

    let report = ValidationReport {
        generation_id,
        failures,
    };
    if report.passed() {
        // Status only advances (building → validated); a published row is
        // never touched by the gate.
        sqlx::query!(
            "UPDATE catalog_generations SET status = 'validated' \
             WHERE generation_id = $1 AND status = 'building'",
            generation_id,
        )
        .execute(pool)
        .await?;
    }
    Ok(report)
}

/// Every declared event must have its FTS and trigram search-projection rows.
async fn validate_search_projections(
    pool: &PgPool,
    generation_id: Uuid,
    failures: &mut Vec<ValidationFailure>,
) -> Result<(), sqlx::Error> {
    let missing_fts: Vec<String> = sqlx::query_scalar!(
        "SELECT e.slug FROM generation_life_events e \
         WHERE e.generation_id = $1 \
           AND NOT EXISTS (SELECT 1 FROM generation_fts_text f \
                           WHERE f.generation_id = e.generation_id AND f.slug = e.slug)",
        generation_id,
    )
    .fetch_all(pool)
    .await?;
    for slug in &missing_fts {
        failures.push(ValidationFailure {
            kind: "search_projection",
            detail: format!("declared event {slug:?} has no generation_fts_text row"),
        });
    }
    let missing_trigram: Vec<String> = sqlx::query_scalar!(
        "SELECT e.slug FROM generation_life_events e \
         WHERE e.generation_id = $1 \
           AND NOT EXISTS (SELECT 1 FROM generation_trigram_surface t \
                           WHERE t.generation_id = e.generation_id AND t.slug = e.slug)",
        generation_id,
    )
    .fetch_all(pool)
    .await?;
    for slug in &missing_trigram {
        failures.push(ValidationFailure {
            kind: "search_projection",
            detail: format!("declared event {slug:?} has no generation_trigram_surface row"),
        });
    }
    Ok(())
}

/// Relation integrity: every projected card resolves to a projected
/// procedure detail and to a projected event.
async fn validate_relation_integrity(
    pool: &PgPool,
    generation_id: Uuid,
    failures: &mut Vec<ValidationFailure>,
) -> Result<(), sqlx::Error> {
    let cards = sqlx::query!(
        r#"SELECT e.slug AS "event_slug!",
                  (jsonb_array_elements(c.cards) ->> 'slug') AS "procedure_slug!"
           FROM generation_event_cards c
           JOIN generation_life_events e
             ON e.generation_id = c.generation_id AND e.slug = c.slug
           WHERE c.generation_id = $1"#,
        generation_id,
    )
    .fetch_all(pool)
    .await?;

    let mut dangling: Vec<String> = Vec::new();
    for card in cards {
        let (event_slug, procedure_slug) = (card.event_slug, card.procedure_slug);
        let detail = sqlx::query!(
            "SELECT 1 AS \"ok!\" FROM generation_procedure_details \
             WHERE generation_id = $1 AND slug = $2",
            generation_id,
            procedure_slug,
        )
        .fetch_optional(pool)
        .await?;
        if detail.is_none() {
            dangling.push(format!("{event_slug}→{procedure_slug}"));
        }
    }
    if !dangling.is_empty() {
        failures.push(ValidationFailure {
            kind: "relation_integrity",
            detail: format!(
                "cards reference procedures with no projected details: {}",
                dangling.join(", ")
            ),
        });
    }
    Ok(())
}

/// Match the API's `decode_card`/`decode_detail` contract exactly for
/// mandatory values, and reject malformed non-null optional strings rather
/// than silently erasing their content. Absent/null optional values are fine.
async fn validate_projection_schema(
    pool: &PgPool,
    generation_id: Uuid,
    failures: &mut Vec<ValidationFailure>,
) -> Result<(), sqlx::Error> {
    let cards = sqlx::query!(
        "SELECT slug, cards FROM generation_event_cards WHERE generation_id = $1",
        generation_id,
    )
    .fetch_all(pool)
    .await?;
    for row in cards {
        let valid = row.cards.as_array().is_some_and(|cards| {
            cards.iter().all(|card| {
                required_strings(card, &["slug", "name", "status"])
                    && optional_strings(
                        card,
                        &[
                            "importance",
                            "organization_short_name",
                            "cost",
                            "official_url",
                        ],
                    )
                    && card
                        .get("order_index")
                        .and_then(|v| v.as_i64())
                        .is_some_and(|v| i32::try_from(v).is_ok())
                    && card.get("required").and_then(|v| v.as_bool()).is_some()
                    && valid_timestamp(card)
            })
        });
        if !valid {
            failures.push(ValidationFailure {
                kind: "schema",
                detail: format!("card projection for event {:?} cannot be decoded", row.slug),
            });
        }
    }

    let details = sqlx::query!(
        "SELECT slug, details FROM generation_procedure_details WHERE generation_id = $1",
        generation_id,
    )
    .fetch_all(pool)
    .await?;
    for row in details {
        if !(required_strings(&row.details, &["external_id", "name", "status"])
            && optional_strings(
                &row.details,
                &["description", "organization_name", "official_url"],
            )
            && valid_timestamp(&row.details))
        {
            failures.push(ValidationFailure {
                kind: "schema",
                detail: format!(
                    "procedure-detail projection {:?} cannot be decoded",
                    row.slug
                ),
            });
        }
    }
    Ok(())
}

fn required_strings(value: &serde_json::Value, fields: &[&str]) -> bool {
    fields
        .iter()
        .all(|field| value.get(*field).and_then(|v| v.as_str()).is_some())
}

fn optional_strings(value: &serde_json::Value, fields: &[&str]) -> bool {
    fields.iter().all(|field| {
        value
            .get(*field)
            .is_none_or(|v| v.is_null() || v.is_string())
    })
}

fn valid_timestamp(value: &serde_json::Value) -> bool {
    value
        .get("last_seen_at")
        .and_then(|v| v.as_str())
        .is_some_and(|raw| chrono::DateTime::parse_from_rfc3339(raw).is_ok())
}

/// Taxonomy: the YAML taxonomy actually used in the build must align with the
/// projection — same event slugs, names, categories, and the same keyword
/// sets (canonical term + negative flag per event).
async fn validate_taxonomy(
    pool: &PgPool,
    generation_id: Uuid,
    taxonomy: &Taxonomy,
    failures: &mut Vec<ValidationFailure>,
) -> Result<(), sqlx::Error> {
    let projected = sqlx::query!(
        r#"SELECT slug, name, category_slug FROM generation_life_events
           WHERE generation_id = $1 ORDER BY slug"#,
        generation_id,
    )
    .fetch_all(pool)
    .await?;
    let mut projected_by_slug: std::collections::HashMap<String, (String, String)> = projected
        .into_iter()
        .map(|row| (row.slug, (row.name, row.category_slug)))
        .collect();

    let mut source_events = taxonomy.events.clone();
    source_events.sort_by(|a, b| a.event.slug.cmp(&b.event.slug));
    for source in source_events {
        let event = &source.event;
        match projected_by_slug.remove(&event.slug) {
            None => failures.push(ValidationFailure {
                kind: "taxonomy",
                detail: format!(
                    "the YAML event {:?} is missing from the projection",
                    event.slug
                ),
            }),
            Some((name, category)) => {
                if name != event.name || category != event.category {
                    failures.push(ValidationFailure {
                        kind: "taxonomy",
                        detail: format!(
                            "the YAML event {:?} drifted from the projection",
                            event.slug
                        ),
                    });
                }
                validate_event_keywords(pool, generation_id, event, failures).await?;
            }
        }
    }
    // A projected event the YAML taxonomy does not declare is drift too.
    for slug in projected_by_slug.keys() {
        failures.push(ValidationFailure {
            kind: "taxonomy",
            detail: format!("the projected event {slug:?} is not declared by the YAML taxonomy"),
        });
    }
    Ok(())
}

/// One event's projected keywords (canonical term + negative flag) must match
/// the YAML taxonomy's keywords exactly.
async fn validate_event_keywords(
    pool: &PgPool,
    generation_id: Uuid,
    event: &taxonomy::model::Event,
    failures: &mut Vec<ValidationFailure>,
) -> Result<(), sqlx::Error> {
    let projected = sqlx::query!(
        r#"SELECT positive_keywords AS "positive!", negative_keywords AS "negative!"
           FROM generation_life_events
           WHERE generation_id = $1 AND slug = $2"#,
        generation_id,
        event.slug,
    )
    .fetch_optional(pool)
    .await?;
    let Some(row) = projected else {
        return Ok(()); // already reported as a missing event
    };

    let mut projected_terms: std::collections::BTreeSet<(String, bool)> =
        std::collections::BTreeSet::new();
    // The array membership IS the negative flag: the projection splits the
    // keywords into positive/negative arrays (build.rs), and each entry
    // carries term/canonical/type/weight.
    let positive_values = row.positive.as_array().cloned().unwrap_or_default();
    let negative_values = row.negative.as_array().cloned().unwrap_or_default();
    for value in positive_values {
        projected_terms.insert((effective_term(&value), false));
    }
    for value in negative_values {
        projected_terms.insert((effective_term(&value), true));
    }
    let expected: std::collections::BTreeSet<(String, bool)> = event
        .keywords
        .iter()
        .map(|k| (k.canonical_or_term().to_string(), k.negative))
        .collect();
    if projected_terms != expected {
        failures.push(ValidationFailure {
            kind: "taxonomy",
            detail: format!(
                "the projected keywords of event {:?} drifted from the YAML taxonomy",
                event.slug
            ),
        });
    }
    Ok(())
}

/// The projected keyword's effective matching term: the canonical term when
/// present, otherwise the term itself (the engine's canonical-term rule).
fn effective_term(value: &serde_json::Value) -> String {
    let term = value
        .get("term")
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_string();
    let canonical = value
        .get("canonical")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
    if canonical.is_empty() {
        term
    } else {
        canonical
    }
}
