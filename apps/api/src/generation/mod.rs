//! The in-memory catalog snapshot per generation (S7 task 19, design §1.3,
//! OPT-01/OPT-03/OPT-04, R8): `ActiveGeneration` is an immutable read
//! snapshot loaded from a durable `published` generation — manifest plus
//! projections plus the taxonomy/engine identity — without any AGESIC
//! download, and without version history or search logs in RAM.
//!
//! Serving contract:
//! - every lookup is a slug/id `HashMap` hit: a catalog read never issues
//!   SQL, and an unknown identifier resolves to `None` (the handlers map it
//!   to the public 404) with no database query;
//! - inactive procedures stay fetchable with the current contract;
//! - the loader walks published candidates newest-first and falls back to
//!   the previous generation; a `building`/incomplete or taxonomy-mismatched
//!   candidate is rejected and never adopted;
//! - an invalid or failed load never changes the served generation: the
//!   loader only produces a snapshot, and `AppState` swaps one in atomically
//!   (task 20) only after a complete, successful load.
//!
//! Stage-3 interim note (recorded in the change's apply-progress): the
//! generation projections (migration 0015) carry events, cards, and
//! procedure details, but categories, event descriptions, and organizations
//! are not per-generation projected yet. The loader reads those three from
//! the same legacy tables the build itself read (dual-write stays in place
//! during stages 2–3), once per load — the per-request serving cost stays
//! zero SQL, and the taxonomy remains the ranker's source of truth (TX-1).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::DateTime;
use search::engine::SearchEngine;
use search::tokenizer::SynonymMap;
use search::types::{
    CombinationRule as EngineRule, EventLexicon, Keyword as EngineKeyword,
    KeywordKind as EngineKeywordKind,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// The manifest identity of one durable generation (design §1.2, migration
/// 0013): everything `/ready` (task 22) and the cache key (stage 4) need.
#[derive(Debug, Clone)]
pub struct GenerationManifest {
    pub generation_id: Uuid,
    pub content_hash: String,
    pub taxonomy_version: String,
    pub engine_version: String,
    /// The maximum observable `last_seen_at` — what the API serves as "last
    /// successful sync" (build contract, task 15).
    pub source_synced_at: DateTime<chrono::Utc>,
    pub published_at: DateTime<chrono::Utc>,
    pub event_count: i32,
    pub procedure_count: i32,
}

/// One ordered category view (API-7): slug, name, icon, catalog order.
#[derive(Debug, Clone)]
pub struct CategoryView {
    pub slug: String,
    pub name: String,
    pub icon: Option<String>,
    pub order_index: i32,
}

/// One event in the snapshot (API-6 inputs): identity, display data, status,
/// and its category. Inactive events keep their view exactly like inactive
/// procedures keep theirs (never deleted, IN-7).
#[derive(Debug)]
pub struct EventSnapshot {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub category_slug: String,
}

/// One organization view (API-8 inputs).
#[derive(Debug)]
pub struct OrganizationView {
    pub external_id: String,
    pub name: String,
    pub short_name: Option<String>,
}

/// The catalog payload of one snapshot: everything the loader decodes from
/// the durable generation and its legacy supplements, grouped so the
/// constructor stays at three arguments.
#[derive(Default)]
struct SnapshotParts {
    manifest: Option<GenerationManifest>,
    events: HashMap<String, Arc<EventSnapshot>>,
    categories: Vec<CategoryView>,
    cards: HashMap<String, Arc<Vec<db::repos::procedures::EventCard>>>,
    procedures: HashMap<String, Arc<db::repos::procedures::ProcedureDetail>>,
    organizations: HashMap<String, Arc<OrganizationView>>,
}

impl SnapshotParts {
    fn empty() -> Self {
        SnapshotParts::default()
    }
}

/// The per-generation provider scope (design §1.3 `GenerationProviders`):
/// every provider query carries this generation id, and the fetch policy is
/// fixed by configuration at boot. The concrete providers are constructed
/// per request over the captured generation (task 21); what is immutable
/// here is the scope they must query.
#[derive(Debug, Clone, Copy)]
pub struct GenerationProviders {
    pub generation_id: Uuid,
    pub fetch: db::providers::orchestrator::ProviderFetch,
}

/// The immutable serving snapshot of one catalog generation.
pub struct ActiveGeneration {
    /// `None` only for the cold-start baseline: the pre-first-load serving
    /// state that carries the boot taxonomy/engine and no catalog data.
    manifest: Option<GenerationManifest>,
    /// The deterministic engine, built from the generation's YAML taxonomy.
    pub engine: Arc<SearchEngine>,
    /// The loaded YAML taxonomy: the ranker's source of truth (TX-1).
    pub taxonomy: Arc<taxonomy::model::Taxonomy>,
    /// The taxonomy synonyms (canonical-term resolution) of this generation.
    pub synonyms: SynonymMap,
    /// Per-generation slug maps replacing the old `AppState` linear scans
    /// (task 19 GREEN): YAML display names for events and categories.
    event_names: HashMap<String, String>,
    category_names: HashMap<String, String>,
    /// The ordered category catalog (order_index ascending).
    categories: Vec<CategoryView>,
    /// Events indexed by slug.
    events: HashMap<String, Arc<EventSnapshot>>,
    /// Procedure cards per event slug, in relation order.
    cards: HashMap<String, Arc<Vec<db::repos::procedures::EventCard>>>,
    /// Procedure details indexed by external id (inactive included).
    procedures: HashMap<String, Arc<db::repos::procedures::ProcedureDetail>>,
    /// Organizations indexed by external id.
    organizations: HashMap<String, Arc<OrganizationView>>,
    /// The provider scope every search of this generation must use.
    pub providers: GenerationProviders,
}

impl ActiveGeneration {
    /// The cold-start baseline (design §1.4): the boot taxonomy/engine with
    /// an empty catalog. Catalog reads report 503 in this state (task 22);
    /// search still serves through the legacy (dual-written) tables until
    /// the first valid snapshot is adopted.
    pub fn cold(bundle: TaxonomyBundle, fetch: db::providers::orchestrator::ProviderFetch) -> Self {
        ActiveGeneration::from_parts(bundle, SnapshotParts::empty(), fetch)
    }

    fn from_parts(
        bundle: TaxonomyBundle,
        parts: SnapshotParts,
        fetch: db::providers::orchestrator::ProviderFetch,
    ) -> Self {
        let SnapshotParts {
            manifest,
            events,
            categories,
            cards,
            procedures,
            organizations,
        } = parts;
        let generation_id = manifest
            .as_ref()
            .map(|manifest| manifest.generation_id)
            .unwrap_or_else(Uuid::nil);
        let event_names: HashMap<String, String> = bundle
            .taxonomy
            .events
            .iter()
            .map(|source| (source.event.slug.clone(), source.event.name.clone()))
            .collect();
        let category_names: HashMap<String, String> = bundle
            .taxonomy
            .categories
            .iter()
            .map(|source| (source.category.slug.clone(), source.category.name.clone()))
            .collect();
        ActiveGeneration {
            manifest,
            engine: bundle.engine,
            taxonomy: bundle.taxonomy,
            synonyms: bundle.synonyms,
            event_names,
            category_names,
            categories,
            events,
            cards,
            procedures,
            organizations,
            providers: GenerationProviders {
                generation_id,
                fetch,
            },
        }
    }

    /// Whether a durable generation is loaded (cold start ⇒ false).
    pub fn is_loaded(&self) -> bool {
        self.manifest.is_some()
    }

    /// The loaded manifest, or `None` in the cold-start baseline.
    pub fn manifest(&self) -> Option<&GenerationManifest> {
        self.manifest.as_ref()
    }

    /// The generation scope providers (and payload consumers) must use.
    pub fn generation_id(&self) -> Uuid {
        self.providers.generation_id
    }

    /// The YAML display name of an event slug (replaces the `AppState`
    /// linear scan; task 19 GREEN).
    pub fn event_name(&self, slug: &str) -> Option<&str> {
        self.event_names.get(slug).map(String::as_str)
    }

    /// The YAML display name of a category slug.
    pub fn category_name(&self, slug: &str) -> Option<&str> {
        self.category_names.get(slug).map(String::as_str)
    }

    /// The ordered category catalog.
    pub fn categories(&self) -> &[CategoryView] {
        &self.categories
    }

    /// One event by slug, or `None` for an unknown slug (404, no SQL).
    pub fn event(&self, slug: &str) -> Option<Arc<EventSnapshot>> {
        self.events.get(slug).cloned()
    }

    /// The category's events, slug-ascending (the DB listing's order).
    pub fn events_of_category(&self, category_slug: &str) -> Vec<Arc<EventSnapshot>> {
        let mut events: Vec<Arc<EventSnapshot>> = self
            .events
            .values()
            .filter(|event| event.category_slug == category_slug)
            .cloned()
            .collect();
        events.sort_by(|a, b| a.slug.cmp(&b.slug));
        events
    }

    /// The event's ordered procedure cards, or `None` for an unknown event.
    pub fn cards(&self, event_slug: &str) -> Option<Arc<Vec<db::repos::procedures::EventCard>>> {
        self.cards.get(event_slug).cloned()
    }

    /// One procedure detail by external id, or `None` (404, no SQL).
    pub fn procedure(
        &self,
        external_id: &str,
    ) -> Option<Arc<db::repos::procedures::ProcedureDetail>> {
        self.procedures.get(external_id).cloned()
    }

    /// One organization by external id.
    pub fn organization(&self, external_id: &str) -> Option<Arc<OrganizationView>> {
        self.organizations.get(external_id).cloned()
    }
}

/// The taxonomy/engine bundle every generation (and the cold baseline)
/// carries: the YAML source of truth, the engine built from it, and the
/// manifest's `taxonomy_version` identity of those exact bytes. `Clone` is
/// cheap: every field is an `Arc` over immutable data (shared, not copied —
/// design §2.3 memory budget).
#[derive(Clone)]
pub struct TaxonomyBundle {
    pub taxonomy: Arc<taxonomy::model::Taxonomy>,
    pub engine: Arc<SearchEngine>,
    pub synonyms: SynonymMap,
    pub version: String,
}

/// Error surface of the snapshot loading (typed per crate rules).
#[derive(Debug)]
pub enum GenerationError {
    /// The YAML taxonomy could not be loaded, parsed, or hashed.
    Taxonomy(String),
    /// A durable candidate was unusable: incomplete, invalid, or mismatched
    /// with the boot taxonomy. The served generation is never changed.
    Rejected { generation_id: Uuid, reason: String },
    /// The durable read itself failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerationError::Taxonomy(detail) => write!(f, "taxonomy load failed: {detail}"),
            GenerationError::Rejected {
                generation_id,
                reason,
            } => write!(f, "generation {generation_id} rejected: {reason}"),
            GenerationError::Database(error) => write!(f, "snapshot load failed: {error}"),
        }
    }
}

impl std::error::Error for GenerationError {}

impl From<sqlx::Error> for GenerationError {
    fn from(error: sqlx::Error) -> Self {
        GenerationError::Database(error)
    }
}

/// Computes the generation's `taxonomy_version` (design §1.2): SHA-256 over
/// the effective YAML content — every `events/`, `categories/`, and
/// `synonyms/` file under `data_dir`, in sorted file order, with the scheme
/// header. This is the byte-for-byte same scheme the worker pins at build
/// time (`apps/ingest/src/support.rs::compute_taxonomy_version`); the two
/// implementations MUST change together — the loader rejects any candidate
/// whose manifest version does not match this hash, so a drift fails loudly
/// at load time instead of silently serving a mismatched taxonomy.
pub fn taxonomy_version(data_dir: &Path) -> Result<String, GenerationError> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for (label, dir) in [
        ("events", data_dir.join("events")),
        ("categories", data_dir.join("categories")),
        ("synonyms", data_dir.join("synonyms")),
    ] {
        let names = std::fs::read_dir(&dir)
            .map_err(|error| {
                GenerationError::Taxonomy(format!("taxonomy dir {}: {error}", dir.display()))
            })?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".yaml") || name.ends_with(".yml"))
            .collect::<Vec<_>>();
        for name in names {
            let path = dir.join(&name);
            let bytes = std::fs::read(&path).map_err(|error| {
                GenerationError::Taxonomy(format!("taxonomy file {}: {error}", path.display()))
            })?;
            files.push((format!("{label}/{name}"), bytes));
        }
    }
    files.sort();

    let mut hasher = Sha256::new();
    hasher.update(b"tramitesuy:taxonomy-version:v1\n");
    for (label, bytes) in &files {
        hasher.update(label.as_bytes());
        hasher.update(b"\n");
        hasher.update(bytes);
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // unwrap justification: fmt::Write into an owned String is infallible
        // (no recoverable allocator error), so `write!` cannot fail here.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// Loads the YAML taxonomy and builds the generation's engine (the exact
/// mapping `AppState::build` used before stage 3 — moved here so the engine
/// lives in the snapshot, not in global state; task 20 GREEN).
pub fn load_taxonomy_bundle(data_dir: &Path) -> Result<TaxonomyBundle, GenerationError> {
    let version = taxonomy_version(data_dir)?;
    let taxonomy = taxonomy::loader::load_data_dir(data_dir)
        .map_err(|error| GenerationError::Taxonomy(format!("taxonomy load failed: {error}")))?;
    let synonyms_map: SynonymMap = taxonomy
        .synonyms
        .iter()
        .map(|source| {
            (
                source.synonym.term.clone(),
                source.synonym.canonical.clone(),
            )
        })
        .collect();
    let events: Vec<EventLexicon> = taxonomy
        .events
        .iter()
        .map(|source| event_lexicon(&source.event))
        .collect();
    Ok(TaxonomyBundle {
        engine: Arc::new(SearchEngine::new(events, synonyms_map.clone())),
        taxonomy: Arc::new(taxonomy),
        synonyms: synonyms_map,
        version,
    })
}

/// Loads the newest usable published generation as an in-memory snapshot,
/// falling back to the previous one; `Ok(None)` when nothing is published
/// (cold start). A candidate that exists but cannot be used (incomplete,
/// taxonomy mismatch, projection gap) is rejected and the previous one is
/// tried; if every candidate fails, the error explains why — the served
/// generation is never changed by a failed load (task 22).
pub async fn load_published(
    pool: &sqlx::PgPool,
    data_dir: &Path,
) -> Result<Option<ActiveGeneration>, GenerationError> {
    let bundle = load_taxonomy_bundle(data_dir)?;
    load_published_with_bundle(
        pool,
        &bundle,
        db::providers::orchestrator::ProviderFetch::Sequential,
    )
    .await
}

/// [`load_published`] over an already-loaded taxonomy bundle (the boot path
/// reuses the state's own YAML load) with the configured provider fetch
/// policy.
pub async fn load_published_with_bundle(
    pool: &sqlx::PgPool,
    bundle: &TaxonomyBundle,
    fetch: db::providers::orchestrator::ProviderFetch,
) -> Result<Option<ActiveGeneration>, GenerationError> {
    let candidates = sqlx::query!(
        r#"SELECT generation_id, content_hash, taxonomy_version, engine_version,
                  source_synced_at, published_at AS "published_at!",
                  event_count, procedure_count, projection_status
           FROM catalog_generations
           WHERE status = 'published' AND published_at IS NOT NULL
           ORDER BY published_at DESC, created_at DESC"#,
    )
    .fetch_all(pool)
    .await?;

    if candidates.is_empty() {
        return Ok(None);
    }

    let mut last_rejection: Option<GenerationError> = None;
    for candidate in candidates {
        let candidate = CandidateManifest {
            generation_id: candidate.generation_id,
            content_hash: candidate.content_hash,
            taxonomy_version: candidate.taxonomy_version,
            engine_version: candidate.engine_version,
            source_synced_at: candidate.source_synced_at,
            published_at: candidate.published_at,
            event_count: candidate.event_count,
            procedure_count: candidate.procedure_count,
            projection_status: candidate.projection_status,
        };
        match load_candidate(pool, candidate, bundle, fetch).await {
            Ok(generation) => return Ok(Some(generation)),
            Err(error) => {
                eprintln!("api generation: candidate rejected: {error}");
                last_rejection = Some(error);
            }
        }
    }
    // Justified inline: this line runs only when `candidates` was
    // non-empty and every candidate failed, so `last_rejection` was set —
    // the loop body guarantees it.
    Err(last_rejection.expect("at least one rejected candidate"))
}

/// One durable published manifest row (the candidate the loader evaluates).
struct CandidateManifest {
    generation_id: Uuid,
    content_hash: String,
    taxonomy_version: String,
    engine_version: String,
    source_synced_at: DateTime<chrono::Utc>,
    published_at: DateTime<chrono::Utc>,
    event_count: i32,
    procedure_count: i32,
    projection_status: String,
}

async fn load_candidate(
    pool: &sqlx::PgPool,
    candidate: CandidateManifest,
    bundle: &TaxonomyBundle,
    fetch: db::providers::orchestrator::ProviderFetch,
) -> Result<ActiveGeneration, GenerationError> {
    let CandidateManifest {
        generation_id,
        content_hash,
        taxonomy_version,
        engine_version,
        source_synced_at,
        published_at,
        event_count,
        procedure_count,
        projection_status,
    } = candidate;

    // An interrupted build is never a candidate (task 19 TRIANGULATE):
    // complete projections are the load precondition.
    if projection_status != "complete" {
        return Err(GenerationError::Rejected {
            generation_id,
            reason: format!("projection_status is {projection_status}, not complete"),
        });
    }

    if taxonomy_version != bundle.version {
        return Err(GenerationError::Rejected {
            generation_id,
            reason: format!(
                "manifest taxonomy_version {taxonomy_version} does not match the boot taxonomy {}",
                bundle.version
            ),
        });
    }

    // Events: the generation's own projection, supplemented by the legacy
    // description (stage-3 interim: the description is not per-generation
    // projected; see the module note).
    let event_rows = sqlx::query!(
        r#"SELECT DISTINCT ON (g.slug) g.slug, g.name, g.status, g.category_slug,
                  COALESCE(l.description, '') AS "description!"
           FROM generation_life_events g
           LEFT JOIN life_events l ON l.slug = g.slug
           WHERE g.generation_id = $1
           ORDER BY g.slug"#,
        generation_id,
    )
    .fetch_all(pool)
    .await?;
    let events: HashMap<String, Arc<EventSnapshot>> = event_rows
        .into_iter()
        .map(|row| {
            (
                row.slug.clone(),
                Arc::new(EventSnapshot {
                    slug: row.slug,
                    name: row.name,
                    description: Some(row.description).filter(|d| !d.is_empty()),
                    status: row.status,
                    category_slug: row.category_slug,
                }),
            )
        })
        .collect();

    // Cards: the pre-projected per-event card JSONB, decoded into the same
    // `EventCard` shape the transition query serves — one DTO composition
    // path for attribution and cost (API-3/API-4).
    let card_rows = sqlx::query!(
        "SELECT slug, cards FROM generation_event_cards WHERE generation_id = $1",
        generation_id,
    )
    .fetch_all(pool)
    .await?;
    let mut cards: HashMap<String, Arc<Vec<db::repos::procedures::EventCard>>> = HashMap::new();
    for row in card_rows {
        let decoded: Vec<db::repos::procedures::EventCard> = row
            .cards
            .as_array()
            .map(|cards| cards.iter().filter_map(decode_card).collect())
            .unwrap_or_default();
        cards.insert(row.slug, Arc::new(decoded));
    }

    // Procedure details: the pre-projected detail JSONB keyed by external id.
    let detail_rows = sqlx::query!(
        "SELECT slug, details FROM generation_procedure_details WHERE generation_id = $1",
        generation_id,
    )
    .fetch_all(pool)
    .await?;
    let mut procedures: HashMap<String, Arc<db::repos::procedures::ProcedureDetail>> =
        HashMap::new();
    for row in detail_rows {
        if let Some(detail) = decode_detail(&row.details) {
            procedures.insert(row.slug, Arc::new(detail));
        }
    }

    // Ordered categories (stage-3 interim: legacy projection read once at
    // load, never per request).
    let category_rows = sqlx::query!(
        "SELECT slug, name, icon, order_index FROM categories ORDER BY order_index, slug",
    )
    .fetch_all(pool)
    .await?;
    let categories: Vec<CategoryView> = category_rows
        .into_iter()
        .map(|row| CategoryView {
            slug: row.slug,
            name: row.name,
            icon: row.icon,
            order_index: row.order_index,
        })
        .collect();

    // Organizations (stage-3 interim: same load-time legacy read).
    let organization_rows = sqlx::query!(
        "SELECT external_id, name, short_name FROM organizations ORDER BY external_id",
    )
    .fetch_all(pool)
    .await?;
    let organizations: HashMap<String, Arc<OrganizationView>> = organization_rows
        .into_iter()
        .map(|row| {
            (
                row.external_id.clone(),
                Arc::new(OrganizationView {
                    external_id: row.external_id,
                    name: row.name,
                    short_name: row.short_name,
                }),
            )
        })
        .collect();

    let manifest = Some(GenerationManifest {
        generation_id,
        content_hash,
        taxonomy_version,
        engine_version,
        source_synced_at,
        published_at,
        event_count,
        procedure_count,
    });
    Ok(ActiveGeneration::from_parts(
        bundle.clone(),
        SnapshotParts {
            manifest,
            events,
            categories,
            cards,
            procedures,
            organizations,
        },
        fetch,
    ))
}

/// Decodes one projected card (the build's JSONB shape, task 15) into the
/// transition `EventCard` the API payloads consume.
fn decode_card(card: &serde_json::Value) -> Option<db::repos::procedures::EventCard> {
    let get = |key: &str| card.get(key);
    let string = |key: &str| {
        get(key)
            .and_then(serde_json::Value::as_str)
            .map(String::from)
    };
    Some(db::repos::procedures::EventCard {
        slug: string("slug")?,
        name: string("name")?,
        order_index: get("order_index")?.as_i64()? as i32,
        importance: string("importance"),
        required: get("required")?.as_bool()?,
        organization_short_name: string("organization_short_name"),
        cost: string("cost"),
        status: string("status")?,
        official_url: string("official_url"),
        last_seen_at: parse_timestamp(string("last_seen_at")?)?,
    })
}

/// Decodes one projected procedure detail (the build's JSONB shape).
fn decode_detail(details: &serde_json::Value) -> Option<db::repos::procedures::ProcedureDetail> {
    let string = |key: &str| {
        details
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(String::from)
    };
    Some(db::repos::procedures::ProcedureDetail {
        external_id: string("external_id")?,
        name: string("name")?,
        description: string("description"),
        organization_name: string("organization_name"),
        official_url: string("official_url"),
        status: string("status")?,
        raw_data: details.get("raw_data").cloned(),
        last_seen_at: parse_timestamp(string("last_seen_at")?)?,
    })
}

/// Parses the timestamp Postgres serialized into the projection JSONB
/// (RFC 3339, e.g. `2026-09-18T03:00:00+00:00`).
fn parse_timestamp(raw: String) -> Option<DateTime<chrono::Utc>> {
    DateTime::parse_from_rfc3339(&raw)
        .ok()
        .map(|parsed| parsed.with_timezone(&chrono::Utc))
}

/// Projects one taxonomy event into the engine-side scoring lexicon
/// (moved verbatim from `state.rs` so the engine lives in the snapshot).
fn event_lexicon(event: &taxonomy::model::Event) -> EventLexicon {
    EventLexicon {
        slug: event.slug.clone(),
        category: event.category.clone(),
        keywords: event
            .keywords
            .iter()
            .map(|keyword| EngineKeyword {
                term: keyword.term.clone(),
                canonical: keyword.canonical_or_term().to_string(),
                kind: match keyword.keyword_type {
                    taxonomy::model::KeywordType::Action => EngineKeywordKind::Action,
                    taxonomy::model::KeywordType::Entity => EngineKeywordKind::Entity,
                    taxonomy::model::KeywordType::Modifier => EngineKeywordKind::Modifier,
                    taxonomy::model::KeywordType::Context => EngineKeywordKind::Context,
                },
                weight: keyword.weight,
                negative: keyword.negative,
            })
            .collect(),
        rules: event
            .rules
            .iter()
            .map(|rule| EngineRule {
                action: rule.action.clone(),
                entity: rule.entity.clone(),
                bonus: rule.bonus,
            })
            .collect(),
    }
}
