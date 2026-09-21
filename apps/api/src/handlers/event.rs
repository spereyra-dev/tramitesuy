//! `GET /events/:slug` (task 71, API-6): the event's name, description,
//! category, and its procedures ordered by `order_index`, each carrying
//! `order`, `required`, `official_url`, the cost pair, and the attribution
//! block; an unknown slug returns 404.
//!
//! S7 task 20: the page serves the CAPTURED generation snapshot — zero
//! catalog SQL. Before the first valid snapshot load the cold-start gate
//! (api delta) returns 503.

use axum::Json;
use axum::extract::{Path, State};

use crate::dto;
use crate::error::ApiError;
use crate::state::AppState;

/// Field-wise clone of the snapshot's shared cards: the `crates/db` record
/// types carry no `Clone` (outside this slice's edit surfaces), and the
/// payload assembly consumes owned values. The data is immutable, so the
/// copy is byte-identical.
fn cloned_cards(
    cards: &[db::repos::procedures::EventCard],
) -> Vec<db::repos::procedures::EventCard> {
    cards
        .iter()
        .map(|card| db::repos::procedures::EventCard {
            slug: card.slug.clone(),
            name: card.name.clone(),
            order_index: card.order_index,
            importance: card.importance.clone(),
            required: card.required,
            organization_short_name: card.organization_short_name.clone(),
            cost: card.cost.clone(),
            status: card.status.clone(),
            official_url: card.official_url.clone(),
            last_seen_at: card.last_seen_at,
        })
        .collect()
}

/// This route's low-cardinality metrics label (task 1).
const ROUTE: &str = "/api/v1/events/{slug}";

pub async fn get(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<dto::EventPage>, ApiError> {
    // Design §1.4: the request's first operation captures its generation.
    let generation = state.active.load_full();
    if !generation.is_loaded() {
        return Err(ApiError::ColdStart);
    }
    let Some(event) = generation.event(&slug) else {
        return Err(ApiError::NotFound);
    };
    let cards = generation.cards(&slug).unwrap_or_default();
    // Snapshot cards decoded into the same `EventCard` shape the transition
    // query serves — one DTO composition path for attribution + cost
    // (API-3/API-4).
    let procedures = dto::procedure_cards_from_event_cards(cloned_cards(&cards));
    state.metrics.observe_sql_ops(ROUTE, 0);
    Ok(Json(dto::EventPage {
        slug: event.slug.clone(),
        name: event.name.clone(),
        description: event.description.clone(),
        category: event.category_slug.clone(),
        procedures,
    }))
}
