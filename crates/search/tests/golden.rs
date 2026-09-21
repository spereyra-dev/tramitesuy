//! Golden-dataset evaluation harness tests (SE-12, D-7, tasks 34–38).
//!
//! The harness runs every `tests/search/golden_dataset.yaml` case over the
//! real taxonomy seed with a deterministic DB-free `StubProvider`, reports
//! the Top1 / Top3 / no-result / ambiguous metrics table, and gates the
//! recorded baselines: a ranking regression fails the suite naming the
//! regressing query. The dataset file is read here (tests may touch the
//! filesystem); `crates/search/src::golden` stays pure and receives the
//! parsed dataset and the already-loaded engine.

mod support;

use std::collections::HashMap;
use std::path::Path;

use search::engine::SearchEngine;
use search::golden::{compute_metrics, gate, parse_dataset, run_cases};
use search::types::{CombinationRule, EventLexicon, Keyword, KeywordKind, SelectionMode};

/// Repo-relative golden dataset location, fixed by D-7's layout contract.
const DATASET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/search/golden_dataset.yaml"
);

fn load_dataset() -> search::golden::GoldenDataset {
    let raw = std::fs::read_to_string(Path::new(DATASET_PATH))
        .expect("golden dataset file exists at tests/search/golden_dataset.yaml");
    parse_dataset(&raw).expect("golden dataset parses against the v1 schema")
}

/// Real taxonomy seed loaded exactly as the per-event tests do (task 32):
/// validated against the committed external-id snapshot, then projected
/// into the engine lexicons.
fn seed_engine() -> SearchEngine {
    let (lexicons, synonyms) = support::real_seed();
    SearchEngine::new(lexicons, synonyms)
}

/// The harness-owned fixture for the falsifiability and accounting tests:
/// two minimal events with a Rioplatense synonym, independent of the real
/// seed so a deliberately degraded weight can never touch `data/`.
fn harness_fixture(vehiculo_weight: i64) -> SearchEngine {
    let comprar = EventLexicon {
        slug: "comprar-vehiculo".to_string(),
        category: "vehiculos".to_string(),
        keywords: vec![
            keyword("comprar", KeywordKind::Action, 10, false),
            keyword("vehiculo", KeywordKind::Entity, vehiculo_weight, false),
            keyword("usado", KeywordKind::Modifier, 3, false),
        ],
        rules: vec![CombinationRule {
            action: "comprar".to_string(),
            entity: "vehiculo".to_string(),
            bonus: 15,
        }],
    };
    let vender = EventLexicon {
        slug: "vender-vehiculo".to_string(),
        category: "vehiculos".to_string(),
        keywords: vec![
            keyword("vender", KeywordKind::Action, 10, false),
            keyword("vehiculo", KeywordKind::Entity, 8, false),
        ],
        rules: vec![CombinationRule {
            action: "vender".to_string(),
            entity: "vehiculo".to_string(),
            bonus: 15,
        }],
    };
    let synonyms: HashMap<String, String> = [("auto".to_string(), "vehiculo".to_string())]
        .into_iter()
        .collect();
    SearchEngine::new(vec![comprar, vender], synonyms)
}

fn keyword(term: &str, kind: KeywordKind, weight: i64, negative: bool) -> Keyword {
    Keyword {
        term: term.to_string(),
        canonical: term.to_string(),
        kind,
        weight,
        negative,
    }
}

/// SE-12 + D-7 (tasks 34, 35): every dataset case must satisfy its
/// expectations and every recorded baseline must hold over the real seed.
#[test]
fn golden_gate_passes_over_the_real_seed() {
    let dataset = load_dataset();
    assert_eq!(dataset.version, 1, "golden dataset must be version 1");
    let case_count = dataset.cases.len();
    assert!(
        (40..=140).contains(&case_count),
        "dataset v1 must carry 40-140 cases, found {case_count}"
    );

    let engine = seed_engine();
    let stub = support::StubProvider {
        name: "STUB_TEXT",
        contributions: Vec::new(),
    };

    match support::block_on(gate(&engine, support::STUB_GENERATION, &[&stub], &dataset)) {
        Ok(metrics) => println!("{}", metrics.metrics_table()),
        Err(report) => panic!("golden gate failed over the real seed:\n{report}"),
    }
}

/// Design verification checklist "golden gate fails on a deliberately
/// degraded weight" (task 36): inside a harness-owned fixture, dropping one
/// keyword weight must make the gate fail and its failure report must name
/// the regressing query.
#[test]
fn degrading_a_keyword_weight_fails_the_gate_naming_the_query() {
    let dataset = search::golden::GoldenDataset {
        version: 1,
        baselines: search::golden::Baselines {
            top1: 1.0,
            top3: 1.0,
            max_no_result_rate: 0.0,
            max_ambiguous_rate: 1.0,
        },
        cases: vec![search::golden::GoldenCase {
            query: "auto usado".to_string(),
            expect_top1: Some("comprar-vehiculo".to_string()),
            expect_top3: Vec::new(),
            expect_not_top1: vec!["vender-vehiculo".to_string()],
        }],
    };

    let healthy = harness_fixture(8);
    assert!(
        support::block_on(gate(&healthy, support::STUB_GENERATION, &[], &dataset)).is_ok(),
        "the healthy harness fixture must pass its own gate"
    );

    let degraded = harness_fixture(4);
    let report = support::block_on(gate(&degraded, support::STUB_GENERATION, &[], &dataset))
        .expect_err("degrading one keyword weight must regress the case");
    assert!(
        report.contains("auto usado"),
        "the failure report must name the regressing query, got: {report}"
    );
    println!("falsifiability evidence - the gate failed as required:\n{report}");
}

/// SE-12 scenario "metrics are reported per run" (task 37): a zero-match
/// query is counted as no-result, an ambiguous-confidence query is counted
/// in the ambiguous rate, and both appear in the printed metrics table.
#[test]
fn zero_match_and_ambiguous_cases_are_visible_in_the_metrics_table() {
    let engine = harness_fixture(8);
    let dataset = search::golden::GoldenDataset {
        version: 1,
        baselines: search::golden::Baselines {
            top1: 0.90,
            top3: 0.95,
            max_no_result_rate: 0.40,
            max_ambiguous_rate: 0.40,
        },
        cases: vec![
            search::golden::GoldenCase {
                query: "compre un auto usado".to_string(),
                expect_top1: Some("comprar-vehiculo".to_string()),
                expect_top3: Vec::new(),
                expect_not_top1: Vec::new(),
            },
            // Genuine tie: both events score 8 -> disambiguation band.
            search::golden::GoldenCase {
                query: "auto".to_string(),
                expect_top1: None,
                expect_top3: Vec::new(),
                expect_not_top1: Vec::new(),
            },
            // Zero matches: neither event keyword nor rule fires.
            search::golden::GoldenCase {
                query: "xyzzy qwerty".to_string(),
                expect_top1: None,
                expect_top3: Vec::new(),
                expect_not_top1: Vec::new(),
            },
        ],
    };
    let stub = support::StubProvider {
        name: "STUB_TEXT",
        contributions: Vec::new(),
    };

    let outcomes = support::block_on(run_cases(
        &engine,
        support::STUB_GENERATION,
        &[&stub],
        &dataset,
    ));
    assert_eq!(outcomes.len(), 3, "every dataset case produces one outcome");
    assert_eq!(
        outcomes[1].mode,
        Some(SelectionMode::Disambiguation),
        "the tied query must land in the disambiguation band"
    );
    assert_eq!(
        outcomes[2].mode,
        Some(SelectionMode::Categories),
        "the zero-match query must land in the categories no-result path"
    );

    let metrics = compute_metrics(&outcomes);
    assert_eq!(metrics.no_result_cases, 1, "zero-match counts as no-result");
    assert_eq!(metrics.ambiguous_cases, 1, "the tie counts as ambiguous");
    assert_eq!(metrics.top1_cases, 1, "only one case declares expect_top1");
    assert_eq!(metrics.top1_hits, 1);

    let table = metrics.metrics_table();
    assert!(table.contains("Top1"), "metrics table names Top1: {table}");
    assert!(table.contains("Top3"), "metrics table names Top3: {table}");
    assert!(
        table.contains("No-result"),
        "metrics table names no-result: {table}"
    );
    assert!(
        table.contains("Ambiguous"),
        "metrics table names ambiguous: {table}"
    );

    let expected_no_result = 1.0 / 3.0;
    assert!(
        (metrics.no_result_rate() - expected_no_result).abs() < 1e-9,
        "no-result rate must be 1/3, got {}",
        metrics.no_result_rate()
    );
    assert!(
        (metrics.ambiguous_rate() - expected_no_result).abs() < 1e-9,
        "ambiguous rate must be 1/3, got {}",
        metrics.ambiguous_rate()
    );

    // The harness reports the table on every run, including gate runs.
    assert!(
        support::block_on(gate(&engine, support::STUB_GENERATION, &[&stub], &dataset)).is_ok(),
        "the accounting fixture satisfies its own baselines"
    );
}

/// The dataset parse rejects a structurally invalid dataset (wrong version
/// key) instead of silently running an unknown format.
#[test]
fn dataset_parser_rejects_an_unknown_version() {
    let raw = "version: 99\nbaselines:\n  top1: 0.9\ncases: []\n";
    assert!(
        parse_dataset(raw).is_err(),
        "an unknown dataset version must not parse"
    );
}

/// The real seed must expose its Rioplatense synonym surfaces to the
/// harness (the same map the per-event tests build).
#[test]
fn real_seed_loads_synonym_surfaces() {
    let (_, synonyms) = support::real_seed();
    for (surface, canonical) in [
        ("auto", "vehiculo"),
        ("arrendamiento", "alquiler"),
        ("certificado", "partida"),
    ] {
        assert_eq!(
            synonyms.get(surface).map(String::as_str),
            Some(canonical),
            "the real seed must map {surface} -> {canonical}"
        );
    }
}

/// T3/T4: the global regression suite covers verified Documents, Housing,
/// and Family actions with citizen vocabulary, rather than only exercising
/// their YAML-local query cases. Consumer coverage lives in the event's
/// embedded cases plus the golden dataset's consumer positives.
#[test]
fn golden_dataset_covers_verified_taxonomy_events() {
    let dataset = load_dataset();
    for (query, slug) in [
        ("quiero sacar mi pasaporte", "sacar-pasaporte"),
        (
            "solicitar garantia de arrendamiento",
            "solicitar-garantia-alquiler",
        ),
        ("registrar el nacimiento de mi hija", "inscribir-nacimiento"),
        (
            "pedir certificado de nacimiento",
            "solicitar-partida-nacimiento",
        ),
        ("quiero inscribir mi matrimonio", "inscribir-matrimonio"),
        (
            "pedir certificado de matrimonio",
            "solicitar-partida-matrimonio",
        ),
    ] {
        assert!(
            dataset
                .cases
                .iter()
                .any(|case| case.query == query && case.expect_top1.as_deref() == Some(slug)),
            "golden dataset must cover {slug} with {query:?}"
        );
    }
}
