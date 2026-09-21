//! Per-event positive/negative query tests (SE-13, TX-5, tasks 29–30):
//! the real `data/` seed must satisfy its own contract. Every
//! `tests.positive` query of every event ranks that event TOP1; every
//! `tests.negative` query must NOT rank that event TOP1. The taxonomy is
//! loaded from the real YAML files through the taxonomy loader (the test
//! may use the filesystem; `crates/search/src` may not).
//!
//! Tests fail (RED) until the seed satisfies its own contract (task 31).

use std::collections::HashMap;
use std::path::Path;

use search::engine::SearchEngine;
use search::tokenizer::SynonymMap;
use search::types::{EventLexicon, SearchOutcome};
use taxonomy::model::{Event, Taxonomy};
use taxonomy::validator::validate_dir_against_snapshot;

mod support;

/// Repo root: the per-event tests run against the real committed seed.
fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
}

/// Loads and validates the real seed exactly as CI's taxonomy-validate CLI
/// does (task 32): every declared relation must resolve against the
/// committed external-id snapshot.
fn load_validated_taxonomy() -> Taxonomy {
    let data_dir = repo_root().join("data");
    let snapshot = repo_root().join("data/external_ids.snapshot.txt");
    let errors = validate_dir_against_snapshot(&data_dir, &snapshot);
    assert!(
        errors.is_empty(),
        "the real seed must validate with zero errors, got: {errors:?}"
    );
    taxonomy::loader::load_data_dir(&data_dir).expect("seed loads")
}

/// Converts the taxonomy crate's event model into the engine's scoring
/// lexicon (the same projection `apps/api` will perform at boot).
fn event_lexicon(event: &Event) -> EventLexicon {
    EventLexicon {
        slug: event.slug.clone(),
        category: event.category.clone(),
        keywords: event
            .keywords
            .iter()
            .map(|k| search::types::Keyword {
                term: k.term.clone(),
                canonical: if k.canonical.is_empty() {
                    k.term.clone()
                } else {
                    k.canonical.clone()
                },
                kind: match k.keyword_type {
                    taxonomy::model::KeywordType::Action => search::types::KeywordKind::Action,
                    taxonomy::model::KeywordType::Entity => search::types::KeywordKind::Entity,
                    taxonomy::model::KeywordType::Modifier => search::types::KeywordKind::Modifier,
                    taxonomy::model::KeywordType::Context => search::types::KeywordKind::Context,
                },
                weight: k.weight,
                negative: k.negative,
            })
            .collect(),
        rules: event
            .rules
            .iter()
            .map(|r| search::types::CombinationRule {
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

/// Runs the full engine over one query with no providers (the seed's own
/// keyword/rules layer must decide; provider entries come later, in slice c).
fn run_query(engine: &SearchEngine, query: &str) -> SearchOutcome {
    support::block_on(engine.search(support::STUB_GENERATION, query, &[]))
        .expect("engine search succeeds")
}

/// SE-13: every `tests.positive` query ranks its own event TOP1.
#[test]
fn every_positive_query_ranks_its_event_top1() {
    let taxonomy = load_validated_taxonomy();
    assert!(
        taxonomy.events.len() >= 9,
        "expected at least the 9 seed events, found {}",
        taxonomy.events.len()
    );
    let synonyms = synonym_map(&taxonomy);
    let lexicons: Vec<EventLexicon> = taxonomy
        .events
        .iter()
        .map(|e| event_lexicon(&e.event))
        .collect();
    let engine = SearchEngine::new(lexicons, synonyms);

    let mut checked = 0usize;
    for source in &taxonomy.events {
        for query in &source.event.tests.positive {
            let outcome = run_query(&engine, query);
            let top1 = outcome.results.first().map(|r| r.slug.as_str());
            assert_eq!(
                top1,
                Some(source.event.slug.as_str()),
                "positive query {query:?} of event {} must rank it TOP1 (ranked: {:?})",
                source.event.slug,
                outcome
                    .results
                    .iter()
                    .take(3)
                    .map(|r| (r.slug.as_str(), r.score))
                    .collect::<Vec<_>>()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "the seed must declare positive tests");
}

/// SE-13: every `tests.negative` query must NOT rank its own event TOP1.
#[test]
fn every_negative_query_does_not_rank_its_event_top1() {
    let taxonomy = load_validated_taxonomy();
    let synonyms = synonym_map(&taxonomy);
    let lexicons: Vec<EventLexicon> = taxonomy
        .events
        .iter()
        .map(|e| event_lexicon(&e.event))
        .collect();
    let engine = SearchEngine::new(lexicons, synonyms);

    let mut checked = 0usize;
    for source in &taxonomy.events {
        for query in &source.event.tests.negative {
            let outcome = run_query(&engine, query);
            let top1 = outcome.results.first().map(|r| r.slug.as_str());
            assert_ne!(
                top1,
                Some(source.event.slug.as_str()),
                "negative query {query:?} of event {} must NOT rank it TOP1 (ranked: {:?})",
                source.event.slug,
                outcome
                    .results
                    .iter()
                    .take(3)
                    .map(|r| (r.slug.as_str(), r.score))
                    .collect::<Vec<_>>()
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "the seed must declare negative tests");
}

/// T2 taxonomy coverage: each Documents, Housing, and Family event must
/// declare both positive and negative citizen-query examples. Their actual
/// ranking is exercised by the generic per-event query contract above.
#[test]
fn coverage_events_have_embedded_query_cases() {
    let taxonomy = load_validated_taxonomy();
    const COVERAGE_SLUGS: [&str; 6] = [
        "sacar-pasaporte",
        "solicitar-garantia-alquiler",
        "inscribir-nacimiento",
        "solicitar-partida-nacimiento",
        "inscribir-matrimonio",
        "solicitar-partida-matrimonio",
    ];

    for slug in COVERAGE_SLUGS {
        let event = taxonomy
            .events
            .iter()
            .find(|source| source.event.slug == slug)
            .unwrap_or_else(|| panic!("missing T2 coverage event {slug}"));
        assert!(
            !event.event.tests.positive.is_empty(),
            "T2 coverage event {slug} needs positive query cases"
        );
        assert!(
            !event.event.tests.negative.is_empty(),
            "T2 coverage event {slug} needs negative query cases"
        );
    }
}

/// This verified taxonomy slice declares embedded citizen-query cases for
/// every new event; their ranking contract is exercised above.
#[test]
fn verified_taxonomy_slice_events_have_embedded_query_cases() {
    let taxonomy = load_validated_taxonomy();
    const SLUGS: [&str; 6] = [
        "obtener-certificado-vacunacion",
        "solicitar-subsidio-desempleo",
        "solicitar-jubilacion",
        "solicitar-residencia-legal",
        "obtener-historia-laboral",
        "registrar-voluntad-donacion-organos",
    ];

    for slug in SLUGS {
        let event = taxonomy
            .events
            .iter()
            .find(|source| source.event.slug == slug)
            .unwrap_or_else(|| panic!("missing verified taxonomy slice event {slug}"));
        assert!(
            !event.event.tests.positive.is_empty(),
            "verified taxonomy slice event {slug} needs positive query cases"
        );
        assert!(
            !event.event.tests.negative.is_empty(),
            "verified taxonomy slice event {slug} needs negative query cases"
        );
    }
}

/// This taxonomy slice must declare citizen-query cases for its verified events.
#[test]
fn benefits_justice_consumer_education_events_have_embedded_query_cases() {
    let taxonomy = load_validated_taxonomy();
    const SLUGS: [&str; 4] = [
        "solicitar-asignacion-familiar",
        "solicitar-antecedentes-judiciales",
        "consultar-reclamar-o-denunciar-como-consumidor",
        "buscar-becas-formacion-exterior",
    ];

    for slug in SLUGS {
        let event = taxonomy
            .events
            .iter()
            .find(|source| source.event.slug == slug)
            .unwrap_or_else(|| panic!("missing taxonomy slice event {slug}"));
        assert!(
            !event.event.tests.positive.is_empty(),
            "taxonomy slice event {slug} needs positive query cases"
        );
        assert!(
            !event.event.tests.negative.is_empty(),
            "taxonomy slice event {slug} needs negative query cases"
        );
    }
}

/// TX-5 scenario "near-duplicate events are separable" (task 30):
/// `compre un auto` ranks `comprar-vehiculo` TOP1 and `vendi mi auto`
/// ranks `vender-vehiculo` TOP1.
#[test]
fn near_duplicate_buy_sell_events_are_separable() {
    let taxonomy = load_validated_taxonomy();
    let synonyms = synonym_map(&taxonomy);
    let lexicons: Vec<EventLexicon> = taxonomy
        .events
        .iter()
        .map(|e| event_lexicon(&e.event))
        .collect();
    let engine = SearchEngine::new(lexicons, synonyms);

    let buy = run_query(&engine, "compre un auto");
    assert_eq!(
        buy.results.first().map(|r| r.slug.as_str()),
        Some("comprar-vehiculo"),
        "`compre un auto` must rank comprar-vehiculo TOP1 (ranked: {:?})",
        buy.results
            .iter()
            .take(3)
            .map(|r| (r.slug.as_str(), r.score))
            .collect::<Vec<_>>()
    );

    let sell = run_query(&engine, "vendi mi auto");
    assert_eq!(
        sell.results.first().map(|r| r.slug.as_str()),
        Some("vender-vehiculo"),
        "`vendi mi auto` must rank vender-vehiculo TOP1 (ranked: {:?})",
        sell.results
            .iter()
            .take(3)
            .map(|r| (r.slug.as_str(), r.score))
            .collect::<Vec<_>>()
    );
}
