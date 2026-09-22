//! Memory-budget guard (S8 task 25, design §2.3/§7.2, OPT-10, R4): before a
//! candidate generation is loaded or adopted, RAM is sized for the active
//! generation + the candidate + the previous generation still in use + the
//! shared immutable bundle + the caches/PostgreSQL/system reserve. With no
//! budget the current generation stays active and the failure is reported
//! operationally — the process never reaches OOM or sustained swap growth.
//!
//! Sizing model: a generation's footprint splits into
//!
//! - `owned` — the per-generation catalog data the snapshot itself
//!   materializes (events, cards, procedure details, organizations,
//!   categories, slug maps). Additive per generation.
//! - `shared` — the immutable bundle data (engine, taxonomy, synonyms) that
//!   generations reuse behind `Arc`s: counted once, not per generation.

use std::collections::HashMap;

use super::ActiveGeneration;

/// The RAM budget the serving process must respect (configuration: the
/// total process budget and the reserve that accounts for caches,
/// PostgreSQL shared memory, and the system).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBudget {
    pub total_bytes: u64,
    pub reserve_bytes: u64,
}

impl MemoryBudget {
    /// Whether one more generation fits: active + candidate + previous still
    /// in use + the shared bundle + the reserve must stay within the total.
    pub fn permits(
        &self,
        active_bytes: u64,
        candidate_bytes: u64,
        previous_in_use_bytes: u64,
        shared_bytes: u64,
    ) -> bool {
        active_bytes
            .saturating_add(candidate_bytes)
            .saturating_add(previous_in_use_bytes)
            .saturating_add(shared_bytes)
            .saturating_add(self.reserve_bytes)
            <= self.total_bytes
    }
}

/// One generation's estimated RAM footprint (owned vs shared bundle data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footprint {
    pub owned_bytes: u64,
    pub shared_bytes: u64,
}

/// Estimates one generation's footprint: owned per-generation catalog data
/// plus the shared immutable bundle. The estimate is deterministic for the
/// same generation (stable sizes over immutable data).
pub fn estimate(generation: &ActiveGeneration) -> Footprint {
    let mut owned: u64 = 0;
    // Manifest identity strings.
    if let Some(manifest) = generation.manifest() {
        owned += str_bytes(&manifest.content_hash);
        owned += str_bytes(&manifest.taxonomy_version);
        owned += str_bytes(&manifest.engine_version);
    }
    // Slug display-name maps (per-generation materialized from the bundle).
    for (slug, name) in generation.event_name_map() {
        owned += str_bytes(slug) + str_bytes(name);
    }
    for (slug, name) in generation.category_name_map() {
        owned += str_bytes(slug) + str_bytes(name);
    }
    // Event snapshots.
    for (slug, event) in generation.event_map() {
        owned += str_bytes(slug);
        owned += str_bytes(&event.name);
        owned += event.description.as_deref().map(str_bytes).unwrap_or(0);
        owned += str_bytes(&event.status);
        owned += str_bytes(&event.category_slug);
    }
    // Ordered categories.
    for category in generation.categories() {
        owned += str_bytes(&category.slug)
            + str_bytes(&category.name)
            + category.icon.as_deref().map(str_bytes).unwrap_or(0);
    }
    // Cards per event.
    for (slug, cards) in generation.card_map() {
        owned += str_bytes(slug);
        for card in cards.iter() {
            owned += str_bytes(&card.slug)
                + str_bytes(&card.name)
                + card.importance.as_deref().map(str_bytes).unwrap_or(0)
                + card
                    .organization_short_name
                    .as_deref()
                    .map(str_bytes)
                    .unwrap_or(0)
                + card.cost.as_deref().map(str_bytes).unwrap_or(0)
                + str_bytes(&card.status)
                + card.official_url.as_deref().map(str_bytes).unwrap_or(0);
        }
    }
    // Procedure details (including inactive), organizations.
    for (external_id, detail) in generation.procedure_map() {
        owned += str_bytes(external_id)
            + str_bytes(&detail.name)
            + detail.description.as_deref().map(str_bytes).unwrap_or(0)
            + detail
                .organization_name
                .as_deref()
                .map(str_bytes)
                .unwrap_or(0)
            + detail.official_url.as_deref().map(str_bytes).unwrap_or(0)
            + str_bytes(&detail.status)
            + detail
                .raw_data
                .as_ref()
                .map(|raw| serde_json::to_string(raw).map_or(0, |text| text.len() as u64))
                .unwrap_or(0);
    }
    for (external_id, organization) in generation.organization_map() {
        owned += str_bytes(external_id)
            + str_bytes(&organization.name)
            + organization
                .short_name
                .as_deref()
                .map(str_bytes)
                .unwrap_or(0);
    }

    // Shared immutable bundle: the taxonomy model (events with keywords and
    // rules, categories) plus the synonyms map the engine and tokenizer
    // reuse behind Arcs across generations.
    let mut shared: u64 = 0;
    for source in &generation.taxonomy.events {
        owned_or_shared_event(&mut shared, &source.event);
    }
    for source in &generation.taxonomy.categories {
        owned_or_shared_category(&mut shared, &source.category);
    }
    owned += shared_bytes_of_synonyms(&generation.synonyms);
    Footprint {
        owned_bytes: owned,
        shared_bytes: shared,
    }
}

/// The taxonomy event's contribution is shared bundle data.
fn owned_or_shared_event(shared: &mut u64, event: &taxonomy::model::Event) {
    *shared += str_bytes(&event.slug)
        + str_bytes(&event.name)
        + str_bytes(&event.description)
        + str_bytes(&event.category);
    for keyword in &event.keywords {
        *shared += str_bytes(&keyword.term) + str_bytes(&keyword.canonical);
    }
    for rule in &event.rules {
        *shared += str_bytes(&rule.action) + str_bytes(&rule.entity);
    }
}

/// The category definitions are also shared bundle data.
fn owned_or_shared_category(shared: &mut u64, category: &taxonomy::model::Category) {
    *shared += str_bytes(&category.slug) + str_bytes(&category.name);
}

fn shared_bytes_of_synonyms(synonyms: &HashMap<String, String>) -> u64 {
    synonyms
        .iter()
        .map(|(term, canonical)| str_bytes(term) + str_bytes(canonical))
        .sum()
}

fn str_bytes(value: &str) -> u64 {
    u64::try_from(value.len()).unwrap_or(u64::MAX)
}

/// Coarse pre-load projection of a candidate's owned bytes from its manifest
/// counts, over the active snapshot's measured per-item rate. The guard
/// runs BEFORE the candidate is materialized, so an over-budget adoption
/// never allocates a second snapshot.
pub fn project_candidate(
    generation: &ActiveGeneration,
    candidate_events: i32,
    candidate_procedures: i32,
) -> u64 {
    let events = u64::try_from(candidate_events.max(0)).unwrap_or(0);
    let procedures = u64::try_from(candidate_procedures.max(0)).unwrap_or(0);
    let footprint = estimate(generation);
    let units = u64::try_from(
        generation
            .manifest()
            .map(|manifest| {
                manifest
                    .event_count
                    .saturating_add(manifest.procedure_count)
            })
            .unwrap_or(0)
            .max(1),
    )
    .unwrap_or(1);
    let per_unit = footprint.owned_bytes / units;
    per_unit.saturating_mul(events.saturating_add(procedures))
}
