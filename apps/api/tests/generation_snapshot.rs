//! S7 task 19 (OPT-01/OPT-03, catalog-generations delta, R8): the
//! in-memory snapshot. `ActiveGeneration` loads from a durable published
//! generation WITHOUT an AGESIC download and answers category/event/
//! procedure lookups entirely in memory — zero catalog SQL per request.
//!
//! Load semantics proven here:
//! - a known category/event/procedure answers with 0 catalog SQL statements;
//! - a nonexistent procedure id resolves to None with 0 SQL (404 upstream);
//! - an inactive procedure stays fetchable with `status: "inactive"` and its
//!   attribution inputs intact;
//! - the loader issues only the snapshot projection reads (a recorded,
//!   small statement count) — no version history or search-log query ever
//! - runs, and the snapshot carries no log/version surface;
//! - TRIANGULATE: reloading the same durable generation yields identical
//!   lookups (deterministic load), and a `building`/incomplete generation
//!   is rejected as a candidate (the previous one stays adoptable).

mod support;

use std::sync::Arc;
use support::*;

/// A corrupt projection in the newest published generation must not produce
/// a partial snapshot. The existing loader fallback keeps G1 intact both
/// on direct load and through the reconciliation tick.
#[tokio::test(flavor = "multi_thread")]
async fn malformed_card_or_detail_rejects_the_whole_candidate_and_keeps_g1_serving() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let g1 = publish_sample_generation(&pool).await.generation_id;
    let metrics = Arc::new(api::metrics::MemoryMetrics::new());
    let limits = api::config::ApiLimits {
        lag_alert_after: std::time::Duration::ZERO,
        ..Default::default()
    };
    let state = api::state::AppState::boot_with_metrics(
        pool.clone(),
        &repo_root().join("data"),
        limits,
        metrics.clone(),
    )
    .await
    .expect("boot G1");
    let app = api::build_router(state.clone());

    sqlx::query("UPDATE procedures SET name = name || ' (G2)' WHERE external_id = '4551'")
        .execute(&pool)
        .await
        .expect("change content for G2");
    let g2 = publish_sample_generation(&pool).await.generation_id;
    assert_ne!(g1, g2);
    let original_cards: serde_json::Value = sqlx::query_scalar(
        "SELECT cards FROM generation_event_cards WHERE generation_id = $1 AND slug = 'comprar-vehiculo'",
    )
    .bind(g2)
    .fetch_one(&pool)
    .await
    .expect("G2 cards");

    for (table, statement) in [
        (
            "card",
            "UPDATE generation_event_cards SET cards = jsonb_set(cards, '{0,last_seen_at}', 'null'::jsonb) WHERE generation_id = $1 AND slug = 'comprar-vehiculo'",
        ),
        (
            "detail",
            "UPDATE generation_procedure_details SET details = jsonb_set(details, '{last_seen_at}', 'null'::jsonb) WHERE generation_id = $1 AND slug = '4551'",
        ),
    ] {
        sqlx::query(statement)
            .bind(g2)
            .execute(&pool)
            .await
            .expect("corrupt G2");
        let fallback = api::generation::load_published(&pool, &repo_root().join("data"))
            .await
            .expect("previous generation remains loadable")
            .expect("G1 available");
        assert_eq!(
            fallback.generation_id(),
            g1,
            "{table} corruption must reject G2 as a whole"
        );
        let tick = api::generation::reconcile::tick(&state)
            .await
            .expect("reconcile");
        assert!(
            tick.adopted.is_none() && tick.already_current && tick.lagging,
            "{table}: {tick:?}"
        );
        assert_eq!(
            state.generation_id(),
            g1,
            "{table}: no partial snapshot installed"
        );
        let (status, body) = request(&app, "GET", "/api/v1/procedures/4551").await;
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(
            body["name"], "Solicitud de empadronamientos",
            "{table}: G1 still serves"
        );
        assert_eq!(
            fallback.cards("comprar-vehiculo").expect("G1 cards").len(),
            2
        );
        assert!(
            metrics.alert_total(api::metrics::OperationalAlert::PublicationLag) >= 1,
            "{table}: lagging publication is observable"
        );
        if table == "card" {
            // Test the detail decoder independently; a still-broken card
            // must not hide a detail that would otherwise be dropped.
            sqlx::query(
                "UPDATE generation_event_cards SET cards = $2 WHERE generation_id = $1 AND slug = 'comprar-vehiculo'",
            )
            .bind(g2)
            .bind(&original_cards)
            .execute(&pool)
            .await
            .expect("restore G2 cards");
        }
    }
    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_serves_known_catalog_lookups_without_sql() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;

    // The load itself is the only SQL the snapshot ever needs; its
    // statement count is recorded (manifest, events, cards, details,
    // categories, organizations).
    section.reset();
    let generation = api::generation::load_published(&pool, &repo_root().join("data"))
        .await
        .expect("published generation loads")
        .expect("a published generation exists");
    let load_statements = section.count();
    assert_eq!(
        load_statements, 6,
        "recorded load cost: one statement per snapshot source (observed \
         {load_statements}) — no version-history or search-log query runs"
    );

    // Every serving lookup is pure memory: zero catalog SQL.
    section.reset();
    let categories = generation.categories();
    assert_eq!(
        categories
            .iter()
            .map(|category| category.slug.as_str())
            .collect::<Vec<_>>(),
        vec!["vehiculos", "trabajo"],
        "ordered by order_index ascending (vehiculos first)"
    );
    assert_eq!(generation.category_name("vehiculos"), Some("Vehículos"));
    assert_eq!(
        generation.event_name("comprar-vehiculo"),
        Some("Comprar un vehículo")
    );

    let event = generation
        .event("comprar-vehiculo")
        .expect("known event resolves from the snapshot");
    assert_eq!(event.slug, "comprar-vehiculo");
    assert_eq!(event.name, "Comprar un vehículo");
    assert_eq!(
        event.description.as_deref(),
        Some("Requisitos y trámites para comprar un vehículo.")
    );
    assert_eq!(event.status, "active");
    assert_eq!(event.category_slug, "vehiculos");

    let events = generation.events_of_category("vehiculos");
    assert_eq!(
        events
            .iter()
            .map(|event| event.slug.as_str())
            .collect::<Vec<_>>(),
        vec!["comprar-vehiculo", "vender-vehiculo"]
    );

    let cards = generation
        .cards("comprar-vehiculo")
        .expect("known event resolves its cards from the snapshot");
    assert_eq!(
        cards
            .iter()
            .map(|card| card.slug.as_str())
            .collect::<Vec<_>>(),
        vec!["4551", "2368"],
        "cards keep the relation order"
    );

    let detail = generation
        .procedure("4551")
        .expect("known procedure resolves from the snapshot");
    assert_eq!(detail.name, "Solicitud de empadronamientos");
    assert_eq!(detail.status, "active");
    assert_eq!(
        detail.organization_name.as_deref(),
        Some("Ministerio de Transporte")
    );

    assert!(
        generation.procedure("no-existe").is_none(),
        "a nonexistent procedure id resolves to None with no database query"
    );
    assert!(
        generation.cards("no-existe").is_none(),
        "an unknown event carries no card row in the snapshot"
    );
    assert_eq!(
        section.count(),
        0,
        "all serving lookups ran with zero SQL statements"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn inactive_procedure_stays_fetchable_from_the_snapshot_with_attribution() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;

    let generation = api::generation::load_published(&pool, &repo_root().join("data"))
        .await
        .expect("published generation loads")
        .expect("a published generation exists");

    let detail = generation
        .procedure("6995")
        .expect("the deactivated procedure stays in the snapshot");
    assert_eq!(detail.status, "inactive", "current contract behavior");
    assert_eq!(detail.name, "Registro de Automotoras");
    assert_eq!(
        detail.official_url.as_deref(),
        Some("https://www.gub.uy/tramite/6995")
    );
    // Attribution inputs (API-4): the official URL and the last-synced stamp
    // survive deactivation, so the payload keeps its attribution block.
    let expected = last_seen_of(&pool, "6995").await;
    assert_eq!(detail.last_seen_at.to_rfc3339(), expected);
    assert!(
        generation.cards("vender-vehiculo").is_some(),
        "the deactivated procedure keeps its event relation (IN-7)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshot_reloads_deterministically_from_the_same_durable_generation() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;

    let first = api::generation::load_published(&pool, &repo_root().join("data"))
        .await
        .expect("first load succeeds")
        .expect("a published generation exists");
    let second = api::generation::load_published(&pool, &repo_root().join("data"))
        .await
        .expect("second load succeeds")
        .expect("a published generation exists");

    // Same durable generation ⇒ identical manifest identity and lookups.
    assert_eq!(
        first.manifest().expect("loaded").generation_id,
        second.manifest().expect("loaded").generation_id
    );
    assert_eq!(
        first.manifest().expect("loaded").content_hash,
        second.manifest().expect("loaded").content_hash
    );
    assert_eq!(first.categories().len(), second.categories().len());
    for category in first.categories() {
        assert_eq!(
            first.category_name(&category.slug),
            second.category_name(&category.slug)
        );
    }
    for event_slug in ["comprar-vehiculo", "vender-vehiculo"] {
        let a = first.event(event_slug).expect("event present");
        let b = second.event(event_slug).expect("event present");
        assert_eq!(a.name, b.name);
        assert_eq!(a.description, b.description);
        assert_eq!(a.status, b.status);
        let ca = first.cards(event_slug).expect("cards present");
        let cb = second.cards(event_slug).expect("cards present");
        assert_eq!(
            ca.iter().map(|c| c.slug.as_str()).collect::<Vec<_>>(),
            cb.iter().map(|c| c.slug.as_str()).collect::<Vec<_>>()
        );
    }
    for procedure in ["4551", "2368", "6995", "7001"] {
        let a = first.procedure(procedure).expect("detail present");
        let b = second.procedure(procedure).expect("detail present");
        assert_eq!(a.status, b.status);
        assert_eq!(a.name, b.name);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_incomplete_generation_is_rejected_and_the_previous_one_adopts() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let published = publish_sample_generation(&pool).await;

    // A later interrupted build over changed content: manifest row only,
    // projections incomplete, taxonomy version identical to the boot YAML so
    // the only disqualifier is incompleteness.
    sqlx::query("UPDATE procedures SET name = name || ' (cambiado)' WHERE external_id = '4551'")
        .execute(&pool)
        .await
        .expect("content change for the interrupted build");
    let taxonomy_version =
        api::generation::taxonomy_version(&repo_root().join("data")).expect("version hash");
    let orphan = db::generations::build::begin_build(&pool, &taxonomy_version)
        .await
        .expect("interrupted build starts");

    let generation = api::generation::load_published(&pool, &repo_root().join("data"))
        .await
        .expect("the loader reports why the candidate failed")
        .expect("the previous complete generation is still adoptable");

    assert_ne!(
        generation.manifest().expect("loaded").generation_id,
        orphan.generation_id,
        "an interrupted (building) generation is never a candidate"
    );
    assert_eq!(
        generation.manifest().expect("loaded").generation_id,
        published.generation_id
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn no_published_generation_leaves_the_api_cold() {
    let (pool, _db) = fresh_migrated_db().await;
    let loaded = api::generation::load_published(&pool, &repo_root().join("data")).await;
    assert!(
        matches!(loaded, Ok(None)),
        "nothing published means a cold start, not an error"
    );
}
