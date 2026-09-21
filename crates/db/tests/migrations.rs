//! Migration contracts: the embedded migrations preserve the ten base tables,
//! add catalog-generation projections, and create no extensions (portability,
//! design §6 / D-6).

mod common;

use sqlx::Row;

#[tokio::test]
async fn migrations_preserve_base_tables_and_add_catalog_generation_manifest() {
    let (pool, name) = common::fresh_migrated_db().await;

    // Explicit allowlist: the ten DM-1 tables plus the additive catalog
    // generation manifest (and sqlx's internal bookkeeping table).
    let mut expected: Vec<String> = vec![
        "categories",
        "organizations",
        "life_events",
        "life_event_keywords",
        "procedures",
        "procedure_versions",
        "life_event_procedures",
        "synonyms",
        "search_logs",
        "search_feedback",
        "catalog_generations",
        "ingestion_runs",
        "generation_life_events",
        "generation_fts_text",
        "generation_trigram_surface",
        "generation_event_cards",
        "generation_procedure_details",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    expected.push("_sqlx_migrations".to_string());
    expected.sort();

    let mut actual: Vec<String> = sqlx::query(
        "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
    )
    .fetch_all(&pool)
    .await
    .expect("list tables")
    .into_iter()
    .map(|row| row.get::<String, _>(0))
    .collect();
    actual.sort();

    assert_eq!(
        actual, expected,
        "schema after migrations must preserve the ten base tables and add only the catalog-generation manifest"
    );

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn catalog_generation_manifest_has_required_columns_and_status_constraint() {
    let (pool, name) = common::fresh_migrated_db().await;

    let mut columns: Vec<String> = sqlx::query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'catalog_generations' \
         ORDER BY ordinal_position",
    )
    .fetch_all(&pool)
    .await
    .expect("list catalog generation columns")
    .into_iter()
    .map(|row| row.get::<String, _>(0))
    .collect();
    columns.sort();

    let mut expected = vec![
        "generation_id",
        "status",
        "content_hash",
        "taxonomy_version",
        "engine_version",
        "source_synced_at",
        "created_at",
        "published_at",
        "retired_at",
        "event_count",
        "procedure_count",
        "projection_status",
        "active_generation_id",
        "adopted_at",
        "inflight_generation_ids",
    ];
    expected.sort();
    assert_eq!(
        columns, expected,
        "manifest columns must match the contract"
    );

    let generation_id_type: String = sqlx::query_scalar(
        "SELECT data_type FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'catalog_generations' \
         AND column_name = 'generation_id'",
    )
    .fetch_one(&pool)
    .await
    .expect("read generation ID type");
    assert_eq!(generation_id_type, "uuid", "generation ID must be a UUID");

    let invalid_status = sqlx::query(
        "INSERT INTO catalog_generations \
         (generation_id, status, content_hash, taxonomy_version, engine_version, source_synced_at, event_count, procedure_count, projection_status) \
         VALUES (gen_random_uuid(), 'invalid', 'hash', 'taxonomy', 'engine', now(), 0, 0, 'pending')",
    )
    .execute(&pool)
    .await
    .expect_err("manifest status constraint must reject values outside the lifecycle");
    assert!(
        matches!(&invalid_status, sqlx::Error::Database(db) if db.code().as_deref() == Some("23514")),
        "unexpected error for invalid manifest status: {invalid_status:?}"
    );

    common::apply_migrations(&pool).await;
    let applied: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE version = 13")
            .fetch_one(&pool)
            .await
            .expect("count manifest migration records");
    assert_eq!(applied, 1, "re-running migrations must not reapply 0013");

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn ingestion_runs_enforce_contract_and_preserve_committed_records_on_rollback() {
    let (pool, name) = common::fresh_migrated_db().await;

    let mut columns: Vec<String> = sqlx::query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'ingestion_runs' \
         ORDER BY ordinal_position",
    )
    .fetch_all(&pool)
    .await
    .expect("list ingestion run columns")
    .into_iter()
    .map(|row| row.get::<String, _>(0))
    .collect();
    columns.sort();

    let mut expected = vec![
        "run_id",
        "trigger",
        "started_at",
        "finished_at",
        "status",
        "counts",
        "candidate_generation_id",
        "published_generation_id",
        "attempt",
    ];
    expected.sort();
    assert_eq!(
        columns, expected,
        "run-record columns must match the contract"
    );

    let generation_reference_columns: Vec<(String, String, String)> = sqlx::query(
        "SELECT column_name, data_type, is_nullable FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'ingestion_runs' \
         AND column_name IN ('counts', 'candidate_generation_id', 'published_generation_id') \
         ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .expect("read ingestion run contract column metadata")
    .into_iter()
    .map(|row| {
        (
            row.get("column_name"),
            row.get("data_type"),
            row.get("is_nullable"),
        )
    })
    .collect();
    assert_eq!(
        generation_reference_columns,
        vec![
            (
                "candidate_generation_id".to_string(),
                "uuid".to_string(),
                "YES".to_string()
            ),
            ("counts".to_string(), "jsonb".to_string(), "NO".to_string()),
            (
                "published_generation_id".to_string(),
                "uuid".to_string(),
                "YES".to_string()
            ),
        ],
        "counts must be JSONB and generation references must remain nullable"
    );

    let candidate_generation_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO catalog_generations \
         (generation_id, content_hash, taxonomy_version, engine_version, source_synced_at, event_count, procedure_count, projection_status) \
         VALUES (gen_random_uuid(), 'candidate-hash', 'taxonomy', 'engine', now(), 0, 0, 'pending') \
         RETURNING generation_id",
    )
    .fetch_one(&pool)
    .await
    .expect("create candidate generation");
    let published_generation_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO catalog_generations \
         (generation_id, status, content_hash, taxonomy_version, engine_version, source_synced_at, event_count, procedure_count, projection_status) \
         VALUES (gen_random_uuid(), 'published', 'published-hash', 'taxonomy', 'engine', now(), 0, 0, 'complete') \
         RETURNING generation_id",
    )
    .fetch_one(&pool)
    .await
    .expect("create published generation");

    let run_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO ingestion_runs \
         (run_id, trigger, finished_at, status, counts, candidate_generation_id, published_generation_id, attempt) \
         VALUES (gen_random_uuid(), 'manual', now(), 'skipped', '{\"read\": 0}'::jsonb, $1, $2, 1) \
         RETURNING run_id",
    )
    .bind(candidate_generation_id)
    .bind(published_generation_id)
    .fetch_one(&pool)
    .await
    .expect("a skipped run record is accepted");

    let invalid_trigger = sqlx::query(
        "INSERT INTO ingestion_runs (run_id, trigger, status, counts, attempt) \
         VALUES (gen_random_uuid(), 'invalid', 'skipped', '{}'::jsonb, 1)",
    )
    .execute(&pool)
    .await
    .expect_err("trigger constraint must reject values outside the contract");
    assert!(
        matches!(&invalid_trigger, sqlx::Error::Database(db) if db.code().as_deref() == Some("23514")),
        "unexpected error for invalid trigger: {invalid_trigger:?}"
    );

    let invalid_attempt = sqlx::query(
        "INSERT INTO ingestion_runs (run_id, trigger, status, counts, attempt) \
         VALUES (gen_random_uuid(), 'manual', 'skipped', '{}'::jsonb, 4)",
    )
    .execute(&pool)
    .await
    .expect_err("attempt constraint must reject values outside 1..3");
    assert!(
        matches!(&invalid_attempt, sqlx::Error::Database(db) if db.code().as_deref() == Some("23514")),
        "unexpected error for invalid attempt: {invalid_attempt:?}"
    );

    let missing_generation = sqlx::query(
        "INSERT INTO ingestion_runs \
         (run_id, trigger, status, counts, candidate_generation_id, attempt) \
         VALUES (gen_random_uuid(), 'manual', 'skipped', '{}'::jsonb, gen_random_uuid(), 1)",
    )
    .execute(&pool)
    .await
    .expect_err("generation references must be foreign keys");
    assert!(
        common::is_fk_violation(&missing_generation),
        "unexpected error for missing generation: {missing_generation:?}"
    );

    let missing_published_generation = sqlx::query(
        "INSERT INTO ingestion_runs \
         (run_id, trigger, status, counts, published_generation_id, attempt) \
         VALUES (gen_random_uuid(), 'manual', 'skipped', '{}'::jsonb, gen_random_uuid(), 1)",
    )
    .execute(&pool)
    .await
    .expect_err("published generation reference must be a foreign key");
    assert!(
        common::is_fk_violation(&missing_published_generation),
        "unexpected error for missing published generation: {missing_published_generation:?}"
    );

    let mut transaction = pool.begin().await.expect("begin run update transaction");
    sqlx::query("UPDATE ingestion_runs SET status = 'failed', counts = '{\"read\": 1}'::jsonb WHERE run_id = $1")
        .bind(run_id)
        .execute(&mut *transaction)
        .await
        .expect("stage partial run update");
    transaction
        .rollback()
        .await
        .expect("roll back partial run update");

    let (status, counts): (String, serde_json::Value) =
        sqlx::query_as("SELECT status, counts FROM ingestion_runs WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .expect("committed run record survives rollback");
    assert_eq!(status, "skipped");
    assert_eq!(counts, serde_json::json!({"read": 0}));

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn generation_projections_have_generation_scoped_unique_slugs_and_a_surface_trigram_index() {
    let (pool, name) = common::fresh_migrated_db().await;

    let projection_tables = [
        "generation_life_events",
        "generation_fts_text",
        "generation_trigram_surface",
        "generation_event_cards",
        "generation_procedure_details",
    ];
    for table in projection_tables {
        let generation_id_type: String = sqlx::query_scalar(
            "SELECT data_type FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = $1 AND column_name = 'generation_id'",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .expect("projection must have a generation ID");
        assert_eq!(
            generation_id_type, "uuid",
            "{table} generation ID must be UUID"
        );

        let unique_slug_key: bool = sqlx::query_scalar(
            "SELECT EXISTS ( \
                 SELECT 1 FROM pg_index index_definition \
                 WHERE index_definition.indrelid = $1::regclass \
                   AND index_definition.indisunique \
                   AND pg_get_indexdef(index_definition.indexrelid) \
                       LIKE '%(generation_id, slug)%' \
             )",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .expect("read projection unique keys");
        assert!(
            unique_slug_key,
            "{table} must have a unique key on (generation_id, slug)"
        );
    }

    let trigram_index_definition: String = sqlx::query_scalar(
        "SELECT pg_get_indexdef(index_definition.indexrelid) \
         FROM pg_index index_definition \
         WHERE index_definition.indrelid = 'generation_trigram_surface'::regclass \
           AND index_definition.indisvalid \
           AND pg_get_indexdef(index_definition.indexrelid) \
               LIKE 'CREATE INDEX % USING gin (surface_text gin_trgm_ops)'",
    )
    .fetch_one(&pool)
    .await
    .expect("generation trigram index must target surface_text");
    assert!(
        trigram_index_definition.contains("surface_text gin_trgm_ops"),
        "trigram index must target generation_trigram_surface.surface_text"
    );
    assert!(
        !trigram_index_definition.contains("life_events"),
        "projection trigram index must not target life_events.name"
    );

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn migration_renames_casarse_without_changing_event_id_or_relations() {
    let (pool, name) = common::fresh_migrated_db().await;

    let category_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO categories (slug, name, order_index) \
         VALUES ('familia', 'Familia', 1) \
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed family category");
    let event_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO life_events (slug, name, category_id, updated_at) \
         VALUES ('casarse', 'Casarse', $1, TIMESTAMPTZ '2000-01-01 00:00:00+00') \
         RETURNING id",
    )
    .bind(category_id)
    .fetch_one(&pool)
    .await
    .expect("seed pre-0016 marriage event");
    let procedure_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO procedures (external_id, name, status) \
         VALUES ('4594', 'Inscripción de matrimonio', 'active') \
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed official marriage registration procedure");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         VALUES ($1, $2, 1, TRUE)",
    )
    .bind(event_id)
    .bind(procedure_id)
    .execute(&pool)
    .await
    .expect("seed event-procedure relation");

    // Fresh setup includes every embedded migration, so make 0016 pending
    // after introducing the pre-0016 row it must transform.
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 16")
        .execute(&pool)
        .await
        .expect("make the rename migration pending");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("apply pending 0016 rename migration");

    let (renamed_id, updated_at_changed): (sqlx::types::Uuid, bool) = sqlx::query_as(
        "SELECT id, updated_at > TIMESTAMPTZ '2000-01-01 00:00:00+00' \
         FROM life_events WHERE slug = 'inscribir-matrimonio'",
    )
    .fetch_one(&pool)
    .await
    .expect("0016 renames the existing marriage event");
    assert_eq!(
        renamed_id, event_id,
        "the rename must preserve the event UUID"
    );
    assert!(updated_at_changed, "the rename must refresh updated_at");

    let relation_event_id: sqlx::types::Uuid = sqlx::query_scalar(
        "SELECT life_event_id FROM life_event_procedures WHERE procedure_id = $1",
    )
    .bind(procedure_id)
    .fetch_one(&pool)
    .await
    .expect("event-procedure relation remains after the rename");
    assert_eq!(
        relation_event_id, event_id,
        "the rename must preserve foreign-key relations"
    );

    let old_slug_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM life_events WHERE slug = 'casarse'")
            .fetch_one(&pool)
            .await
            .expect("count old slug rows");
    assert_eq!(old_slug_count, 0, "the old slug must no longer be present");

    common::drop_test_db(&name).await;
}

// Verified real-world divergence: the dev DB seeded the post-rename slug
// ('inscribir-matrimonio') BEFORE 0016 ran, so life_events briefly carries
// BOTH slugs and 0016's plain UPDATE violates life_events_slug_key at boot.
// A hardened 0016 must absorb the duplicate's children into the canonical
// 'casarse' row (keeping its UUID), drop the duplicate, then rename —
// leaving one 'inscribir-matrimonio' event with the union of keywords and
// procedure relations and no duplicated (event, term) keyword rows.
#[tokio::test]
async fn migration_reconciles_seeded_duplicate_before_the_rename() {
    let (pool, name) = common::fresh_migrated_db().await;

    let category_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO categories (slug, name, order_index) \
         VALUES ('familia', 'Familia', 1) \
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed family category");
    let canonical_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO life_events (slug, name, category_id, updated_at) \
         VALUES ('casarse', 'Casarse', $1, TIMESTAMPTZ '2000-01-01 00:00:00+00') \
         RETURNING id",
    )
    .bind(category_id)
    .fetch_one(&pool)
    .await
    .expect("seed canonical pre-0016 marriage event");
    // Keywords of the divergent real-world projection: casarse kept its
    // original rows while inscribir-matrimonio was seeded from the renamed
    // YAML, overlapping on inscribir/matrimonio/partida.
    for (term, kind, weight) in [
        ("casar", "ACTION", 3),
        ("inscribir", "ACTION", 3),
        ("matrimonio", "ENTITY", 2),
        ("partida", "ENTITY", 2),
    ] {
        sqlx::query(
            "INSERT INTO life_event_keywords (life_event_id, term, type, weight) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(canonical_id)
        .bind(term)
        .bind(kind)
        .bind(weight)
        .execute(&pool)
        .await
        .expect("seed canonical event keyword");
    }
    let marriage_procedure_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO procedures (external_id, name, status) \
         VALUES ('4594', 'Inscripción de matrimonio', 'active') \
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed official marriage registration procedure");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         VALUES ($1, $2, 1, TRUE)",
    )
    .bind(canonical_id)
    .bind(marriage_procedure_id)
    .execute(&pool)
    .await
    .expect("seed canonical event-procedure relation");

    let duplicate_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO life_events (slug, name, category_id, updated_at) \
         VALUES ('inscribir-matrimonio', 'Inscribir matrimonio', $1, TIMESTAMPTZ '2000-01-01 00:00:00+00') \
         RETURNING id",
    )
    .bind(category_id)
    .fetch_one(&pool)
    .await
    .expect("seed divergent post-rename duplicate event");
    for (term, kind, weight) in [
        ("inscribir", "ACTION", 3),
        ("matrimonio", "ENTITY", 2),
        ("partida", "ENTITY", 2),
        ("registrar", "ACTION", 3),
    ] {
        sqlx::query(
            "INSERT INTO life_event_keywords (life_event_id, term, type, weight) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(duplicate_id)
        .bind(term)
        .bind(kind)
        .bind(weight)
        .execute(&pool)
        .await
        .expect("seed duplicate event keyword");
    }
    let second_procedure_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO procedures (external_id, name, status) \
         VALUES ('231-3', 'Anotación de matrimonio', 'active') \
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("seed second marriage procedure");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         VALUES ($1, $2, 2, FALSE)",
    )
    .bind(duplicate_id)
    .bind(second_procedure_id)
    .execute(&pool)
    .await
    .expect("seed duplicate event-procedure relation");

    // Fresh setup already recorded 0016 as applied, so make it pending again
    // after introducing the divergent both-slugs state (the verified dev-DB
    // history: 0016 recorded, old slug re-seeded afterwards).
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 16")
        .execute(&pool)
        .await
        .expect("make the rename migration pending");

    // RED expectation before the hardening: the plain rename UPDATE hits
    // life_events_slug_key (23505) while the duplicate row still exists.
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("0016 must reconcile the divergent seed before renaming");

    // The canonical row survives under the new slug, keeping its UUID.
    let (surviving_id, updated_at_changed): (sqlx::types::Uuid, bool) = sqlx::query_as(
        "SELECT id, updated_at > TIMESTAMPTZ '2000-01-01 00:00:00+00' \
         FROM life_events WHERE slug = 'inscribir-matrimonio'",
    )
    .fetch_one(&pool)
    .await
    .expect("exactly one inscribir-matrimonio row after reconciliation");
    assert_eq!(
        surviving_id, canonical_id,
        "the canonical casarse UUID must survive the reconciliation"
    );
    assert!(updated_at_changed, "the rename must refresh updated_at");

    let old_slug_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM life_events WHERE slug = 'casarse'")
            .fetch_one(&pool)
            .await
            .expect("count old slug rows");
    assert_eq!(old_slug_count, 0, "the old slug must no longer be present");

    // The duplicate's non-conflicting relation now points at the canonical row.
    let moved_relation_event_id: sqlx::types::Uuid = sqlx::query_scalar(
        "SELECT life_event_id FROM life_event_procedures WHERE procedure_id = $1",
    )
    .bind(second_procedure_id)
    .fetch_one(&pool)
    .await
    .expect("absorbed relation points somewhere after reconciliation");
    assert_eq!(
        moved_relation_event_id, canonical_id,
        "the duplicate's non-conflicting relation must move to the canonical event"
    );
    let kept_relation_event_id: sqlx::types::Uuid = sqlx::query_scalar(
        "SELECT life_event_id FROM life_event_procedures WHERE procedure_id = $1",
    )
    .bind(marriage_procedure_id)
    .fetch_one(&pool)
    .await
    .expect("canonical relation remains after reconciliation");
    assert_eq!(
        kept_relation_event_id, canonical_id,
        "the canonical event's own relation must remain"
    );

    // Keywords merge: the canonical event holds every distinct term from both
    // rows, with no duplicated (event, term) pairs.
    let mut terms: Vec<String> = sqlx::query_scalar(
        "SELECT term FROM life_event_keywords WHERE life_event_id = $1 ORDER BY term",
    )
    .bind(canonical_id)
    .fetch_all(&pool)
    .await
    .expect("list canonical event keywords")
    .into_iter()
    .collect();
    terms.dedup();
    assert_eq!(
        terms,
        vec!["casar", "inscribir", "matrimonio", "partida", "registrar"],
        "the canonical event must hold the merged, deduplicated keyword set"
    );

    common::drop_test_db(&name).await;
}

#[tokio::test]
async fn migrations_create_no_extensions() {
    let (pool, name) = common::fresh_provisioned_db().await;
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_extension")
        .fetch_one(&pool)
        .await
        .expect("count extensions before migrations");
    common::apply_migrations(&pool).await;
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_extension")
        .fetch_one(&pool)
        .await
        .expect("count extensions after migrations");
    assert_eq!(
        before, after,
        "migrations must create no extensions; pg_trgm/unaccent stay in docker/init/01-extensions.sql (D-6 portability)"
    );
    common::drop_test_db(&name).await;
}
