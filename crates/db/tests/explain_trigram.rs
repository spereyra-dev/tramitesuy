//! Task 16 (S6, design §2.2) Verify evidence: `EXPLAIN (ANALYZE, BUFFERS)`
//! over the generation-scoped trigram surface query, captured as committed
//! evidence. Index usage is NOT asserted as a success criterion on small
//! tables (design §2.2): the test measures time/worker rows from the plan and
//! prints it; the provider-equivalence tests in `providers.rs` carry the
//! correctness guarantee.

#[path = "c2support/mod.rs"]
mod c2support;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;

use c2support::*;

/// The repository's `data/` directory (same helper as providers.rs's local
/// copy — each test binary compiles its support independently).
fn repo_data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data")
}

/// The exact query the generation-scoped trigram provider runs (task 16),
/// prefixed by the EXPLAIN options — a single static literal so sqlx treats
/// it as a safe constant.
const PROVIDER_QUERY: &str = "EXPLAIN (ANALYZE, BUFFERS) SELECT g.slug, \
 similarity(g.surface_text, $1) AS sim FROM generation_trigram_surface g \
 JOIN generation_life_events e ON e.generation_id = g.generation_id AND e.slug = g.slug \
 WHERE g.generation_id = $2 AND g.surface_text % $1 \
   AND similarity(g.surface_text, $1) > $3 AND e.status = 'active'";

#[tokio::test(flavor = "multi_thread")]
async fn explain_analyze_buffers_output_is_captured_as_evidence() {
    let (pool, db_name) = fresh_migrated_db().await;
    catalog_fixture::apply(&pool, &repo_data_dir(), 42)
        .await
        .expect("task 3 catalog fixture applies");
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("generation build succeeds");

    // The provider's transaction first sets the GUC the `%` operator reads;
    // the explain probe replicates that session state so the captured plan
    // is the plan the provider actually executes.
    let mut conn = pool.acquire().await.expect("pool connection");
    sqlx::query("SET LOCAL pg_trgm.similarity_threshold = 0.3")
        .execute(&mut *conn)
        .await
        .expect("SET LOCAL similarity_threshold applied");
    let rows: Vec<(String,)> = sqlx::query_as(PROVIDER_QUERY)
        .bind("registro vehiculo")
        .bind(report.generation_id)
        .bind(0.3_f32)
        .fetch_all(&mut *conn)
        .await
        .expect("EXPLAIN (ANALYZE, BUFFERS) executes");
    drop(conn);

    let plan: String = rows
        .into_iter()
        .map(|(line,)| line)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        plan.contains("generation_trigram_surface"),
        "the captured plan must reference the generation projection table, got:\n{plan}"
    );
    assert!(
        plan.contains("actual time"),
        "the captured plan must be ANALYZE output with real timings, got:\n{plan}"
    );
    assert!(
        plan.contains("Buffers:"),
        "the captured plan must include BUFFERS instrumentation, got:\n{plan}"
    );

    // Evidence lands in the test log (visible with --nocapture); the plan
    // text is the committed evidence artifact for design §2.2.
    println!("EXPLAIN (ANALYZE, BUFFERS) for the generation trigram provider query:\n{plan}");

    drop_db(&db_name).await;
}
