//! Task 78 (SE-7): the DB-backed candidate providers. `FtsProvider` queries
//! `life_events.generated_tsvector` with a `tsquery` built from the query's
//! canonical tokens and reports under `FTS_TEXT`; `TrigramProvider` scores
//! `similarity()` over name+positive-keywords and reports under `TRIGRAM`.
//! Both implement the pure engine's `CandidateProvider` trait, and no
//! embedding implementation exists anywhere.
//!
//! The fixture events carry accented names/descriptions: migration 0012
//! rebuilds `life_events.generated_tsvector` through the `unaccent_immutable`
//! wrapper, so accented surfaces produce de-accented lexemes that match the
//! engine's de-accented query tokens through the FTS_TEXT path. Fuzzy
//! subsequence coverage of accented surfaces remains the trigram provider's
//! job.

#[path = "c2support/mod.rs"]
mod c2support;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;

use c2support::*;
use search::engine::CandidateProvider;
use search::normalizer::normalize;
use search::types::NormalizedQuery;
use uuid::Uuid;

/// Cold-start sentinel: no durable generation has been adopted yet.
const GENERATION: Uuid = db::providers::LEGACY_GENERATION_ID;

/// Seeds two events: `alta-vehiculo` (name/description lexemes that match the
/// `alta vehiculo` tsquery, one positive keyword, one negative keyword) and
/// `otro-tramite` (a generic event whose ONLY mention of a vehicle-sale term
/// is a NEGATIVE keyword, so a leak of negative keywords into the trigram
/// surface would produce a false candidate).
async fn seed_provider_fixture(pool: &sqlx::PgPool) {
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
        "INSERT INTO life_event_keywords (life_event_id, term, type, weight, negative) \
         SELECT e.id, k.term, k.kind, k.weight, k.negative \
         FROM life_events e \
         JOIN (VALUES \
             ('alta-vehiculo', 'registro', 'ACTION', 5, false), \
             ('alta-vehiculo', 'vender', 'ACTION', 15, true), \
             ('otro-tramite', 'vender', 'ACTION', 15, true) \
         ) AS k(slug, term, kind, weight, negative) ON k.slug = e.slug",
    )
    .execute(pool)
    .await
    .expect("seed keywords");
}

fn query_of(text: &str) -> NormalizedQuery {
    normalize(text)
}

/// The real YAML fixture directory: task 3's representative catalog seeds
/// these taxonomy events before adding the synthetic 3,600-procedure load.
fn repo_data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data")
}

/// The exact provider-side text construction used before S4b: canonical
/// normalized tokens joined by spaces. Keeping this local reference makes
/// the fixture comparison below a real-PostgreSQL equivalence test against
/// the pre-async provider query behavior, not another call through the
/// provider implementation under test.
fn pre_async_query_text(query: &NormalizedQuery) -> String {
    query
        .tokens
        .iter()
        .map(|token| token.canonical.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn canonical_candidates(
    mut candidates: Vec<search::types::Candidate>,
) -> Vec<search::types::Candidate> {
    candidates.sort_by(|a, b| {
        (&a.event_slug, &a.rule_name, a.value).cmp(&(&b.event_slug, &b.rule_name, b.value))
    });
    candidates
}

/// Direct copy of the pre-S4b FTS SQL/reference mapping: this test-side
/// query is intentionally runtime-checked, matching the repository's
/// integration-test style without changing a production query or `.sqlx`.
async fn pre_async_fts_candidates(
    pool: &sqlx::PgPool,
    query: &NormalizedQuery,
) -> Result<Vec<search::types::Candidate>, sqlx::Error> {
    let text = pre_async_query_text(query);
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, f32)> = sqlx::query_as(
        "WITH q AS (SELECT plainto_tsquery('simple', $1) AS tsq) \
         SELECT e.slug, ts_rank(e.generated_tsvector, q.tsq) \
         FROM life_events e, q \
         WHERE e.status = 'active' \
           AND e.generated_tsvector @@ q.tsq",
    )
    .bind(text)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(event_slug, rank)| {
            let value = (f64::from(rank) * 100.0).round() as i64;
            (value > 0).then(|| search::types::Candidate {
                event_slug,
                rule_name: "FTS_TEXT".to_string(),
                value,
            })
        })
        .collect())
}

/// Direct copy of the pre-S4b trigram SQL/reference mapping, preserving its
/// negative-keyword exclusion, strict `>` threshold and rounded score scale.
async fn pre_async_trigram_candidates(
    pool: &sqlx::PgPool,
    query: &NormalizedQuery,
) -> Result<Vec<search::types::Candidate>, sqlx::Error> {
    const SIMILARITY_THRESHOLD: f32 = 0.3;
    let text = pre_async_query_text(query);
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, f32)> = sqlx::query_as(
        "WITH kw AS ( \
             SELECT life_event_id, \
                    string_agg(term || ' ' || COALESCE(canonical_term, ''), ' ' ORDER BY term, type) AS terms \
             FROM life_event_keywords \
             WHERE NOT negative \
             GROUP BY life_event_id \
           ) \
           SELECT e.slug, \
                  similarity(e.name || ' ' || COALESCE(kw.terms, ''), $1) AS sim \
           FROM life_events e \
           LEFT JOIN kw ON kw.life_event_id = e.id \
           WHERE e.status = 'active' \
             AND similarity(e.name || ' ' || COALESCE(kw.terms, ''), $1) > $2",
    )
    .bind(text)
    .bind(SIMILARITY_THRESHOLD)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(event_slug, similarity)| {
            let value = (f64::from(similarity) * 10.0).round() as i64;
            (value > 0).then(|| search::types::Candidate {
                event_slug,
                rule_name: "TRIGRAM".to_string(),
                value,
            })
        })
        .collect())
}

/// The nil sentinel means cold start, not an older durable generation.
/// Before adoption it searches mutable legacy surfaces; an unknown non-nil
/// generation must not silently fall back to those same rows.
#[tokio::test(flavor = "multi_thread")]
async fn nil_generation_uses_only_the_cold_start_fallback() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;
    assert_eq!(GENERATION, Uuid::nil());
    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    let trigram = db::providers::trigram::TrigramProvider::new(pool.clone());
    assert!(
        fts.candidates(GENERATION, &query_of("alta vehiculo"))
            .await
            .expect("cold-start FTS")
            .iter()
            .any(|c| c.event_slug == "alta-vehiculo")
    );
    assert!(
        trigram
            .candidates(GENERATION, &query_of("registro vehiculo"))
            .await
            .expect("cold-start trigram")
            .iter()
            .any(|c| c.event_slug == "alta-vehiculo")
    );
    let unknown = Uuid::now_v7();
    assert!(
        fts.candidates(unknown, &query_of("alta vehiculo"))
            .await
            .expect("unknown generation FTS")
            .is_empty()
    );
    assert!(
        trigram
            .candidates(unknown, &query_of("registro vehiculo"))
            .await
            .expect("unknown generation trigram")
            .is_empty()
    );
    let built = db::generations::build::build_generation(&pool, "taxonomy-cold-start")
        .await
        .expect("mint durable generation");
    assert_ne!(
        built.generation_id, GENERATION,
        "durable builds never mint the nil sentinel"
    );
    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fts_provider_implements_the_candidate_provider_trait() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;

    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    // Trait-object assignment proves `FtsProvider: CandidateProvider`.
    let _trait_object: &dyn CandidateProvider = &fts;
    assert_eq!(fts.rule_name(), "FTS_TEXT");

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fts_provider_matches_the_generated_tsvector() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;

    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    let candidates = fts
        .candidates(GENERATION, &query_of("alta vehiculo"))
        .await
        .expect("fts provider query succeeds");

    assert_eq!(
        candidates
            .iter()
            .filter(|c| c.event_slug == "alta-vehiculo")
            .count(),
        1,
        "exactly one FTS_TEXT candidate for the matching event, got: {candidates:?}"
    );
    let candidate = candidates
        .iter()
        .find(|c| c.event_slug == "alta-vehiculo")
        .expect("matching event present");
    assert_eq!(candidate.rule_name, "FTS_TEXT");
    assert!(
        candidate.value > 0,
        "the ts_rank contribution must be positive, got {candidate:?}"
    );
    assert!(
        !candidates.iter().any(|c| c.event_slug == "otro-tramite"),
        "a non-matching event must receive no FTS_TEXT contribution"
    );

    // TRIANGULATE over the accented fixture: the A-weight path (name token
    // plus a description token) and the B-weight path (description-only
    // tokens) each select exactly the matching event, and the generic event
    // stays absent (data-model scenario "Weights and GIN index are preserved").
    for probe in ["alta vehiculos", "registro vehiculo"] {
        let weighted = fts
            .candidates(GENERATION, &query_of(probe))
            .await
            .expect("weighted probe query succeeds");
        assert_eq!(
            weighted
                .iter()
                .filter(|c| c.event_slug == "alta-vehiculo")
                .count(),
            1,
            "exactly one FTS_TEXT candidate for probe {probe:?}, got: {weighted:?}"
        );
        assert!(
            weighted
                .iter()
                .find(|c| c.event_slug == "alta-vehiculo")
                .expect("weighted candidate present")
                .value
                > 0,
            "the weighted contribution must be positive for probe {probe:?}"
        );
        assert!(
            !weighted.iter().any(|c| c.event_slug == "otro-tramite"),
            "the generic event must stay absent for probe {probe:?}"
        );
    }

    // Schema contract, asserted directly through sqlx: the GIN index exists,
    // the wrapper is IMMUTABLE, and the column is a stored generated column.
    let index: (String,) = sqlx::query_as(
        "SELECT indexdef FROM pg_indexes \
         WHERE indexname = 'life_events_generated_tsvector_gin_idx'",
    )
    .fetch_one(&pool)
    .await
    .expect("the generated tsvector GIN index exists");
    assert!(
        index.0.contains("USING gin"),
        "the generated tsvector index must be a GIN index, got: {index:?}"
    );
    let volatility: (String,) = sqlx::query_as(
        "SELECT provolatile::text FROM pg_proc WHERE proname = 'unaccent_immutable'",
    )
    .fetch_one(&pool)
    .await
    .expect("unaccent_immutable exists");
    assert_eq!(volatility.0, "i", "the unaccent wrapper must be IMMUTABLE");
    let attgenerated: (String,) = sqlx::query_as(
        "SELECT attgenerated::text FROM pg_attribute \
         WHERE attrelid = 'life_events'::regclass AND attname = 'generated_tsvector'",
    )
    .fetch_one(&pool)
    .await
    .expect("generated_tsvector attribute exists");
    assert_eq!(
        attgenerated.0, "s",
        "generated_tsvector must be a STORED generated column"
    );

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fts_provider_is_deterministic_across_calls() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;

    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    let first = fts
        .candidates(GENERATION, &query_of("alta vehiculo"))
        .await
        .expect("first call succeeds");
    let second = fts
        .candidates(GENERATION, &query_of("alta vehiculo"))
        .await
        .expect("second call succeeds");
    assert_eq!(
        first, second,
        "identical inputs must give identical candidates"
    );

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn trigram_provider_implements_the_candidate_provider_trait() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;

    let trigram = db::providers::trigram::TrigramProvider::new(pool.clone());
    let _trait_object: &dyn CandidateProvider = &trigram;
    assert_eq!(trigram.rule_name(), "TRIGRAM");

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn trigram_provider_scores_similarity_over_name_and_keywords() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;

    let trigram = db::providers::trigram::TrigramProvider::new(pool.clone());
    let candidates = trigram
        .candidates(GENERATION, &query_of("registro vehiculo"))
        .await
        .expect("trigram provider query succeeds");

    let candidate = candidates
        .iter()
        .find(|c| c.event_slug == "alta-vehiculo")
        .expect("the name+keyword surface must similarity-match the query");
    assert_eq!(candidate.rule_name, "TRIGRAM");
    assert!(
        candidate.value > 0,
        "the similarity contribution must be positive, got {candidate:?}"
    );
    assert!(
        !candidates.iter().any(|c| c.event_slug == "otro-tramite"),
        "a low-similarity event must receive no TRIGRAM contribution"
    );

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn trigram_provider_excludes_negative_keywords_from_its_surface() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;

    let trigram = db::providers::trigram::TrigramProvider::new(pool.clone());
    let candidates = trigram
        .candidates(GENERATION, &query_of("vender"))
        .await
        .expect("trigram provider query succeeds");

    // `otro-tramite` declares `vender` ONLY as a negative keyword; its name
    // is deliberately generic. If negative keywords leaked into the
    // similarity surface, "Tramite generico vender" would similarity-match
    // the query above the threshold and produce a false candidate.
    assert!(
        !candidates.iter().any(|c| c.event_slug == "otro-tramite"),
        "negative keywords must not feed the trigram surface, got: {candidates:?}"
    );

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn providers_return_no_candidates_for_a_stop_word_only_query() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;

    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    let trigram = db::providers::trigram::TrigramProvider::new(pool.clone());
    let empty = query_of("de la el un");
    assert!(
        empty.tokens.is_empty(),
        "the fixture query must normalize to zero tokens"
    );
    assert!(
        fts.candidates(GENERATION, &empty)
            .await
            .expect("fts ok")
            .is_empty(),
        "no FTS_TEXT candidates for a stop-word-only query"
    );
    assert!(
        trigram
            .candidates(GENERATION, &empty)
            .await
            .expect("trigram ok")
            .is_empty(),
        "no TRIGRAM candidates for a stop-word-only query"
    );

    drop_db(&db_name).await;
}

/// Task 11 TRIANGULATE: over task 3's PII-free representative catalog,
/// each async provider must match the exact query and score mapping that
/// existed before the async refactor. The direct SQL helpers above are the
/// pre-async reference; provider output is canonically sorted only for the
/// assertion because neither SQL query promises an output order.
#[tokio::test(flavor = "multi_thread")]
async fn async_providers_match_pre_async_queries_on_the_task3_catalog_fixture() {
    let (pool, db_name) = fresh_migrated_db().await;
    catalog_fixture::apply(&pool, &repo_data_dir(), 42)
        .await
        .expect("task 3 catalog fixture applies");
    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    let trigram = db::providers::trigram::TrigramProvider::new(pool.clone());

    for raw_query in [
        "compre un auto usado",
        "compré un auto usado",
        "vender vehículo usado",
        "vendér un vehículo",
        "pagar la patente",
        "pagár paténte",
        "quiero abrir una cuenta bancaria",
    ] {
        let normalized = query_of(raw_query);
        let async_fts = fts
            .candidates(GENERATION, &normalized)
            .await
            .expect("async FTS provider succeeds");
        let reference_fts = pre_async_fts_candidates(&pool, &normalized)
            .await
            .expect("pre-async FTS reference query succeeds");
        assert_eq!(
            canonical_candidates(async_fts),
            canonical_candidates(reference_fts),
            "FTS candidates must match the pre-async query for {raw_query:?}"
        );

        let async_trigram = trigram
            .candidates(GENERATION, &normalized)
            .await
            .expect("async trigram provider succeeds");
        let reference_trigram = pre_async_trigram_candidates(&pool, &normalized)
            .await
            .expect("pre-async trigram reference query succeeds");
        assert_eq!(
            canonical_candidates(async_trigram),
            canonical_candidates(reference_trigram),
            "trigram candidates must match the pre-async query for {raw_query:?}"
        );
    }

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn no_embedding_implementation_exists_in_the_db_crate() {
    // SE-7 "embedding seam is empty", extended to the provider
    // implementations: scanning crates/db/src (comments stripped) for any
    // embedding/vector/model symbol must find nothing.
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    let mut stack = vec![src_dir.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable src directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable source file");
            let stripped: String = text
                .lines()
                .map(|line| line.split("//").next().unwrap_or(line))
                .collect::<Vec<_>>()
                .join("\n");
            for needle in [
                "Embedding",
                "embedding",
                "VectorStore",
                "vector_store",
                "OpenAI",
                "openai",
                "huggingface",
                "sentence_transformer",
            ] {
                if stripped.contains(needle) {
                    offenders.push(format!("{} references `{needle}`", path.display()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "the MVP ships no embeddings, models, or vector stores: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// Task 16 (S6, design §2.2): the generation-scoped trigram provider reads the
// precomputed `generation_trigram_surface` written at build time. It must
// replicate the legacy per-request content, with ordered keyword bytes for
// new builds — the same canonical terms, negative-keyword exclusion, and
// `round(similarity * 10)` scale and the same strict `>` threshold — with the
// threshold applied through `SET LOCAL pg_trgm.similarity_threshold` inside
// the provider transaction instead of the pool session state.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn generation_trigram_provider_implements_the_candidate_provider_trait() {
    let (pool, db_name) = fresh_migrated_db().await;
    let provider = db::providers::generation_trigram::GenerationTrigramProvider::new(pool.clone());
    let _trait_object: &dyn CandidateProvider = &provider;
    assert_eq!(provider.rule_name(), "TRIGRAM");

    drop_db(&db_name).await;
}

/// Equivalence RED: for the same generation, the precomputed surface must
/// yield identical candidates (same similarity values after the round scale,
/// same threshold, same exclusions) as the ordered legacy `string_agg`
/// query over task 3's representative catalog fixture.
#[tokio::test(flavor = "multi_thread")]
async fn generation_trigram_provider_matches_the_legacy_computation_on_the_task3_catalog_fixture() {
    let (pool, db_name) = fresh_migrated_db().await;
    catalog_fixture::apply(&pool, &repo_data_dir(), 42)
        .await
        .expect("task 3 catalog fixture applies");
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("generation build succeeds");

    let provider = db::providers::generation_trigram::GenerationTrigramProvider::new(pool.clone());
    for raw_query in [
        "compre un auto usado",
        "compré un auto usado",
        "vender vehículo usado",
        "vendér un vehículo",
        "pagar la patente",
        "pagár paténte",
        "quiero abrir una cuenta bancaria",
    ] {
        let normalized = query_of(raw_query);
        let generation_candidates = provider
            .candidates(report.generation_id, &normalized)
            .await
            .expect("generation trigram provider succeeds");
        let legacy_candidates = db::providers::trigram::TrigramProvider::new(pool.clone())
            .candidates(report.generation_id, &normalized)
            .await
            .expect("legacy trigram provider succeeds");
        let reference_candidates = pre_async_trigram_candidates(&pool, &normalized)
            .await
            .expect("pre-async reference query succeeds");
        assert_eq!(
            canonical_candidates(generation_candidates),
            canonical_candidates(reference_candidates.clone()),
            "generation trigram candidates must match the per-request computation for {raw_query:?}"
        );
        assert_eq!(
            canonical_candidates(legacy_candidates),
            canonical_candidates(reference_candidates),
            "legacy trigram candidates must keep matching the reference for {raw_query:?}"
        );
    }

    drop_db(&db_name).await;
}

/// TRIANGULATE: a negative keyword excludes the event identically through
/// the precomputed surface — a query matching only a negative keyword must
/// produce no candidate for that event in either computation.
#[tokio::test(flavor = "multi_thread")]
async fn generation_trigram_provider_excludes_negative_keywords_identically() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("generation build succeeds");

    let provider = db::providers::generation_trigram::GenerationTrigramProvider::new(pool.clone());
    let legacy = db::providers::trigram::TrigramProvider::new(pool.clone());
    let normalized = query_of("vender");

    let generation_candidates = provider
        .candidates(report.generation_id, &normalized)
        .await
        .expect("generation trigram provider succeeds");
    let legacy_candidates = legacy
        .candidates(report.generation_id, &normalized)
        .await
        .expect("legacy trigram provider succeeds");

    assert_eq!(
        canonical_candidates(generation_candidates.clone()),
        canonical_candidates(legacy_candidates.clone()),
        "the negative-keyword exclusion must be identical in both computations"
    );
    assert!(
        !generation_candidates
            .iter()
            .any(|c| c.event_slug == "otro-tramite"),
        "an event whose only mention is a negative keyword must not become a candidate, got: {generation_candidates:?}"
    );

    drop_db(&db_name).await;
}

/// TRIANGULATE: the threshold no longer depends on pool session state — the
/// provider sets `pg_trgm.similarity_threshold` with `SET LOCAL` inside its
/// transaction, so connections polluted with a different session GUC produce
/// exactly the default-threshold results.
#[tokio::test(flavor = "multi_thread")]
async fn generation_trigram_threshold_does_not_depend_on_pool_session_state() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;
    let report = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("generation build succeeds");

    // Taint every pooled connection with a session-level threshold far above
    // the default: without the provider's SET LOCAL, the `%` predicate would
    // behave differently per connection.
    let mut tainted: Vec<sqlx::pool::PoolConnection<sqlx::Postgres>> = Vec::new();
    while tainted.len() < 3 {
        let mut conn = pool
            .acquire()
            .await
            .expect("pool connection acquired for tainting");
        sqlx::query("SET pg_trgm.similarity_threshold = 0.9")
            .execute(&mut *conn)
            .await
            .expect("session threshold taint applied");
        tainted.push(conn);
    }
    drop(tainted);

    let provider = db::providers::generation_trigram::GenerationTrigramProvider::new(pool.clone());
    let normalized = query_of("registro vehiculo");
    let tainted_pool_candidates = provider
        .candidates(report.generation_id, &normalized)
        .await
        .expect("provider query succeeds on a tainted pool");
    // Reference on a completely different pool whose session state is clean.
    let reference_candidates = pre_async_trigram_candidates(&pool, &normalized)
        .await
        .expect("reference query succeeds");
    assert_eq!(
        canonical_candidates(tainted_pool_candidates.clone()),
        canonical_candidates(reference_candidates),
        "SET LOCAL must override pool session state: both connections see the same result"
    );
    assert!(
        tainted_pool_candidates
            .iter()
            .any(|c| c.event_slug == "alta-vehiculo"),
        "the expected candidate survives the tainted session state: {tainted_pool_candidates:?}"
    );

    drop_db(&db_name).await;
}

// ---------------------------------------------------------------------------
// WU-5a (F15 / T12): generation isolation for the candidate providers. Both
// providers receive the serving generation id; each must rank the REQUESTED
// generation's immutable projection, never the mutable legacy tables, whose
// content can be NEWER than the captured generation after a rollback.
// ---------------------------------------------------------------------------

/// Mutates the mutable taxonomy AFTER the first generation was built:
/// renames the matching event, drops its positive keyword, and gives it a
/// different one. A generation-blind provider would then rank this NEW
/// content even when asked for the OLD generation.
async fn mutate_provider_fixture(pool: &sqlx::PgPool) {
    sqlx::query(
        "UPDATE life_events SET name = 'Cambio de titularidad', \
         description = 'Transferencia de un automotor.' \
         WHERE slug = 'alta-vehiculo'",
    )
    .execute(pool)
    .await
    .expect("rename the fixture event");
    sqlx::query(
        "DELETE FROM life_event_keywords \
         WHERE life_event_id = (SELECT id FROM life_events WHERE slug = 'alta-vehiculo')",
    )
    .execute(pool)
    .await
    .expect("drop the old positive keyword");
    sqlx::query(
        "INSERT INTO life_event_keywords (life_event_id, term, type, weight, negative) \
         SELECT id, 'transferencia', 'ACTION', 5, false FROM life_events \
         WHERE slug = 'alta-vehiculo'",
    )
    .execute(pool)
    .await
    .expect("add the new positive keyword");
}

/// RED (WU-5a): the FTS provider must rank the requested generation's own
/// captured vector. The mutable table now carries the NEW content, so a
/// generation-blind query returns the newest taxonomy for the OLD id.
#[tokio::test(flavor = "multi_thread")]
async fn fts_provider_ranks_the_requested_generation_not_the_newest() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;
    let old = db::generations::build::build_generation(&pool, "taxonomy-g1")
        .await
        .expect("old generation builds");
    mutate_provider_fixture(&pool).await;
    let new = db::generations::build::build_generation(&pool, "taxonomy-g2")
        .await
        .expect("new generation builds");
    assert_ne!(
        old.generation_id, new.generation_id,
        "changed content must mint a new generation"
    );

    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    let old_match = fts
        .candidates(old.generation_id, &query_of("alta vehiculo"))
        .await
        .expect("fts query for the old generation");
    assert!(
        old_match.iter().any(|c| c.event_slug == "alta-vehiculo"),
        "the requested OLD generation must rank its own captured name, got: {old_match:?}"
    );
    let old_blind = fts
        .candidates(old.generation_id, &query_of("titularidad"))
        .await
        .expect("fts query for the old generation");
    assert!(
        !old_blind.iter().any(|c| c.event_slug == "alta-vehiculo"),
        "the OLD generation must not see the NEW content, got: {old_blind:?}"
    );

    let new_match = fts
        .candidates(new.generation_id, &query_of("titularidad"))
        .await
        .expect("fts query for the new generation");
    assert!(
        new_match.iter().any(|c| c.event_slug == "alta-vehiculo"),
        "the requested NEW generation must rank its own captured name, got: {new_match:?}"
    );

    drop_db(&db_name).await;
}

/// RED (WU-5a): the trigram provider must score the requested generation's
/// own precomputed surface, never the mutable name/keywords.
#[tokio::test(flavor = "multi_thread")]
async fn trigram_provider_ranks_the_requested_generation_not_the_newest() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;
    let old = db::generations::build::build_generation(&pool, "taxonomy-g1")
        .await
        .expect("old generation builds");
    mutate_provider_fixture(&pool).await;
    let new = db::generations::build::build_generation(&pool, "taxonomy-g2")
        .await
        .expect("new generation builds");
    assert_ne!(
        old.generation_id, new.generation_id,
        "changed content must mint a new generation"
    );

    let trigram = db::providers::trigram::TrigramProvider::new(pool.clone());
    let old_match = trigram
        .candidates(old.generation_id, &query_of("registro vehiculo"))
        .await
        .expect("trigram query for the old generation");
    assert!(
        old_match.iter().any(|c| c.event_slug == "alta-vehiculo"),
        "the requested OLD generation must rank its own captured surface, got: {old_match:?}"
    );
    let old_blind = trigram
        .candidates(old.generation_id, &query_of("transferencia titularidad"))
        .await
        .expect("trigram query for the old generation");
    assert!(
        !old_blind.iter().any(|c| c.event_slug == "alta-vehiculo"),
        "the OLD generation must not see the NEW surface, got: {old_blind:?}"
    );

    let new_match = trigram
        .candidates(new.generation_id, &query_of("transferencia titularidad"))
        .await
        .expect("trigram query for the new generation");
    assert!(
        new_match.iter().any(|c| c.event_slug == "alta-vehiculo"),
        "the requested NEW generation must rank its own captured surface, got: {new_match:?}"
    );

    drop_db(&db_name).await;
}

/// TRIANGULATE (WU-5a): the generation FTS projection carries the EXACT
/// legacy weighting. Before any mutation, `generation_fts_text.fts_tsvector`
/// must equal `life_events.generated_tsvector` for the same content, and the
/// generation-scoped provider's ranked value must equal the legacy reference
/// for known queries — proving the scoping change did not alter ranking.
#[tokio::test(flavor = "multi_thread")]
async fn generation_fts_projection_preserves_the_legacy_weighting() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;
    let report = db::generations::build::build_generation(&pool, "taxonomy-weighting")
        .await
        .expect("generation builds");

    // Exact vector equality: the projection reproduces the legacy expression
    // (name='A' + description='B', `simple` config, unaccent_immutable).
    let mismatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM generation_fts_text g \
         JOIN life_events e ON e.slug = g.slug \
         WHERE g.generation_id = $1 \
           AND g.fts_tsvector IS DISTINCT FROM e.generated_tsvector",
    )
    .bind(report.generation_id)
    .fetch_one(&pool)
    .await
    .expect("compare the projected and legacy vectors");
    assert_eq!(
        mismatches, 0,
        "the projection must store the exact legacy weighted vector"
    );

    // Ranked-value equality against the pre-async legacy reference.
    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    for probe in ["alta vehiculo", "registro vehiculo", "vehiculo"] {
        let normalized = query_of(probe);
        let generation = fts
            .candidates(report.generation_id, &normalized)
            .await
            .expect("generation-scoped provider succeeds");
        let reference = pre_async_fts_candidates(&pool, &normalized)
            .await
            .expect("legacy reference query succeeds");
        assert_eq!(
            canonical_candidates(generation),
            canonical_candidates(reference),
            "the generation-scoped FTS ranking must match the legacy weighting for {probe:?}"
        );
    }

    drop_db(&db_name).await;
}
