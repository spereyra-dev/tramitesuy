//! Taxonomy seed projection (DM-1, design §2 `repos::taxonomy_seed`): writes
//! the YAML taxonomy — categories, life events, keywords, synonyms, and
//! event→procedure relations — into the database, idempotent per slug: a
//! second run over an unchanged taxonomy inserts nothing, updates nothing,
//! and leaves every `order_index` byte-identical.
//!
//! This is the one sqlx module that projects YAML seed data; the YAML stays
//! the single source of truth (TX-1) and these tables are projections.

use sqlx::PgPool;
use sqlx::postgres::PgConnection;
use sqlx::types::Uuid;
use taxonomy::model::{Category, Event, Keyword, KeywordType, Taxonomy};

/// Counts from one seeding pass. Second runs over an unchanged taxonomy
/// report zero inserts, zero updates, zero removals, and zero pending
/// relations (idempotency observable at the command boundary, task 69).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SeedReport {
    pub categories_inserted: usize,
    pub categories_updated: usize,
    pub events_inserted: usize,
    pub events_updated: usize,
    pub keywords_inserted: usize,
    pub keywords_removed: usize,
    pub synonyms_inserted: usize,
    pub synonyms_removed: usize,
    pub relations_written: usize,
    pub relations_pending: usize,
    pub warnings: Vec<String>,
}

impl SeedReport {
    /// Deterministic, byte-stable rendering (the worker prints it to stdout).
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "categories inserted={} updated={}\n",
            self.categories_inserted, self.categories_updated
        ));
        out.push_str(&format!(
            "events inserted={} updated={}\n",
            self.events_inserted, self.events_updated
        ));
        out.push_str(&format!(
            "keywords inserted={} removed={}\n",
            self.keywords_inserted, self.keywords_removed
        ));
        out.push_str(&format!(
            "synonyms inserted={} removed={}\n",
            self.synonyms_inserted, self.synonyms_removed
        ));
        out.push_str(&format!(
            "relations written={} pending={}\n",
            self.relations_written, self.relations_pending
        ));
        for warning in &self.warnings {
            out.push_str(&format!("pending {warning}\n"));
        }
        out
    }
}

fn keyword_type_name(kind: KeywordType) -> &'static str {
    match kind {
        KeywordType::Action => "ACTION",
        KeywordType::Entity => "ENTITY",
        KeywordType::Modifier => "MODIFIER",
        KeywordType::Context => "CONTEXT",
    }
}

/// Seeds the whole taxonomy in one transaction. Missing procedures (the
/// ingestion run has not happened yet) leave their relations pending with a
/// warning — seeding must not fail while `make dev` seeds before ingest.
pub async fn seed_taxonomy(pool: &PgPool, taxonomy: &Taxonomy) -> Result<SeedReport, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut report = SeedReport::default();

    for source in &taxonomy.categories {
        upsert_category(&mut tx, &source.category, &mut report).await?;
    }
    for source in &taxonomy.events {
        upsert_event(&mut tx, &source.event, &mut report).await?;
    }
    for source in &taxonomy.events {
        seed_relations(&mut tx, &source.event, &mut report).await?;
    }
    seed_synonyms(&mut tx, taxonomy, &mut report).await?;

    tx.commit().await?;
    Ok(report)
}

async fn upsert_category(
    tx: &mut PgConnection,
    category: &Category,
    report: &mut SeedReport,
) -> Result<(), sqlx::Error> {
    let existing: Option<(Uuid, String, Option<String>, i32)> =
        sqlx::query_as("SELECT id, name, icon, order_index FROM categories WHERE slug = $1")
            .bind(&category.slug)
            .fetch_optional(&mut *tx)
            .await?;

    match existing {
        None => {
            sqlx::query(
                "INSERT INTO categories (slug, name, icon, order_index) VALUES ($1, $2, $3, $4)",
            )
            .bind(&category.slug)
            .bind(&category.name)
            .bind(&category.icon)
            .bind(category.order_index as i32)
            .execute(&mut *tx)
            .await?;
            report.categories_inserted += 1;
        }
        Some((_, name, icon, order_index))
            if name == category.name
                && icon == category.icon
                && order_index == category.order_index as i32 => {}
        Some((id, ..)) => {
            sqlx::query(
                "UPDATE categories SET name = $2, icon = $3, order_index = $4 WHERE id = $1",
            )
            .bind(id)
            .bind(&category.name)
            .bind(&category.icon)
            .bind(category.order_index as i32)
            .execute(&mut *tx)
            .await?;
            report.categories_updated += 1;
        }
    }
    Ok(())
}

async fn upsert_event(
    tx: &mut PgConnection,
    event: &Event,
    report: &mut SeedReport,
) -> Result<(), sqlx::Error> {
    let category_id: Uuid = sqlx::query_scalar("SELECT id FROM categories WHERE slug = $1")
        .bind(&event.category)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            sqlx::Error::Configuration(
                format!("category {:?} is not seeded (TX-3)", event.category).into(),
            )
        })?;

    let existing: Option<(Uuid, String, Option<String>, Uuid)> = sqlx::query_as(
        "SELECT id, name, description, category_id FROM life_events WHERE slug = $1",
    )
    .bind(&event.slug)
    .fetch_optional(&mut *tx)
    .await?;

    let event_id = match existing {
        None => {
            report.events_inserted += 1;
            sqlx::query_as::<_, (Uuid,)>(
                "INSERT INTO life_events (slug, name, description, category_id, status) \
                 VALUES ($1, $2, $3, $4, 'active') RETURNING id",
            )
            .bind(&event.slug)
            .bind(&event.name)
            .bind(&event.description)
            .bind(category_id)
            .fetch_one(&mut *tx)
            .await?
            .0
        }
        Some((id, name, description, category))
            if name == event.name
                && description.as_deref() == Some(event.description.as_str())
                && category == category_id =>
        {
            id // byte-identical: no write (idempotency)
        }
        Some((id, ..)) => {
            report.events_updated += 1;
            sqlx::query(
                "UPDATE life_events SET name = $2, description = $3, category_id = $4, \
                 status = 'active', updated_at = now() WHERE id = $1",
            )
            .bind(id)
            .bind(&event.name)
            .bind(&event.description)
            .bind(category_id)
            .execute(&mut *tx)
            .await?;
            id
        }
    };

    seed_keywords(tx, event, event_id, report).await?;
    Ok(())
}

/// Projects the event's typed keywords, idempotent on the natural key
/// (term, type, negative, canonical): missing keywords are inserted, changed
/// weights are updated in place, and keywords the YAML no longer declares
/// are removed — the table is a projection of the YAML (TX-1).
async fn seed_keywords(
    tx: &mut PgConnection,
    event: &Event,
    event_id: Uuid,
    report: &mut SeedReport,
) -> Result<(), sqlx::Error> {
    // (id, term, canonical, type, negative, weight)
    let existing: Vec<(Uuid, String, Option<String>, String, bool, i32)> = sqlx::query_as(
        "SELECT id, term, COALESCE(canonical_term, ''), type, negative, weight \
         FROM life_event_keywords WHERE life_event_id = $1",
    )
    .bind(event_id)
    .fetch_all(&mut *tx)
    .await?;

    let desired_key = |keyword: &Keyword| {
        (
            keyword.term.clone(),
            keyword.canonical.clone(),
            keyword_type_name(keyword.keyword_type).to_string(),
            keyword.negative,
        )
    };

    for keyword in &event.keywords {
        let key = desired_key(keyword);
        let existing_match = existing
            .iter()
            .find(|(_, term, canonical, kind, negative, _)| {
                (
                    term.as_str(),
                    canonical.as_deref().unwrap_or(""),
                    kind.as_str(),
                    *negative,
                ) == (key.0.as_str(), key.1.as_str(), key.2.as_str(), key.3)
            });
        match existing_match {
            None => {
                let canonical = if keyword.canonical.is_empty() {
                    None
                } else {
                    Some(keyword.canonical.clone())
                };
                sqlx::query(
                    "INSERT INTO life_event_keywords \
                     (life_event_id, term, canonical_term, type, weight, negative) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(event_id)
                .bind(&keyword.term)
                .bind(canonical)
                .bind(keyword_type_name(keyword.keyword_type))
                .bind(keyword.weight as i32)
                .bind(keyword.negative)
                .execute(&mut *tx)
                .await?;
                report.keywords_inserted += 1;
            }
            Some((id, .., stored_weight)) if *stored_weight != keyword.weight as i32 => {
                sqlx::query("UPDATE life_event_keywords SET weight = $2 WHERE id = $1")
                    .bind(id)
                    .bind(keyword.weight as i32)
                    .execute(&mut *tx)
                    .await?;
            }
            Some(_) => {} // byte-identical: no write
        }
    }

    // Keywords the YAML no longer declares are removed (projection).
    let desired: std::collections::HashSet<(String, String, String, bool)> = event
        .keywords
        .iter()
        .map(|keyword| {
            (
                keyword.term.clone(),
                keyword.canonical.clone(),
                keyword_type_name(keyword.keyword_type).to_string(),
                keyword.negative,
            )
        })
        .collect();
    for (id, term, canonical, kind, negative, _) in &existing {
        let key = (
            term.clone(),
            canonical.clone().unwrap_or_default(),
            kind.clone(),
            *negative,
        );
        if !desired.contains(&key) {
            sqlx::query("DELETE FROM life_event_keywords WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            report.keywords_removed += 1;
        }
    }
    Ok(())
}

/// Projects the event→procedure relations (TX-6): resolves the procedure by
/// its external id; a procedure that ingestion has not created yet leaves
/// the relation pending with a deterministic warning.
async fn seed_relations(
    tx: &mut PgConnection,
    event: &Event,
    report: &mut SeedReport,
) -> Result<(), sqlx::Error> {
    if event.relations.is_empty() {
        return Ok(());
    }
    let event_id: Uuid = sqlx::query_scalar("SELECT id FROM life_events WHERE slug = $1")
        .bind(&event.slug)
        .fetch_one(&mut *tx)
        .await?;

    for relation in &event.relations {
        let procedure_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM procedures WHERE external_id = $1 \
             ORDER BY (status = 'active') DESC, created_at DESC LIMIT 1",
        )
        .bind(&relation.external_id)
        .fetch_optional(&mut *tx)
        .await?;

        let procedure_id = match procedure_id {
            Some(id) => id,
            None => {
                report.relations_pending += 1;
                report.warnings.push(format!(
                    "relation external_id={} of event {} pending: procedure not ingested yet",
                    relation.external_id, event.slug
                ));
                continue;
            }
        };

        let existing: Option<(i32, bool)> = sqlx::query_as(
            "SELECT order_index, required FROM life_event_procedures \
             WHERE life_event_id = $1 AND procedure_id = $2",
        )
        .bind(event_id)
        .bind(procedure_id)
        .fetch_optional(&mut *tx)
        .await?;

        match existing {
            Some((order_index, required))
                if order_index == relation.order as i32 && required == relation.required => {}
            None => {
                sqlx::query(
                    "INSERT INTO life_event_procedures \
                     (life_event_id, procedure_id, order_index, required) \
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(event_id)
                .bind(procedure_id)
                .bind(relation.order as i32)
                .bind(relation.required)
                .execute(&mut *tx)
                .await?;
                report.relations_written += 1;
            }
            Some(_) => {
                sqlx::query(
                    "UPDATE life_event_procedures SET order_index = $3, required = $4 \
                     WHERE life_event_id = $1 AND procedure_id = $2",
                )
                .bind(event_id)
                .bind(procedure_id)
                .bind(relation.order as i32)
                .bind(relation.required)
                .execute(&mut *tx)
                .await?;
                report.relations_written += 1;
            }
        }
    }
    Ok(())
}

/// Projects the global synonyms, idempotent on (term, canonical, category):
/// missing surfaces are inserted sorted, stale rows are removed.
async fn seed_synonyms(
    tx: &mut PgConnection,
    taxonomy: &Taxonomy,
    report: &mut SeedReport,
) -> Result<(), sqlx::Error> {
    let existing: Vec<(Uuid, String, String, Option<String>)> =
        sqlx::query_as("SELECT id, term, canonical_term, category FROM synonyms")
            .fetch_all(&mut *tx)
            .await?;

    let mut desired: Vec<(String, String)> = taxonomy
        .synonyms
        .iter()
        .map(|source| {
            (
                source.synonym.term.clone(),
                source.synonym.canonical.clone(),
            )
        })
        .collect();
    desired.sort();
    desired.dedup();

    let mut existing_keys: std::collections::HashSet<(String, String, Option<String>)> = existing
        .iter()
        .map(|(_, term, canonical, category)| (term.clone(), canonical.clone(), category.clone()))
        .collect();

    for (term, canonical) in &desired {
        if existing_keys.remove(&(term.clone(), canonical.clone(), None)) {
            continue; // byte-identical: no write
        }
        sqlx::query("INSERT INTO synonyms (term, canonical_term, category) VALUES ($1, $2, NULL)")
            .bind(term)
            .bind(canonical)
            .execute(&mut *tx)
            .await?;
        report.synonyms_inserted += 1;
    }

    // Any remaining existing row (including category-tagged stale rows) is
    // no longer declared by the YAML: delete by id, matching on the key.
    for (id, term, canonical, category) in &existing {
        let key = (term.clone(), canonical.clone(), category.clone());
        if existing_keys.contains(&key) {
            sqlx::query("DELETE FROM synonyms WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            report.synonyms_removed += 1;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Read-side queries for the API category endpoints (task 72; design §7 maps
// GET /categories and GET /categories/:slug/events to
// `repos::taxonomy_seed::categories` / `events_by_category`). Justified
// crates/db addition: D-5 forbids SQL in apps/api, and these are exactly
// the read queries the C1 category contracts need — the same seeded
// projection the write side maintains.
// ---------------------------------------------------------------------------

/// One category on the ordered list (API-7): slug, name, order_index.
#[derive(Debug)]
pub struct CategorySummaryRow {
    pub slug: String,
    pub name: String,
    pub order_index: i32,
}

/// Lists every category ordered by `order_index` ascending (vehiculos
/// first for the seed; slug ascending as the deterministic tie-break).
pub async fn categories(pool: &sqlx::PgPool) -> Result<Vec<CategorySummaryRow>, sqlx::Error> {
    let rows =
        sqlx::query!("SELECT slug, name, order_index FROM categories ORDER BY order_index, slug")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|r| CategorySummaryRow {
            slug: r.slug,
            name: r.name,
            order_index: r.order_index,
        })
        .collect())
}

/// One event on a category's listing (API-7): slug and name.
#[derive(Debug)]
pub struct EventSummaryRow {
    pub slug: String,
    pub name: String,
}

/// Lists a category's events ordered by slug, or None when the category
/// slug is unknown (the handler maps None to 404).
pub async fn events_by_category(
    pool: &sqlx::PgPool,
    category_slug: &str,
) -> Result<Option<Vec<EventSummaryRow>>, sqlx::Error> {
    let known = sqlx::query!(
        "SELECT 1 AS one FROM categories WHERE slug = $1",
        category_slug
    )
    .fetch_optional(pool)
    .await?;
    if known.is_none() {
        return Ok(None);
    }
    let rows = sqlx::query!(
        "SELECT e.slug, e.name FROM life_events e \
         JOIN categories c ON c.id = e.category_id \
         WHERE c.slug = $1 ORDER BY e.slug",
        category_slug,
    )
    .fetch_all(pool)
    .await?;
    Ok(Some(
        rows.into_iter()
            .map(|r| EventSummaryRow {
                slug: r.slug,
                name: r.name,
            })
            .collect(),
    ))
}
