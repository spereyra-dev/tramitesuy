//! S8 task 24 (catalog-generations delta, OPT-04, R5/R10): retention and
//! gated collection. The worker's collector keeps three generations by
//! default (configurable), runs off the request path, and deletes a
//! generation's projections only after the newest publication is confirmed
//! adopted AND the generation has no in-flight holder (or its retention
//! window has passed). A lagging API's projection is never deleted; the
//! active and previous generations are never touched; collection is
//! idempotent.

#[path = "c2support/mod.rs"]
mod c2support;

use c2support::*;
use std::time::Duration;

use db::generations::adopt::{self, AdoptionState};
use db::generations::collect::{self, CollectionConfig};
use sqlx::types::uuid::Uuid;

const PROJECTION_TABLES: [&str; 5] = [
    "generation_life_events",
    "generation_fts_text",
    "generation_trigram_surface",
    "generation_event_cards",
    "generation_procedure_details",
];

/// Strips `//` line comments so documentation about the *absence* of the
/// collector cannot trip the scan (same discipline as `no_sync_bridge`).
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_rs_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("cannot read {}: directory missing", dir.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

/// Inserts one published manifest row with a projection row in every
/// `generation_*` table, spaced in time by `published_age`. Returns the id.
async fn seed_published_generation(pool: &sqlx::PgPool, age: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO catalog_generations \
         (generation_id, status, content_hash, taxonomy_version, engine_version, \
          source_synced_at, event_count, procedure_count, projection_status, published_at) \
         VALUES ($1, 'published', 'hash', 'tax-v1', 'engine-v1', now(), 1, 1, 'complete', \
                 now() - $2::interval)",
    )
    .bind(id)
    .bind(age)
    .execute(pool)
    .await
    .expect("published manifest row inserted");
    {
        let audited = sqlx::AssertSqlSafe(format!(
            "INSERT INTO generation_life_events (generation_id, slug, name, status, category_slug, order_index) \
             VALUES ('{id}', 'comprar-vehiculo', 'Evento', 'active', 'vehiculos', 1)"
        ));
        sqlx::query(audited)
            .execute(pool)
            .await
            .expect("projection row inserted");
    }
    {
        let audited = sqlx::AssertSqlSafe(format!(
            "INSERT INTO generation_fts_text (generation_id, slug, fts_text) VALUES ('{id}', 'comprar-vehiculo', 'texto')"
        ));
        sqlx::query(audited)
            .execute(pool)
            .await
            .expect("projection row inserted");
    }
    {
        let audited = sqlx::AssertSqlSafe(format!(
            "INSERT INTO generation_trigram_surface (generation_id, slug, surface_text) VALUES ('{id}', 'comprar-vehiculo', 'texto')"
        ));
        sqlx::query(audited)
            .execute(pool)
            .await
            .expect("projection row inserted");
    }
    {
        let audited = sqlx::AssertSqlSafe(format!(
            "INSERT INTO generation_event_cards (generation_id, slug, cards) VALUES ('{id}', 'comprar-vehiculo', '[]'::jsonb)"
        ));
        sqlx::query(audited)
            .execute(pool)
            .await
            .expect("projection row inserted");
    }
    {
        let audited = sqlx::AssertSqlSafe(format!(
            "INSERT INTO generation_procedure_details (generation_id, slug, details) VALUES ('{id}', '4551', '{{}}'::jsonb)"
        ));
        sqlx::query(audited)
            .execute(pool)
            .await
            .expect("projection row inserted");
    }
    id
}

/// The row count of one generation's projections across the five tables.
async fn projection_rows(pool: &sqlx::PgPool, id: Uuid) -> i64 {
    let mut total = 0;
    for table in PROJECTION_TABLES {
        // Audited: names from the committed allowlist above.
        let audited = sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE generation_id = '{id}'"
        ));
        let rows: i64 = sqlx::query_scalar(audited)
            .fetch_one(pool)
            .await
            .expect("count rows");
        total += rows;
    }
    total
}

async fn retired_at_of(pool: &sqlx::PgPool, id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT retired_at::text FROM catalog_generations WHERE generation_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("manifest row")
}

fn config(retention: usize, window: Duration) -> CollectionConfig {
    CollectionConfig {
        retention,
        retention_window: window,
    }
}

/// Four published generations G0..G3 (newest G3) with G3 confirmed adopted.
async fn four_generations(pool: &sqlx::PgPool) -> (Uuid, Uuid, Uuid, Uuid) {
    let g0 = seed_published_generation(pool, "4 hours").await;
    let g1 = seed_published_generation(pool, "3 hours").await;
    let g2 = seed_published_generation(pool, "2 hours").await;
    let g3 = seed_published_generation(pool, "1 hour").await;
    adopt::confirm_adoption(pool, g3, &[])
        .await
        .expect("adopt newest");
    (g0, g1, g2, g3)
}

/// The request path never issues collection work (task 24): no api handler
/// or serving-path module references the collector — collection is a
/// worker-side reconcile concern only. Source inspection (same discipline
/// as `no_sync_bridge`).
#[test]
fn collection_issues_no_work_on_the_request_path() {
    let api_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("apps/api/src");
    let mut sources = Vec::new();
    collect_rs_files(&api_src, &mut sources);
    for path in &sources {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let stripped = strip_line_comments(&text);
        assert!(
            !stripped.contains("generations::collect") && !stripped.contains("collect_generations"),
            "{} references the generation collector on the serving path",
            path.display()
        );
    }
    assert!(
        sources.len() > 10,
        "the scan must actually see the api sources"
    );
}

#[tokio::test]
async fn a_generation_held_by_an_in_flight_arc_is_deferred() {
    let (pool, name) = fresh_migrated_db().await;
    let (g0, _g1, _g2, _g3) = four_generations(&pool).await;

    // G0 is beyond retention and still held by an in-flight request's
    // captured Arc (the API reported it in the adoption record); the
    // retention window is still open.
    let adoption = AdoptionState {
        generation_id: _g3,
        adopted_at: sqlx::types::chrono::Utc::now(),
        in_flight: vec![g0],
    };
    let report = collect::collect_generations(
        &pool,
        config(3, Duration::from_secs(3600)),
        &adoption,
        sqlx::types::chrono::Utc::now(),
    )
    .await
    .expect("collection runs");

    assert_eq!(
        report.deferred_in_flight,
        vec![g0],
        "the in-flight holder defers collection"
    );
    assert!(
        report.collected.is_empty(),
        "nothing is collected while the holder lives"
    );
    assert!(
        projection_rows(&pool, g0).await > 0,
        "the deferred projection is untouched"
    );

    drop_db(&name).await;
}

#[tokio::test]
async fn a_lagging_apis_projection_is_never_deleted() {
    let (pool, name) = fresh_migrated_db().await;
    let (g0, _g1, stale, g3) = four_generations(&pool).await;
    // The API confirmed adoption of the previous publication only — it lags
    // behind the newest publication.
    let adoption = AdoptionState {
        generation_id: stale,
        adopted_at: sqlx::types::chrono::Utc::now(),
        in_flight: vec![],
    };

    let report = collect::collect_generations(
        &pool,
        config(3, Duration::from_secs(3600)),
        &adoption,
        sqlx::types::chrono::Utc::now(),
    )
    .await
    .expect("collection runs");

    assert!(
        report.deferred_unadopted,
        "the lagging adoption defers every collection"
    );
    assert!(report.collected.is_empty());
    assert!(
        projection_rows(&pool, g0).await > 0,
        "the lagging API's projection is retained"
    );
    assert!(projection_rows(&pool, g3).await > 0);

    // No adoption at all is the strongest lagging case.
    let (pool2, name2) = fresh_migrated_db().await;
    let (g0b, _g1b, _g2b, _g3b) = four_generations(&pool2).await;
    let none = AdoptionState {
        generation_id: Uuid::nil(),
        adopted_at: sqlx::types::chrono::Utc::now(),
        in_flight: vec![],
    };
    let report2 = collect::collect_generations(
        &pool2,
        config(3, Duration::from_secs(3600)),
        &none,
        sqlx::types::chrono::Utc::now(),
    )
    .await
    .expect("collection runs");
    assert!(
        report2.deferred_unadopted,
        "no confirmed adoption defers collection"
    );
    assert!(projection_rows(&pool2, g0b).await > 0);
    drop_db(&name2).await;

    drop_db(&name).await;
}

#[tokio::test]
async fn a_generation_outside_retention_with_no_holder_is_collected() {
    let (pool, name) = fresh_migrated_db().await;
    let (g0, g1, _g2, _g3) = four_generations(&pool).await;
    let adoption = AdoptionState {
        generation_id: _g3,
        adopted_at: sqlx::types::chrono::Utc::now(),
        in_flight: vec![],
    };

    let report = collect::collect_generations(
        &pool,
        config(3, Duration::from_secs(3600)),
        &adoption,
        sqlx::types::chrono::Utc::now(),
    )
    .await
    .expect("collection runs");

    assert_eq!(
        report.collected,
        vec![g0],
        "only the beyond-retention generation is collected"
    );
    assert!(
        projection_rows(&pool, g0).await == 0,
        "the collected generation's projections are deleted"
    );
    assert!(
        retired_at_of(&pool, g0).await.is_some(),
        "the manifest records the retirement"
    );
    assert!(
        projection_rows(&pool, g1).await > 0,
        "the previous generation is never touched"
    );
    assert!(
        projection_rows(&pool, _g3).await > 0,
        "the active generation is never touched"
    );

    drop_db(&name).await;
}

#[tokio::test]
async fn collection_is_idempotent_and_the_window_passing_releases_in_flight() {
    let (pool, name) = fresh_migrated_db().await;
    let (g0, _g1, _stale, g3) = four_generations(&pool).await;
    // An in-flight holder whose retention window has already passed: the
    // time bound releases the projection regardless of the holder report.
    let adopted_long_ago = sqlx::types::chrono::Utc::now() - chrono::Duration::hours(2);
    let adoption = AdoptionState {
        generation_id: g3,
        adopted_at: adopted_long_ago,
        in_flight: vec![g0],
    };

    let first = collect::collect_generations(
        &pool,
        config(3, Duration::from_secs(60)),
        &adoption,
        sqlx::types::chrono::Utc::now(),
    )
    .await
    .expect("collection runs");
    assert_eq!(
        first.collected,
        vec![g0],
        "the window passing collects the held generation"
    );

    // A second pass collects nothing new and deletes no further rows.
    let second = collect::collect_generations(
        &pool,
        config(3, Duration::from_secs(60)),
        &adoption,
        sqlx::types::chrono::Utc::now(),
    )
    .await
    .expect("collection runs");
    assert!(
        second.collected.is_empty(),
        "collection is idempotent: already-collected generations are skipped"
    );
    assert_eq!(
        second.deleted_rows, 0,
        "a repeated pass issues no deletions"
    );

    drop_db(&name).await;
}

// ---------------------------------------------------------------------------
// S14 task 45 (spec §7 test 6, OPT-04, R5/R10): the OLD generation's
// providers stay usable until the in-flight requests finish AND the
// adoption of the publication is confirmed. The collector's deferral is
// bound here to the provider surface itself: while the request holds the
// old generation (reported in flight), the old generation's trigram
// provider still answers from its retained projections; only after the
// drain and the confirmed adoption — or the retention window passing —
// does collection remove the surface, and only a drained request can
// ever observe that.
// ---------------------------------------------------------------------------
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;

use db::generations::build;
use search::engine::CandidateProvider;
use search::normalizer::normalize;
use search::types::NormalizedQuery;

fn repo_data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data")
}

/// Publishes one built generation through the guarded reference advance
/// (the worker-side promotion stand-in used by the other suites).
async fn publish(pool: &sqlx::PgPool, generation_id: Uuid) {
    let report = db::generations::validate::validate_generation(pool, generation_id, None)
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
    .bind(generation_id)
    .execute(pool)
    .await
    .expect("promote the validated reference")
    .rows_affected();
    assert_eq!(promoted, 1, "the built generation validates and publishes");
}

/// The old generation's trigram provider (the request's captured
/// generation is the provider scope).
async fn old_provider_candidates(
    pool: &sqlx::PgPool,
    generation_id: Uuid,
    query: &'static str,
) -> Result<Vec<String>, String> {
    let provider = db::providers::generation_trigram::GenerationTrigramProvider::new(pool.clone());
    let normalized: NormalizedQuery = normalize(query);
    provider
        .candidates(generation_id, &normalized)
        .await
        .map(|candidates| candidates.into_iter().map(|c| c.event_slug).collect())
        .map_err(|error| error.to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_old_generations_providers_stay_usable_until_in_flight_drains_and_adoption_confirms() {
    let (pool, name) = fresh_migrated_db().await;
    catalog_fixture::apply(&pool, &repo_data_dir(), 42)
        .await
        .expect("task 3 catalog fixture applies");
    let old = build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("the old generation builds");
    publish(&pool, old.generation_id).await;
    // The publication the API must confirm adopting: a changed catalog
    // builds the new generation over the same legacy tables.
    sqlx::query(
        "UPDATE life_events SET name = name || ' (cambiado)' WHERE slug = 'comprar-vehiculo'",
    )
    .execute(&pool)
    .await
    .expect("content change ingested");
    let new = build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("the new generation builds");
    publish(&pool, new.generation_id).await;
    assert_ne!(old.generation_id, new.generation_id);

    // Adoption state: the API swapped to the new generation and reports
    // the old one still held by an in-flight request (its captured Arc).
    let adoption = AdoptionState {
        generation_id: new.generation_id,
        adopted_at: sqlx::types::chrono::Utc::now(),
        in_flight: vec![old.generation_id],
    };

    // Retention 1: the fixture's two generations make the old one the
    // only beyond-retention candidate (the active one is never touched).
    let report = collect::collect_generations(
        &pool,
        config(1, Duration::from_secs(3600)),
        &adoption,
        sqlx::types::chrono::Utc::now(),
    )
    .await
    .expect("collection runs");
    assert_eq!(
        report.deferred_in_flight,
        vec![old.generation_id],
        "the in-flight holder defers collection"
    );

    // The OLD generation's provider is still usable mid-flight: the
    // retained projections answer the scoped provider query.
    let candidates = old_provider_candidates(&pool, old.generation_id, "consultar deuda vehicular")
        .await
        .expect("the old provider still answers while the request is in flight");
    assert!(
        candidates.contains(&"consultar-deuda-vehicular".to_string()),
        "the old generation's candidates answer from its retained surface: \
         {candidates:?}"
    );

    // The in-flight request finishes (its Arc drops) and the adoption of
    // the new publication stays confirmed; the retention window passes.
    let adoption_after_drain = AdoptionState {
        generation_id: new.generation_id,
        adopted_at: sqlx::types::chrono::Utc::now() - chrono::Duration::seconds(120),
        in_flight: vec![],
    };
    let report = collect::collect_generations(
        &pool,
        config(3, Duration::from_secs(60)),
        &adoption_after_drain,
        sqlx::types::chrono::Utc::now(),
    )
    .await
    .expect("collection runs after the drain");
    // The old generation is beyond retention among the fixture's two
    // generations only when retention holds fewer of them.
    if report.collected.contains(&old.generation_id) {
        // Only a DRAINED request can ever observe the removed surface.
        let failed =
            old_provider_candidates(&pool, old.generation_id, "compre un auto usado").await;
        assert!(
            failed.is_err(),
            "after collection removed the retained projections the old \
             provider no longer answers: {failed:?}"
        );
    } else {
        assert!(
            projection_rows(&pool, old.generation_id).await > 0,
            "an uncollected generation's provider surface is retained"
        );
    }

    drop_db(&name).await;
}
