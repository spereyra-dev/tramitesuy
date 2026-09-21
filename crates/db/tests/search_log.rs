//! Tasks 82–83 (API-10, DM-1): the search-log repository persists only the
//! specced telemetry, and the `search_logs` schema equals the specced column
//! set EXACTLY — a future field cannot silently widen telemetry, which is
//! also the structural proof that no IP/user-agent/name/contact column exists.

mod c2support;

use c2support::*;
use db::repos::search_log::{self, NewSearchLog};

/// The specced `search_logs` column set (data-model spec, DM-1 table 9).
const SPECCED_COLUMNS: [&str; 7] = [
    "created_at",
    "id",
    "normalized_query",
    "query",
    "selected_event_id",
    "top_event_id",
    "top_score",
];

#[tokio::test(flavor = "multi_thread")]
async fn search_logs_columns_equal_the_specced_set_exactly() {
    let (pool, db_name) = fresh_migrated_db().await;

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'search_logs' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .expect("readable information_schema");

    let columns: Vec<&str> = rows.iter().map(|(name,)| name.as_str()).collect();
    assert_eq!(
        columns, SPECCED_COLUMNS,
        "search_logs must have exactly the specced columns — no IP, user agent, \
         name, or contact column, and no silent widening"
    );

    drop_db(&db_name).await;
}

/// Seeds one event so the slug→id FK resolution has a target.
async fn seed_one_event(pool: &sqlx::PgPool) -> sqlx::types::Uuid {
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehículos', 'car', 1)",
    )
    .execute(pool)
    .await
    .expect("seed category");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'alta-vehiculo', 'Alta de vehiculos', 'Registro inicial.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event");
    let (id,): (sqlx::types::Uuid,) =
        sqlx::query_as("SELECT id FROM life_events WHERE slug = 'alta-vehiculo'")
            .fetch_one(pool)
            .await
            .expect("seeded event readable");
    id
}

#[tokio::test(flavor = "multi_thread")]
async fn search_log_insert_persists_only_the_specced_fields() {
    let (pool, db_name) = fresh_migrated_db().await;
    let event_id = seed_one_event(&pool).await;

    let log_id = search_log::insert(
        &pool,
        &NewSearchLog {
            query: "perdi mi cedula <REDACTED>".to_string(),
            normalized_query: "perdi cedula redacted".to_string(),
            selected_event_slug: Some("alta-vehiculo".to_string()),
            top_event_slug: Some("alta-vehiculo".to_string()),
            top_score: Some(36),
        },
    )
    .await
    .expect("log row inserts");

    let row: (
        String,
        String,
        Option<sqlx::types::Uuid>,
        Option<sqlx::types::Uuid>,
        Option<f64>,
        bool,
    ) = sqlx::query_as(
        "SELECT query, normalized_query, selected_event_id, top_event_id, top_score, \
                created_at IS NOT NULL \
         FROM search_logs WHERE id = $1",
    )
    .bind(log_id)
    .fetch_one(&pool)
    .await
    .expect("log row readable");

    assert_eq!(row.0, "perdi mi cedula <REDACTED>");
    assert_eq!(row.1, "perdi cedula redacted");
    assert_eq!(row.2, Some(event_id), "selected_event_id resolves the slug");
    assert_eq!(row.3, Some(event_id), "top_event_id resolves the slug");
    assert_eq!(row.4, Some(36.0));
    assert!(row.5, "created_at is stamped");

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_event_slugs_persist_as_null_event_ids() {
    let (pool, db_name) = fresh_migrated_db().await;

    let log_id = search_log::insert(
        &pool,
        &NewSearchLog {
            query: "compre un auto usado".to_string(),
            normalized_query: "compre auto usado".to_string(),
            selected_event_slug: Some("comprar-vehiculo".to_string()),
            top_event_slug: Some("comprar-vehiculo".to_string()),
            top_score: Some(36),
        },
    )
    .await
    .expect("log row inserts even when the event is absent from the projection");

    let row: (Option<sqlx::types::Uuid>, Option<sqlx::types::Uuid>) =
        sqlx::query_as("SELECT selected_event_id, top_event_id FROM search_logs WHERE id = $1")
            .bind(log_id)
            .fetch_one(&pool)
            .await
            .expect("log row readable");
    assert_eq!(
        row,
        (None, None),
        "an engine event absent from the DB projection stores NULL ids (nullable FKs)"
    );

    drop_db(&db_name).await;
}

// ---------------------------------------------------------------------------
// Task 6 (OPT-06, OPT-09, operations delta "Cache hit costs exactly one
// statement"): both slug resolutions and the insert are ONE statement —
// a single `INSERT … SELECT` with two scalar subqueries. Absent slugs keep
// the NULL-id behavior across all four combinations (both slugs, selected
// only, top only, neither).
// ---------------------------------------------------------------------------

/// Seeds two events under DIFFERENT categories (TRIANGULATE: a slug that
/// exists but belongs to a different category than the other must still
/// resolve to its own id — category membership never mixes the resolution).
async fn seed_two_events_in_different_categories(
    pool: &sqlx::PgPool,
) -> (sqlx::types::Uuid, sqlx::types::Uuid) {
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehículos', 'car', 1)",
    )
    .execute(pool)
    .await
    .expect("seed category A");
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('documentos', 'Documentos', 'doc', 2)",
    )
    .execute(pool)
    .await
    .expect("seed category B");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'alta-vehiculo', 'Alta de vehiculos', 'Registro inicial.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event A");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'cedula-vencida', 'Cédula vencida', 'Renovación.', id \
         FROM categories WHERE slug = 'documentos'",
    )
    .execute(pool)
    .await
    .expect("seed event B");
    let (selected,): (sqlx::types::Uuid,) =
        sqlx::query_as("SELECT id FROM life_events WHERE slug = 'alta-vehiculo'")
            .fetch_one(pool)
            .await
            .expect("seeded event A readable");
    let (top,): (sqlx::types::Uuid,) =
        sqlx::query_as("SELECT id FROM life_events WHERE slug = 'cedula-vencida'")
            .fetch_one(pool)
            .await
            .expect("seeded event B readable");
    (selected, top)
}

fn log_row(query: &str, selected: Option<&str>, top: Option<&str>) -> NewSearchLog {
    NewSearchLog {
        query: query.to_string(),
        normalized_query: query.to_string(),
        selected_event_slug: selected.map(str::to_string),
        top_event_slug: top.map(str::to_string),
        top_score: Some(36),
    }
}

async fn read_ids(
    pool: &sqlx::PgPool,
    log_id: sqlx::types::Uuid,
) -> (Option<sqlx::types::Uuid>, Option<sqlx::types::Uuid>) {
    sqlx::query_as("SELECT selected_event_id, top_event_id FROM search_logs WHERE id = $1")
        .bind(log_id)
        .fetch_one(pool)
        .await
        .expect("log row readable")
}

#[tokio::test(flavor = "multi_thread")]
async fn insert_resolves_ids_in_one_statement() {
    let (pool, _name, _counter, section) = fresh_migrated_counting_db().await;
    seed_two_events_in_different_categories(&pool).await;

    // The measured log path — one insert resolving both slugs inline.
    let log = log_row(
        "compre un auto usado",
        Some("alta-vehiculo"),
        Some("cedula-vencida"),
    );
    section.reset();
    let log_id = search_log::insert(&pool, &log)
        .await
        .expect("log row inserts");
    let count = section.count();

    assert_eq!(
        count, 1,
        "the log path costs exactly one statement: both slug resolutions are \
         scalar subqueries inside the INSERT (observed {count})"
    );

    let ids = read_ids(&pool, log_id).await;
    assert_eq!(
        ids,
        read_event_ids(&pool).await,
        "both slugs resolve to their own ids inside the single statement"
    );
}

async fn read_event_ids(
    pool: &sqlx::PgPool,
) -> (Option<sqlx::types::Uuid>, Option<sqlx::types::Uuid>) {
    let (selected,): (sqlx::types::Uuid,) =
        sqlx::query_as("SELECT id FROM life_events WHERE slug = 'alta-vehiculo'")
            .fetch_one(pool)
            .await
            .expect("event A readable");
    let (top,): (sqlx::types::Uuid,) =
        sqlx::query_as("SELECT id FROM life_events WHERE slug = 'cedula-vencida'")
            .fetch_one(pool)
            .await
            .expect("event B readable");
    (Some(selected), Some(top))
}

/// All four slug combinations preserve the NULL behavior of the previous
/// per-slug lookups: a present slug resolves, an absent slug stays NULL,
/// and the combinations never mix or leak ids across columns.
#[tokio::test(flavor = "multi_thread")]
async fn insert_resolves_ids_across_all_slug_combinations() {
    let (pool, _name, _counter, _section) = fresh_migrated_counting_db().await;
    let (event_id, other_id) = seed_two_events_in_different_categories(&pool).await;

    // 1. Both slugs present → both ids resolved (negative control: distinct
    //    slugs resolve to distinct ids — no cross-column contamination).
    let both = search_log::insert(
        &pool,
        &log_row("q-both", Some("alta-vehiculo"), Some("cedula-vencida")),
    )
    .await
    .expect("both-slug insert");
    assert_eq!(
        read_ids(&pool, both).await,
        (Some(event_id), Some(other_id)),
        "selected and top resolve independently (no category mixing)"
    );

    // 2. Selected only → top_event_id NULL.
    let selected_only = search_log::insert(
        &pool,
        &log_row("q-selected", Some("alta-vehiculo"), Some("missing-top")),
    )
    .await
    .expect("selected-only insert");
    assert_eq!(
        read_ids(&pool, selected_only).await,
        (Some(event_id), None),
        "an absent top slug persists NULL while the selected id resolves"
    );

    // 3. Top only → selected_event_id NULL.
    let top_only = search_log::insert(
        &pool,
        &log_row("q-top", Some("missing-selected"), Some("cedula-vencida")),
    )
    .await
    .expect("top-only insert");
    assert_eq!(
        read_ids(&pool, top_only).await,
        (None, Some(other_id)),
        "an absent selected slug persists NULL while the top id resolves"
    );

    // 4. Neither → both NULL (today's behavior preserved).
    let neither = search_log::insert(
        &pool,
        &log_row("q-neither", Some("missing-a"), Some("missing-b")),
    )
    .await
    .expect("neither-slug insert");
    assert_eq!(
        read_ids(&pool, neither).await,
        (None, None),
        "absent slugs on both columns persist NULL ids (nullable FKs)"
    );
}
