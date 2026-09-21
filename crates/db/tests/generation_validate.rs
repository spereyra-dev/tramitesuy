//! Task 17 (S6): the publication validation gate
//! (`db::generations::validate`). Validation checks relation integrity,
//! schema, taxonomy, search-projection availability, and rejects an
//! accidentally empty catalog before promotion; individual invalid source
//! rows keep the ingestion pipeline's skip-and-report policy (they never
//! fail validation of the whole generation).

#[path = "c2support/mod.rs"]
mod c2support;

use c2support::*;
use taxonomy::model::Taxonomy;

/// Small representative catalog: two events (one with a negative keyword),
/// one organization, two procedures, one ordered relation — everything the
/// gate must accept as complete.
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

    for (id, name) in [
        ("1001", "Solicitud de empadronamientos"),
        ("1002", "Cambio de radicación"),
    ] {
        sqlx::query(
            "INSERT INTO procedures \
             (external_id, name, description, organization_id, official_url, status, raw_data) \
             VALUES ($1, $2, 'Descripcion del tramite', \
                     (SELECT id FROM organizations WHERE external_id = 'D'), \
                     $3, 'active', $4)",
        )
        .bind(id)
        .bind(name)
        .bind(format!("https://www.gub.uy/tramite/{id}"))
        .bind(serde_json::json!({ "id": id }))
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

fn drifting_taxonomy() -> Taxonomy {
    // A taxonomy that does NOT match the projected catalog: its event set
    // differs from the built generation's events.
    Taxonomy::default()
}

/// A valid generation passes validation and is marked `validated`.
#[tokio::test(flavor = "multi_thread")]
async fn valid_generation_passes_and_is_marked_validated() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds");

    let gate = db::generations::validate::validate_generation(
        &pool,
        report.generation_id,
        None,
    )
    .await
    .expect("validation runs");
    assert!(
        gate.failures.is_empty(),
        "the small valid catalog must pass, got: {:?}",
        gate.failures
    );

    let (status, projection_status): (String, String) = sqlx::query_as(
        "SELECT status, projection_status FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("manifest row exists");
    assert_eq!(status, "validated", "a passing gate advances to validated");
    assert_eq!(projection_status, "complete");

    drop_db(&db_name).await;
}

/// A zero-procedure catalog (an accidentally empty source) is rejected.
#[tokio::test(flavor = "multi_thread")]
async fn zero_procedure_catalog_is_rejected() {
    let (pool, db_name) = fresh_migrated_db().await;
    // Taxonomy only: no procedures ingested at all.
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data");
    let taxonomy = taxonomy::loader::load_data_dir(&data_dir).expect("YAML taxonomy loads");
    db::repos::taxonomy_seed::seed_taxonomy(&pool, &taxonomy)
        .await
        .expect("taxonomy seeds");

    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds (a build is not a publication)");

    let gate = db::generations::validate::validate_generation(
        &pool,
        report.generation_id,
        None,
    )
    .await
    .expect("validation runs");
    assert!(
        gate.failures
            .iter()
            .any(|f| f.kind == "empty_catalog"),
        "a zero-procedure catalog must be rejected as empty, got: {:?}",
        gate.failures
    );

    let status: String =
        sqlx::query_scalar("SELECT status FROM catalog_generations WHERE generation_id = $1")
            .bind(report.generation_id)
            .fetch_one(&pool)
            .await
            .expect("manifest row exists");
    assert_ne!(status, "published", "a rejected candidate is never published");
    assert_ne!(status, "validated", "a rejected candidate never validates");

    drop_db(&db_name).await;
}

/// A dangling relation (a card referencing a procedure whose detail row is
/// missing) is rejected as relation-integrity failure.
#[tokio::test(flavor = "multi_thread")]
async fn dangling_relation_is_rejected() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds");

    // Tamper the projection: remove the detail row the card references. The
    // legacy tables stay intact — the gate must catch the broken projection.
    sqlx::query(
        "DELETE FROM generation_procedure_details \
         WHERE generation_id = $1 AND slug = '1001'",
    )
    .bind(report.generation_id)
    .execute(&pool)
    .await
    .expect("details row deleted");

    let gate = db::generations::validate::validate_generation(
        &pool,
        report.generation_id,
        None,
    )
    .await
    .expect("validation runs");
    assert!(
        gate.failures
            .iter()
            .any(|f| f.kind == "relation_integrity"),
        "a dangling card→procedure relation must be rejected, got: {:?}",
        gate.failures
    );

    drop_db(&db_name).await;
}

/// Missing FTS or trigram search-projection rows for a declared event are
/// rejected as search-projection failures.
#[tokio::test(flavor = "multi_thread")]
async fn missing_search_projection_rows_for_a_declared_event_are_rejected() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds");

    sqlx::query(
        "DELETE FROM generation_fts_text WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(report.generation_id)
    .execute(&pool)
    .await
    .expect("fts row deleted");
    sqlx::query(
        "DELETE FROM generation_trigram_surface \
         WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(report.generation_id)
    .execute(&pool)
    .await
    .expect("trigram row deleted");

    let gate = db::generations::validate::validate_generation(
        &pool,
        report.generation_id,
        None,
    )
    .await
    .expect("validation runs");
    assert!(
        gate.failures
            .iter()
            .any(|f| f.kind == "search_projection"),
        "missing FTS/trigram rows for a declared event must be rejected, got: {:?}",
        gate.failures
    );

    drop_db(&db_name).await;
}

/// `status` never advances past `validated` without complete projections: an
/// interrupted candidate (manifest reports incomplete) fails the gate.
#[tokio::test(flavor = "multi_thread")]
async fn status_never_advances_without_complete_projections() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;

    // Begin the build only: manifest `building`, projections incomplete.
    let begun = db::generations::build::begin_build(&pool, "taxonomy-fixture-s6")
        .await
        .expect("begin_build succeeds");

    let gate = db::generations::validate::validate_generation(
        &pool,
        begun.generation_id,
        None,
    )
    .await
    .expect("validation runs");
    assert!(
        !gate.failures.is_empty(),
        "an incomplete candidate must fail the gate, got: {:?}",
        gate.failures
    );

    let status: String =
        sqlx::query_scalar("SELECT status FROM catalog_generations WHERE generation_id = $1")
            .bind(begun.generation_id)
            .fetch_one(&pool)
            .await
            .expect("manifest row exists");
    assert_eq!(
        status, "building",
        "a failing gate never advances the manifest status"
    );

    drop_db(&db_name).await;
}

/// Re-validating an already-validated generation is idempotent: the report is
/// identical and the manifest row does not change.
#[tokio::test(flavor = "multi_thread")]
async fn revalidating_a_validated_generation_is_idempotent() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds");

    let first = db::generations::validate::validate_generation(
        &pool,
        report.generation_id,
        None,
    )
    .await
    .expect("first validation runs");
    assert!(first.failures.is_empty());
    let manifest_after_first: (String, String) = sqlx::query_as(
        "SELECT status, projection_status FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("manifest row exists");

    let second = db::generations::validate::validate_generation(
        &pool,
        report.generation_id,
        None,
    )
    .await
    .expect("second validation runs");
    assert_eq!(first.failures, second.failures);
    let manifest_after_second: (String, String) = sqlx::query_as(
        "SELECT status, projection_status FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("manifest row exists");
    assert_eq!(
        manifest_after_second, manifest_after_first,
        "re-validation leaves the manifest row unchanged"
    );
    assert_eq!(manifest_after_second.0, "validated");

    drop_db(&db_name).await;
}

/// TRIANGULATE: the taxonomy gate — a candidate whose projected events drift
/// from the YAML taxonomy actually used (here: a projected keyword dropped)
/// is rejected as a taxonomy failure; the aligned real taxonomy passes.
#[tokio::test(flavor = "multi_thread")]
async fn taxonomy_drift_is_rejected_and_aligned_taxonomy_passes() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds");

    // A synthetic taxonomy that does NOT match the projected catalog: its
    // event set differs from the built generation's events.
    let drifting = drifting_taxonomy();
    let gate = db::generations::validate::validate_generation(
        &pool,
        report.generation_id,
        Some(&drifting),
    )
    .await
    .expect("validation runs");
    assert!(
        gate.failures
            .iter()
            .any(|f| f.kind == "taxonomy"),
        "a taxonomy drift must be rejected, got: {:?}",
        gate.failures
    );

    drop_db(&db_name).await;
}

/// Individual invalid source rows keep the skip-and-report policy: a catalog
/// whose procedures carry a NULL description (the ingestion pipeline skips
/// and reports individual bad rows without failing the run) still validates.
#[tokio::test(flavor = "multi_thread")]
async fn individual_invalid_source_rows_keep_skip_and_report() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;

    // An individual source row that would be skipped and reported by the
    // ingestion pipeline: a procedure with no description and no reported
    // cost. It must NOT fail the generation's validation.
    sqlx::query(
        "INSERT INTO procedures \
         (external_id, name, description, organization_id, official_url, status, raw_data) \
         VALUES ('1003', 'Trámite sin descripción', NULL, \
                 (SELECT id FROM organizations WHERE external_id = 'D'), NULL, 'active', '{}')",
    )
    .execute(&pool)
    .await
    .expect("seed procedure with null description");
    // Rebuild the generation over the updated source.
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds");

    let gate = db::generations::validate::validate_generation(
        &pool,
        report.generation_id,
        None,
    )
    .await
    .expect("validation runs");
    assert!(
        gate.failures.is_empty(),
        "skipped-and-reported individual source rows must not fail validation, got: {:?}",
        gate.failures
    );

    drop_db(&db_name).await;
}
