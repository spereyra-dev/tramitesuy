//! Task 3: the representative synthetic PII-free catalog fixture generator
//! (spec §7 load-plan prerequisite, OPT-01 fixture surface).
//!
//! Shape: the real YAML taxonomy (≈9 events today) is seeded through
//! `repos::taxonomy_seed::seed_taxonomy`, plus synthetic filler events up
//! to [`EVENT_TARGET`] total, then [`PROCEDURE_TARGET`] synthetic
//! procedures (missing-cost rows the API renders as "Sin costo informado",
//! inactive rows, populated costs, NULL raw_data) distributed across all
//! events with deterministic `order_index`/`required`.
//!
//! Determinism: a SplitMix64 stream from the caller's seed drives every
//! choice; timestamps are fixed instants. Same seed → byte-identical
//! catalog (asserted by the fixture test via [`dump`]).
//!
//! Privacy: every generated string is synthetic (names, organizations,
//! example-domain URLs). The only cédula/phone/email-shaped strings are in
//! [`sample_queries`] — redaction-requiring INPUTS with repeating-digit
//! shapes that cannot identify a real person. The catalog itself carries
//! none (asserted by the fixture tests).
//!
//! SQL style deviation (recorded in apply-progress): this test-support
//! module uses runtime-checked queries (`sqlx::query`/`query_as`), the same
//! pattern as `tests/common`; no `sqlx::query!` macro changed, so the
//! committed `.sqlx` cache is untouched.

use std::path::Path;

use sqlx::PgPool;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, TimeZone, Utc};
use taxonomy::model::Taxonomy;

/// Total event count of the fixture (≈20 per the task).
pub const EVENT_TARGET: usize = 20;
/// Total procedure count (≥3,500 per the task).
pub const PROCEDURE_TARGET: usize = 3_600;
/// Synthetic filler organizations (≈100 procedures each).
const ORGANIZATION_TARGET: usize = 36;

/// What `apply` produced, counted from the database itself.
#[derive(Debug, Clone, Copy)]
pub struct FixtureSummary {
    pub events: usize,
    pub procedures: usize,
    pub inactive_procedures: usize,
    pub missing_cost_procedures: usize,
}

/// SplitMix64: ten lines of fully deterministic randomness (no external
/// dependency), suitable for the byte-identity requirement.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Fixed base instant (deterministic; never `now()`).
fn stamp(index: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 18, 3, 0, 0)
        .single()
        .expect("fixed fixture timestamp")
        + std::time::Duration::from_secs((index.max(0) as u64) * 60)
}

/// The scenario query set for load runs: real-event queries, accented
/// variants, categories-fallback, and redaction-requiring input shapes
/// (repeating-digit cédula/phone + example-domain email — synthetic by
/// construction, never real identities).
pub fn sample_queries() -> Vec<String> {
    [
        "compre un auto usado",
        "compré un auto usado",
        "vender vehículo usado",
        "vendér un vehículo",
        "pagar la patente",
        "pagár paténte",
        "consultar deuda vehicular",
        "consulta déuda vehicular",
        "transferir un vehiculo",
        "quiero abrir una cuenta bancaria",
        "cambiar matrícula de la cédula 1.111.111-1",
        "consulta al teléfono 0900 111 222",
        "escribir a ejemplo@ejemplo.uy",
    ]
    .iter()
    .map(|query| (*query).to_string())
    .collect()
}

/// Applies the fixture to a migrated scratch database: real taxonomy
/// events + synthetic filler, then synthetic procedures and relations.
pub async fn apply(
    pool: &PgPool,
    data_dir: &Path,
    seed: u64,
) -> Result<FixtureSummary, sqlx::Error> {
    let taxonomy = load_taxonomy(data_dir)?;
    db::repos::taxonomy_seed::seed_taxonomy(pool, &taxonomy).await?;

    let mut event_slugs: Vec<String> =
        sqlx::query_scalar("SELECT slug FROM life_events ORDER BY slug")
            .fetch_all(pool)
            .await?;
    event_slugs.dedup();

    // Synthetic filler events up to EVENT_TARGET (the real taxonomy is the
    // source of truth; fillers only broaden the catalog-read surface).
    let synthetic = EVENT_TARGET.saturating_sub(event_slugs.len());
    if synthetic > 0 {
        let max_order: Option<i32> =
            sqlx::query_scalar("SELECT COALESCE(MAX(order_index), 0) FROM categories")
                .fetch_one(pool)
                .await?;
        sqlx::query(
            "INSERT INTO categories (slug, name, icon, order_index) \
             VALUES ($1, 'Eventos sintéticos', 'synthetic', $2)",
        )
        .bind(format!("catalogo-sintetico-{seed}"))
        .bind(max_order.unwrap_or(0) + 1)
        .execute(pool)
        .await?;
        for index in 0..synthetic {
            sqlx::query(
                "INSERT INTO life_events (slug, name, description, category_id) \
                 SELECT $1, $2, $3, id FROM categories WHERE slug = $4",
            )
            .bind(format!("evento-sintetico-{:02}", index + 1))
            .bind(format!("Evento sintético {:02}", index + 1))
            .bind("Evento de vida sintético para escenarios de carga.")
            .bind(format!("catalogo-sintetico-{seed}"))
            .execute(pool)
            .await?;
        }
        let filler_slugs: Vec<String> = (0..synthetic)
            .map(|index| format!("evento-sintetico-{:02}", index + 1))
            .collect();
        event_slugs.extend(filler_slugs);
    }

    // Organizations (deterministic synthetic institutions).
    let mut org_ids: Vec<Uuid> = Vec::with_capacity(ORGANIZATION_TARGET);
    for index in 0..ORGANIZATION_TARGET {
        let id: sqlx::types::Uuid = sqlx::query_scalar(
            "INSERT INTO organizations (external_id, name, short_name, official_url) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (external_id) DO UPDATE SET external_id = EXCLUDED.external_id \
             RETURNING id",
        )
        .bind(format!("org-syn-{index:03}"))
        .bind(format!("Organización sintética {index:03}"))
        .bind(format!("SYN{index:03}"))
        .bind(format!("https://example.uy/organizacion/{index:03}"))
        .fetch_one(pool)
        .await?;
        org_ids.push(id);
    }

    // Procedures, batched with UNNEST: 4 statements for 3,600 rows.
    let chunk_size = 900;
    let mut assigned = 0usize;
    while assigned < PROCEDURE_TARGET {
        let count = (PROCEDURE_TARGET - assigned).min(chunk_size);
        insert_procedure_chunk(pool, assigned, count, &org_ids, seed).await?;
        assigned += count;
    }

    // Relations: every fixture procedure is related to exactly one fixture
    // event, with a deterministic order_index/required split (TX-6).
    let mut related = 0usize;
    while related < PROCEDURE_TARGET {
        let count = (PROCEDURE_TARGET - related).min(chunk_size);
        insert_relation_chunk(pool, related, count, &event_slugs).await?;
        related += count;
    }

    summarize(pool).await
}

/// One UNNEST batch of synthetic procedures.
async fn insert_procedure_chunk(
    pool: &PgPool,
    start: usize,
    count: usize,
    org_ids: &[sqlx::types::Uuid],
    seed: u64,
) -> Result<(), sqlx::Error> {
    let mut rng = SplitMix64(seed ^ (start as u64).wrapping_mul(0xD1B5_4A32_D192_ED03));
    let mut external_ids = Vec::with_capacity(count);
    let mut names = Vec::with_capacity(count);
    let mut descriptions = Vec::with_capacity(count);
    let mut organizations = Vec::with_capacity(count);
    let mut urls = Vec::with_capacity(count);
    let mut statuses = Vec::with_capacity(count);
    let mut raws: Vec<Option<String>> = Vec::with_capacity(count);
    let mut firsts = Vec::with_capacity(count);
    let mut lasts = Vec::with_capacity(count);
    let mut deactivations: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(count);

    for offset in 0..count {
        let index = (start + offset) as i64;
        let external_id = format!("SYN-{index:05}");
        external_ids.push(external_id.clone());
        names.push(format!("Trámite sintético {index:05}"));
        descriptions.push(format!(
            "Trámite de prueba sintético {index:05} para carga."
        ));
        organizations.push(org_ids[(index as usize) % ORGANIZATION_TARGET]);
        urls.push(format!("https://example.uy/tramite/{index:05}"));
        // Every 11th row is inactive (soft-deleted, never deleted).
        if index % 11 == 5 {
            statuses.push("inactive".to_string());
            deactivations.push(Some(stamp(index)));
        } else {
            statuses.push("active".to_string());
            deactivations.push(None);
        }
        // Cost variants: missing cost (renders as "Sin costo informado"),
        // populated cost, zero cost, and NULL raw_data.
        raws.push(match index % 4 {
            0 => Some(serde_json::json!({"tiene_costo": "", "valor": ""}).to_string()),
            1 => Some(serde_json::json!({"tiene_costo": "1", "valor": "123.45"}).to_string()),
            2 => Some(serde_json::json!({"tiene_costo": "1", "valor": "0.00"}).to_string()),
            _ => None,
        });
        firsts.push(stamp(index));
        lasts.push(stamp(index + 1));
        let _ = rng.next(); // consumed per row: the stream is deterministic
    }

    sqlx::query(
        "INSERT INTO procedures \
             (external_id, name, description, organization_id, official_url, \
              status, raw_data, first_seen_at, last_seen_at, deactivated_at) \
         SELECT ext, name, description, org, url, status, raw::jsonb, \
                first::timestamptz, last::timestamptz, deact::timestamptz \
         FROM UNNEST($1::text[], $2::text[], $3::text[], $4::uuid[], $5::text[], \
                     $6::text[], $7::text[], $8::timestamptz[], $9::timestamptz[], \
                     $10::timestamptz[]) \
              AS t(ext, name, description, org, url, status, raw, first, last, deact)",
    )
    .bind(&external_ids)
    .bind(&names)
    .bind(&descriptions)
    .bind(organizations)
    .bind(&urls)
    .bind(&statuses)
    .bind(&raws)
    .bind(&firsts)
    .bind(&lasts)
    .bind(&deactivations)
    .execute(pool)
    .await?;
    Ok(())
}

/// One UNNEST batch of relations: event = index % events, order_index
/// derived from the index (chunk-independent), every third row required.
async fn insert_relation_chunk(
    pool: &PgPool,
    start: usize,
    count: usize,
    event_slugs: &[String],
) -> Result<(), sqlx::Error> {
    let mut slugs = Vec::with_capacity(count);
    let mut external_ids = Vec::with_capacity(count);
    let mut orders = Vec::with_capacity(count);
    let mut requireds = Vec::with_capacity(count);
    for offset in 0..count {
        let index = start + offset;
        slugs.push(event_slugs[index % event_slugs.len()].clone());
        external_ids.push(format!("SYN-{index:05}"));
        orders.push((index / event_slugs.len() + 1).to_string());
        requireds.push(index.is_multiple_of(3).to_string());
    }

    sqlx::query(
        "INSERT INTO life_event_procedures \
             (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, ord::int, req::boolean \
         FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[]) \
              AS t(slug, ext, ord, req) \
         JOIN life_events e ON e.slug = t.slug \
         JOIN procedures p ON p.external_id = t.ext",
    )
    .bind(&slugs)
    .bind(&external_ids)
    .bind(&orders)
    .bind(&requireds)
    .execute(pool)
    .await?;
    Ok(())
}

/// Counts the fixture from the database itself (never trusted inputs).
async fn summarize(pool: &PgPool) -> Result<FixtureSummary, sqlx::Error> {
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM life_events")
        .fetch_one(pool)
        .await?;
    let procedures: i64 = sqlx::query_scalar("SELECT count(*) FROM procedures")
        .fetch_one(pool)
        .await?;
    let inactive: i64 =
        sqlx::query_scalar("SELECT count(*) FROM procedures WHERE status = 'inactive'")
            .fetch_one(pool)
            .await?;
    let missing_cost: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM procedures \
         WHERE raw_data->>'tiene_costo' = ''",
    )
    .fetch_one(pool)
    .await?;
    Ok(FixtureSummary {
        events: events as usize,
        procedures: procedures as usize,
        inactive_procedures: inactive as usize,
        missing_cost_procedures: missing_cost as usize,
    })
}

/// Canonical dump of the whole fixture catalog for the byte-identity check:
/// every generated table, ordered, JSON-serialized.
pub async fn dump(pool: &PgPool) -> Result<String, sqlx::Error> {
    #[derive(Debug, serde::Serialize, PartialEq)]
    struct Row(String);
    let mut sections: Vec<String> = Vec::new();

    let mut section = String::from("# categories");
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT slug, name, order_index::text FROM categories ORDER BY slug")
            .fetch_all(pool)
            .await?;
    for (slug, name, order) in rows {
        section.push_str(&format!("\n{slug}|{name}|{order}"));
    }
    sections.push(section);

    let mut section = String::from("# life_events");
    let rows: Vec<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT e.slug, e.name, e.description, c.slug \
         FROM life_events e JOIN categories c ON c.id = e.category_id \
         ORDER BY e.slug",
    )
    .fetch_all(pool)
    .await?;
    for (slug, name, description, category) in rows {
        section.push_str(&format!(
            "\n{slug}|{name}|{}|{category}",
            description.unwrap_or_default()
        ));
    }
    sections.push(section);

    let mut section = String::from("# life_event_keywords");
    let rows: Vec<(String, String, String, i32, bool)> = sqlx::query_as(
        "SELECT e.slug, k.term, k.type, k.weight, k.negative \
         FROM life_event_keywords k JOIN life_events e ON e.id = k.life_event_id \
         ORDER BY e.slug, k.term",
    )
    .fetch_all(pool)
    .await?;
    for (slug, term, kind, weight, negative) in rows {
        section.push_str(&format!("\n{slug}|{term}|{kind}|{weight}|{negative}"));
    }
    sections.push(section);

    let mut section = String::from("# organizations");
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT external_id, name, short_name FROM organizations ORDER BY external_id",
    )
    .fetch_all(pool)
    .await?;
    for (external_id, name, short_name) in rows {
        section.push_str(&format!(
            "\n{external_id}|{name}|{}",
            short_name.unwrap_or_default()
        ));
    }
    sections.push(section);

    let mut section = String::from("# procedures");
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT external_id, name, status, \
                COALESCE(raw_data::text, ''), \
                COALESCE(deactivated_at::text, '') \
         FROM procedures ORDER BY external_id",
    )
    .fetch_all(pool)
    .await?;
    for (external_id, name, status, raw_data, deactivated_at) in rows {
        section.push_str(&format!(
            "\n{external_id}|{name}|{status}|{raw_data}|{deactivated_at}"
        ));
    }
    sections.push(section);

    let mut section = String::from("# life_event_procedures");
    let rows: Vec<(String, String, i32, bool)> = sqlx::query_as(
        "SELECT e.slug, p.external_id, r.order_index, r.required \
         FROM life_event_procedures r \
         JOIN life_events e ON e.id = r.life_event_id \
         JOIN procedures p ON p.id = r.procedure_id \
         ORDER BY e.slug, r.order_index",
    )
    .fetch_all(pool)
    .await?;
    for (slug, external_id, order_index, required) in rows {
        section.push_str(&format!("\n{slug}|{external_id}|{order_index}|{required}"));
    }
    sections.push(section);

    Ok(sections.join("\n"))
}

/// All catalog text (procedure names/descriptions + organization names), for
/// the PII-free scan.
pub async fn catalog_texts(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let mut texts: Vec<String> =
        sqlx::query_scalar("SELECT name || ' ' || COALESCE(description, '') FROM procedures")
            .fetch_all(pool)
            .await?;
    let orgs: Vec<String> =
        sqlx::query_scalar("SELECT name || ' ' || COALESCE(short_name, '') FROM organizations")
            .fetch_all(pool)
            .await?;
    texts.extend(orgs);
    Ok(texts)
}

/// Loads the real YAML taxonomy (the ranker's source of truth; the fixture
/// must seed the slugs the real engine matches).
fn load_taxonomy(data_dir: &Path) -> Result<Taxonomy, sqlx::Error> {
    taxonomy::loader::load_data_dir(data_dir)
        .map_err(|error| sqlx::Error::Configuration(Box::new(std::io::Error::other(error))))
}
