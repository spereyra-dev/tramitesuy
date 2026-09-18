//! Shared in-memory taxonomy fixture builder for `crates/search` tests
//! (task 10). Reused by tasks 6–38: matcher/rules tests (unit A2), engine
//! tests (unit A3), per-event and golden tests (units A5/A6).
//!
//! This is test support only: `crates/search/src` stays free of filesystem
//! access, and fixtures never touch disk.
//!
//! `allow(dead_code)`: fixture fields (keywords, rules, negatives) are
//! intentionally ahead of their consumers — the matcher/rules engine in unit
//! A2 (tasks 11–13) starts reading them next, while unit A1 tests consume
//! only `SearchFixture::synonyms`.
#![allow(dead_code)]

use std::collections::HashMap;

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
