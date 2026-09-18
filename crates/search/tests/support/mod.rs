//! Shared in-memory taxonomy fixture builder for `crates/search` tests
//! (task 10). Reused by tasks 6–38: matcher/rules tests (unit A2), engine
//! tests (unit A3), per-event and golden tests (units A5/A6).
//!
//! This is test support only: `crates/search/src` stays free of filesystem
//! access, and fixtures never touch disk.
//!
//! `allow(dead_code)`: `FixtureEvent::name` is still ahead of its consumer
//! (A5 seed tests); every other fixture field is now read (category feeds
//! the engine's Categories band since unit A3).
#![allow(dead_code)]

use std::collections::HashMap;

use search::engine::{CandidateProvider, EngineError};
use search::types::KeywordKind as EngineKeywordKind;
use search::types::{
    Candidate, CombinationRule, EventLexicon, EventScore, Keyword, NormalizedQuery,
};

/// Keyword type in the seed schema (TX-2 allowed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordKind {
    Action,
    Entity,
    Modifier,
    Context,
}

/// One typed keyword of a fixture event.
#[derive(Debug, Clone)]
pub struct FixtureKeyword {
    pub term: &'static str,
    pub canonical: &'static str,
    pub kind: KeywordKind,
    pub weight: i64,
    pub negative: bool,
}

/// One ACTION_ENTITY combination rule (SE-5).
#[derive(Debug, Clone)]
pub struct FixtureRule {
    pub action: &'static str,
    pub entity: &'static str,
    pub bonus: i64,
}

/// One fixture life event: the in-memory shape A2's matcher and rules engine
/// will consume.
#[derive(Debug, Clone)]
pub struct FixtureEvent {
    pub slug: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub keywords: Vec<FixtureKeyword>,
    pub rules: Vec<FixtureRule>,
}

impl FixtureEvent {
    /// Positive (non-negative) keywords only.
    pub fn positive_keywords(&self) -> impl Iterator<Item = &FixtureKeyword> {
        self.keywords.iter().filter(|k| !k.negative)
    }

    /// Negative keywords (penalties, SE-6).
    pub fn negative_keywords(&self) -> impl Iterator<Item = &FixtureKeyword> {
        self.keywords.iter().filter(|k| k.negative)
    }
}

/// The full in-memory taxonomy fixture handed to the engine under test.
#[derive(Debug, Clone)]
pub struct SearchFixture {
    pub events: Vec<FixtureEvent>,
    pub synonyms: HashMap<String, String>,
}

/// Builds the Vehículos-shaped fixture used across the search tests: the
/// near-duplicate pair `comprar-vehiculo` / `vender-vehiculo`, typed
/// keywords, a negative keyword, an ACTION_ENTITY rule, and Rioplatense
/// synonyms (`auto`/`coche`/`automovil` → `vehiculo`).
pub fn vehiculos_fixture() -> SearchFixture {
    let comprar_vehiculo = FixtureEvent {
        slug: "comprar-vehiculo",
        name: "Comprar un vehículo",
        category: "vehiculos",
        keywords: vec![
            action("comprar", 10),
            entity("vehiculo", 8),
            modifier("usado", 3),
            negative("vender", 15),
        ],
        rules: vec![FixtureRule {
            action: "comprar",
            entity: "vehiculo",
            bonus: 15,
        }],
    };

    let vender_vehiculo = FixtureEvent {
        slug: "vender-vehiculo",
        name: "Vender un vehículo",
        category: "vehiculos",
        keywords: vec![action("vender", 10), entity("vehiculo", 8)],
        rules: vec![FixtureRule {
            action: "vender",
            entity: "vehiculo",
            bonus: 15,
        }],
    };

    let mut synonyms = HashMap::new();
    for surface in ["auto", "coche", "automovil"] {
        synonyms.insert(surface.to_string(), "vehiculo".to_string());
    }

    SearchFixture {
        events: vec![comprar_vehiculo, vender_vehiculo],
        synonyms,
    }
}

/// Converts one fixture event into the engine-side scoring lexicon the
/// matcher and rules modules consume (unit A2 onward). The category slug
/// feeds the engine's Categories no-result band (SE-10, unit A3).
pub fn event_lexicon(event: &FixtureEvent) -> EventLexicon {
    EventLexicon {
        slug: event.slug.to_string(),
        category: event.category.to_string(),
        keywords: event
            .keywords
            .iter()
            .map(|k| Keyword {
                term: k.term.to_string(),
                canonical: k.canonical.to_string(),
                kind: match k.kind {
                    KeywordKind::Action => EngineKeywordKind::Action,
                    KeywordKind::Entity => EngineKeywordKind::Entity,
                    KeywordKind::Modifier => EngineKeywordKind::Modifier,
                    KeywordKind::Context => EngineKeywordKind::Context,
                },
                weight: k.weight,
                negative: k.negative,
            })
            .collect(),
        rules: event
            .rules
            .iter()
            .map(|r| CombinationRule {
                action: r.action.to_string(),
                entity: r.entity.to_string(),
                bonus: r.bonus,
            })
            .collect(),
    }
}

/// Scores one fixture event for a tokenized query through the matcher and
/// the ACTION_ENTITY rules — the per-event composition the engine repeats
/// and units A2/A3 tests reuse.
pub fn score_event(query: &search::types::NormalizedQuery, event: &FixtureEvent) -> EventScore {
    let lex = event_lexicon(event);
    let mut entries = search::matcher::match_keywords(query, &lex.keywords);
    entries.extend(search::rules::action_entity_entries(query, &lex.rules));
    EventScore {
        slug: lex.slug,
        entries,
    }
}

/// A deterministic, DB-free candidate provider for engine tests (task 18)
/// and the golden harness (task 34): contributions are declared up front and
/// never depend on the query, so outcomes stay reproducible.
pub struct StubProvider {
    pub name: &'static str,
    pub contributions: Vec<(&'static str, i64)>,
}

impl CandidateProvider for StubProvider {
    fn rule_name(&self) -> &'static str {
        self.name
    }

    fn candidates(&self, _query: &NormalizedQuery) -> Result<Vec<Candidate>, EngineError> {
        Ok(self
            .contributions
            .iter()
            .map(|(slug, value)| Candidate {
                event_slug: slug.to_string(),
                rule_name: self.name.to_string(),
                value: *value,
            })
            .collect())
    }
}

fn action(term: &'static str, weight: i64) -> FixtureKeyword {
    keyword(term, KeywordKind::Action, weight, false)
}

fn entity(term: &'static str, weight: i64) -> FixtureKeyword {
    keyword(term, KeywordKind::Entity, weight, false)
}

fn modifier(term: &'static str, weight: i64) -> FixtureKeyword {
    keyword(term, KeywordKind::Modifier, weight, false)
}

fn negative(term: &'static str, weight: i64) -> FixtureKeyword {
    keyword(term, KeywordKind::Action, weight, true)
}

fn keyword(term: &'static str, kind: KeywordKind, weight: i64, negative: bool) -> FixtureKeyword {
    FixtureKeyword {
        term,
        canonical: term,
        kind,
        weight,
        negative,
    }
}
