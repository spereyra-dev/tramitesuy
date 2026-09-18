//! Cross-cutting response DTOs (tasks 70/74/75/76): the odc-uy attribution
//! block (API-4), the missing-cost rule (API-3), and the hyphen-slug
//! response validator (TX-4). Read-endpoint payload assembly composes these.

use serde::Serialize;
use serde_json::Value;

/// The exact hyphen slug convention every exposed event/category slug must
/// satisfy (TX-4). Reuses the taxonomy crate's implementation so the seed
/// validator and the response validator can never drift apart.
pub const SLUG_PATTERN: &str = r"^[a-z0-9]+(-[a-z0-9]+)*$";

/// The source catalog name every procedure-bearing payload attributes (API-4).
pub const SOURCE_NAME: &str = "Catálogo de trámites y servicios del Estado — AGESIC";

/// The dataset license attributed with every procedure payload (API-4).
pub const SOURCE_LICENSE: &str = "odc-uy";

/// The exact missing-cost wording (API-3). The system never invents,
/// estimates, or defaults a cost value.
pub const SIN_COSTO_INFORMADO: &str = "Sin costo informado";

/// The per-procedure attribution block (API-4) attached to every payload
/// carrying procedure data.
#[derive(Debug, Serialize)]
pub struct SourceAttribution {
    pub official: bool,
    pub name: String,
    pub official_url: Option<String>,
    /// Timestamp of the last ingestion run that touched the procedure
    /// (procedures.last_seen_at; RFC 3339).
    pub last_synced_at: Option<String>,
    pub license: String,
}

/// Builds the attribution block: the catalog is always official; the
/// official URL and the last-synced timestamp come from the procedure row.
pub fn source_attribution(
    official_url: Option<String>,
    last_synced_at: Option<String>,
) -> SourceAttribution {
    SourceAttribution {
        official: true,
        name: SOURCE_NAME.to_string(),
        official_url,
        last_synced_at,
        license: SOURCE_LICENSE.to_string(),
    }
}

/// The cost pair every payload exposing cost carries (API-3).
#[derive(Debug, Serialize)]
pub struct CostFields {
    pub cost: Option<String>,
    pub cost_display: String,
}

/// The missing-cost rule over the source row's `tiene_costo`/`valor`
/// columns (kept in `procedures.raw_data`): either column empty or absent
/// renders `cost: null` with the exact "Sin costo informado" wording; a
/// populated value passes through verbatim with `cost_display` reflecting
/// it. No code path defaults or estimates a cost.
pub fn cost_fields(tiene_costo: Option<&str>, valor: Option<&str>) -> CostFields {
    let reported = tiene_costo.is_some_and(|v| !v.trim().is_empty())
        && valor.is_some_and(|v| !v.trim().is_empty());
    if reported {
        let value = valor.unwrap_or_default().to_string();
        CostFields {
            cost: Some(value.clone()),
            cost_display: value,
        }
    } else {
        CostFields {
            cost: None,
            cost_display: SIN_COSTO_INFORMADO.to_string(),
        }
    }
}

/// Extracts the cost pair from a procedure's `raw_data` JSONB (the source
/// row's columns; NULL raw_data is the absent-source case → missing cost).
pub fn cost_fields_from_raw(raw_data: Option<&Value>) -> CostFields {
    let field = |name: &str| raw_data.and_then(|v| v.get(name)).and_then(Value::as_str);
    cost_fields(field("tiene_costo"), field("valor"))
}

/// Whether one slug satisfies the hyphen convention (TX-4). Delegates to
/// the taxonomy validator so the seed and the API share one implementation.
pub fn is_valid_api_slug(slug: &str) -> bool {
    taxonomy::validator::is_valid_slug(slug)
}

/// Validates every event/category slug carried in an `/api/v1` response
/// payload against the hyphen convention (task 76, TX-4: "API never exposes
/// underscore slugs"). The keys `slug` and `category` hold event and
/// category slugs in every payload shape; the walk is recursive so a nested
/// payload cannot bypass the check.
pub fn validate_slugs_in_response(payload: &Value) -> Result<(), String> {
    let mut offenders = Vec::new();
    walk_slugs(payload, &mut offenders);
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "slugs violating the hyphen convention {SLUG_PATTERN}: {}",
            offenders.join(", ")
        ))
    }
}

fn walk_slugs(value: &Value, offenders: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if (key == "slug" || key == "category") && child.is_string() {
                    let slug = child.as_str().unwrap_or_default();
                    if !is_valid_api_slug(slug) {
                        offenders.push(slug.to_string());
                    }
                }
                walk_slugs(child, offenders);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_slugs(item, offenders);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Read-endpoint payload assembly (task 77, API-6/7/8): composes the
// attribution block (API-4) and the missing-cost rule (API-3) into every
// payload carrying procedure data. `apps/api` stays SQL-free — these
// functions consume the `crates/db` read records.
// ---------------------------------------------------------------------------

use db::repos::procedures::{EventProcedures, ProcedureDetail};
use db::repos::taxonomy_seed::{CategorySummaryRow, EventSummaryRow};

/// One procedure card on an event page (API-6): name, order, required,
/// official_url, the cost pair, and the attribution block.
#[derive(Debug, Serialize)]
pub struct ProcedureCard {
    pub external_id: String,
    pub name: String,
    pub order: i32,
    pub required: bool,
    pub official_url: Option<String>,
    pub cost: Option<String>,
    pub cost_display: String,
    pub source: SourceAttribution,
}

/// The event-page payload (API-6): name, description, category, and the
/// procedures ordered by `order_index`.
#[derive(Debug, Serialize)]
pub struct EventPage {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub procedures: Vec<ProcedureCard>,
}

/// Assembles the procedure cards shared by the event page and the search
/// open-mode summary (one code path for attribution + cost, API-3/API-4).
pub fn procedure_cards(
    procedures: Vec<db::repos::procedures::EventProcedure>,
) -> Vec<ProcedureCard> {
    procedures
        .into_iter()
        .map(|p| {
            let CostFields { cost, cost_display } = cost_fields_from_raw(p.raw_data.as_ref());
            ProcedureCard {
                external_id: p.external_id,
                name: p.name,
                order: p.order_index,
                required: p.required,
                official_url: p.official_url.clone(),
                cost,
                cost_display,
                source: source_attribution(p.official_url, Some(p.last_seen_at.to_rfc3339())),
            }
        })
        .collect()
}

/// Assembles the event page from the db projection (unknown slug never
/// reaches here: the handler maps it to 404).
pub fn event_page(projection: EventProcedures) -> EventPage {
    EventPage {
        slug: projection.event.slug,
        name: projection.event.name,
        description: projection.event.description,
        category: projection.event.category_slug,
        procedures: procedure_cards(projection.procedures),
    }
}

/// The procedure-detail payload (API-8): name, description, organization,
/// official_url, cost fields, status, and the attribution block.
#[derive(Debug, Serialize)]
pub struct ProcedureDetailPage {
    pub external_id: String,
    pub name: String,
    pub description: Option<String>,
    pub organization: Option<String>,
    pub official_url: Option<String>,
    pub cost: Option<String>,
    pub cost_display: String,
    pub status: String,
    pub source: SourceAttribution,
}

/// Assembles the procedure detail from the db record (an inactive procedure
/// keeps its attribution block and reports `status: "inactive"`).
pub fn procedure_detail_page(detail: ProcedureDetail) -> ProcedureDetailPage {
    let CostFields { cost, cost_display } = cost_fields_from_raw(detail.raw_data.as_ref());
    ProcedureDetailPage {
        external_id: detail.external_id,
        name: detail.name,
        description: detail.description,
        organization: detail.organization_name,
        official_url: detail.official_url.clone(),
        cost,
        cost_display,
        status: detail.status,
        source: source_attribution(detail.official_url, Some(detail.last_seen_at.to_rfc3339())),
    }
}

/// One category on the ordered list (API-7).
#[derive(Debug, Serialize)]
pub struct CategoryPage {
    pub slug: String,
    pub name: String,
    pub order_index: i32,
}

/// The categories-list payload (API-7): ordered by `order_index` ascending.
#[derive(Debug, Serialize)]
pub struct CategoriesPage {
    pub categories: Vec<CategoryPage>,
}

pub fn categories_page(rows: Vec<CategorySummaryRow>) -> CategoriesPage {
    CategoriesPage {
        categories: rows
            .into_iter()
            .map(|r| CategoryPage {
                slug: r.slug,
                name: r.name,
                order_index: r.order_index,
            })
            .collect(),
    }
}

/// One event on a category's listing (API-7).
#[derive(Debug, Serialize)]
pub struct EventSummaryPage {
    pub slug: String,
    pub name: String,
}

/// The category-events payload (API-7).
#[derive(Debug, Serialize)]
pub struct CategoryEventsPage {
    pub category: String,
    pub events: Vec<EventSummaryPage>,
}

pub fn category_events_page(
    category_slug: String,
    rows: Vec<EventSummaryRow>,
) -> CategoryEventsPage {
    CategoryEventsPage {
        category: category_slug,
        events: rows
            .into_iter()
            .map(|r| EventSummaryPage {
                slug: r.slug,
                name: r.name,
            })
            .collect(),
    }
}
