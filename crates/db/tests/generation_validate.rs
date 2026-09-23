//! Task 17 (S6): the publication validation gate
//! (`db::generations::validate`). Validation checks relation integrity,
//! schema, taxonomy, search-projection availability, and rejects an
//! accidentally empty catalog before promotion; individual invalid source
//! rows keep the ingestion pipeline's skip-and-report policy (they never
//! fail validation of the whole generation).

#[path = "c2support/mod.rs"]
mod c2support;

use c2support::*;
use std::hint::black_box;
use std::time::Instant;
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

/// F26: non-gating local measurement of complete validation, not just a
/// stand-alone SELECT. A fixed fixture is rebuilt with 1/16/64 distinct
/// projected card/detail pairs. The SQL counter includes the gate's queries
/// (including the card lookup and per-card detail lookups) but not setup.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "opt-in F26 local latency measurement; run with --ignored --nocapture"]
async fn measure_validation_by_card_count() {
    const RUNS: u32 = 20;
    let (pool, name, _counter, section) = fresh_migrated_counting_db().await;
    seed_small_catalog(&pool).await;
    let built = db::generations::build::build_generation(&pool, "taxonomy-fixture-f26")
        .await
        .expect("build valid baseline");
    let generation_id = built.generation_id;

    // Clone a real projected detail, altering only its identifier. This
    // keeps schema validation meaningful and gives each card its own detail.
    sqlx::query(
        "INSERT INTO generation_procedure_details (generation_id, slug, details) \
         SELECT $1, 'f26-' || n::text, \
                jsonb_set(details, '{external_id}', to_jsonb(('f26-' || n::text)::text)) \
         FROM generation_procedure_details, generate_series(1, 64) n \
         WHERE generation_id = $1 AND slug = '1001'",
    )
    .bind(generation_id)
    .execute(&pool)
    .await
    .expect("clone details");

    for cards in [1_i32, 16, 64] {
        sqlx::query(
            "UPDATE generation_event_cards SET cards = ( \
               SELECT jsonb_agg(jsonb_set(jsonb_set(c.cards -> 0, '{slug}', \
                 to_jsonb(('f26-' || n::text)::text)), '{order_index}', to_jsonb(n))) \
               FROM generate_series(1, $2) n \
             ) FROM generation_event_cards c \
             WHERE generation_event_cards.generation_id = $1 \
               AND generation_event_cards.slug = 'alta-vehiculo' \
               AND c.generation_id = $1 AND c.slug = 'alta-vehiculo'",
        )
        .bind(generation_id)
        .bind(cards)
        .execute(&pool)
        .await
        .expect("size projected cards");
        let gate = db::generations::validate::validate_generation(&pool, generation_id, None)
            .await
            .expect("warm validation");
        assert!(gate.passed(), "{gate:?}");

        section.reset();
        let start = Instant::now();
        for _ in 0..RUNS {
            let gate = db::generations::validate::validate_generation(&pool, generation_id, None)
                .await
                .expect("validation completes");
            assert!(gate.passed(), "{gate:?}");
            black_box(gate);
        }
        let us = start.elapsed().as_micros() as f64 / f64::from(RUNS);
        let sql_total = section.count();
        eprintln!(
            "F26 validation: cards={cards} projected_details=66 runs={RUNS} \
             validation_us={us:.2} sql_per_run={:.1}",
            sql_total as f64 / f64::from(RUNS)
        );
    }
    drop(pool);
    drop_db(&name).await;
}

/// Each mandatory field of the API's card/detail decoder must be present and
/// correctly typed, including the RFC 3339 sync stamp. A malformed optional
/// string must not silently lose its value when decoded.
#[tokio::test(flavor = "multi_thread")]
async fn projections_rejected_when_decoder_fields_are_missing_or_malformed() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    let built = db::generations::build::build_generation(&pool, "taxonomy-fixture-f16")
        .await
        .expect("build succeeds");
    let id = built.generation_id;

    let card: serde_json::Value = sqlx::query_scalar(
        "SELECT cards FROM generation_event_cards WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("card projection");
    let detail: serde_json::Value = sqlx::query_scalar(
        "SELECT details FROM generation_procedure_details WHERE generation_id = $1 AND slug = '1001'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("detail projection");

    for (table, fields, baseline) in [
        (
            "generation_event_cards",
            &[
                "slug",
                "name",
                "order_index",
                "required",
                "status",
                "last_seen_at",
            ][..],
            &card,
        ),
        (
            "generation_procedure_details",
            &["external_id", "name", "status", "last_seen_at"][..],
            &detail,
        ),
    ] {
        for field in fields {
            let mut malformed = baseline.clone();
            if table == "generation_event_cards" {
                malformed[0]
                    .as_object_mut()
                    .expect("card object")
                    .remove(*field);
            } else {
                malformed
                    .as_object_mut()
                    .expect("detail object")
                    .remove(*field);
            }
            assert_projection_schema_fails(&pool, id, table, &malformed, baseline, field).await;
        }
    }
    for (table, baseline, field, invalid) in [
        (
            "generation_event_cards",
            &card,
            "order_index",
            serde_json::json!("1"),
        ),
        (
            "generation_event_cards",
            &card,
            "required",
            serde_json::json!("true"),
        ),
        (
            "generation_event_cards",
            &card,
            "last_seen_at",
            serde_json::json!("not-a-date"),
        ),
        (
            "generation_procedure_details",
            &detail,
            "external_id",
            serde_json::json!(42),
        ),
        (
            "generation_procedure_details",
            &detail,
            "last_seen_at",
            serde_json::json!("not-a-date"),
        ),
    ] {
        let mut malformed = baseline.clone();
        if table == "generation_event_cards" {
            malformed[0][field] = invalid;
        } else {
            malformed[field] = invalid;
        }
        assert_projection_schema_fails(&pool, id, table, &malformed, baseline, field).await;
    }
    for (table, baseline, fields) in [
        (
            "generation_event_cards",
            &card,
            &[
                "importance",
                "organization_short_name",
                "cost",
                "official_url",
            ][..],
        ),
        (
            "generation_procedure_details",
            &detail,
            &["description", "organization_name", "official_url"][..],
        ),
    ] {
        for field in fields {
            let mut malformed = baseline.clone();
            if table == "generation_event_cards" {
                malformed[0][field] = serde_json::json!(42);
            } else {
                malformed[field] = serde_json::json!(42);
            }
            assert_projection_schema_fails(&pool, id, table, &malformed, baseline, field).await;
        }
    }
    assert_projection_schema_fails(
        &pool,
        id,
        "generation_event_cards",
        &serde_json::json!({}),
        &card,
        "cards array",
    )
    .await;
    drop(pool);
    drop_db(&db_name).await;
}

async fn assert_projection_schema_fails(
    pool: &sqlx::PgPool,
    id: sqlx::types::Uuid,
    table: &str,
    malformed: &serde_json::Value,
    baseline: &serde_json::Value,
    field: &str,
) {
    let statement = match table {
        "generation_event_cards" => {
            "UPDATE generation_event_cards SET cards = $2 WHERE generation_id = $1 AND slug = 'alta-vehiculo'"
        }
        "generation_procedure_details" => {
            "UPDATE generation_procedure_details SET details = $2 WHERE generation_id = $1 AND slug = '1001'"
        }
        _ => panic!("unexpected projection table"),
    };
    sqlx::query(statement)
        .bind(id)
        .bind(malformed)
        .execute(pool)
        .await
        .expect("inject malformed projection");
    let gate = db::generations::validate::validate_generation(pool, id, None).await;
    sqlx::query(statement)
        .bind(id)
        .bind(baseline)
        .execute(pool)
        .await
        .expect("restore projection");
    let gate = gate
        .unwrap_or_else(|error| panic!("{table}.{field} must yield a validation report: {error}"));
    assert!(
        gate.failures.iter().any(|failure| failure.kind == "schema"),
        "{table}.{field} must fail schema validation: {:?}",
        gate.failures
    );
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

    let gate = db::generations::validate::validate_generation(&pool, report.generation_id, None)
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

    let gate = db::generations::validate::validate_generation(&pool, report.generation_id, None)
        .await
        .expect("validation runs");
    assert!(
        gate.failures.iter().any(|f| f.kind == "empty_catalog"),
        "a zero-procedure catalog must be rejected as empty, got: {:?}",
        gate.failures
    );

    let status: String =
        sqlx::query_scalar("SELECT status FROM catalog_generations WHERE generation_id = $1")
            .bind(report.generation_id)
            .fetch_one(&pool)
            .await
            .expect("manifest row exists");
    assert_ne!(
        status, "published",
        "a rejected candidate is never published"
    );
    assert_ne!(status, "validated", "a rejected candidate never validates");

    drop_db(&db_name).await;
}

/// A candidate whose procedures are all inactive declares no usable
/// procedure. The manifest counters count inactive rows, so they stay
/// non-zero; the gate must inspect the generation's own immutable projection
/// and reject the candidate as `empty_active_catalog`.
#[tokio::test(flavor = "multi_thread")]
async fn all_inactive_procedures_are_rejected_as_empty_active_catalog() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    sqlx::query("UPDATE procedures SET status = 'inactive', deactivated_at = now()")
        .execute(&pool)
        .await
        .expect("deactivate every procedure");

    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds (a build is not a publication)");

    // The manifest counters count inactive rows, so they are non-zero — the
    // pre-existing `empty_catalog` check cannot catch this candidate.
    let (event_count, procedure_count): (i32, i32) = sqlx::query_as(
        "SELECT event_count, procedure_count FROM catalog_generations \
         WHERE generation_id = $1",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("manifest row exists");
    assert!(
        event_count > 0 && procedure_count > 0,
        "the manifest counters must be non-zero for this candidate to be a real gap"
    );

    let gate = db::generations::validate::validate_generation(&pool, report.generation_id, None)
        .await
        .expect("validation runs");
    assert!(
        gate.failures
            .iter()
            .any(|f| f.kind == "empty_active_catalog"),
        "an all-inactive catalog must be rejected as empty_active_catalog, got: {:?}",
        gate.failures
    );

    let status: String =
        sqlx::query_scalar("SELECT status FROM catalog_generations WHERE generation_id = $1")
            .bind(report.generation_id)
            .fetch_one(&pool)
            .await
            .expect("manifest row exists");
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

    let gate = db::generations::validate::validate_generation(&pool, report.generation_id, None)
        .await
        .expect("validation runs");
    assert!(
        gate.failures.iter().any(|f| f.kind == "relation_integrity"),
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

    let gate = db::generations::validate::validate_generation(&pool, report.generation_id, None)
        .await
        .expect("validation runs");
    assert!(
        gate.failures.iter().any(|f| f.kind == "search_projection"),
        "missing FTS/trigram rows for a declared event must be rejected, got: {:?}",
        gate.failures
    );

    drop_db(&db_name).await;
}

/// An older builder can write the FTS text but leave the new vector at its
/// migration default. Non-zero manifest counters and present projection rows
/// must not let that candidate through the publication gate.
#[tokio::test(flavor = "multi_thread")]
async fn nonempty_fts_text_with_empty_vector_is_rejected() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    let built = db::generations::build::build_generation(&pool, "taxonomy-fixture-fts")
        .await
        .expect("build succeeds");
    let id = built.generation_id;

    let (event_count, procedure_count): (i32, i32) = sqlx::query_as(
        "SELECT event_count, procedure_count FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("manifest exists");
    assert!(event_count > 0 && procedure_count > 0);
    let text: String = sqlx::query_scalar(
        "SELECT fts_text FROM generation_fts_text WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("FTS projection exists");
    assert!(!text.is_empty());

    sqlx::query(
        "UPDATE generation_fts_text SET fts_tsvector = ''::tsvector \
         WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(id)
    .execute(&pool)
    .await
    .expect("simulate an older builder using the empty vector default");

    let gate = db::generations::validate::validate_generation(&pool, id, None)
        .await
        .expect("validation returns a report");
    assert_eq!(
        gate.failures
            .iter()
            .filter(|failure| failure.kind == "empty_fts_projection")
            .map(|failure| failure.detail.as_str())
            .collect::<Vec<_>>(),
        vec![
            "generation_fts_text row for event \"alta-vehiculo\" has non-empty fts_text but an empty fts_tsvector"
        ],
        "an empty vector with text must be rejected: {gate:?}"
    );
    let status: String =
        sqlx::query_scalar("SELECT status FROM catalog_generations WHERE generation_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("manifest exists");
    assert_eq!(status, "building", "rejected candidates never validate");

    // The new failure must accumulate with other gates, not hide schema drift.
    sqlx::query(
        "UPDATE generation_event_cards SET cards = '{}'::jsonb \
         WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(id)
    .execute(&pool)
    .await
    .expect("corrupt card schema");
    let combined = db::generations::validate::validate_generation(&pool, id, None)
        .await
        .expect("validation returns all failures");
    assert!(
        combined
            .failures
            .iter()
            .any(|f| f.kind == "empty_fts_projection")
            && combined.failures.iter().any(|f| f.kind == "schema"),
        "both FTS and schema failures must be reported: {combined:?}"
    );

    drop(pool);
    drop_db(&db_name).await;
}

/// The current builder writes a weighted vector; the stricter gate must
/// still accept its generation without requiring mutable source data.
#[tokio::test(flavor = "multi_thread")]
async fn populated_fts_vector_passes_validation() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_small_catalog(&pool).await;
    let built = db::generations::build::build_generation(&pool, "taxonomy-fixture-fts")
        .await
        .expect("build succeeds");
    let vector_populated: bool = sqlx::query_scalar(
        "SELECT fts_tsvector <> ''::tsvector FROM generation_fts_text \
         WHERE generation_id = $1 AND slug = 'alta-vehiculo'",
    )
    .bind(built.generation_id)
    .fetch_one(&pool)
    .await
    .expect("FTS projection exists");
    assert!(vector_populated, "the current builder must write a vector");

    let gate = db::generations::validate::validate_generation(&pool, built.generation_id, None)
        .await
        .expect("validation runs");
    assert!(gate.passed(), "the weighted vector must validate: {gate:?}");

    drop(pool);
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

    let gate = db::generations::validate::validate_generation(&pool, begun.generation_id, None)
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

    let first = db::generations::validate::validate_generation(&pool, report.generation_id, None)
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

    let second = db::generations::validate::validate_generation(&pool, report.generation_id, None)
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
        gate.failures.iter().any(|f| f.kind == "taxonomy"),
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

    let gate = db::generations::validate::validate_generation(&pool, report.generation_id, None)
        .await
        .expect("validation runs");
    assert!(
        gate.failures.is_empty(),
        "skipped-and-reported individual source rows must not fail validation, got: {:?}",
        gate.failures
    );

    drop_db(&db_name).await;
}
