//! Shared helpers for the C1 API integration tests.
//!
//! Tests run against the compose Postgres (service `db`, D-6). Each test
//! creates a uniquely named scratch database (`c1_<pid>_<nanos>`),
//! provisions the extensions the docker init SQL provides on a real dev
//! instance (`pg_trgm`, `unaccent`), applies the embedded migrations, seeds
//! the read-surface fixture, and drops the database at the end.
//!
//! `allow(dead_code)` is justified: this module is compiled into EVERY api
//! test binary while each helper is consumed by only some of them —
//! `assert_hyphen_slugs`/`request_json`/`seed_read_fixture` are unused in
//! the cache suites, the SQL-counter helpers are unused in the read
//! suites, and so on; the allow is per-file test support, never a lint
//! escape on production code.
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use api::dto;

/// The repository root (apps/api → repo root), where the real `data/` seed
/// lives; the search tests boot `AppState` from it exactly like production.
pub fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

/// Audited: names are generated internally (`c1_<pid>_<nanos>`), never from
/// user input; sqlx 0.9 requires an explicit safety assertion for dynamic SQL.
fn audited(sql: String) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(sql)
}

fn admin_url() -> String {
    std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string())
}

async fn admin_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&admin_url())
        .await
        .expect("connect to the compose Postgres admin database")
}

fn unique_db_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    format!("c1_{}_{}", std::process::id(), nanos)
}

/// Creates a uniquely named scratch database, provisions the compose-init
/// extensions, and applies the embedded migrations. The name is retried on
/// a collision: parallel test binaries share the process id space and the
/// wall clock, so two `pid+nanos` draws can theoretically coincide
/// (observed once under full-workspace parallelism).
pub async fn fresh_migrated_db() -> (PgPool, String) {
    let admin = admin_pool().await;
    let mut name = unique_db_name();
    loop {
        let result = sqlx::query(audited(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await;
        match result {
            Ok(_) => break,
            // SQLSTATE 23505 on pg_database.datname = a concurrent parallel
            // test binary drew the same pid+nanos name; redraw and retry.
            Err(err)
                if matches!(
                    &err,
                    sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")
                ) =>
            {
                name = unique_db_name();
            }
            Err(err) => panic!("create scratch test database: {err:?}"),
        }
    }
    let url = format!("{}/{}", admin_url().trim_end_matches("/postgres"), name);
    // Full-workspace parallelism can briefly fail the first connect to the
    // freshly created scratch database (observed once per several
    // full-workspace runs); a short bounded retry keeps the scratch setup
    // deterministic without masking real failures.
    let mut scratch = Err(sqlx::Error::Configuration("unreached".into()));
    for attempt in 0..3u32 {
        match PgPoolOptions::new().max_connections(5).connect(&url).await {
            Ok(pool) => {
                scratch = Ok(pool);
                break;
            }
            Err(error) => {
                scratch = Err(error);
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }
    let pool = scratch.expect("connect to scratch test database");
    for ext in ["pg_trgm", "unaccent"] {
        sqlx::query(audited(format!("CREATE EXTENSION IF NOT EXISTS {ext}")))
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("provision extension {ext}: {e:?}"));
    }
    db::pool::run_migrations(&pool)
        .await
        .expect("embedded migrations apply cleanly");
    (pool, name)
}

/// Builds the API router over the given pool (the in-process test server).
/// Boot loads the YAML taxonomy from the repository's real `data/` seed —
/// the same ranker source of truth as production (task 84).
pub fn spawn_app(pool: PgPool) -> Router {
    let state = api::state::AppState::build(pool, &repo_root().join("data"))
        .expect("boot AppState from the real data seed");
    api::build_router(state)
}

/// Boots the router with an injected metrics sink (task 1 seam): same boot
/// path as `spawn_app`, but the counters surface becomes test-readable.
pub fn spawn_app_with_metrics(
    pool: PgPool,
    metrics: std::sync::Arc<dyn api::metrics::Metrics>,
) -> Router {
    let state = api::state::AppState::build_with_metrics(
        pool,
        &repo_root().join("data"),
        api::config::ApiLimits::default(),
        metrics,
    )
    .expect("boot AppState from the real data seed");
    api::build_router(state)
}

/// [`spawn_app_with_metrics`] that ALSO returns the `AppState`: tests that
/// need to inspect the active generation's cache (S9) capture its holder
/// directly instead of reconstructing boot state.
pub fn spawn_app_with_state_and_metrics(
    pool: PgPool,
    metrics: std::sync::Arc<dyn api::metrics::Metrics>,
) -> (Router, api::state::AppState) {
    let state = api::state::AppState::build_with_metrics(
        pool,
        &repo_root().join("data"),
        api::config::ApiLimits::default(),
        metrics,
    )
    .expect("boot AppState from the real data seed");
    let router = api::build_router(state.clone());
    (router, state)
}

/// Builds the API router over a published generation (S7 task 20): the
/// test publishes one catalog generation from the seeded legacy tables
/// (the worker-side promotion stand-in) and boots the state through the
/// durable-load path, so the router serves the snapshot exactly like a
/// restarted production API. Requires `seed_read_fixture` to have run.
pub async fn spawn_app_with_generation(pool: PgPool) -> Router {
    publish_sample_generation(&pool).await;
    let state =
        api::state::AppState::boot(pool, &repo_root().join("data"), limits_without_warming())
            .await
            .expect("boot AppState from the published generation");
    api::build_router(state)
}

/// [`spawn_app_with_generation`] with an injected metrics sink AND the
/// returned state — the S10 tests inspect the active generation's cache
/// (single-flight holders, entries) directly instead of reconstructing boot
/// state. `limits` drives the configured serving budget (e.g. the search
/// deadline that bounds the single-flight wait window).
pub async fn spawn_app_with_generation_state_and_metrics(
    pool: PgPool,
    metrics: std::sync::Arc<dyn api::metrics::Metrics>,
    limits: api::config::ApiLimits,
) -> (Router, api::state::AppState) {
    publish_sample_generation(&pool).await;
    let state =
        api::state::AppState::boot_with_metrics(pool, &repo_root().join("data"), limits, metrics)
            .await
            .expect("boot AppState from the published generation");
    let router = api::build_router(state.clone());
    (router, state)
}

/// [`spawn_app_with_generation`] with an injected metrics sink.
pub async fn spawn_app_with_generation_and_metrics(
    pool: PgPool,
    metrics: std::sync::Arc<dyn api::metrics::Metrics>,
) -> Router {
    publish_sample_generation(&pool).await;
    let state = api::state::AppState::boot_with_metrics(
        pool,
        &repo_root().join("data"),
        limits_without_warming(),
        metrics,
    )
    .await
    .expect("boot AppState from the published generation");
    api::build_router(state)
}

/// Sends one request to the in-process router and returns the status plus
/// the parsed JSON body (Null for empty bodies).
pub async fn request(app: &Router, method: &str, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("well-formed request"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body readable");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

/// Sends one request carrying a JSON body (content-type application/json)
/// and returns the status plus the parsed JSON body.
pub async fn request_json(
    app: &Router,
    method: &str,
    uri: &str,
    json: &Value,
) -> (StatusCode, Value) {
    request_raw(app, method, uri, json.to_string().as_bytes()).await
}

/// Sends one request carrying a raw byte body (no content type) and returns
/// the status plus the parsed JSON body (Null for unparsable bodies).
pub async fn request_raw(
    app: &Router,
    method: &str,
    uri: &str,
    body_bytes: &[u8],
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body_bytes.to_vec()))
                .expect("well-formed request"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body readable");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

/// Seeds the read-surface fixture: two categories (vehiculos first), two
/// events, one organization, four procedures (populated cost, missing cost,
/// deactivated, no raw data), and relations whose rows are inserted out of
/// order to prove `order_index` ordering.
pub async fn seed_read_fixture(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehículos', 'car', 1), ('trabajo', 'Trabajo', 'work', 2)",
    )
    .execute(pool)
    .await
    .expect("seed categories");

    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'comprar-vehiculo', 'Comprar un vehículo', \
                'Requisitos y trámites para comprar un vehículo.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event comprar-vehiculo");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'vender-vehiculo', 'Vender un vehículo', \
                'Trámites para dar de baja o vender un vehículo.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event vender-vehiculo");

    sqlx::query(
        "INSERT INTO organizations (external_id, name, short_name, official_url) \
         VALUES ('org-1', 'Ministerio de Transporte', 'MTOP', 'https://www.gub.uy/mtop')",
    )
    .execute(pool)
    .await
    .expect("seed organization");

    // Procedures: 4551 (empty cost), 2368 (populated cost), 6995
    // (deactivated), 7001 (NULL raw_data — the never-default-a-cost case).
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '4551', 'Solicitud de empadronamientos', 'Empadronamiento ante la DNT.', \
                o.id, 'https://www.gub.uy/tramite/4551', 'active', \
                '{\"tiene_costo\": \"\", \"valor\": \"\"}'::jsonb, \
                '2026-09-18T03:00:00Z', '2026-09-18T03:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 4551");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '2368', 'Alta de vehículos ante la DNT', 'Alta inicial del vehículo.', \
                o.id, 'https://www.gub.uy/tramite/2368', 'active', \
                '{\"tiene_costo\": \"1\", \"valor\": \"55.70\"}'::jsonb, \
                '2026-09-18T04:00:00Z', '2026-09-18T04:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 2368");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at, deactivated_at) \
         SELECT '6995', 'Registro de Automotoras', 'Registro de automotoras.', \
                o.id, 'https://www.gub.uy/tramite/6995', 'inactive', \
                '{\"tiene_costo\": \"\", \"valor\": \"\"}'::jsonb, \
                '2026-09-18T02:00:00Z', '2026-09-18T02:00:00Z', '2026-09-18T05:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 6995");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '7001', 'Trámite sin datos crudos', 'Sin raw_data.', \
                o.id, NULL, 'active', NULL, \
                '2026-09-18T01:00:00Z', '2026-09-18T01:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 7001");

    // Relations inserted out of order: the API must order by order_index.
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 2, FALSE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '2368'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 2");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, TRUE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '4551'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 1");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, TRUE FROM life_events e, procedures p \
         WHERE e.slug = 'vender-vehiculo' AND p.external_id = '6995'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 1");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 2, FALSE FROM life_events e, procedures p \
         WHERE e.slug = 'vender-vehiculo' AND p.external_id = '7001'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 2");
}

/// Builds, validates, and publishes a NEW generation over changed legacy
/// content (the fixture's first procedure is renamed), then installs it
/// into the running state's holder — the test-side equivalent of the S8
/// adoption path. Returns the adopted manifest.
pub async fn adopt_changed_generation(
    state: &api::state::AppState,
    pool: &PgPool,
) -> db::generations::build::BuildManifest {
    sqlx::query("UPDATE procedures SET name = name || ' (cambiado)' WHERE external_id = '4551'")
        .execute(pool)
        .await
        .expect("content change for the new generation");
    let published = publish_sample_generation(pool).await;
    let generation = api::generation::load_published(pool, &repo_root().join("data"))
        .await
        .expect("the new generation loads")
        .expect("the published generation loads");
    state.install(std::sync::Arc::new(generation));
    published
}

/// Drops the scratch database (explicit cleanup at the end of each test).
pub async fn common_drop(name: &str) {
    let admin = admin_pool().await;
    sqlx::query(audited(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .expect("drop scratch test database");
}

/// Task 74 (API-4): asserts the complete attribution block of one
/// procedure-bearing payload — `official: true`, the exact source catalog
/// name, the procedure's official URL, `last_synced_at` equal to the last
/// ingestion run that touched the procedure, and the `odc-uy` license.
pub fn assert_source_attribution(
    source: &Value,
    expected_official_url: Option<&str>,
    expected_last_synced_at: &str,
) {
    assert_eq!(
        source["official"],
        serde_json::json!(true),
        "source.official must be true"
    );
    assert_eq!(
        source["name"], "Catálogo de trámites y servicios del Estado — AGESIC",
        "source.name must be the exact source catalog name"
    );
    match expected_official_url {
        Some(url) => assert_eq!(
            source["official_url"], url,
            "source.official_url must carry the procedure's official URL"
        ),
        None => assert!(
            source["official_url"].is_null(),
            "source.official_url must be null when the procedure has none"
        ),
    }
    assert_eq!(
        source["last_synced_at"], expected_last_synced_at,
        "source.last_synced_at must equal the last run that touched the procedure"
    );
    assert_eq!(source["license"], "odc-uy", "source.license must be odc-uy");
}

/// Task 76 (TX-4): validates every event/category slug in an `/api/v1`
/// response payload against the hyphen convention. Applied to every payload
/// the read-endpoint tests parse.
pub fn assert_hyphen_slugs(payload: &Value) {
    dto::validate_slugs_in_response(payload)
        .unwrap_or_else(|err| panic!("response exposes an invalid slug: {err}"));
}

/// Reads a procedure's `last_seen_at` from the scratch database and formats
/// it exactly as the API serializes `source.last_synced_at`.
pub async fn last_seen_of(pool: &PgPool, external_id: &str) -> String {
    let row: (sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,) =
        sqlx::query_as("SELECT last_seen_at FROM procedures WHERE external_id = $1")
            .bind(external_id)
            .fetch_one(pool)
            .await
            .expect("procedure row readable");
    row.0.to_rfc3339()
}

pub use db::test_support::sql_counter::{SqlCounter, SqlSection};

/// Publishes one catalog generation from the current legacy-table state
/// (S7 task 19): the test-side stand-in for the worker's promotion flow
/// (`apps/ingest` `publish`): build → validate → mark `published`. The
/// promotion UPDATE mirrors `apps/ingest/src/commands/publish.rs`'s guarded
/// reference advance (`status = 'validated' AND projection_status =
/// 'complete'`), so a test can never publish an incomplete candidate.
///
/// The `taxonomy_version` comes from the same YAML bytes the API loads at
/// boot (repo `data/`), so the snapshot loader's version check accepts the
/// candidate — exactly the production invariant.
pub async fn publish_sample_generation(pool: &PgPool) -> db::generations::build::BuildManifest {
    let data_dir = repo_root().join("data");
    let taxonomy_version = api::generation::taxonomy_version(&data_dir)
        .expect("taxonomy version hash from the repo data seed");

    let built = db::generations::build::build_generation(pool, &taxonomy_version)
        .await
        .expect("generation build over the seeded legacy tables");
    // The taxonomy-alignment gate needs the FULL YAML taxonomy projected
    // (production runs seed-taxonomy first); the api fixtures seed a small
    // catalog, so this helper validates the generation's structural gate
    // only (the same `None`-taxonomy path S6's publish tests use). The
    // snapshot loader enforces the taxonomy_version match itself.
    let report = db::generations::validate::validate_generation(pool, built.generation_id, None)
        .await
        .expect("publication validation runs");
    assert!(
        report.passed(),
        "the fixture generation must validate: {:?}",
        report.failures
    );

    sqlx::query(
        "UPDATE catalog_generations SET status = 'published', published_at = now() \
         WHERE generation_id = $1 AND status = 'validated' AND projection_status = 'complete'",
    )
    .bind(built.generation_id)
    .execute(pool)
    .await
    .expect("promote the validated reference")
    .rows_affected();

    built
}

/// Fresh migrated scratch database plus a statement-counting pool, with
/// the measurement section already held across setup: the test calls
/// `section.reset()` before the measured work and reads `section.count()`
/// after (task 2 instrument, reused from `crates/db`'s test-support).
pub async fn fresh_counting_db_section() -> (PgPool, SqlSection) {
    let counter = SqlCounter::new();
    // The section spans setup too: parallel tests in this binary cannot
    // leak statements into the recorded counts.
    let section = counter.section().await;

    let (pool, name) = fresh_migrated_db().await;
    let base = std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
    let url = format!("{}/{}", base.trim_end_matches("/postgres"), name);
    drop(pool);

    let pool = counter
        .counting_pool(&url)
        .await
        .expect("counting pool over the migrated scratch database");
    (pool, section)
}
/// Seeds the projection rows for the open-mode contract: one category, one
/// event (name/description deliberately disjoint from the fixture query's
/// lexemes so FTS/trigram stay silent), one organization, two procedures
/// (empty cost and populated cost), and relations in declared order.
pub async fn seed_search_fixture(pool: &sqlx::PgPool) {
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehículos', 'car', 1)",
    )
    .execute(pool)
    .await
    .expect("seed category");

    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'comprar-vehiculo', 'Adquisición de rodados', \
                'Pasos para adquirir un rodado en Uruguay.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event");

    sqlx::query(
        "INSERT INTO organizations (external_id, name, short_name, official_url) \
         VALUES ('org-1', 'Ministerio de Transporte', 'MTOP', 'https://www.gub.uy/mtop')",
    )
    .execute(pool)
    .await
    .expect("seed organization");

    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '4551', 'Solicitud de empadronamientos', 'Empadronamiento ante la DNT.', \
                o.id, 'https://www.gub.uy/tramite/4551', 'active', \
                '{\"tiene_costo\": \"\", \"valor\": \"\"}'::jsonb, \
                '2026-09-18T03:00:00Z', '2026-09-18T03:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 4551");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '2368', 'Alta de vehículos ante la DNT', 'Alta inicial del vehículo.', \
                o.id, 'https://www.gub.uy/tramite/2368', 'active', \
                '{\"tiene_costo\": \"1\", \"valor\": \"55.70\"}'::jsonb, \
                '2026-09-18T04:00:00Z', '2026-09-18T04:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 2368");

    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, TRUE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '4551'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 1");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 2, FALSE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '2368'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 2");
}

/// The total projection rows across the five `generation_*` tables
/// (deletion audits for the S8 reconciliation guarantees).
pub async fn projection_row_count(pool: &PgPool) -> i64 {
    let mut total = 0;
    for table in [
        "generation_life_events",
        "generation_fts_text",
        "generation_trigram_surface",
        "generation_event_cards",
        "generation_procedure_details",
    ] {
        // Audited: table names are a fixed allowlist, never input.
        let audited = sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}"));
        let rows: i64 = sqlx::query_scalar(audited)
            .fetch_one(pool)
            .await
            .expect("count rows");
        total += rows;
    }
    total
}

/// [`api::config::ApiLimits`] with background cache warming disabled (S10
/// task 32): the serving default warms the cache after every adoption as a
/// background task, and tests that measure statement counts or cache
/// counters call the warming pass explicitly instead — so the background
/// task can never race a measured window.
pub fn limits_without_warming() -> api::config::ApiLimits {
    api::config::ApiLimits {
        cache_warming: false,
        ..api::config::ApiLimits::default()
    }
}
