//! Task 58 (DM-2, IN-6/IN-7/IN-9, D-5): the sqlx `ProcedureRepository`
//! implementation in `crates/db` must satisfy the exact port contract proven
//! by the canonical `InMemoryProcedureRepository` in unit B3 — same fixture
//! semantics, single-transaction batches, and compile-time-checked queries,
//! exercised against the compose Postgres via scratch databases.

mod common;

use common::*;
use db::repos::procedures::PostgresProcedureRepository;
use ingestion::ports::ProcedureRepository;
use ingestion::summary::ProcedureUpsert;

const RUN_1: &str = "2026-09-18T03:00:00Z";
const RUN_2: &str = "2026-09-18T04:00:00Z";

fn row(id: &str, valor: &str, org: &str, org_name: &str, hash: &str) -> ProcedureUpsert {
    ProcedureUpsert {
        external_id: id.to_string(),
        name: format!("Trámite {id}"),
        description: format!("Descripción del trámite {id}"),
        organization_external_id: org.to_string(),
        organization_name: org_name.to_string(),
        official_url: format!("https://www.gub.uy/tramite/{id}"),
        content_hash: hash.to_string(),
        raw_data: serde_json::json!({ "id": id, "valor": valor }),
    }
}

async fn counts(pool: &sqlx::PgPool) -> (i64, i64, i64, i64) {
    let procedures: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE status = 'active') FROM procedures",
    )
    .fetch_one(pool)
    .await
    .expect("procedures counted");
    let versions: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE valid_until IS NULL) FROM procedure_versions",
    )
    .fetch_one(pool)
    .await
    .expect("versions counted");
    let orgs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM organizations")
        .fetch_one(pool)
        .await
        .expect("organizations counted");
    (procedures.0, procedures.1, versions.0, orgs)
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_batch_creates_procedures_versions_and_organizations() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());

    let counts_out = repo
        .upsert_procedures(
            &[
                row("1001", "100", "O-1", "Ministerio", "hash-1001"),
                row("1002", "200", "O-2", "Intendencia", "hash-1002"),
            ],
            RUN_1.to_string(),
        )
        .expect("batch upserts");
    assert_eq!(counts_out.inserted, 2, "both rows are new");
    assert_eq!(counts_out.updated, 0);

    // Two active procedures, one open version each, one org per source oid.
    assert_eq!(counts(&pool).await, (2, 2, 2, 2));

    // first_seen/last_seen stamped at the run; soft-delete fields clear.
    let record: (String, String, Option<String>) = sqlx::query_as(
        "SELECT to_char(first_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(last_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(deactivated_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1001'",
    )
    .fetch_one(&pool)
    .await
    .expect("procedure 1001 exists");
    assert_eq!(record.0, "2026-09-18T03:00:00Z");
    assert_eq!(record.1, "2026-09-18T03:00:00Z");
    assert_eq!(record.2, None, "no deactivation stamp on a fresh row");

    // raw_data preserves the source payload (IN-8, D-4).
    let raw: serde_json::Value =
        sqlx::query_scalar("SELECT raw_data FROM procedures WHERE external_id = '1001'")
            .fetch_one(&pool)
            .await
            .expect("raw_data present");
    assert_eq!(raw, serde_json::json!({ "id": "1001", "valor": "100" }));

    // Organization mapped by source oid with the source name (IN-8).
    let org: (String, String) =
        sqlx::query_as("SELECT external_id, name FROM organizations WHERE external_id = 'O-1'")
            .fetch_one(&pool)
            .await
            .expect("organization row exists");
    assert_eq!(org, ("O-1".to_string(), "Ministerio".to_string()));

    // latest_hashes exposes the open hash per external id (IN-6 diff input).
    let hashes = repo.latest_hashes().expect("latest hashes");
    assert_eq!(hashes.len(), 2);
    assert_eq!(hashes.get("1001").map(String::as_str), Some("hash-1001"));

    // all_external_ids feeds export-ids: every id, sorted.
    let ids = repo.all_external_ids().expect("external ids");
    assert_eq!(ids, vec!["1001".to_string(), "1002".to_string()]);

    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unchanged_upsert_updates_row_but_creates_no_second_version() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_1.to_string(),
    )
    .expect("first upsert");

    // Same content re-upserted (as the pipeline never does for unchanged rows,
    // but the repository contract must still match the in-memory reference):
    // the row updates, no version opens (IN-9 at the repository layer).
    let counts_out = repo
        .upsert_procedures(
            &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
            RUN_2.to_string(),
        )
        .expect("second upsert");
    assert_eq!(counts_out.updated, 1);
    assert_eq!(counts(&pool).await, (1, 1, 1, 1), "no second version row");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_content_creates_one_version_and_closes_the_prior_at_the_run_stamp() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-v1")],
        RUN_1.to_string(),
    )
    .expect("v1 upsert");

    repo.upsert_procedures(
        &[row("1001", "150", "O-1", "Ministerio", "hash-v2")],
        RUN_2.to_string(),
    )
    .expect("v2 upsert");

    // Repository-layer semantics (in-memory reference parity): the upsert
    // opens exactly one new version; closing the predecessor is the separate
    // `close_versions` step the pipeline issues right after the batch.
    let (total, _open): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE valid_until IS NULL) FROM procedure_versions",
    )
    .fetch_one(&pool)
    .await
    .expect("versions counted");
    assert_eq!(total, 2, "exactly one new version row");

    repo.close_versions(
        &[("1001".to_string(), "hash-v1".to_string())],
        RUN_2.to_string(),
    )
    .expect("close call");
    let open: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM procedure_versions WHERE valid_until IS NULL")
            .fetch_one(&pool)
            .await
            .expect("open count");
    assert_eq!(open, 1, "only the new version stays open after closing");

    let prior: (String, Option<String>) = sqlx::query_as(
        "SELECT to_char(valid_from, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(valid_until, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedure_versions WHERE content_hash = 'hash-v1'",
    )
    .fetch_one(&pool)
    .await
    .expect("prior version exists");
    assert_eq!(prior.0, "2026-09-18T03:00:00Z");
    assert_eq!(
        prior.1,
        Some("2026-09-18T04:00:00Z".to_string()),
        "prior version closed at run 2 (DM-3)"
    );
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn close_versions_closes_only_the_matching_open_version() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-v1")],
        RUN_1.to_string(),
    )
    .expect("v1 upsert");

    repo.close_versions(
        &[("1001".to_string(), "hash-v1".to_string())],
        RUN_2.to_string(),
    )
    .expect("close call");
    let open: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM procedure_versions WHERE valid_until IS NULL")
            .fetch_one(&pool)
            .await
            .expect("open count");
    assert_eq!(open, 0, "the open version is closed at the run stamp");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deactivate_missing_marks_absent_inactive_and_never_deletes() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[
            row("1001", "100", "O-1", "Ministerio", "hash-1001"),
            row("1002", "200", "O-2", "Intendencia", "hash-1002"),
        ],
        RUN_1.to_string(),
    )
    .expect("initial batch");

    let deactivated = repo
        .deactivate_missing(
            &["1001".to_string()].into_iter().collect(),
            RUN_2.to_string(),
        )
        .expect("deactivate call");
    assert_eq!(deactivated, 1, "only 1002 is absent from the source");

    let absent: (String, Option<String>) = sqlx::query_as(
        "SELECT status, to_char(deactivated_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1002'",
    )
    .fetch_one(&pool)
    .await
    .expect("row still exists (never deleted)");
    assert_eq!(absent.0, "inactive");
    assert_eq!(absent.1, Some("2026-09-18T04:00:00Z".to_string()));

    // Idempotent: an already-inactive row is not deactivated twice.
    let again = repo
        .deactivate_missing(
            &["1001".to_string()].into_iter().collect(),
            RUN_2.to_string(),
        )
        .expect("second deactivate call");
    assert_eq!(again, 0, "soft delete flips each row once");
    assert_eq!(counts(&pool).await.0, 2, "no row was ever deleted");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_reactivates_an_inactive_row_without_opening_a_version() {
    // B3 recorded edge: a row present in the source is re-activated in place
    // (status back to active, deactivation stamp cleared); unchanged content
    // opens no new version.
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_1.to_string(),
    )
    .expect("initial upsert");
    repo.deactivate_missing(&Default::default(), RUN_2.to_string())
        .expect("deactivate all");

    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_2.to_string(),
    )
    .expect("re-upsert");

    let state: (String, Option<String>) = sqlx::query_as(
        "SELECT status, to_char(deactivated_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1001'",
    )
    .fetch_one(&pool)
    .await
    .expect("row exists");
    assert_eq!(state.0, "active", "present row re-activated");
    assert_eq!(state.1, None, "stale deactivation stamp cleared");
    assert_eq!(
        counts(&pool).await.2,
        1,
        "unchanged content opens no version"
    );
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reactivate_present_reactivates_inactive_rows_without_a_version() {
    // F12: reactivation is driven by presence, not by content change. The
    // repository method flips the status and clears the stamp without opening
    // a version, and is idempotent on an already-active row.
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_1.to_string(),
    )
    .expect("initial upsert");
    repo.deactivate_missing(&Default::default(), RUN_2.to_string())
        .expect("deactivate all");
    let before_versions = counts(&pool).await.2;

    let reactivated = repo
        .reactivate_present(&["1001".to_string()], RUN_2.to_string())
        .expect("reactivate call");
    assert_eq!(reactivated, 1, "the inactive row is reactivated once");

    let state: (String, Option<String>) = sqlx::query_as(
        "SELECT status, to_char(deactivated_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1001'",
    )
    .fetch_one(&pool)
    .await
    .expect("row exists");
    assert_eq!(state.0, "active", "present row is active again");
    assert_eq!(state.1, None, "stale deactivation stamp cleared");
    assert_eq!(
        counts(&pool).await.2,
        before_versions,
        "reactivation opens no version"
    );

    // Idempotent: an already-active row is not counted again.
    let again = repo
        .reactivate_present(&["1001".to_string()], RUN_2.to_string())
        .expect("second reactivate call");
    assert_eq!(again, 0, "only inactive rows are reactivated");
    assert_eq!(counts(&pool).await.1, 1, "the row stays active");
    drop_test_db(&name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn touch_last_seen_advances_only_last_seen() {
    let (pool, name) = fresh_migrated_db().await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.upsert_procedures(
        &[row("1001", "100", "O-1", "Ministerio", "hash-1001")],
        RUN_1.to_string(),
    )
    .expect("initial upsert");

    repo.touch_last_seen(&["1001".to_string()], RUN_2.to_string())
        .expect("touch call");

    let seen: (String, String) = sqlx::query_as(
        "SELECT to_char(first_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ'), \
         to_char(last_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SSZ') \
         FROM procedures WHERE external_id = '1001'",
    )
    .fetch_one(&pool)
    .await
    .expect("row exists");
    assert_eq!(seen.0, "2026-09-18T03:00:00Z", "first_seen_at preserved");
    assert_eq!(seen.1, "2026-09-18T04:00:00Z", "last_seen_at advanced");

    // The stamp conversion boundary: a malformed RunStamp is a typed error.
    let err = repo
        .touch_last_seen(&["1001".to_string()], "not-a-stamp".to_string())
        .expect_err("malformed run stamp rejected");
    assert!(
        err.to_string().contains("run stamp"),
        "error names the malformed run stamp: {err}"
    );
    drop_test_db(&name).await;
}

// ---------------------------------------------------------------------------
// Task 7 (OPT-06 transition row: intermediate `open` ≤4 SQL ops):
// `cards_by_event` — the search payload's card fields in exactly ONE
// statement, no unused event metadata, no `raw_data` transport. `by_event`
// stays intact as the rollback path.
// ---------------------------------------------------------------------------

use common::fresh_migrated_counting_db;
use db::repos::procedures;

/// Seeds two events (scoping control) with three relations on the target:
/// mixed required/optional, importance set/unset, organizations with and
/// without a short name, one reported cost, one empty-cost pair, and one
/// NULL `raw_data` (the absent-source case).
async fn seed_cards_fixture(pool: &sqlx::PgPool) {
    sqlx::query(
        "INSERT INTO categories (slug, name, order_index) \
         VALUES ('vehiculos', 'Vehículos', 1)",
    )
    .execute(pool)
    .await
    .expect("seed category");
    sqlx::query(
        "INSERT INTO organizations (external_id, name, short_name) \
         VALUES ('O-1', 'Ministerio de Transporte', 'MTOP')",
    )
    .execute(pool)
    .await
    .expect("seed org with short name");
    sqlx::query("INSERT INTO organizations (external_id, name) VALUES ('O-2', 'Intendencia')")
        .execute(pool)
        .await
        .expect("seed org without short name");

    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'comprar-vehiculo', 'Comprar un vehículo', 'Trámites para comprar.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed target event");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'vender-vehiculo', 'Vender un vehículo', 'Trámites para vender.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed other event");

    sqlx::query(
        "INSERT INTO procedures (external_id, name, organization_id, official_url, raw_data) \
         SELECT '2001', 'Trámite 2001', o.id, 'https://www.gub.uy/tramite/2001', \
                '{\"tiene_costo\": \"1\", \"valor\": \"55.70\"}'::jsonb \
         FROM organizations o WHERE o.external_id = 'O-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure with reported cost");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, organization_id, official_url, raw_data) \
         SELECT '2002', 'Trámite 2002', o.id, 'https://www.gub.uy/tramite/2002', \
                '{\"tiene_costo\": \"\", \"valor\": \"\"}'::jsonb \
         FROM organizations o WHERE o.external_id = 'O-2'",
    )
    .execute(pool)
    .await
    .expect("seed procedure with empty cost pair");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, organization_id, official_url) \
         SELECT '2003', 'Trámite 2003', o.id, 'https://www.gub.uy/tramite/2003' \
         FROM organizations o WHERE o.external_id = 'O-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure with NULL raw_data");

    // Declared order 3/1/2 (insertion order ≠ card order): required,
    // optional+importance, optional+NULL importance.
    sqlx::query(
        "INSERT INTO life_event_procedures \
             (life_event_id, procedure_id, order_index, importance, required) \
         SELECT e.id, p.id, 2, NULL, FALSE \
         FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '2002'",
    )
    .execute(pool)
    .await
    .expect("seed relation for 2002");
    sqlx::query(
        "INSERT INTO life_event_procedures \
             (life_event_id, procedure_id, order_index, importance, required) \
         SELECT e.id, p.id, 1, 'alta', TRUE \
         FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '2001'",
    )
    .execute(pool)
    .await
    .expect("seed relation for 2001");
    sqlx::query(
        "INSERT INTO life_event_procedures \
             (life_event_id, procedure_id, order_index, importance, required) \
         SELECT e.id, p.id, 3, 'baja', FALSE \
         FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '2003'",
    )
    .execute(pool)
    .await
    .expect("seed relation for 2003");

    // A relation on the OTHER event proves slug scoping (never leaks).
    sqlx::query(
        "INSERT INTO life_event_procedures \
             (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, TRUE \
         FROM life_events e, procedures p \
         WHERE e.slug = 'vender-vehiculo' AND p.external_id = '2003'",
    )
    .execute(pool)
    .await
    .expect("seed relation on the other event");
}

/// The card set equals today's `by_event` composition (slug, name, order,
/// required, official URL, last-seen attribution stamp) minus the metadata
/// the payload never uses, with the reported-cost rule intact and ordering
/// by `order_index` regardless of insertion order.
#[tokio::test(flavor = "multi_thread")]
async fn cards_by_event_returns_the_same_card_set_and_ordering_as_by_event() {
    let (pool, name) = fresh_migrated_db().await;
    seed_cards_fixture(&pool).await;

    let projection = procedures::by_event(&pool, "comprar-vehiculo")
        .await
        .expect("by_event reads")
        .expect("target event exists");
    let cards = procedures::cards_by_event(&pool, "comprar-vehiculo")
        .await
        .expect("cards_by_event reads")
        .expect("target event exists");

    assert_eq!(cards.len(), projection.procedures.len(), "same card set");
    for (card, procedure) in cards.iter().zip(&projection.procedures) {
        assert_eq!(card.slug, procedure.external_id);
        assert_eq!(card.name, procedure.name);
        assert_eq!(card.order_index, procedure.order_index);
        assert_eq!(card.required, procedure.required);
        assert_eq!(card.official_url, procedure.official_url);
        assert_eq!(
            card.last_seen_at, procedure.last_seen_at,
            "same attribution timestamp"
        );
    }

    // Ordered by order_index (1, 2, 3), not by insertion order.
    let order: Vec<i32> = cards.iter().map(|c| c.order_index).collect();
    assert_eq!(order, vec![1, 2, 3], "declared step order");

    // Card-specific projections: importance, organization short name, status
    // and the missing-cost rule evaluated once here instead of transporting
    // `raw_data` per request.
    assert_eq!(cards[0].slug, "2001");
    assert_eq!(cards[0].importance.as_deref(), Some("alta"));
    assert_eq!(cards[0].organization_short_name.as_deref(), Some("MTOP"));
    assert_eq!(cards[0].status, "active");
    assert_eq!(cards[0].cost.as_deref(), Some("55.70"), "reported cost");
    assert_eq!(cards[1].slug, "2002");
    assert_eq!(cards[1].importance, None, "unset importance stays NULL");
    assert_eq!(
        cards[1].organization_short_name, None,
        "org without a short name stays NULL"
    );
    assert_eq!(cards[1].cost, None, "empty cost pair → missing cost");
    assert_eq!(cards[2].slug, "2003");
    assert_eq!(cards[2].cost, None, "NULL raw_data → missing cost");

    // Scoping control: the other event's relation never leaks in, and an
    // unknown slug returns nothing.
    let other = procedures::cards_by_event(&pool, "vender-vehiculo")
        .await
        .expect("other event reads")
        .expect("other event exists");
    assert_eq!(other.len(), 1, "the other event only carries its own card");
    assert_eq!(other[0].slug, "2003");
    assert!(
        procedures::cards_by_event(&pool, "missing-event")
            .await
            .expect("unknown slug reads")
            .is_none(),
        "an unknown slug returns None like by_event"
    );

    drop_test_db(&name).await;
}

/// Exactly one statement: no event-metadata query, no per-card second trip.
#[tokio::test(flavor = "multi_thread")]
async fn cards_by_event_issues_exactly_one_statement() {
    let (pool, _name, _counter, section) = fresh_migrated_counting_db().await;
    seed_cards_fixture(&pool).await;

    section.reset();
    let cards = procedures::cards_by_event(&pool, "comprar-vehiculo")
        .await
        .expect("cards_by_event reads");
    let count = section.count();

    assert!(cards.is_some());
    assert_eq!(
        count, 1,
        "the transition cards query costs exactly one statement (observed {count})"
    );
}

/// An event with no relations returns no rows (its search payload serves an
/// empty procedures summary either way), and the statement still runs once.
#[tokio::test(flavor = "multi_thread")]
async fn cards_by_event_returns_no_rows_for_an_empty_event() {
    let (pool, name) = fresh_migrated_db().await;
    sqlx::query(
        "INSERT INTO categories (slug, name, order_index) \
         VALUES ('vehiculos', 'Vehículos', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed category");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'evento-vacio', 'Evento vacío', 'Sin relaciones.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(&pool)
    .await
    .expect("seed card-less event");

    let cards = procedures::cards_by_event(&pool, "evento-vacio")
        .await
        .expect("empty event reads");
    assert!(
        cards.is_none_or(|cards| cards.is_empty()),
        "an empty event yields no card rows"
    );

    drop_test_db(&name).await;
}

/// TRIANGULATE: a deactivated procedure keeps its relation and its card, with
/// the current contract's `status` ("inactive") — rows are never deleted.
#[tokio::test(flavor = "multi_thread")]
async fn cards_by_event_keeps_an_inactive_procedure_card_with_its_status() {
    let (pool, name) = fresh_migrated_db().await;
    seed_cards_fixture(&pool).await;
    let repo = PostgresProcedureRepository::new(pool.clone());
    repo.deactivate_missing(
        &["2001".to_string(), "2002".to_string()]
            .into_iter()
            .collect(),
        RUN_2.to_string(),
    )
    .expect("deactivate 2003 (absent from the source)");

    let cards = procedures::cards_by_event(&pool, "comprar-vehiculo")
        .await
        .expect("cards_by_event reads")
        .expect("target event exists");

    assert_eq!(cards.len(), 3, "the inactive card is never dropped");
    let inactive = cards
        .iter()
        .find(|card| card.slug == "2003")
        .expect("2003 keeps its card");
    assert_eq!(inactive.status, "inactive", "current status contract");
    assert_eq!(inactive.name, "Trámite 2003", "attribution fields intact");
    assert_eq!(inactive.importance.as_deref(), Some("baja"));
    let still_active = cards
        .iter()
        .find(|card| card.slug == "2001")
        .expect("2001 untouched");
    assert_eq!(still_active.status, "active", "only the absent row flips");

    drop_test_db(&name).await;
}
