//! S14 task 45 (spec §7 test 4, OPT-02/OPT-03): content-change cases. An
//! ingestion with changed observable content produces a NEW generation
//! (new id, new content hash) whose projections carry the change, while
//! every retained generation's projections stay untouched:
//!
//! - cost change → the new generation's cards show the new cost;
//! - deactivation → the new generation's cards show `inactive`;
//! - new arrival → the new generation counts and carries the procedure;
//! - taxonomy/synonym change (a keyword's canonical term) → the new
//!   generation's search surfaces (trigram surface, keyword projection)
//!   change with it;
//! - no-content ingestion (only the observable sync dates move) → a new
//!   generation whose ONLY observable difference is the sync dates.
//!
//! The legacy tables keep being the ingestion source (dual-write);
//! published generations are immutable data — nothing is ever rewritten.

#[path = "c2support/mod.rs"]
mod c2support;

use c2support::*;
use db::generations::{build, validate};
use serde_json::Value;
use sqlx::types::chrono::{DateTime, Utc};
use sqlx::types::uuid::Uuid;

/// A fixed instant every seeded row shares (deterministic sync dates).
fn fixed_instant() -> DateTime<Utc> {
    DateTime::from_timestamp(1_789_000_000, 0).expect("fixed instant")
}

async fn seed_catalog(pool: &sqlx::PgPool) {
    sqlx::query(
        "INSERT INTO organizations (external_id, name, short_name) \
         VALUES ('D', 'Ministerio de ejemplo', 'ME')",
    )
    .execute(pool)
    .await
    .expect("seed organization");
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehículos', 'car', 1)",
    )
    .execute(pool)
    .await
    .expect("seed category");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'alta-vehiculo', 'Alta de vehículos', \
                'Registro inicial de un vehículo.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event alta-vehiculo");
    sqlx::query(
        "INSERT INTO life_event_keywords (life_event_id, term, canonical_term, type, weight, negative) \
         SELECT e.id, k.term, k.canonical_term, k.kind, k.weight, k.negative \
         FROM life_events e \
         JOIN (VALUES \
             ('alta-vehiculo', 'registro', NULL::text, 'ACTION', 5, false), \
             ('alta-vehiculo', 'vehiculo', 'auto', 'ENTITY', 8, false) \
         ) AS k(slug, term, canonical_term, kind, weight, negative) ON k.slug = e.slug",
    )
    .execute(pool)
    .await
    .expect("seed keywords");

    for (id, valor) in [("1001", "500"), ("1002", "120")] {
        sqlx::query(
            "INSERT INTO procedures \
             (external_id, name, description, organization_id, official_url, status, raw_data, \
              first_seen_at, last_seen_at) \
             VALUES ($1, $2, 'Descripción del trámite.', \
                     (SELECT id FROM organizations WHERE external_id = 'D'), \
                     $3, 'active', $4, $5, $5)",
        )
        .bind(id)
        .bind(format!("Trámite {id}"))
        .bind(format!("https://www.gub.uy/tramite/{id}"))
        .bind(serde_json::json!({ "tiene_costo": "1", "valor": valor }))
        .bind(fixed_instant())
        .execute(pool)
        .await
        .expect("seed procedure");
    }

    for (external_id, order) in [("1001", 1), ("1002", 2)] {
        sqlx::query(
            "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
             SELECT e.id, p.id, $2, true \
             FROM life_events e, procedures p \
             WHERE e.slug = 'alta-vehiculo' AND p.external_id = $1",
        )
        .bind(external_id)
        .bind(order)
        .execute(pool)
        .await
        .expect("seed relation");
    }
}

/// Builds one full generation over the current legacy-table state and
/// promotes it through the guarded reference advance (the worker's
/// publication stand-in), so a following build sees `published`.
async fn build_and_publish(pool: &sqlx::PgPool) -> build::BuildManifest {
    let manifest = build::build_generation(pool, "tax-v1")
        .await
        .expect("generation build over the seeded legacy tables");
    if !manifest.already_published {
        let report = validate::validate_generation(pool, manifest.generation_id, None)
            .await
            .expect("publication validation runs");
        assert!(
            report.passed(),
            "the built generation must validate: {:?}",
            report.failures
        );
        let promoted = sqlx::query(
            "UPDATE catalog_generations SET status = 'published', published_at = now() \
             WHERE generation_id = $1 AND status = 'validated' AND projection_status = 'complete'",
        )
        .bind(manifest.generation_id)
        .execute(pool)
        .await
        .expect("promote the validated reference")
        .rows_affected();
        assert_eq!(promoted, 1, "the built generation validates and publishes");
    }
    manifest
}

/// The card array of one generation's event projection.
async fn cards_of(pool: &sqlx::PgPool, generation_id: Uuid, event: &str) -> Value {
    sqlx::query_scalar(
        "SELECT cards FROM generation_event_cards \
         WHERE generation_id = $1 AND slug = $2",
    )
    .bind(generation_id)
    .bind(event)
    .fetch_one(pool)
    .await
    .expect("generation_event_cards row readable")
}

/// The `last_seen_at`-stripped copy of a card array: the observable
/// payload WITHOUT the sync dates.
fn without_sync_dates(cards: &Value) -> Vec<Value> {
    cards
        .as_array()
        .expect("cards array")
        .iter()
        .map(|card| {
            let mut copy = card.clone();
            copy.as_object_mut()
                .expect("card object")
                .remove("last_seen_at");
            copy
        })
        .collect()
}

#[tokio::test]
async fn a_cost_change_builds_a_new_generation_with_the_new_cost() {
    let (pool, name) = fresh_migrated_db().await;
    seed_catalog(&pool).await;
    let g1 = build_and_publish(&pool).await;

    // The ingestion ingests a cost change for procedure 1001.
    sqlx::query(
        "UPDATE procedures SET raw_data = jsonb_set(raw_data, '{valor}', '\"900\"') \
         WHERE external_id = '1001'",
    )
    .execute(&pool)
    .await
    .expect("cost change ingested");
    let g2 = build_and_publish(&pool).await;

    assert_ne!(
        g1.generation_id, g2.generation_id,
        "a cost change is observable content: a NEW generation"
    );
    assert_ne!(
        g1.content_hash, g2.content_hash,
        "the cost is part of the observable payload hash"
    );

    let g1_cards = cards_of(&pool, g1.generation_id, "alta-vehiculo").await;
    let g2_cards = cards_of(&pool, g2.generation_id, "alta-vehiculo").await;
    assert_eq!(
        g2_cards[0]["cost"], "900",
        "the new generation's card carries the changed cost: {g2_cards}"
    );
    assert_eq!(
        g1_cards[0]["cost"], "500",
        "the published generation's projections are immutable: {g1_cards}"
    );

    drop_db(&name).await;
}

#[tokio::test]
async fn a_deactivation_builds_a_generation_with_the_inactive_status() {
    let (pool, name) = fresh_migrated_db().await;
    seed_catalog(&pool).await;
    let g1 = build_and_publish(&pool).await;

    // Soft delete: the procedure becomes inactive, never deleted.
    sqlx::query(
        "UPDATE procedures SET status = 'inactive', deactivated_at = $1 \
         WHERE external_id = '1002'",
    )
    .bind(Utc::now())
    .execute(&pool)
    .await
    .expect("deactivation ingested");
    let g2 = build_and_publish(&pool).await;

    assert_ne!(g1.generation_id, g2.generation_id);
    let g1_cards = cards_of(&pool, g1.generation_id, "alta-vehiculo").await;
    let g2_cards = cards_of(&pool, g2.generation_id, "alta-vehiculo").await;
    assert_eq!(
        g2_cards[1]["status"], "inactive",
        "the new generation's card carries the deactivation: {g2_cards}"
    );
    assert_eq!(
        g1_cards[1]["status"], "active",
        "the published generation's projections are immutable: {g1_cards}"
    );

    drop_db(&name).await;
}

#[tokio::test]
async fn a_new_arrival_extends_the_new_generation_without_touching_the_old() {
    let (pool, name) = fresh_migrated_db().await;
    seed_catalog(&pool).await;
    let g1 = build_and_publish(&pool).await;

    sqlx::query(
        "INSERT INTO procedures \
         (external_id, name, description, organization_id, official_url, status, raw_data, \
          first_seen_at, last_seen_at) \
         VALUES ('1003', 'Trámite 1003', 'Nueva alta.', \
                 (SELECT id FROM organizations WHERE external_id = 'D'), \
                 'https://www.gub.uy/tramite/1003', 'active', \
                 '{\"tiene_costo\": \"1\", \"valor\": \"80\"}'::jsonb, $1, $1)",
    )
    .bind(fixed_instant())
    .execute(&pool)
    .await
    .expect("new arrival ingested");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 3, false \
         FROM life_events e, procedures p \
         WHERE e.slug = 'alta-vehiculo' AND p.external_id = '1003'",
    )
    .execute(&pool)
    .await
    .expect("new relation ingested");
    let g2 = build_and_publish(&pool).await;

    assert_ne!(g1.generation_id, g2.generation_id);
    assert!(
        g2.procedure_count > g1.procedure_count,
        "the new generation counts the arrival ({} > {})",
        g2.procedure_count,
        g1.procedure_count
    );
    let g1_cards = cards_of(&pool, g1.generation_id, "alta-vehiculo").await;
    let g2_cards = cards_of(&pool, g2.generation_id, "alta-vehiculo").await;
    let g2_slugs: Vec<&str> = g2_cards
        .as_array()
        .expect("cards")
        .iter()
        .map(|card| card["slug"].as_str().expect("slug"))
        .collect();
    assert!(
        g2_slugs.contains(&"1003"),
        "the new generation's cards carry the arrival: {g2_cards}"
    );
    assert!(
        !g1_cards
            .as_array()
            .expect("cards")
            .iter()
            .any(|card| card["slug"] == "1003"),
        "the published generation is untouched: {g1_cards}"
    );

    drop_db(&name).await;
}

#[tokio::test]
async fn a_taxonomy_or_synonym_change_changes_the_search_projections() {
    let (pool, name) = fresh_migrated_db().await;
    seed_catalog(&pool).await;
    let g1 = build_and_publish(&pool).await;

    // A taxonomy/synonym change: the canonical term of `vehiculo` moves
    // from `auto` to `coche` (the YAML seed's canonical-term rule, which
    // seed-taxonomy projects into the keywords table).
    sqlx::query(
        "UPDATE life_event_keywords SET canonical_term = 'coche' \
         WHERE term = 'vehiculo'",
    )
    .execute(&pool)
    .await
    .expect("taxonomy/synonym change ingested");
    let g2 = build_and_publish(&pool).await;

    assert_ne!(
        g1.generation_id, g2.generation_id,
        "the keyword projection is observable content: a NEW generation"
    );
    assert_ne!(g1.content_hash, g2.content_hash);

    let surface_of = |generation_id: Uuid| {
        let pool = &pool;
        async move {
            sqlx::query_scalar::<_, String>(
                "SELECT surface_text FROM generation_trigram_surface \
                 WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
            )
            .bind(generation_id)
            .fetch_one(pool)
            .await
            .expect("generation_trigram_surface row readable")
        }
    };
    let g1_surface = surface_of(g1.generation_id).await;
    let g2_surface = surface_of(g2.generation_id).await;
    assert!(
        g2_surface.contains("coche") && !g2_surface.contains("auto"),
        "the new generation's search surface carries the new canonical \
         term: {g2_surface}"
    );
    assert!(
        g1_surface.contains("auto"),
        "the published generation's search surface is immutable: {g1_surface}"
    );

    drop_db(&name).await;
}

#[tokio::test]
async fn an_ingestion_without_content_change_updates_only_the_sync_dates() {
    let (pool, name) = fresh_migrated_db().await;
    seed_catalog(&pool).await;
    let g1 = build_and_publish(&pool).await;

    // Ingestion with NO content change: only the observable sync dates
    // move (the procedures were re-seen, nothing else changed).
    let later = DateTime::from_timestamp(1_789_100_000, 0).expect("later fixed instant");
    assert!(later > fixed_instant());
    sqlx::query("UPDATE procedures SET last_seen_at = $1")
        .bind(later)
        .execute(&pool)
        .await
        .expect("sync dates updated");
    let g2 = build_and_publish(&pool).await;

    // The sync dates ARE observable payload: a new generation id and a
    // new content hash.
    assert_ne!(
        g1.generation_id, g2.generation_id,
        "the observable sync dates are part of the hashed payload"
    );
    assert_ne!(g1.content_hash, g2.content_hash);
    assert!(
        g2.source_synced_at > g1.source_synced_at,
        "the new manifest's source sync date is the moved one"
    );

    // ...and the sync dates are the ONLY observable difference: every
    // card is identical once its `last_seen_at` is stripped.
    let g1_cards = cards_of(&pool, g1.generation_id, "alta-vehiculo").await;
    let g2_cards = cards_of(&pool, g2.generation_id, "alta-vehiculo").await;
    assert_eq!(
        without_sync_dates(&g1_cards),
        without_sync_dates(&g2_cards),
        "no-content ingestion changes nothing but the observable sync dates"
    );

    // The procedure details follow the same rule.
    let details_of = |generation_id: Uuid| {
        let pool = &pool;
        async move {
            sqlx::query_scalar::<_, Value>(
                "SELECT details FROM generation_procedure_details \
                 WHERE generation_id = $1 AND slug = '1001'",
            )
            .bind(generation_id)
            .fetch_one(pool)
            .await
            .expect("generation_procedure_details row readable")
        }
    };
    let mut g1_detail = details_of(g1.generation_id).await;
    let mut g2_detail = details_of(g2.generation_id).await;
    g1_detail
        .as_object_mut()
        .expect("detail object")
        .remove("last_seen_at");
    g2_detail
        .as_object_mut()
        .expect("detail object")
        .remove("last_seen_at");
    assert_eq!(
        g1_detail, g2_detail,
        "the details are unchanged but for the observable sync dates"
    );

    drop_db(&name).await;
}
