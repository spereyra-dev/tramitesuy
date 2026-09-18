//! Task 78 (SE-7): the DB-backed candidate providers. `FtsProvider` queries
//! `life_events.generated_tsvector` with a `tsquery` built from the query's
//! canonical tokens and reports under `FTS_TEXT`; `TrigramProvider` scores
//! `similarity()` over name+positive-keywords and reports under `TRIGRAM`.
//! Both implement the pure engine's `CandidateProvider` trait, and no
//! embedding implementation exists anywhere.
//!
//! The fixture events carry deliberately de-accented names/descriptions: the
//! `0011` generated column builds the tsvector with the `simple` config over
//! the stored text (no unaccent), so only de-accented lexemes can match the
//! engine's de-accented query tokens. This limitation is recorded in the
//! apply progress; fuzzy coverage of accented surfaces is the trigram
//! provider's job.

#[path = "c2support/mod.rs"]
mod c2support;

use c2support::*;
use search::engine::CandidateProvider;
use search::normalizer::normalize;
use search::types::NormalizedQuery;

/// Seeds two events: `alta-vehiculo` (name/description lexemes that match the
/// `alta vehiculo` tsquery, one positive keyword, one negative keyword) and
/// `otro-tramite` (a generic event whose ONLY mention of a vehicle-sale term
/// is a NEGATIVE keyword, so a leak of negative keywords into the trigram
/// surface would produce a false candidate).
async fn seed_provider_fixture(pool: &sqlx::PgPool) {
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehiculos', 'car', 1)",
    )
    .execute(pool)
    .await
    .expect("seed category");

    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'alta-vehiculo', 'Alta de vehiculos', \
                'Registro inicial de un vehiculo.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event alta-vehiculo");

    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'otro-tramite', 'Tramite generico', 'Otro tramite.', id \
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
        .candidates(&query_of("alta vehiculo"))
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

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fts_provider_is_deterministic_across_calls() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_provider_fixture(&pool).await;

    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    let first = fts
        .candidates(&query_of("alta vehiculo"))
        .expect("first call succeeds");
    let second = fts
        .candidates(&query_of("alta vehiculo"))
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
        .candidates(&query_of("registro vehiculo"))
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
        .candidates(&query_of("vender"))
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
        fts.candidates(&empty).expect("fts ok").is_empty(),
        "no FTS_TEXT candidates for a stop-word-only query"
    );
    assert!(
        trigram.candidates(&empty).expect("trigram ok").is_empty(),
        "no TRIGRAM candidates for a stop-word-only query"
    );

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
