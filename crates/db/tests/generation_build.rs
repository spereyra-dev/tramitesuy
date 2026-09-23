//! Task 15 (S6): the generation build (`db::generations::build`). The build
//! reads the observable catalog payload from the same legacy tables the
//! ingestion pipeline writes (dual-write stays in place), computes the
//! SHA-256 `content_hash` over a canonical ordered serialization including
//! the observable sync dates (`last_seen_at`), pins `taxonomy_version` and
//! `engine_version` in the manifest, and writes every `generation_*`
//! projection idempotently per `(generation_id, slug)`.
//!
//! RED contract (tasks.md task 15): building the same input twice yields the
//! same `generation_id`/`content_hash` with no duplicate projection rows;
//! changing only sync dates changes the hash; writing a projection row twice
//! is idempotent; an interrupted build leaves `status = 'building'` with
//! incomplete projections and is not a publication candidate.

#[path = "c2support/mod.rs"]
mod c2support;

use c2support::*;

use uuid::Uuid;

/// Seeds a small representative catalog in the legacy tables: two events
/// (one with a negative keyword), one organization, two procedures, and one
/// ordered relation. The shape mirrors the real engine's surface rules.
async fn seed_small_catalog(pool: &sqlx::PgPool) {
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
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'otro-tramite', 'Trámite genérico', 'Otro trámite.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event otro-tramite");

    sqlx::query(
        "INSERT INTO life_event_keywords (life_event_id, term, canonical_term, type, weight, negative) \
         SELECT e.id, k.term, k.canonical_term, k.kind, k.weight, k.negative \
         FROM life_events e \
         JOIN (VALUES \
             ('alta-vehiculo', 'registro', NULL::text, 'ACTION', 5, false), \
             ('alta-vehiculo', 'vehiculo', 'auto', 'ENTITY', 8, false), \
             ('alta-vehiculo', 'vender', NULL::text, 'ACTION', 15, true), \
             ('otro-tramite', 'vender', NULL::text, 'ACTION', 15, true) \
         ) AS k(slug, term, canonical_term, kind, weight, negative) ON k.slug = e.slug",
    )
    .execute(pool)
    .await
    .expect("seed keywords");

    for (id, name, valor) in [
        ("1001", "Solicitud de empadronamientos", "1000"),
        ("1002", "Cambio de radicación", "500"),
    ] {
        sqlx::query(
            "INSERT INTO procedures \
             (external_id, name, description, organization_id, official_url, status, raw_data) \
             VALUES ($1, $2, 'Descripción del trámite.', \
                     (SELECT id FROM organizations WHERE external_id = 'D'), \
                     $3, 'active', $4)",
        )
        .bind(id)
        .bind(name)
        .bind(format!("https://www.gub.uy/tramite/{id}"))
        .bind(serde_json::json!({ "id": id, "valor": valor }))
        .execute(pool)
        .await
        .expect("seed procedure");
    }

    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, true \
         FROM life_events e, procedures p \
         WHERE e.slug = 'alta-vehiculo' AND p.external_id = '1001'",
    )
    .execute(pool)
    .await
    .expect("seed relation");
}

async fn projection_row_counts(
    pool: &sqlx::PgPool,
    generation_id: Uuid,
) -> (i64, i64, i64, i64, i64) {
    let life_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_life_events WHERE generation_id = $1")
            .bind(generation_id)
            .fetch_one(pool)
            .await
            .expect("count generation_life_events");
    let fts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_fts_text WHERE generation_id = $1")
            .bind(generation_id)
            .fetch_one(pool)
            .await
            .expect("count generation_fts_text");
    let trigram: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM generation_trigram_surface WHERE generation_id = $1",
    )
    .bind(generation_id)
    .fetch_one(pool)
    .await
    .expect("count generation_trigram_surface");
    let cards: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM generation_event_cards WHERE generation_id = $1")
            .bind(generation_id)
            .fetch_one(pool)
            .await
            .expect("count generation_event_cards");
    let details: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM generation_procedure_details WHERE generation_id = $1",
    )
    .bind(generation_id)
    .fetch_one(pool)
    .await
    .expect("count generation_procedure_details");
    (life_events, fts, trigram, cards, details)
}

/// The RED contract: identical inputs build twice → same generation id and
/// content hash, and the projection tables hold exactly one row per
/// `(generation_id, slug)` — no duplicates from the second write.
#[tokio::test(flavor = "multi_thread")]
async fn building_the_same_input_twice_yields_same_id_and_hash_without_duplicates() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;

    let first = db::generations::build::build_generation(&pool, "taxonomy-fixture-1")
        .await
        .expect("first build succeeds");
    let second = db::generations::build::build_generation(&pool, "taxonomy-fixture-1")
        .await
        .expect("second build succeeds");

    assert_eq!(
        first.generation_id, second.generation_id,
        "identical content must reuse the same generation id"
    );
    assert_eq!(
        first.content_hash, second.content_hash,
        "identical content must produce the same content hash"
    );
    assert!(second.reused, "the second build must report reuse");

    let (life_events, fts, trigram, cards, details) =
        projection_row_counts(&pool, first.generation_id).await;
    assert_eq!(life_events, 2, "exactly one life-events row per event");
    assert_eq!(fts, 2, "exactly one fts_text row per event");
    assert_eq!(trigram, 2, "exactly one trigram surface row per event");
    assert_eq!(cards, 2, "exactly one cards row per event");
    assert_eq!(details, 2, "exactly one details row per procedure");

    let manifest_status: String =
        sqlx::query_scalar("SELECT status FROM catalog_generations WHERE generation_id = $1")
            .bind(first.generation_id)
            .fetch_one(&pool)
            .await
            .expect("manifest row exists");
    assert_eq!(manifest_status, "building", "a built candidate is building");

    drop_db(&db_name).await;
}

/// Changing only an observable sync date (`last_seen_at`) changes the
/// content hash — dates are part of the hashed observable payload.
#[tokio::test(flavor = "multi_thread")]
async fn changing_only_sync_dates_changes_the_hash() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;

    let first = db::generations::build::build_generation(&pool, "taxonomy-fixture-1")
        .await
        .expect("first build succeeds");

    sqlx::query("UPDATE procedures SET last_seen_at = now() WHERE external_id = '1001'")
        .execute(&pool)
        .await
        .expect("touch last_seen_at");

    let second = db::generations::build::build_generation(&pool, "taxonomy-fixture-1")
        .await
        .expect("second build succeeds");

    assert_ne!(
        first.content_hash, second.content_hash,
        "a last_seen_at change must change the content hash"
    );
    assert_ne!(
        first.generation_id, second.generation_id,
        "different content must not reuse the previous generation id"
    );
    assert!(
        second.source_synced_at >= first.source_synced_at,
        "source_synced_at reflects the observable sync dates"
    );

    drop_db(&db_name).await;
}

/// Rebuilding over a candidate whose projections were interrupted (the
/// manifest reports incomplete projections) rewrites every projection
/// idempotently: the counts never grow.
#[tokio::test(flavor = "multi_thread")]
async fn rewriting_projections_for_the_same_generation_is_idempotent() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;

    let first = db::generations::build::build_generation(&pool, "taxonomy-fixture-1")
        .await
        .expect("first build succeeds");

    // Simulate an interrupted projection phase: back to an incomplete state.
    sqlx::query("DELETE FROM generation_fts_text WHERE generation_id = $1")
        .bind(first.generation_id)
        .execute(&pool)
        .await
        .expect("delete fts rows");
    sqlx::query(
        "UPDATE catalog_generations SET projection_status = 'building' WHERE generation_id = $1",
    )
    .bind(first.generation_id)
    .execute(&pool)
    .await
    .expect("mark projections incomplete");

    db::generations::build::write_projections(&pool, first.generation_id)
        .await
        .expect("projection rewrite succeeds");

    let (life_events, fts, trigram, cards, details) =
        projection_row_counts(&pool, first.generation_id).await;
    assert_eq!(
        (life_events, fts, trigram, cards, details),
        (2, 2, 2, 2, 2),
        "a projection rewrite must leave exactly one row per (generation_id, slug)"
    );

    drop_db(&db_name).await;
}

/// TRIANGULATE: an interrupted build leaves `status = 'building'` with
/// incomplete projections, and the manifest never reports complete
/// projections for it.
#[tokio::test(flavor = "multi_thread")]
async fn interrupted_build_leaves_building_status_and_incomplete_projections() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;

    // Begin the build only (manifest row, no projections): the interrupted
    // candidate state.
    let begun = db::generations::build::begin_build(&pool, "taxonomy-fixture-1")
        .await
        .expect("begin_build succeeds");

    let (status, projection_status): (String, String) = sqlx::query_as(
        "SELECT status, projection_status FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(begun.generation_id)
    .fetch_one(&pool)
    .await
    .expect("manifest row exists");
    assert_eq!(status, "building", "an interrupted build stays building");
    assert!(
        projection_status != "complete",
        "an interrupted build never reports complete projections"
    );
    let (life_events, fts, trigram, cards, details) =
        projection_row_counts(&pool, begun.generation_id).await;
    assert_eq!(
        (life_events, fts, trigram, cards, details),
        (0, 0, 0, 0, 0),
        "an interrupted build has no projection rows yet"
    );

    drop_db(&db_name).await;
}

/// Keyword insertion order cannot decide the bytes of a generation's
/// captured trigram surface: the positive terms are sorted by term and type.
#[tokio::test(flavor = "multi_thread")]
async fn trigram_surface_orders_positive_keywords_independent_of_insert_order() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    sqlx::query(
        "INSERT INTO life_event_keywords (life_event_id, term, type, weight) \
         SELECT id, 'abrir', 'ACTION', 1 FROM life_events WHERE slug = 'alta-vehiculo'",
    )
    .execute(&pool)
    .await
    .expect("insert a lexically earlier keyword last");
    let report = db::generations::build::build_generation(&pool, "taxonomy-order")
        .await
        .expect("build generation");
    let actual: String = sqlx::query_scalar(
        "SELECT surface_text FROM generation_trigram_surface \
         WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("read deterministic trigram surface");
    assert_eq!(
        actual, "Alta de vehículos abrir  registro  vehiculo auto",
        "positive keywords must be aggregated by term and type, not insertion order"
    );
    let legacy_surface: String = sqlx::query_scalar(
        "SELECT e.name || ' ' || COALESCE(\
             (SELECT string_agg(k.term || ' ' || COALESCE(k.canonical_term, ''), ' ' \
                     ORDER BY k.term, k.type) \
              FROM life_event_keywords k \
              WHERE k.life_event_id = e.id AND NOT k.negative), '') \
         FROM life_events e WHERE e.slug = 'alta-vehiculo'",
    )
    .fetch_one(&pool)
    .await
    .expect("read ordered legacy surface");
    assert_eq!(
        actual, legacy_surface,
        "byte parity requires both aggregates to pin the same ordering"
    );
    drop_db(&db_name).await;
}

/// The projection content itself follows today's canonical rules: the
/// trigram surface is the name plus the positive keywords with their
/// canonical terms (negatives excluded), matching the legacy provider's
/// per-request `string_agg` composition.
#[tokio::test(flavor = "multi_thread")]
async fn trigram_surface_replicates_the_legacy_canonical_rules() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;

    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-1")
        .await
        .expect("build succeeds");

    let surface: String = sqlx::query_scalar(
        "SELECT surface_text FROM generation_trigram_surface \
         WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("trigram surface row exists");
    // New builds pin the aggregate by (term, type); content excludes negative
    // keywords, and a NULL canonical term retains its internal space.
    assert_eq!(
        surface, "Alta de vehículos registro  vehiculo auto",
        "surface = name + ordered positive keywords and canonical terms"
    );

    let no_keywords_surface: String = sqlx::query_scalar(
        "SELECT surface_text FROM generation_trigram_surface \
         WHERE generation_id = $1 AND slug = 'otro-tramite'",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("trigram surface row exists for the keyword-less event");
    // Exact legacy equivalence: an event without positive keywords keeps the
    // trailing space the legacy `name || ' ' || COALESCE(terms, '')` produced.
    assert_eq!(
        no_keywords_surface, "Trámite genérico ",
        "the keyword-less surface must replicate the legacy trailing-space composition"
    );

    let fts_text: String = sqlx::query_scalar(
        "SELECT fts_text FROM generation_fts_text WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("fts_text row exists");
    assert!(
        fts_text.contains("Alta de vehiculos"),
        "the FTS surface is de-accented through the unaccent wrapper, got: {fts_text:?}"
    );

    // WU-5a: the projection also stores the weighted tsvector the provider
    // ranks, byte-equal to the legacy `life_events.generated_tsvector` for
    // the same content (name='A' + description='B', unaccented).
    let fts_vector_matches_legacy: bool = sqlx::query_scalar(
        "SELECT g.fts_tsvector = e.generated_tsvector \
         FROM generation_fts_text g JOIN life_events e ON e.slug = g.slug \
         WHERE g.generation_id = $1 AND g.slug = 'alta-vehiculo'",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("fts_tsvector row exists");
    assert!(
        fts_vector_matches_legacy,
        "the projected fts_tsvector must reproduce the legacy weighted vector"
    );

    let cards: serde_json::Value = sqlx::query_scalar(
        "SELECT cards FROM generation_event_cards WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("cards row exists");
    assert_eq!(cards.as_array().expect("cards array").len(), 1);
    assert_eq!(cards[0]["slug"], "1001");
    assert_eq!(cards[0]["order_index"], 1);

    let (status, event_count, procedure_count): (String, i32, i32) = sqlx::query_as(
        "SELECT status, event_count, procedure_count FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("manifest row exists");
    assert_eq!(status, "building");
    assert_eq!(event_count, 2);
    assert_eq!(procedure_count, 2);

    drop_db(&db_name).await;
}
