//! Serde model of the YAML taxonomy (TX-1, TX-2). Every file schema uses
//! `deny_unknown_fields`: any field not present in this published schema
//! fails taxonomy validation, so community YAML cannot drift silently.

use serde::Deserialize;

/// Keyword type: the allowed set is exactly `ACTION | ENTITY | MODIFIER |
/// CONTEXT` (TX-2). Any other value is a hard validation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum KeywordType {
    Action,
    Entity,
    Modifier,
    Context,
}

/// One typed keyword of a life event. Required fields: `term`, `type`,
/// `weight` (TX-2). `canonical` and `negative` default (`negative: true`
/// marks a penalty keyword, SE-6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyword {
    pub term: String,
    #[serde(rename = "type")]
    pub keyword_type: KeywordType,
    pub weight: i64,
    #[serde(default)]
    pub canonical: String,
    #[serde(default)]
    pub negative: bool,
}

impl Keyword {
    /// The canonical matching term: the declared `canonical` when present,
    /// otherwise the term itself.
    pub fn canonical_or_term(&self) -> &str {
        if self.canonical.is_empty() {
            &self.term
        } else {
            &self.canonical
        }
    }
}

/// One ACTION_ENTITY combination rule (SE-5): fires the bonus when both the
/// action and the entity are matched by the query tokens.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombinationRule {
    pub action: String,
    pub entity: String,
    pub bonus: i64,
}

/// One event→procedure relation (TX-6): `order` is a positive integer,
/// unique within the event; `required` marks the procedure as mandatory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub external_id: String,
    pub order: u32,
    pub required: bool,
}

/// Per-event positive/negative query tests (SE-13, TX-5): both lists
/// default to empty for events that declare none yet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventTests {
    #[serde(default)]
    pub positive: Vec<String>,
    #[serde(default)]
    pub negative: Vec<String>,
}

/// One life event, fully described by one YAML file under `data/events/`
/// (TX-1). Required fields: `slug`, `name`, `description`, `category`,
/// `keywords`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub keywords: Vec<Keyword>,
    #[serde(default)]
    pub rules: Vec<CombinationRule>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub tests: EventTests,
}

/// One category definition under `data/categories/`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub order_index: u32,
}

/// One global synonym surface under `data/synonyms/`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Synonym {
    pub term: String,
    pub canonical: String,
}

/// Root schema of a synonyms YAML file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SynonymFile {
    #[serde(default)]
    pub synonyms: Vec<Synonym>,
}

/// A loaded event plus the file it came from (validator messages must name
/// the offending file, TX-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSource {
    pub file: String,
    pub event: Event,
}

/// A loaded category plus the file it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorySource {
    pub file: String,
    pub category: Category,
}

/// A loaded synonym plus the file it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynonymSource {
    pub file: String,
    pub synonym: Synonym,
}

/// The whole loaded taxonomy: events, categories, and synonyms, each with
/// its source file provenance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Taxonomy {
    pub events: Vec<EventSource>,
    pub categories: Vec<CategorySource>,
    pub synonyms: Vec<SynonymSource>,
}
