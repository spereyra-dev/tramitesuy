//! Generation build (S6 task 15, OPT-02/OPT-03, design §1.1/§1.2/§2.1):
//! reads the observable catalog payload from the same legacy tables the
//! ingestion pipeline writes (dual-write stays in place during stages 2–3),
//! computes the manifest identity, and writes every `generation_*`
//! projection idempotently per `(generation_id, slug)`.
//!
//! Identity rules (design §1.1):
//! - `generation_id` is a UUIDv7 minted by the worker; a retry over the same
//!   observable content reuses the id already recorded for that
//!   `content_hash` instead of creating new rows.
//! - `content_hash` is SHA-256 over a canonical, lexicographically ordered
//!   serialization of the full observable payload — categories with order,
//!   events, keywords with types/weights, ordered relations, cards,
//!   procedure details, organizations, attribution, costs, statuses, and
//!   the observable sync dates (`last_seen_at`, which the API serves as
//!   `source.last_synced_at`). Engine combination rules are engine
//!   configuration, covered by `taxonomy_version` (YAML) and
//!   `engine_version`.
//! - `source_synced_at` is the maximum observable `last_seen_at`.
//!
//! Interruption semantics (design §6.4): the manifest row is written first
//! with `status = 'building'`, then each projection table is written in its
//! own transaction, and only when all projections are complete is
//! `projection_status` advanced to `complete`. An interrupted build stays
//! `building` with incomplete projections and is therefore never a
//! publication candidate (the publication gate validates before promoting).

use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

/// The hex-encoded digest prefix lives in the hashed stream so a hashing
/// scheme change can never alias a previous scheme's hashes.
const CONTENT_HASH_SCHEME: &[u8] = b"tramitesuy:content-hash:v1\n";

/// The manifest identity the build pins (design §1.2). `reused` reports an
/// id reused from an existing manifest row with the same `content_hash`;
/// `already_published` reports that the matching generation is already
/// `published` (the caller must not create a new content version).
#[derive(Debug)]
pub struct BuildManifest {
    pub generation_id: Uuid,
    pub content_hash: String,
    pub taxonomy_version: String,
    pub source_synced_at: DateTime<Utc>,
    pub event_count: i32,
    pub procedure_count: i32,
    pub reused: bool,
    pub already_published: bool,
    pub projections_complete: bool,
}

/// The full observable catalog payload, read from the legacy tables in the
/// exact same shape the ingestion pipeline writes (design §2.1: the build
/// reads the same source ingestion uses today).
struct CatalogPayload {
    categories: Vec<(String, String, Option<String>, i32)>,
    organizations: Vec<(String, String, Option<String>)>,
    events: Vec<(String, String, String, String)>,
    keywords: Vec<(String, String, Option<String>, String, i32, bool)>,
    relations: Vec<(String, String, i32, bool)>,
    procedures: Vec<ProcedurePayload>,
    cards: Vec<CardPayload>,
}

struct ProcedurePayload {
    external_id: String,
    name: String,
    description: Option<String>,
    official_url: Option<String>,
    status: String,
    raw_data: Option<serde_json::Value>,
    last_seen_at: DateTime<Utc>,
}

struct CardPayload {
    event_slug: String,
    slug: String,
    order_index: i32,
    importance: Option<String>,
    required: bool,
    organization_short_name: Option<String>,
    cost: Option<String>,
    status: String,
    official_url: Option<String>,
    last_seen_at: DateTime<Utc>,
}

impl CatalogPayload {
    /// Canonical, ordered serialization: one JSON object per observable
    /// entity, every list sorted lexicographically in Rust so the hash never
    /// depends on the database's row order.
    fn canonical_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        for (slug, name, icon, order) in &self.categories {
            lines.push(canonical_line(serde_json::json!({
                "k": "category", "slug": slug, "name": name,
                "icon": icon, "order_index": order,
            })));
        }
        for (external_id, name, short_name) in &self.organizations {
            lines.push(canonical_line(serde_json::json!({
                "k": "organization", "external_id": external_id,
                "name": name, "short_name": short_name,
            })));
        }
        for (slug, name, status, category) in &self.events {
            lines.push(canonical_line(serde_json::json!({
                "k": "event", "slug": slug, "name": name,
                "status": status, "category": category,
            })));
        }
        for (event_slug, term, canonical_term, keyword_type, weight, negative) in &self.keywords {
            lines.push(canonical_line(serde_json::json!({
                "k": "keyword", "event": event_slug, "term": term,
                "canonical_term": canonical_term, "type": keyword_type,
                "weight": weight, "negative": negative,
            })));
        }
        for (event_slug, external_id, order, required) in &self.relations {
            lines.push(canonical_line(serde_json::json!({
                "k": "relation", "event": event_slug, "external_id": external_id,
                "order_index": order, "required": required,
            })));
        }
        for procedure in &self.procedures {
            lines.push(canonical_line(serde_json::json!({
                "k": "procedure", "external_id": procedure.external_id,
                "name": procedure.name, "description": procedure.description,
                "official_url": procedure.official_url, "status": procedure.status,
                "raw_data": procedure.raw_data,
                "last_seen_at": rfc3339_micros(&procedure.last_seen_at),
            })));
        }
        for card in &self.cards {
            lines.push(canonical_line(serde_json::json!({
                "k": "card", "event": card.event_slug, "slug": card.slug,
                "order_index": card.order_index, "importance": card.importance,
                "required": card.required,
                "organization_short_name": card.organization_short_name,
                "cost": card.cost, "status": card.status,
                "official_url": card.official_url,
                "last_seen_at": rfc3339_micros(&card.last_seen_at),
            })));
        }
        lines.sort();
        lines
    }
}

fn canonical_line(value: serde_json::Value) -> String {
    // Expect justification: serializing a `serde_json::Value` cannot fail —
    // a Value is by construction valid JSON.
    serde_json::to_string(&value).expect("serde_json::Value serialization is infallible")
}

fn rfc3339_micros(at: &DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// SHA-256 over the canonical ordered serialization of the full observable
/// payload including the observable sync dates (design §1.1).
fn content_hash(payload: &CatalogPayload) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CONTENT_HASH_SCHEME);
    for line in payload.canonical_lines() {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        // unwrap justification: writing two hex chars into an owned String
        // cannot fail (no allocator error is recoverable, and fmt to String
        // is infallible).
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Reads the whole observable payload from the legacy tables — the same
/// source ingestion writes today (dual-write, design §2.1).
async fn read_payload(pool: &PgPool) -> Result<CatalogPayload, sqlx::Error> {
    let categories =
        sqlx::query!("SELECT slug, name, icon, order_index FROM categories ORDER BY slug",)
            .fetch_all(pool)
            .await?;
    let organizations = sqlx::query!(
        "SELECT external_id, name, short_name FROM organizations ORDER BY external_id",
    )
    .fetch_all(pool)
    .await?;
    let events = sqlx::query!(
        r#"SELECT e.slug, e.name, e.status, c.slug AS "category_slug!"
           FROM life_events e JOIN categories c ON c.id = e.category_id
           ORDER BY e.slug"#,
    )
    .fetch_all(pool)
    .await?;
    let keywords = sqlx::query!(
        r#"SELECT e.slug AS "event_slug!", k.term, k.canonical_term, k.type, k.weight, k.negative
           FROM life_event_keywords k JOIN life_events e ON e.id = k.life_event_id
           ORDER BY e.slug, k.term, k.type"#,
    )
    .fetch_all(pool)
    .await?;
    let relations = sqlx::query!(
        r#"SELECT e.slug AS "event_slug!", r.order_index, r.required,
                  p.external_id AS "procedure_external_id!"
           FROM life_event_procedures r
           JOIN life_events e ON e.id = r.life_event_id
           JOIN procedures p ON p.id = r.procedure_id
           ORDER BY e.slug, r.order_index, p.external_id"#,
    )
    .fetch_all(pool)
    .await?;
    let procedure_rows = sqlx::query!(
        r#"SELECT DISTINCT ON (p.external_id)
                  p.external_id, p.name, p.description, p.official_url, p.status,
                  p.raw_data, p.last_seen_at
           FROM procedures p
           ORDER BY p.external_id, (p.status = 'active') DESC, p.created_at DESC"#,
    )
    .fetch_all(pool)
    .await?;
    let card_rows = sqlx::query!(
        r#"SELECT e.slug AS "event_slug!", r.order_index AS "order_index?",
                  r.required AS "required?", r.importance AS "importance?",
                  p.external_id AS "procedure_slug?", p.name AS "procedure_name?",
                  p.status AS "status?", p.official_url AS "official_url?",
                  p.last_seen_at AS "last_seen_at?",
                  o.short_name AS "organization_short_name?",
                  CASE
                      WHEN jsonb_typeof(p.raw_data -> 'tiene_costo') = 'string'
                           AND btrim(p.raw_data ->> 'tiene_costo') <> ''
                           AND jsonb_typeof(p.raw_data -> 'valor') = 'string'
                           AND btrim(p.raw_data ->> 'valor') <> ''
                  THEN p.raw_data ->> 'valor'
                  END AS "cost?"
           FROM life_events e
           LEFT JOIN life_event_procedures r ON r.life_event_id = e.id
           LEFT JOIN procedures p ON p.id = r.procedure_id
           LEFT JOIN organizations o ON o.id = p.organization_id
           ORDER BY e.slug, r.order_index, p.external_id"#,
    )
    .fetch_all(pool)
    .await?;

    Ok(CatalogPayload {
        categories: categories
            .into_iter()
            .map(|row| (row.slug, row.name, row.icon, row.order_index))
            .collect(),
        organizations: organizations
            .into_iter()
            .map(|row| (row.external_id, row.name, row.short_name))
            .collect(),
        events: events
            .into_iter()
            .map(|row| (row.slug, row.name, row.status, row.category_slug))
            .collect(),
        keywords: keywords
            .into_iter()
            .map(|row| {
                (
                    row.event_slug,
                    row.term,
                    row.canonical_term,
                    row.r#type,
                    row.weight,
                    row.negative,
                )
            })
            .collect(),
        relations: relations
            .into_iter()
            .map(|row| {
                (
                    row.event_slug,
                    row.procedure_external_id,
                    row.order_index,
                    row.required,
                )
            })
            .collect(),
        procedures: procedure_rows
            .into_iter()
            .map(|row| ProcedurePayload {
                external_id: row.external_id,
                name: row.name,
                description: row.description,
                official_url: row.official_url,
                status: row.status,
                raw_data: row.raw_data,
                last_seen_at: row.last_seen_at,
            })
            .collect(),
        cards: card_rows
            .into_iter()
            .filter_map(|row| {
                // LEFT-JOIN rows without a procedure (p.id IS NULL) decode as
                // NULLs; they contribute no card line.
                let slug = row.procedure_slug?;
                Some(CardPayload {
                    event_slug: row.event_slug,
                    slug,
                    order_index: row.order_index?,
                    importance: row.importance,
                    required: row.required?,
                    organization_short_name: row.organization_short_name,
                    cost: row.cost,
                    status: row.status?,
                    official_url: row.official_url,
                    last_seen_at: row.last_seen_at?,
                })
            })
            .collect(),
    })
}

/// Writes every `generation_*` projection for one generation, idempotently
/// per `(generation_id, slug)`. The published-row immutability contract
/// (task 14) is honored at the data-access boundary: a published generation
/// or one whose projections already report complete is never rewritten.
pub async fn write_projections(pool: &PgPool, generation_id: Uuid) -> Result<(), sqlx::Error> {
    let Some(row) = sqlx::query!(
        "SELECT status, projection_status FROM catalog_generations WHERE generation_id = $1",
        generation_id,
    )
    .fetch_optional(pool)
    .await?
    else {
        return Err(sqlx::Error::RowNotFound);
    };
    if row.status == "published" || row.projection_status == "complete" {
        // Immutable once published/complete: a rewrite would be a mutation of
        // a published artifact, which no code path is allowed to do.
        return Ok(());
    }

    // Per-event projections, one transaction each: an interruption leaves
    // some projections written (status stays 'building') and a retry
    // rewrites each table idempotently.
    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"INSERT INTO generation_life_events
               (generation_id, slug, name, status, category_slug, order_index,
                positive_keywords, negative_keywords)
           SELECT $1, e.slug, e.name, e.status, c.slug, c.order_index,
                  COALESCE((SELECT jsonb_agg(jsonb_build_object(
                                 'term', k.term,
                                 'canonical', COALESCE(k.canonical_term, ''),
                                 'type', k.type,
                                 'weight', k.weight)
                             ORDER BY k.term, k.type)
                            FROM life_event_keywords k
                            WHERE k.life_event_id = e.id AND NOT k.negative),
                           '[]'::jsonb),
                  COALESCE((SELECT jsonb_agg(jsonb_build_object(
                                 'term', k.term,
                                 'canonical', COALESCE(k.canonical_term, ''),
                                 'type', k.type,
                                 'weight', k.weight)
                             ORDER BY k.term, k.type)
                            FROM life_event_keywords k
                            WHERE k.life_event_id = e.id AND k.negative),
                           '[]'::jsonb)
           FROM life_events e JOIN categories c ON c.id = e.category_id
           ON CONFLICT (generation_id, slug) DO UPDATE
               SET name = EXCLUDED.name, status = EXCLUDED.status,
                   category_slug = EXCLUDED.category_slug,
                   order_index = EXCLUDED.order_index,
                   positive_keywords = EXCLUDED.positive_keywords,
                   negative_keywords = EXCLUDED.negative_keywords"#,
        generation_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"INSERT INTO generation_fts_text (generation_id, slug, fts_text)
           SELECT $1, e.slug,
                  public.unaccent_immutable(e.name || ' ' || COALESCE(e.description, ''))
           FROM life_events e
           ON CONFLICT (generation_id, slug) DO UPDATE
               SET fts_text = EXCLUDED.fts_text"#,
        generation_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut tx = pool.begin().await?;
    // Exact legacy trigram-surface replication (design §2.2): name plus the
    // positive keywords with their canonical terms — negatives excluded —
    // including the trailing space the legacy
    // `e.name || ' ' || COALESCE(kw.terms, '')` composition produced for
    // keyword-less events, so the precomputed `similarity()` values are
    // identical to the per-request computation.
    sqlx::query!(
        r#"INSERT INTO generation_trigram_surface (generation_id, slug, surface_text)
           SELECT $1, e.slug, e.name || ' ' || COALESCE(kw.terms, '')
           FROM life_events e
           LEFT JOIN (
               SELECT life_event_id,
                      string_agg(term || ' ' || COALESCE(canonical_term, ''), ' ') AS terms
               FROM life_event_keywords
               WHERE NOT negative
               GROUP BY life_event_id
           ) kw ON kw.life_event_id = e.id
           ON CONFLICT (generation_id, slug) DO UPDATE
               SET surface_text = EXCLUDED.surface_text"#,
        generation_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"INSERT INTO generation_event_cards (generation_id, slug, cards)
           SELECT $1, e.slug,
                  COALESCE(jsonb_agg(jsonb_build_object(
                          'slug', p.external_id,
                          'name', p.name,
                          'order_index', r.order_index,
                          'importance', r.importance,
                          'required', r.required,
                          'organization_short_name', o.short_name,
                          'cost', CASE
                              WHEN jsonb_typeof(p.raw_data -> 'tiene_costo') = 'string'
                                   AND btrim(p.raw_data ->> 'tiene_costo') <> ''
                                   AND jsonb_typeof(p.raw_data -> 'valor') = 'string'
                                   AND btrim(p.raw_data ->> 'valor') <> ''
                              THEN p.raw_data ->> 'valor'
                          END,
                          'status', p.status,
                          'official_url', p.official_url,
                          'last_seen_at', p.last_seen_at)
                      ORDER BY r.order_index, p.external_id)
                      FILTER (WHERE p.id IS NOT NULL),
                      '[]'::jsonb)
           FROM life_events e
           LEFT JOIN life_event_procedures r ON r.life_event_id = e.id
           LEFT JOIN procedures p ON p.id = r.procedure_id
           LEFT JOIN organizations o ON o.id = p.organization_id
           GROUP BY e.slug
           ON CONFLICT (generation_id, slug) DO UPDATE
               SET cards = EXCLUDED.cards"#,
        generation_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut tx = pool.begin().await?;
    // Procedure details: one row per external id with the same selection the
    // detail endpoint uses — the active row wins, otherwise the latest row.
    sqlx::query!(
        r#"INSERT INTO generation_procedure_details (generation_id, slug, details)
           SELECT $1, d.external_id,
                  jsonb_build_object(
                      'external_id', d.external_id,
                      'name', d.name,
                      'description', d.description,
                      'organization_name', d.organization_name,
                      'official_url', d.official_url,
                      'status', d.status,
                      'raw_data', d.raw_data,
                      'last_seen_at', d.last_seen_at)
           FROM (
               SELECT DISTINCT ON (p.external_id)
                      p.external_id, p.name, p.description, p.official_url,
                      p.status, p.raw_data, p.last_seen_at, o.name AS organization_name
               FROM procedures p LEFT JOIN organizations o ON o.id = p.organization_id
               ORDER BY p.external_id, (p.status = 'active') DESC, p.created_at DESC
           ) d
           ON CONFLICT (generation_id, slug) DO UPDATE
               SET details = EXCLUDED.details"#,
        generation_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(())
}

/// Advances `projection_status` to `complete` once every projection table is
/// written. Never touches a published generation.
pub async fn finalize_build(pool: &PgPool, generation_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE catalog_generations SET projection_status = 'complete' \
         WHERE generation_id = $1 AND status <> 'published'",
        generation_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Begins the build (design §1.1 identity rules): hashes the observable
/// payload, reuses the generation id already recorded for the same
/// `content_hash` when present, and leaves the manifest row `building` with
/// incomplete projections when the projections still have to be (re)written.
pub async fn begin_build(
    pool: &PgPool,
    taxonomy_version: &str,
) -> Result<BuildManifest, sqlx::Error> {
    let payload = read_payload(pool).await?;
    let hash = content_hash(&payload);
    let event_count = payload.events.len() as i32;
    let procedure_count = payload.procedures.len() as i32;
    let source_synced_at = payload
        .procedures
        .iter()
        .map(|p| p.last_seen_at)
        .max()
        .unwrap_or_else(Utc::now);

    let existing = sqlx::query!(
        "SELECT generation_id, status, projection_status FROM catalog_generations \
         WHERE content_hash = $1 ORDER BY created_at DESC LIMIT 1",
        hash,
    )
    .fetch_optional(pool)
    .await?;

    let (generation_id, reused, already_published, projections_complete) = match &existing {
        Some(row) => (
            row.generation_id,
            true,
            row.status == "published",
            row.projection_status == "complete",
        ),
        None => (Uuid::now_v7(), false, false, false),
    };

    let published_row = matches!(&existing, Some(row) if row.status == "published");
    if !projections_complete && !published_row {
        sqlx::query!(
            r#"INSERT INTO catalog_generations
                   (generation_id, status, content_hash, taxonomy_version, engine_version,
                    source_synced_at, event_count, procedure_count, projection_status)
               VALUES ($1, 'building', $2, $3, $4, $5, $6, $7, 'building')
               ON CONFLICT (generation_id) DO UPDATE
                   SET status = 'building', projection_status = 'building',
                       taxonomy_version = $3, engine_version = $4,
                       source_synced_at = $5, event_count = $6, procedure_count = $7"#,
            generation_id,
            hash,
            taxonomy_version,
            super::ENGINE_VERSION,
            source_synced_at,
            event_count,
            procedure_count,
        )
        .execute(pool)
        .await?;
    }

    Ok(BuildManifest {
        generation_id,
        content_hash: hash,
        taxonomy_version: taxonomy_version.to_string(),
        source_synced_at,
        event_count,
        procedure_count,
        reused,
        already_published,
        projections_complete,
    })
}

/// The complete build: hash + manifest → projections → finalize. Retrying
/// the same content reuses the id and never duplicates projection rows; a
/// build whose artifacts are already complete and validated is a no-op.
pub async fn build_generation(
    pool: &PgPool,
    taxonomy_version: &str,
) -> Result<BuildManifest, sqlx::Error> {
    let manifest = begin_build(pool, taxonomy_version).await?;
    if !manifest.already_published && !manifest.projections_complete {
        write_projections(pool, manifest.generation_id).await?;
        finalize_build(pool, manifest.generation_id).await?;
    }
    Ok(manifest)
}
