//! High-importance search integration (audit finding F18, WU-1b T3). The
//! fast golden suite (`crates/search/tests/golden.rs`) runs with a DB-free
//! `StubProvider`, so it certifies only the keyword/rules layer of the
//! engine: no real PostgreSQL FTS contribution, no pg_trgm contribution, and
//! nothing about whether the procedures an event returns are pertinent. This
//! ignored test closes that gap for a small curated set of citizen queries.
//!
//! It runs the REAL engine — taxonomy lexicons loaded from the committed
//! `data/` YAML through the taxonomy loader (exactly the projection
//! `crates/search/tests/per_event.rs` builds) — over the REAL `FtsProvider`
//! and `TrigramProvider` against a live PostgreSQL catalog, then asserts, per
//! case, (a) the TOP1 event slug and (b) that the TOP1 event's returned
//! procedures are exactly its declared relations from
//! `data/events/<slug>.yaml` and are non-empty. A dedicated assertion also
//! pins `comprar-vehiculo`'s relation safety (F1/WU-1): no returned
//! procedure may come from the Dirección Nacional de Catastro and none may
//! be one of the forbidden ids `4551`/`2368`/`6995`/`2198`. The organization
//! is read from the database, never from a hand-copied title.
//!
//! Prerequisites (this test reads only — it never migrates, seeds, or mutates
//! the database): a reachable PostgreSQL holding the real ingested AGESIC
//! catalog; the `pg_trgm` and `unaccent` extensions; and the committed
//! taxonomy projected by `make seed-taxonomy`. The compose dev database
//! (`tramitesuy-db-1`, published on port 5432) satisfies all three. Point
//! `TRAMITESUY_TEST_DB_URL` at another instance to override; it defaults to
//! `postgres://postgres:postgres@localhost:5432/tramitesuy`.
//!
//! It is `#[ignore]`d so the default `cargo test --workspace` sweep never
//! depends on a populated catalog. Run it explicitly:
//!
//! ```text
//! cargo test -p db --test search_integration -- --ignored --nocapture
//! ```
//!
//! Expected RED until task T10 reconciles removed relations: the currently
//! projected catalog still carries the pre-WU-1a `4551` (Dirección Nacional
//! de Catastro) relation for `comprar-vehiculo` because `seed-taxonomy` does
//! not yet delete relations dropped from the YAML. Do not weaken the
//! assertion: the test is expected to turn GREEN after T10's reconciliation
//! plus a re-seed.

use std::collections::HashMap;
use std::path::Path;

use search::engine::{CandidateProvider, SearchEngine};
use search::tokenizer::SynonymMap;
use search::types::{CombinationRule, EventLexicon, Keyword, KeywordKind, SearchOutcome};
use sqlx::postgres::{PgPool, PgPoolOptions};
use taxonomy::model::{Event, EventSource, Taxonomy};
use taxonomy::validator::validate_dir_against_snapshot;

/// Repo root: the integration test runs against the real committed seed.
fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
}

/// Loads and validates the committed seed exactly as the per-event and
/// golden suites do, so a relation failure can never be masked by a broken
/// seed.
fn load_validated_taxonomy() -> Taxonomy {
    let data_dir = repo_root().join("data");
    let snapshot = repo_root().join("data/external_ids.snapshot.txt");
    let errors = validate_dir_against_snapshot(&data_dir, &snapshot);
    assert!(
        errors.is_empty(),
        "the real seed must validate with zero errors, got: {errors:?}"
    );
    taxonomy::loader::load_data_dir(&data_dir)
        .expect("the real seed loads through the taxonomy loader")
}

/// Converts the taxonomy crate's event model into the engine's scoring
/// lexicon (the same projection `apps/api` performs at boot and
/// `crates/search/tests/per_event.rs` uses).
fn event_lexicon(event: &Event) -> EventLexicon {
    EventLexicon {
        slug: event.slug.clone(),
        category: event.category.clone(),
        keywords: event
            .keywords
            .iter()
            .map(|k| Keyword {
                term: k.term.clone(),
                canonical: if k.canonical.is_empty() {
                    k.term.clone()
                } else {
                    k.canonical.clone()
                },
                kind: match k.keyword_type {
                    taxonomy::model::KeywordType::Action => KeywordKind::Action,
                    taxonomy::model::KeywordType::Entity => KeywordKind::Entity,
                    taxonomy::model::KeywordType::Modifier => KeywordKind::Modifier,
                    taxonomy::model::KeywordType::Context => KeywordKind::Context,
                },
                weight: k.weight,
                negative: k.negative,
            })
            .collect(),
        rules: event
            .rules
            .iter()
            .map(|r| CombinationRule {
                action: r.action.clone(),
                entity: r.entity.clone(),
                bonus: r.bonus,
            })
            .collect(),
    }
}

fn synonym_map(taxonomy: &Taxonomy) -> SynonymMap {
    taxonomy
        .synonyms
        .iter()
        .map(|s| (s.synonym.term.clone(), s.synonym.canonical.clone()))
        .collect::<HashMap<_, _>>()
}

/// The catalog connection: `TRAMITESUY_TEST_DB_URL` when set, otherwise the
/// compose dev database documented in the module header. The pool is
/// read-only for the lifetime of this test.
async fn catalog_pool() -> PgPool {
    let url = std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/tramitesuy".to_string());
    PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "connect to the search-integration catalog at {url}: {error:?}\n\
                 This test needs a running PostgreSQL with the real ingested catalog, \
                 `pg_trgm`/`unaccent`, and `make seed-taxonomy` already applied \
                 (see the module header)."
            )
        })
}

/// One curated high-importance citizen query and its expected TOP1 event.
struct HighImportanceCase {
    query: &'static str,
    expected_top1: &'static str,
}

/// Curated citizen queries whose real-provider ranking and relation
/// pertinence the fast stub-provider suite cannot certify. Add a case only
/// with a confident expected TOP1.
const HIGH_IMPORTANCE_CASES: &[HighImportanceCase] = &[
    HighImportanceCase {
        query: "compre un auto usado",
        expected_top1: "comprar-vehiculo",
    },
    HighImportanceCase {
        query: "perdi la cedula de identidad",
        expected_top1: "renovar-cedula",
    },
    HighImportanceCase {
        query: "perdi mi libreta de conducir",
        expected_top1: "perder-libreta",
    },
    HighImportanceCase {
        query: "transferir un auto a otro titular",
        expected_top1: "transferir-vehiculo",
    },
    HighImportanceCase {
        query: "sacar la cedula por primera vez",
        expected_top1: "sacar-cedula",
    },
];

/// The declared relation ids of one event, sorted for order-insensitive
/// comparison against the projected relation set.
fn declared_relation_ids(taxonomy: &Taxonomy, slug: &str) -> Vec<String> {
    let source: &EventSource = taxonomy
        .events
        .iter()
        .find(|source| source.event.slug == slug)
        .unwrap_or_else(|| panic!("the real seed must declare event {slug}"));
    let mut ids: Vec<String> = source
        .event
        .relations
        .iter()
        .map(|relation| relation.external_id.clone())
        .collect();
    ids.sort();
    ids
}

fn ranked_preview(outcome: &SearchOutcome) -> Vec<(&str, i64)> {
    outcome
        .results
        .iter()
        .take(3)
        .map(|result| (result.slug.as_str(), result.score))
        .collect()
}

/// F18: for every curated citizen query, the REAL FTS/trigram providers and
/// the committed taxonomy must rank the expected event TOP1 and return
/// exactly that event's declared procedure relations (non-empty).
///
/// The failures are collected and reported together so one RED never hides
/// the state of the remaining cases.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "search integration (F18): needs a reachable catalog seeded with the real taxonomy"]
async fn high_importance_queries_rank_top1_with_pertinent_relations() {
    let pool = catalog_pool().await;

    let procedures: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM procedures")
        .fetch_one(&pool)
        .await
        .expect("count procedures in the catalog");
    assert!(
        procedures > 0,
        "the target database holds no procedures: seed the real ingested AGESIC \
         catalog and project the taxonomy with `make seed-taxonomy` first"
    );

    let taxonomy = load_validated_taxonomy();
    let synonyms = synonym_map(&taxonomy);
    let lexicons: Vec<EventLexicon> = taxonomy
        .events
        .iter()
        .map(|source| event_lexicon(&source.event))
        .collect();
    let engine = SearchEngine::new(lexicons, synonyms);

    // The real production provider pair for the legacy generation (the API
    // uses exactly these two when no generation snapshot is loaded).
    let fts = db::providers::fts::FtsProvider::new(pool.clone());
    let trigram = db::providers::trigram::TrigramProvider::new(pool.clone());
    let providers: [&dyn CandidateProvider; 2] = [&fts, &trigram];

    let mut failures: Vec<String> = Vec::new();
    for case in HIGH_IMPORTANCE_CASES {
        let outcome = engine
            .search(db::providers::LEGACY_GENERATION_ID, case.query, &providers)
            .await
            .unwrap_or_else(|error| panic!("provider failure for {:?}: {error}", case.query));
        let top1 = outcome.results.first().map(|result| result.slug.as_str());
        let ranked = ranked_preview(&outcome);
        println!(
            "query {:?} -> top1 {:?} (ranked {:?}, mode {:?})",
            case.query, top1, ranked, outcome.selection.mode
        );

        if top1 != Some(case.expected_top1) {
            failures.push(format!(
                "query {:?} must rank {:?} TOP1, got {:?} (top scores: {:?})",
                case.query, case.expected_top1, top1, ranked
            ));
            // The relation contract only makes sense for the event that
            // actually ranked TOP1; do not report a relation mismatch for a
            // different event.
            continue;
        }

        let declared = declared_relation_ids(&taxonomy, case.expected_top1);
        let projected = db::repos::procedures::by_event(&pool, case.expected_top1)
            .await
            .unwrap_or_else(|error| panic!("load procedures for {:?}: {error}", case.expected_top1))
            .unwrap_or_else(|| {
                panic!(
                    "event {:?} has no projected page in the catalog",
                    case.expected_top1
                )
            });
        let mut returned: Vec<String> = projected
            .procedures
            .iter()
            .map(|procedure| procedure.external_id.clone())
            .collect();
        returned.sort();
        println!(
            "  {:?} declared relations {:?}; projected relations {:?}",
            case.expected_top1, declared, returned
        );

        if returned.is_empty() {
            failures.push(format!(
                "{:?} must return at least one procedure relation, got none",
                case.expected_top1
            ));
        }
        if returned != declared {
            failures.push(format!(
                "{:?} must return exactly its declared relations {:?}, got {:?}",
                case.expected_top1, declared, returned
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "high-importance search integration failures (real providers + committed taxonomy over the ingested catalog):\n{}",
        failures.join("\n")
    );
}

/// F1/F18 relation safety for `comprar-vehiculo`: the event ranks TOP1 for a
/// generic used-car purchase query, so a wrong relation is a wrong
/// recommendation to a citizen. None of its projected procedures may come
/// from the Dirección Nacional de Catastro and none may be one of the
/// forbidden ids `4551`/`2368`/`6995`/`2198`. The organization is read from
/// the database (never a hand-copied title).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "search integration (F18): needs a reachable catalog seeded with the real taxonomy"]
async fn comprar_vehiculo_never_returns_catastro_or_forbidden_relations() {
    let pool = catalog_pool().await;

    // (external_id, organization_name) for the projected buyer flow.
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT p.external_id, o.name \
         FROM life_event_procedures r \
         JOIN life_events e ON e.id = r.life_event_id \
         JOIN procedures p ON p.id = r.procedure_id \
         LEFT JOIN organizations o ON o.id = p.organization_id \
         WHERE e.slug = 'comprar-vehiculo' \
         ORDER BY r.order_index, p.external_id",
    )
    .fetch_all(&pool)
    .await
    .expect("load the projected comprar-vehiculo relations from the catalog");
    println!("comprar-vehiculo projected relations: {rows:?}");
    assert!(
        !rows.is_empty(),
        "comprar-vehiculo must return at least one projected procedure relation"
    );

    const FORBIDDEN_IDS: [&str; 4] = ["4551", "2368", "6995", "2198"];
    const CATASTRO: &str = "Dirección Nacional de Catastro";

    let mut violations: Vec<String> = Vec::new();
    for (external_id, organization) in &rows {
        if FORBIDDEN_IDS.contains(&external_id.as_str()) {
            violations.push(format!(
                "forbidden relation {external_id} (organization: {organization:?})"
            ));
        }
        if organization.as_deref() == Some(CATASTRO) {
            violations.push(format!("relation {external_id} belongs to {CATASTRO}"));
        }
    }
    assert!(
        violations.is_empty(),
        "comprar-vehiculo must keep only the buyer-oriented Canelones pair (6978/6980) and \
         no Catastro/forbidden relation; violations: {violations:?}"
    );
}
