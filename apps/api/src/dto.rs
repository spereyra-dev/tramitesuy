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
